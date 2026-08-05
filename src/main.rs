mod cascade;
mod config;
mod error;
mod fsm;
mod infer;
mod ingest;
mod logger;
mod metrics;
mod pipeline;
mod snapshot;
mod track;
mod viz;
mod zones;

use cascade::{CascadeRule, CascadeScheduler};
use config::{
    AppConfig, load_config, load_app_config, load_fsm_catalog, load_model_catalog,
    load_zone_catalog, validate_fsm, load_viz_data, load_metrics_log, load_rerun_blueprint,
    RerunBlueprintConfig,
};
use error::{ConfigError, ManaError, Result};
use fsm::FsmEngine;
use infer::{Detection, InferEngine};
use ingest::{AnyReader, Frame, IngestEngine, QueuedReader, RawKeyframe, RetinaReader};
use logger::{DetRecord, Event, JsonlLevel, Logger};
use mana_types::RawFrameV1;
use metrics::{Health, MetricsEngine, PerClassFrameStats};
use pipeline::PipelineState;
use snapshot::{FrameBuffer, FrameDecoder, SnapshotSaver};
use track::{Tracker, track_event_to_log};
use viz::VizBridge;
use zones::{ZoneEngine, zone_event_to_log};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::Instant;

static VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config_path = parse_args()?;
    let app_config = load_app_config(&config_path)?;
    let mut app = App::bootstrap(&app_config, &config_path).await?;
    app.run(&app_config).await
}

struct App {
    infer: InferEngine,
    tracker: Tracker,
    zone_engine: Option<ZoneEngine>,
    fsm_engine: Option<FsmEngine>,
    cascade: CascadeScheduler,
    model_tasks: HashMap<String, String>,
    ingest: IngestEngine<AnyReader>,
    metrics: MetricsEngine,
    health: Health,
    decoder: FrameDecoder,
    snapshots: SnapshotSaver,
    viz: VizBridge,
    state: PipelineState,
    log: Logger,
}

impl App {
    async fn bootstrap(config: &AppConfig, config_path: &std::path::Path) -> Result<Self> {
        let model_catalog = load_model_catalog(&config.inference.model_catalog)?;
        let zones = config.inference.zones_file.as_ref()
            .map(|p| load_zone_catalog(p))
            .transpose()?;
        let fsm = config.inference.fsm_file.as_ref()
            .map(|p| load_fsm_catalog(p))
            .transpose()?;

        let viz_data = config.viz_file.as_ref()
            .map(|p| load_viz_data(p))
            .transpose()?
            .unwrap_or_default();
        let _metrics_log = config.metrics_file.as_ref()
            .map(|p| load_metrics_log(p))
            .transpose()?
            .unwrap_or_default();
        let rerun_blueprint = config.rerun_file.as_ref()
            .map(|p| load_rerun_blueprint(p))
            .transpose()?
            .unwrap_or_else(RerunBlueprintConfig::default);

        let default_model = model_catalog.models.get(&config.inference.default_model)
            .ok_or_else(|| ManaError::ModelNotFound(config.inference.default_model.clone()))?;
        log::info!("default model: {} ({})", config.inference.default_model, default_model.path.display());

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

        let jsonl_level = JsonlLevel::from_str(&config.output.jsonl_level);
        let mut log = if let Some(ref dir) = config.output.save_dir {
            Logger::rotating(dir.clone(), &config.output.rotate, jsonl_level)?
        } else {
            Logger::new(jsonl_level)
        };

        log::info!("mana-lite v{VERSION} starting (output {})", config.output.format);
        log::info!("source: {}", config.source.url);
        log.emit(Event::meta_startup(VERSION, &config_path.display().to_string()));
        log.emit(Event::meta_model_loaded(
            &config.inference.default_model,
            &default_model.path.display().to_string(),
            &default_model.task,
            0,
        ));

        let infer = InferEngine::from_catalog(&model_catalog)?;
        log::info!("inference: {} model(s) loaded", infer.model_count());
        ultralytics_inference::logging::set_verbose(false);

        let tracker = Tracker::new();
        let zone_engine = zones.as_ref().map(|z| ZoneEngine::from_catalog(z));
        let fsm_engine = fsm.as_ref().map(|f| FsmEngine::from_catalog(f));
        let cascade_rules = if let Some(ref path) = config.inference.cascade_file {
            let cfg: cascade::CascadeConfig = load_config(path)?;
            cfg.rules
        } else {
            vec![
                CascadeRule { model: "detect-fast".into(), requires: None, requires_class: None },
                CascadeRule { model: "detect-large".into(), requires: None, requires_class: None },
                CascadeRule { model: "detect-v2".into(), requires: None, requires_class: None },
            ]
        };
        let cascade = CascadeScheduler::from_rules(&cascade_rules);
        let model_tasks: HashMap<String, String> = model_catalog.models.iter()
            .map(|(k, v)| (k.clone(), v.task.clone()))
            .collect();

        let demo_mode = config.source.demo || args_has_flag("--demo");

        let ingest: IngestEngine<AnyReader> = if demo_mode {
            IngestEngine::new(AnyReader::Queued(QueuedReader::new(demo_frames())))
        } else {
            let reader = RetinaReader::connect(
                &config.source.url,
                config.source.username.as_deref(),
                config.source.password.as_deref(),
                &config.source.transport,
                &config.ingest,
            ).await?;
            IngestEngine::new(AnyReader::Retina(reader))
        };

        let metrics = MetricsEngine::new(config.health.report_interval_s);
        let health = Health::new(config.health.data_stale_ms);
        let decoder = FrameDecoder::new()?;
        let snapshots = SnapshotSaver::new(
            config.output.snapshot_dir.clone(),
            config.output.snapshot_verbose,
        )?;

        let viz = if config.viz.enabled {
            log::info!("viz: will connect to rerun at {} when viewer opens", config.viz.rerun_addr);
            VizBridge::new(&config.viz.rerun_addr, &viz_data.viz.send, &rerun_blueprint.rerun)
        } else {
            VizBridge::disabled()
        };

        let state = PipelineState::new(demo_mode);

        Ok(Self {
            infer, model_tasks, tracker, zone_engine, fsm_engine, cascade,
            ingest, metrics, health, decoder, snapshots, viz, state, log,
        })
    }

