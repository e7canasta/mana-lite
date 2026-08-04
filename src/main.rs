mod config;
mod error;
mod ingest;
mod logger;
mod metrics;
mod snapshot;

use config::*;
use error::*;
use ingest::*;
use logger::*;
use metrics::*;
use snapshot::*;
use std::collections::VecDeque;
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

    let mut log = if let Some(ref dir) = app_config.output.save_dir {
        Logger::rotating(dir.clone(), &app_config.output.rotate)?
    } else {
        Logger::new()
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

    let mut frame_count: u64 = 0;
    #[allow(unused_variables)]
    let panic_count: u32 = 0;
    let mut current_state: Option<String> = fsm.as_ref().map(|f| f.fsm.initial.clone());
    let mut fsm_stub_fired = false;

    let demo_mode = app_config.source.url.contains("demo") || args_has_flag("--demo");
    let mut ingest: IngestEngine<AnyReader> = if demo_mode {
        let frames = demo_frames();
        IngestEngine::new(AnyReader::Queued(QueuedReader::new(frames)))
    } else {
        let reader = RetinaReader::connect(
            &app_config.source.url,
            app_config.source.username.as_deref(),
            app_config.source.password.as_deref(),
            &app_config.source.transport,
        ).await?;
        IngestEngine::new(AnyReader::Retina(reader))
    };

    let mut metrics = MetricsEngine::new(app_config.health.report_interval_s);
    let mut health = Health::new(app_config.health.data_stale_ms);

    let snapshots = app_config.output.snapshot_dir.as_ref().map(|dir| {
        SnapshotSaver::new(dir.clone()).expect("create snapshot dir")
    });

    loop {
        metrics.tick_cycle();

        // PHASE 1: TIMERS (no-op until fsm.rs)

        // PHASE 2: EVALUATE (no-op until inference pipeline)

        // PHASE 3: INGEST
        if let Some(decoded) = ingest.poll_freshest_keyframe().await {
            frame_count += 1;
            health.touch();
            metrics.tick_keyframe();
            metrics.tick_decode(decoded.decode_us);
            log.emit(Event::frame_ingest(frame_count, true, decoded.decode_us));

            if let Some(ref s) = snapshots {
                if let Err(e) = s.save(&decoded.data) {
                    log::error!("snapshot save failed: {e}");
                }
            }
        }

        // PHASE 4: INFER (stub)
        if !fsm_stub_fired && frame_count >= 1 && current_state.is_some() {
            log.emit(Event::fsm_transition("idle", "watching", "bed_occupied", 0));
            current_state = Some("watching".into());
            fsm_stub_fired = true;
        }

        // PHASE 5: ZONES (no-op until inference)

        // PHASE 6: FSM (no-op until fsm.rs)

        // PHASE 7: PUBLISH
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
            log.emit(Event::metrics(
                report.window_s,
                report.cycles,
                report.frames_total,
                report.keyframes,
                report.pframes_dropped,
                report.inferences,
                report.infer_total_ms,
                report.decode_total_ms,
                report.blind_cycles,
            ));
        }

        log.flush();

        if demo_mode {
            if frame_count >= 5 {
                log::info!("demo: exiting after 5 frames");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    #[allow(unreachable_code)]
    log.shutdown("loop_exit");
    Ok(())
}

fn demo_frames() -> Vec<Frame> {
    let mut frames = VecDeque::new();
    for i in 1u8..=5 {
        frames.push_back(Frame {
            data: vec![i; 64],
            is_keyframe: true,
            timestamp: i as i64 * 2000,
        });
    }
    frames.into()
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
