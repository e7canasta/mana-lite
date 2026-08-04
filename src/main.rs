mod config;
mod error;
mod ingest;
mod logger;
mod metrics;
mod snapshot;
mod viz;

use config::*;
use error::*;
use ingest::*;
use logger::*;
use mana_types::RawFrameV1;
use metrics::*;
use snapshot::*;
use viz::*;
use std::path::PathBuf;

static VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config_path = parse_args()?;
    let app_config = load_app_config(&config_path)?;

    let model_catalog = load_model_catalog(&app_config.inference.model_catalog)?;
    let zones = app_config.inference.zones_file.as_ref()
        .map(|p| load_zone_catalog(p))
        .transpose()?;
    let fsm = app_config.inference.fsm_file.as_ref()
        .map(|p| load_fsm_catalog(p))
        .transpose()?;

    let default_model = model_catalog.models.get(&app_config.inference.default_model)
        .ok_or_else(|| ManaError::ModelNotFound(app_config.inference.default_model.clone()))?;
    log::info!("default model: {} ({})", app_config.inference.default_model, default_model.path.display());

    if let Some(ref z) = zones {
        log::info!("zones loaded: {} zones", z.zones.len());
    }
    if let Some(ref f) = fsm {
        log::info!("fsm loaded: {} states, {} transitions", f.fsm.states.len(), f.fsm.transitions.len());
        let errors = validate_fsm(f, &model_catalog, &zones);
        for e in &errors {
            log::error!("fsm validation: {e}");
        }
        if !errors.is_empty() {
            return Err(ManaError::FsmGuardError(format!("{} FSM validation errors", errors.len())));
        }
    }

    let jsonl_level = JsonlLevel::from_str(&app_config.output.jsonl_level);
    let mut log = if let Some(ref dir) = app_config.output.save_dir {
        Logger::rotating(dir.clone(), &app_config.output.rotate, jsonl_level)?
    } else {
        Logger::new(jsonl_level)
    };

    log::info!("mana-lite v{VERSION} starting");
    log::info!("source: {}", app_config.source.url);
    log.emit(Event::meta_startup(VERSION, &config_path.display().to_string()));
    log.emit(Event::meta_model_loaded(
        &app_config.inference.default_model,
        &default_model.path.display().to_string(),
        &default_model.task,
        0,
    ));

    let demo_mode = app_config.source.demo || args_has_flag("--demo");

    let mut ingest: IngestEngine<AnyReader> = if demo_mode {
        IngestEngine::new(AnyReader::Queued(QueuedReader::new(demo_frames())))
    } else {
        let reader = RetinaReader::connect(
            &app_config.source.url,
            app_config.source.username.as_deref(),
            app_config.source.password.as_deref(),
            &app_config.source.transport,
            &app_config.ingest,
        ).await?;
        IngestEngine::new(AnyReader::Retina(reader))
    };

    let mut metrics = MetricsEngine::new(app_config.health.report_interval_s);
    let mut health = Health::new(app_config.health.data_stale_ms);

    let mut snapshots = app_config.output.snapshot_dir.as_ref().map(|dir| {
        SnapshotSaver::new(dir.clone(), app_config.output.snapshot_verbose).expect("create snapshot dir")
    });

    let mut viz = if app_config.viz.enabled {
        log::info!("viz: connecting to rerun at {}", app_config.viz.rerun_addr);
        match VizBridge::new(&app_config.viz.rerun_addr) {
            Ok(v) => {
                log::info!("viz: connected to {}", app_config.viz.rerun_addr);
                Some(v)
            }
            Err(e) => {
                log::warn!("viz: connect failed: {e}");
                None
            }
        }
    } else {
        None
    };

    let mut state = PipelineState {
        frame_count: 0,
        current_state: fsm.as_ref().map(|f| f.fsm.initial.clone()),
        fsm_stub_fired: false,
        demo_mode,
    };

    loop {
        metrics.tick_cycle();

        let keyframe = ingest.poll_freshest_keyframe().await;
        if let Some(decoded) = keyframe {
            state.on_keyframe(&decoded, &mut metrics, &mut health, &mut log);
            let frame_buf = snapshots.as_mut().and_then(|s| s.capture(&decoded.data));
            if let (Some(ref mut v), Some(fb)) = (viz.as_mut(), &frame_buf) {
                v.log_frame(&raw_frame_header(fb, state.frame_count), &fb.rgb);
            }
        }

        if let Some(c) = ingest.drain_retina_counters() {
            metrics.tick_retina_counters(
                c.timeouts, c.ssrc_changes, c.rtp_errors, c.stream_ends, c.reconnect_attempts,
            );
        }

        state.on_stub_fsm(&mut log);
        state.on_health_eval(&mut health, &mut log, &mut metrics);
        log.flush();

        if let Some(ref mut v) = viz {
            v.tick();
        }

        if state.should_exit() {
            break;
        }

        if demo_mode {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    #[allow(unreachable_code)]
    log.shutdown("loop_exit");
    Ok(())
}

struct PipelineState {
    frame_count: u64,
    current_state: Option<String>,
    fsm_stub_fired: bool,
    demo_mode: bool,
}

impl PipelineState {
    fn on_keyframe(&mut self, decoded: &DecodedFrame, metrics: &mut MetricsEngine, health: &mut Health, log: &mut Logger) {
        self.frame_count += 1;
        health.touch();
        metrics.tick_keyframe();
        metrics.tick_decode(decoded.decode_us);
        log.emit(Event::frame_ingest(self.frame_count, true, decoded.decode_us));
    }

    fn on_stub_fsm(&mut self, log: &mut Logger) {
        if !self.fsm_stub_fired && self.frame_count >= 1 && self.current_state.is_some() {
            log.emit(Event::fsm_transition("idle", "watching", "bed_occupied", 0));
            self.current_state = Some("watching".into());
            self.fsm_stub_fired = true;
        }
    }

    fn on_health_eval(&mut self, health: &mut Health, log: &mut Logger, metrics: &mut MetricsEngine) {
        match health.evaluate() {
            HealthTransition::Blind { ms_since_frame } => {
                log.emit(Event::health_blind(ms_since_frame));
            }
            HealthTransition::Stale { component, ms_since_frame } => {
                log.emit(Event::health_stale(component, ms_since_frame));
            }
            HealthTransition::Recovered => {
                log.emit(Event::health_heartbeat(0, "ingest", 0));
            }
            HealthTransition::None => {}
        }
        if health.is_blind() {
            metrics.tick_blind();
        }
        if let Some(report) = metrics.take_report() {
            log.emit(Event::metrics(&report));
        }
    }

    fn should_exit(&self) -> bool {
        if self.demo_mode && self.frame_count >= 5 {
            log::info!("demo: exiting after 5 frames");
            return true;
        }
        false
    }
}

fn raw_frame_header(fb: &FrameBuffer, frame_id: u64) -> RawFrameV1 {
    RawFrameV1 {
        width: fb.w,
        height: fb.h,
        frame_id,
        timestamp_ns: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        ..Default::default()
    }
}

fn demo_frames() -> Vec<Frame> {
    (1u8..=5).map(|i| Frame {
        data: vec![i; 64],
        is_keyframe: true,
        timestamp: i as i64 * 2000,
    }).collect()
}

fn parse_args() -> Result<PathBuf> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        println!("mana-lite v{VERSION}");
        std::process::exit(0);
    }

    if args.len() >= 3 && args[1] == "--config" {
        return Ok(PathBuf::from(&args[2]));
    }

    if args.len() >= 2 && !args[1].starts_with('-') {
        return Ok(PathBuf::from(&args[1]));
    }

    Err(ManaError::Config(ConfigError::InvalidValue {
        field: "args".into(),
        msg: "Usage: mana-lite --config <mana.toml>".into(),
    }))
}

fn args_has_flag(flag: &str) -> bool {
    std::env::args().any(|a| a == flag)
}