    async fn run(&mut self, config: &AppConfig) -> Result<()> {
        loop {
            let loop_start = Instant::now();
            self.metrics.tick_cycle();

            if let Some(kf) = self.ingest.poll_freshest_keyframe().await {
                let max_panics = config.health.max_consecutive_panics;
                let result = catch_unwind(AssertUnwindSafe(|| {
                    self.process_keyframe(kf, config, loop_start);
                }));
                match result {
                    Ok(()) => self.state.on_ok(),
                    Err(e) => {
                        let msg: String = e.downcast_ref::<String>()
                            .cloned()
                            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or_else(|| "unknown panic".into());
                        log::error!("keyframe processing panicked: {msg}");
                        if self.state.on_panic(max_panics) {
                            log::error!("{} consecutive panics — exiting", max_panics);
                            break;
                        }
                    }
                }
            }

            self.drain_ingest_counters();
            self.evaluate_fsm_wildcard(config);
            let result = self.state.evaluate_health(&mut self.health, &mut self.log, &mut self.metrics);
            if let Some((r, _)) = result.as_ref() {
                self.viz.log_metrics_report(r);
            }
            self.log.flush();

            self.viz.tick();

            if self.state.should_exit() {
                break;
            }

            if self.state.is_demo() {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        #[allow(unreachable_code)]
        self.log.shutdown("loop_exit");
        Ok(())
    }

    fn process_keyframe(&mut self, kf: RawKeyframe, config: &AppConfig, loop_start: Instant) {
        let (frame_buf, decode_us) = self.decoder.decode_timed(&kf.h264);
        let dt_ms = self.state.on_keyframe(decode_us, &mut self.metrics, &mut self.health, &mut self.log);
        self.log_decode_latency_to_viz(decode_us);
        self.viz.log_keyframe_gap(dt_ms);
        self.save_snapshot_if_enabled(&kf.h264, &frame_buf, config);

        let Some(ref fb) = frame_buf else { return };

        if config.pipeline.infer {
            self.run_inference(fb, config);
        }
        self.evaluate_scene(config);
        self.flush_viz_metrics(&frame_buf, loop_start);
    }

    fn log_decode_latency_to_viz(&self, decode_us: u64) {
        self.viz.log_decode_latency(decode_us);
    }

    fn save_snapshot_if_enabled(&self, h264: &[u8], frame_buf: &Option<FrameBuffer>, config: &AppConfig) {
        if config.pipeline.snapshot {
            self.snapshots.save(h264, frame_buf.as_ref());
        }
    }

    fn run_inference(&mut self, fb: &FrameBuffer, config: &AppConfig) {
        let requested = self.resolve_models(config);
        let ordered = self.cascade.ordered(&requested);
        let mut model_dets: HashMap<String, Vec<Detection>> = HashMap::new();

        for model_key in &ordered {
            if !self.cascade.should_run(model_key, &model_dets) {
                self.metrics.tick_infer_skip(model_key);
                continue;
            }
            if let Some((detections, infer_ms)) = self.infer.run(model_key, &fb.rgb, fb.w, fb.h) {
                self.record_model_result(model_key, infer_ms, &detections);
                if config.pipeline.track {
                    self.run_tracking(&detections);
                }
                model_dets.insert(model_key.clone(), detections);
            }
        }
    }

    fn resolve_models(&self, config: &AppConfig) -> Vec<String> {
        let models = self.fsm_engine.as_ref()
            .map(|f| f.current_models())
            .unwrap_or_else(|| self.cascade.all_models());
        models.into_iter()
            .filter(|name| {
                self.model_tasks.get(name)
                    .map(|task| !config.inference.disabled_tasks.contains(task))
                    .unwrap_or(true)
            })
            .collect()
    }

    fn record_model_result(&mut self, model_key: &str, infer_ms: u64, detections: &[Detection]) {
        let per_class = PerClassFrameStats::from_detections(detections);
        self.metrics.tick_inference_model(model_key, infer_ms, detections);
        self.viz.log_infer_latency(model_key, infer_ms);
        self.viz.log_detection_boxes(model_key, detections);
        self.viz.log_per_frame_class_stats(model_key, &per_class);
        self.log.emit(Event::detection(
            self.state.frame_number(), model_key, infer_ms,
            detections.iter().map(DetRecord::from).collect(),
        ));
    }

    fn run_tracking(&mut self, detections: &[Detection]) {
        let track_input: Vec<(String, f32, [f32; 4])> = detections.iter()
            .map(|d| (d.class.clone(), d.confidence, d.bbox))
            .collect();
        let track_events = self.tracker.update(&track_input);
        for ev in &track_events {
            self.log.emit(track_event_to_log(ev, self.state.frame_number()));
        }
    }

    fn evaluate_scene(&mut self, config: &AppConfig) {
        if !config.pipeline.zones && !config.pipeline.fsm {
            return;
        }
        let (Some(ref mut zone), Some(ref mut fsm)) =
            (self.zone_engine.as_mut(), self.fsm_engine.as_mut())
        else {
            return;
        };

        let zone_events = if config.pipeline.zones {
            let active: Vec<&track::Track> = self.tracker.active_tracks();
            let events = zone.evaluate(&active);
            for ev in &events {
                self.log.emit(zone_event_to_log(ev, self.state.frame_number()));
            }
            events
        } else {
            vec![]
        };

        if config.pipeline.fsm {
            Self::try_advance_fsm(fsm, &zone_events, zone, &self.health, &mut self.log);
        }
    }

    fn try_advance_fsm(
        fsm: &mut FsmEngine,
        zone_events: &[zones::ZoneEvent],
        zone: &ZoneEngine,
        health: &Health,
        log: &mut Logger,
    ) {
        if let Some(tr) = fsm.evaluate(zone_events, zone, health) {
            log.emit(Event::fsm_transition(
                &tr.from, tr.from_label.as_deref(), &tr.to, tr.to_label.as_deref(), &tr.trigger, tr.dwell_ms,
            ));
        }
    }

    fn flush_viz_metrics(&mut self, frame_buf: &Option<FrameBuffer>, loop_start: Instant) {
        self.viz.log_track_counts(self.tracker.track_count(), self.tracker.active_tracks().len());
        self.viz.log_health_ms_since_frame(self.health.last_frame_elapsed_ms());
        if let Some(fb) = frame_buf.as_ref() {
            self.viz.log_frame(
                &raw_frame_header(fb, self.state.frame_number()),
                &fb.rgb,
                loop_start.elapsed().as_micros() as u64,
            );
        }
    }

    fn drain_ingest_counters(&mut self) {
        let c = self.ingest.drain_ingest_counters();
        self.metrics.tick_ingest(c.pframes_dropped, c.keyframes_dup);
        if let Some(r) = c.retina {
            self.metrics.tick_retina_counters(
                r.timeouts, r.ssrc_changes, r.rtp_errors, r.stream_ends, r.reconnect_attempts,
            );
        }
    }

    fn evaluate_fsm_wildcard(&mut self, config: &AppConfig) {
        if !config.pipeline.fsm {
            return;
        }
        if let (Some(ref mut fsm), Some(ref zone)) =
            (self.fsm_engine.as_mut(), self.zone_engine.as_ref())
        {
            Self::try_advance_fsm(fsm, &[], zone, &self.health, &mut self.log);
        }
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
        h264: vec![i; 64],
        is_keyframe: true,
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
