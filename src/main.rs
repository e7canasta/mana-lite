mod assignment;
mod cascade;
mod config;
mod depth;
mod detection;
mod error;
mod fsm;
mod infer;
mod ingest;
mod kalman;
mod logger;
mod metrics;
mod pipeline;
mod snapshot;
mod track;
mod viz;
mod zones;

use cascade::{CascadeRule, CascadeScheduler, CascadeTarget};
use config::{
    AppConfig, CropType, MetricsLogConfig, RerunBlueprintConfig, load_app_config, load_config,
    load_depth_rules, load_fsm_catalog, load_metrics_log, load_model_catalog, load_rerun_blueprint,
    load_viz_data, load_zone_catalog, validate_fsm,
};
use detection::{ConsolidatedObservation, DetectionConsolidator, DetectionRole, ModelDetections};
use error::{ConfigError, ManaError, Result};
use fsm::FsmEngine;
use infer::{CropRect, InferEngine, InferenceResult, compute_bbox_roi, compute_upper_square_roi};
use ingest::{IngestEngine, RawKeyframe, RetinaReader};
use logger::{DetRecord, Event, JsonlLevel, Logger};
use mana_types::RawFrameV1;
use metrics::{Health, MetricsEngine, PerClassFrameStats};
use pipeline::PipelineState;
use snapshot::{FrameBuffer, FrameDecoder, SnapshotSaver};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use track::{Tracker, TrackerConfig, track_event_to_log};
use ultralytics_inference::DepthMap;
use viz::VizBridge;
use zones::{ZoneEngine, zone_event_to_log};

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
    detection_consolidator: DetectionConsolidator,
    model_tasks: HashMap<String, String>,
    model_enabled: HashMap<String, bool>,
    ingest: IngestEngine<RetinaReader>,
    metrics: MetricsEngine,
    health: Health,
    depth_roi: Option<CropRect>,
    depth_rules: depth::DepthRules,
    decoder: FrameDecoder,
    snapshots: SnapshotSaver,
    viz: VizBridge,
    state: PipelineState,
    log: Logger,
    crop_frames_pending: Vec<CropFrameQueue>,
}

struct CropFrameQueue {
    model: String,
    crop_frame: Option<infer::CropFrameInfo>,
}

struct PendingModelOutput {
    model_key: String,
    output: InferenceResult,
    crop_frame: Option<infer::CropFrameInfo>,
    crop_rect: Option<CropRect>,
}

impl App {
    async fn bootstrap(config: &AppConfig, config_path: &std::path::Path) -> Result<Self> {
        if !config.detection.face_component_coverage.is_finite()
            || !config.detection.face_max_center_y_ratio.is_finite()
            || !(0.0..=1.0).contains(&config.detection.face_component_coverage)
            || !(0.0..=1.0).contains(&config.detection.face_max_center_y_ratio)
        {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: "detection".into(),
                msg: "face association ratios must be within 0..=1".into(),
            }));
        }
        let model_catalog = load_model_catalog(&config.inference.model_catalog)?;
        if let Some((model, _)) = model_catalog
            .models
            .iter()
            .find(|(_, entry)| !entry.is_valid())
        {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: format!("models.{model}"),
                msg: "confidence, iou, max_det and imgsz must be valid positive model settings"
                    .into(),
            }));
        }
        if let Some((model, _)) = model_catalog
            .models
            .iter()
            .find(|(_, entry)| !entry.postprocess.is_valid())
        {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: format!("models.{model}.postprocess"),
                msg: "confidence and area thresholds must be finite and ordered".into(),
            }));
        }
        if let Some((model, _)) = model_catalog
            .models
            .iter()
            .find(|(_, entry)| entry.crop.as_ref().is_some_and(|crop| !crop.is_valid()))
        {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: format!("models.{model}.crop"),
                msg: "square_size must be positive and upper_fraction must be within 0..=1".into(),
            }));
        }
        let zones = config
            .inference
            .zones_file
            .as_ref()
            .map(|p| load_zone_catalog(p))
            .transpose()?;
        let fsm = config
            .inference
            .fsm_file
            .as_ref()
            .map(|p| load_fsm_catalog(p))
            .transpose()?;
        let depth_rules = config
            .inference
            .depth_rules_file
            .as_ref()
            .map(|p| load_depth_rules(p))
            .transpose()?
            .unwrap_or_default();
        let depth_rule_errors = depth_rules.validate();
        if !depth_rule_errors.is_empty() {
            return Err(ManaError::Config(ConfigError::ValidationError(
                depth_rule_errors.join("; "),
            )));
        }

        let viz_data = config
            .viz_file
            .as_ref()
            .map(|p| load_viz_data(p))
            .transpose()?
            .unwrap_or_default();
        let metrics_log: MetricsLogConfig = config
            .metrics_file
            .as_ref()
            .map(|p| load_metrics_log(p))
            .transpose()?
            .unwrap_or_default();
        let rerun_blueprint = config
            .rerun_file
            .as_ref()
            .map(|p| load_rerun_blueprint(p))
            .transpose()?
            .unwrap_or_else(RerunBlueprintConfig::default);

        let default_model = model_catalog
            .models
            .get(&config.inference.default_model)
            .ok_or_else(|| ManaError::ModelNotFound(config.inference.default_model.clone()))?;
        if !default_model.enabled {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: format!("models.{}", config.inference.default_model),
                msg: "default model must be enabled".into(),
            }));
        }
        log::info!(
            "default model: {} ({})",
            config.inference.default_model,
            default_model.path.display()
        );

        if let Some(ref z) = zones {
            log::info!("zones loaded: {} zones", z.zones.len());
        }
        if let Some(ref f) = fsm {
            log::info!(
                "fsm loaded: {} states, {} transitions",
                f.fsm.states.len(),
                f.fsm.transitions.len()
            );
            let errors = validate_fsm(f, &model_catalog, &zones);
            for e in &errors {
                log::error!("fsm validation: {e}");
            }
            if !errors.is_empty() {
                return Err(ManaError::FsmGuardError(format!(
                    "{} FSM validation errors",
                    errors.len()
                )));
            }
        }

        let jsonl_level = JsonlLevel::from_str(&config.output.jsonl_level);
        let mut log = if let Some(ref dir) = config.output.save_dir {
            Logger::rotating(dir.clone(), &config.output.rotate, jsonl_level)?
        } else {
            Logger::new(jsonl_level)
        };
        log.set_jsonl_config(metrics_log.metrics.jsonl.clone());

        log::info!(
            "mana-lite v{VERSION} starting (output {})",
            config.output.format
        );
        log::info!("source: {}", config.source.url);
        log.emit(Event::meta_startup(
            VERSION,
            &config_path.display().to_string(),
        ));
        log.emit(Event::meta_model_loaded(
            &config.inference.default_model,
            &default_model.path.display().to_string(),
            &default_model.task,
            0,
        ));

        let depth_roi = model_catalog
            .models
            .get("depth-standard")
            .and_then(|entry| entry.crop.as_ref())
            .filter(|crop| crop.crop_type == CropType::Static)
            .and_then(|crop| crop.region)
            .map(CropRect::from_array);

        let infer = InferEngine::from_catalog(&model_catalog)?;
        log::info!("inference: {} model(s) loaded", infer.model_count());
        ultralytics_inference::logging::set_verbose(false);

        let tracker = Tracker::with_config(TrackerConfig {
            min_hits: config.tracking.min_hits,
            max_age: config.tracking.max_age,
            tentative_max_age: config.tracking.tentative_max_age,
            iou_threshold: config.tracking.iou_threshold,
        });
        let zone_engine = zones.as_ref().map(|z| ZoneEngine::from_catalog(z));
        let fsm_engine = fsm.as_ref().map(|f| FsmEngine::from_catalog(f));
        let (cascade_rules, cascade_regions) = if let Some(ref path) = config.inference.cascade_file
        {
            let cfg: cascade::CascadeConfig = load_config(path)?;
            let errors = cfg.validate(&model_catalog, &config.inference.default_model);
            if !errors.is_empty() {
                return Err(ManaError::Config(ConfigError::InvalidValue {
                    field: "inference.cascade_file".into(),
                    msg: errors.join("; "),
                }));
            }
            (cfg.rules, cfg.regions)
        } else {
            (
                vec![
                    CascadeRule {
                        model: "detect-fast".into(),
                        requires: None,
                        requires_class: None,
                        requires_exact_count: None,
                        same_frame: false,
                        requires_min_confidence: None,
                        requires_min_area_ratio: None,
                        requires_region: None,
                        requires_region_coverage: None,
                    },
                    CascadeRule {
                        model: "detect-large".into(),
                        requires: None,
                        requires_class: None,
                        requires_exact_count: None,
                        same_frame: false,
                        requires_min_confidence: None,
                        requires_min_area_ratio: None,
                        requires_region: None,
                        requires_region_coverage: None,
                    },
                    CascadeRule {
                        model: "detect-v2".into(),
                        requires: None,
                        requires_class: None,
                        requires_exact_count: None,
                        same_frame: false,
                        requires_min_confidence: None,
                        requires_min_area_ratio: None,
                        requires_region: None,
                        requires_region_coverage: None,
                    },
                ],
                HashMap::new(),
            )
        };
        let cascade = CascadeScheduler::from_rules_and_regions(&cascade_rules, cascade_regions);
        let model_tasks: HashMap<String, String> = model_catalog
            .models
            .iter()
            .map(|(k, v)| (k.clone(), v.task.clone()))
            .collect();
        let model_enabled: HashMap<String, bool> = model_catalog
            .models
            .iter()
            .map(|(k, v)| (k.clone(), v.enabled))
            .collect();

        let reader = RetinaReader::connect(
            &config.source.url,
            config.source.username.as_deref(),
            config.source.password.as_deref(),
            &config.source.transport,
            &config.ingest,
        )
        .await?;
        let ingest = IngestEngine::new(reader);

        let metrics = MetricsEngine::new(metrics_log.metrics.report_interval_s);
        let health = Health::new(config.health.data_stale_ms);
        let decoder = FrameDecoder::new()?;
        let snapshots = SnapshotSaver::new(
            config.output.snapshot_dir.clone(),
            config.output.snapshot_verbose,
        )?;

        let viz = if config.viz.enabled {
            log::info!(
                "viz: will connect to rerun at {} when viewer opens",
                config.viz.rerun_addr
            );
            VizBridge::new(
                &config.viz.rerun_addr,
                &viz_data.viz.send,
                &rerun_blueprint.rerun,
            )
        } else {
            VizBridge::disabled()
        };

        let state = PipelineState::new(metrics_log.metrics.text.clone());

        Ok(Self {
            infer,
            model_tasks,
            model_enabled,
            tracker,
            zone_engine,
            fsm_engine,
            cascade,
            detection_consolidator: DetectionConsolidator::new(
                config.detection.face_component_coverage,
                config.detection.face_max_center_y_ratio,
            ),
            ingest,
            metrics,
            health,
            depth_roi,
            depth_rules,
            decoder,
            snapshots,
            viz,
            state,
            log,
            crop_frames_pending: Vec::new(),
        })
    }

    async fn run(&mut self, config: &AppConfig) -> Result<()> {
        loop {
            self.metrics.tick_cycle();

            if let Some(kf) = self.ingest.poll_freshest_keyframe().await {
                let max_panics = config.health.max_consecutive_panics;
                let result = catch_unwind(AssertUnwindSafe(|| {
                    self.process_keyframe(kf, config);
                }));
                match result {
                    Ok(()) => self.state.on_ok(),
                    Err(e) => {
                        let msg: String = e
                            .downcast_ref::<String>()
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
            self.state
                .evaluate_health(&mut self.health, &mut self.log, &mut self.metrics);
            self.log.flush();

            self.viz.tick();
        }

        #[allow(unreachable_code)]
        self.log.shutdown("loop_exit");
        Ok(())
    }

    fn process_keyframe(&mut self, kf: RawKeyframe, config: &AppConfig) {
        self.viz.set_frame_time();
        self.viz.log_keyframe_selection(
            kf.keyframes_seen,
            kf.keyframes_dropped,
            kf.source_window_ms,
        );

        let (frame_buf, decode_us) = self.decoder.decode_timed(&kf.h264);
        let dt_ms = self.state.on_keyframe(
            decode_us,
            &mut self.metrics,
            &mut self.health,
            &mut self.log,
        );
        self.viz.log_decode_latency(decode_us);
        self.viz.log_keyframe_gap(dt_ms);
        self.save_snapshot_if_enabled(&kf.h264, &frame_buf, config);

        let Some(ref fb) = frame_buf else { return };

        if config.pipeline.infer {
            self.run_inference(fb, config);
        }
        self.evaluate_scene(config);
        self.flush_viz_metrics(&frame_buf);
    }

    fn save_snapshot_if_enabled(
        &self,
        h264: &[u8],
        frame_buf: &Option<FrameBuffer>,
        config: &AppConfig,
    ) {
        if config.pipeline.snapshot {
            self.snapshots.save(h264, frame_buf.as_ref());
        }
    }

    fn run_inference(&mut self, fb: &FrameBuffer, config: &AppConfig) {
        self.viz.clear_depth_context_boxes();
        let requested = self.resolve_models(config);
        let ordered = self.cascade.ordered(&requested);
        let mut pending: Vec<PendingModelOutput> = Vec::new();

        for model_key in ordered {
            let is_static = self
                .infer
                .crop_info(model_key)
                .map_or(false, |c| c.crop_type == CropType::Static);

            let target = if self.cascade.same_frame(model_key) {
                self.cascade
                    .parent_of(model_key)
                    .and_then(|parent| {
                        pending
                            .iter()
                            .find(|item| item.model_key == parent)
                            .map(|item| item.output.detections.as_slice())
                    })
                    .and_then(|detections| {
                        self.cascade.target_for_detections(model_key, detections)
                    })
            } else {
                let current_tracks = self.tracker.current_tracks();
                self.cascade
                    .target_for(model_key, &current_tracks, fb.w, fb.h)
            };
            if self.cascade.parent_of(model_key).is_some() && target.is_none() {
                self.metrics.tick_infer_skip(model_key);
                continue;
            }

            let crop_rect = self.resolve_crop_rect(model_key, target, fb);

            let manual_crop = if is_static { None } else { crop_rect };
            if let Some(mut output) = self.infer.run(model_key, &fb.rgb, fb.w, fb.h, manual_crop) {
                let crop_frame = output.crop_frame.take();
                if config.pipeline.track && model_key == &config.inference.default_model {
                    let observations =
                        self.detection_consolidator.consolidate(&[ModelDetections {
                            model: model_key,
                            role: DetectionRole::Primary,
                            detections: &output.detections,
                        }]);
                    self.run_tracking(&observations);
                }
                pending.push(PendingModelOutput {
                    model_key: model_key.to_owned(),
                    output,
                    crop_frame,
                    crop_rect,
                });
            }
        }

        let model_outputs: Vec<ModelDetections> = pending
            .iter()
            .filter(|item| {
                self.model_tasks
                    .get(&item.model_key)
                    .is_none_or(|task| task != "depth")
            })
            .map(|item| ModelDetections {
                model: item.model_key.as_str(),
                role: if item.model_key == config.inference.default_model {
                    DetectionRole::Primary
                } else {
                    DetectionRole::Secondary
                },
                detections: &item.output.detections,
            })
            .collect();
        let observations = self.detection_consolidator.consolidate(&model_outputs);
        for observation in &observations {
            let mut sources: Vec<String> = observation
                .evidence
                .iter()
                .chain(observation.components.iter())
                .map(|e| e.model.clone())
                .collect();
            sources.sort();
            sources.dedup();
            self.log.emit(Event::consolidated_detection(
                self.state.frame_number(),
                &observation.class,
                observation.confidence,
                observation.bbox,
                &observation.primary_model,
                sources,
            ));
        }
        self.viz.log_consolidated_observations(&observations);
        if config.pipeline.track {
            self.tracker.enrich_observations(&observations);
        }
        for item in pending {
            self.record_model_result(
                &item.model_key,
                &item.output,
                item.crop_frame,
                item.crop_rect,
                fb.w,
                fb.h,
            );
        }
        if config.pipeline.track {
            self.publish_entities();
        }
    }

    fn resolve_crop_rect(
        &self,
        model_key: &str,
        target: Option<CascadeTarget>,
        fb: &FrameBuffer,
    ) -> Option<infer::CropRect> {
        let crop_cfg = self.infer.crop_info(model_key)?;

        if crop_cfg.crop_type == CropType::Static {
            return crop_cfg.region.map(CropRect::from_array);
        }

        let target = target?;
        if let Some(square_size) = crop_cfg.square_size {
            return compute_upper_square_roi(
                target.bbox,
                square_size,
                crop_cfg.upper_fraction.unwrap_or(0.5),
                fb.w,
                fb.h,
            );
        }
        compute_bbox_roi(
            target.bbox,
            crop_cfg.margin,
            fb.w,
            fb.h,
            crop_cfg.min_region,
            crop_cfg.max_region,
        )
    }

    fn is_model_enabled(&self, config: &AppConfig, name: &str) -> bool {
        self.model_enabled.get(name).copied().unwrap_or(true)
            && self
                .model_tasks
                .get(name)
                .map(|task| !config.inference.disabled_tasks.contains(task))
                .unwrap_or(true)
    }

    fn resolve_models(&self, config: &AppConfig) -> Vec<String> {
        let models = if config.pipeline.fsm {
            self.fsm_engine
                .as_ref()
                .map(|f| f.current_models())
                .unwrap_or_else(|| self.cascade.all_models().to_vec())
        } else {
            self.cascade.all_models().to_vec()
        };
        models
            .into_iter()
            .filter(|name| self.is_model_enabled(config, name))
            .collect()
    }

    fn record_model_result(
        &mut self,
        model_key: &str,
        output: &InferenceResult,
        crop_frame: Option<infer::CropFrameInfo>,
        crop_rect: Option<CropRect>,
        frame_w: u32,
        frame_h: u32,
    ) {
        if self
            .model_tasks
            .get(model_key)
            .is_some_and(|task| task == "depth")
        {
            self.record_depth_result(model_key, output, crop_rect, frame_w, frame_h);
            return;
        }
        if output.postprocess_rejected > 0 || output.post_nms_suppressed > 0 {
            log::info!(
                "model {model_key}: postprocess rejected={} nms_suppressed={}",
                output.postprocess_rejected,
                output.post_nms_suppressed,
            );
        }
        let per_class = PerClassFrameStats::from_detections(&output.detections);
        self.metrics.tick_inference_model(
            model_key,
            output.pipeline_us,
            &output.detections,
            crop_rect.map(|r| r.to_array()),
        );
        self.viz
            .log_infer_latency(model_key, output.infer_ms * 1000, output.pipeline_us);
        if let Some(rect) = crop_rect {
            self.viz.log_roi_boxes(model_key, rect);
        }
        self.viz.log_per_frame_class_stats(model_key, &per_class);
        self.viz
            .log_model_detections(model_key, &output.detections, crop_rect);
        self.viz.log_model_pose(model_key, &output.detections);
        self.viz
            .log_depth_context_boxes(model_key, &output.detections, self.depth_roi);
        self.viz.log_depth_context_polygons(
            model_key,
            &output.detections,
            self.depth_roi,
            frame_w,
            frame_h,
        );
        if output.detections.iter().any(|d| d.mask.is_some()) {
            self.viz
                .log_model_masks(model_key, &output.detections, frame_w, frame_h);
        }
        if crop_rect.is_some() && !(model_key == "face-yolo" && output.detections.is_empty()) {
            self.crop_frames_pending.push(CropFrameQueue {
                model: model_key.to_string(),
                crop_frame,
            });
        }
        self.log.emit(Event::detection(
            self.state.frame_number(),
            model_key,
            output.infer_ms,
            output.pipeline_us / 1000,
            output.detections.iter().map(DetRecord::from).collect(),
            output.postprocess_rejected,
            output.post_nms_suppressed,
            Some(per_class),
            crop_rect.map(|r| r.to_array()),
        ));
    }

    fn record_depth_result(
        &mut self,
        model_key: &str,
        output: &InferenceResult,
        crop_rect: Option<CropRect>,
        frame_w: u32,
        frame_h: u32,
    ) {
        let (width, height, valid_pixels, min_depth_m, max_depth_m) =
            depth_summary(output.depth.as_ref(), frame_w, frame_h);
        let map_area = (width as u64) * (height as u64);
        let valid_ratio = (map_area > 0).then(|| valid_pixels as f32 / map_area as f32);
        self.metrics.tick_inference_depth(
            model_key,
            output.pipeline_us,
            output.depth.as_ref(),
            crop_rect.map(|rect| rect.to_array()),
        );
        self.viz
            .log_infer_latency(model_key, output.infer_ms * 1000, output.pipeline_us);
        if let Some(rect) = crop_rect {
            self.viz.log_roi_boxes(model_key, rect);
        }
        self.viz.log_model_depth(model_key, output.depth.as_ref());
        self.log.emit(Event::depth(
            self.state.frame_number(),
            model_key,
            output.infer_ms,
            output.pipeline_us / 1000,
            crop_rect.map(|rect| rect.to_array()),
            width,
            height,
            valid_pixels,
            valid_ratio,
            min_depth_m,
            max_depth_m,
        ));
        self.evaluate_depth_rules(output, crop_rect);
    }

    fn evaluate_depth_rules(&mut self, output: &InferenceResult, crop_rect: Option<CropRect>) {
        let Some(depth) = output.depth.as_ref() else {
            return;
        };
        let Some(roi) = crop_rect.or(self.depth_roi).map(|rect| rect.to_array()) else {
            return;
        };
        for result in self.depth_rules.evaluate(depth, roi) {
            self.log.emit(Event::depth_region(
                self.state.frame_number(),
                &result.rule,
                result.region,
                &format!("{:?}", result.metric).to_lowercase(),
                result.value,
                result.threshold_m,
                result.triggered,
                result.valid_pixels,
                result.valid_ratio,
            ));
        }
    }

    fn run_tracking(&mut self, observations: &[ConsolidatedObservation]) {
        let track_events = self.tracker.update_observations(observations);
        for ev in &track_events {
            self.log
                .emit(track_event_to_log(ev, self.state.frame_number()));
        }
    }

    fn publish_entities(&mut self) {
        let active_tracks = self.tracker.active_tracks();
        self.viz.log_entity_boxes(&active_tracks);
        for track in self.tracker.current_tracks() {
            let mut sources: Vec<String> = track.evidence.iter().map(|e| e.model.clone()).collect();
            sources.sort();
            sources.dedup();
            self.log.emit(Event::entity(
                track.id,
                &track.class,
                track.bbox,
                sources,
                self.state.frame_number(),
            ));
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
                self.log
                    .emit(zone_event_to_log(ev, self.state.frame_number()));
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
                &tr.from,
                tr.from_label.as_deref(),
                &tr.to,
                tr.to_label.as_deref(),
                &tr.trigger,
                tr.dwell_ms,
            ));
        }
    }

    fn flush_viz_metrics(&mut self, frame_buf: &Option<FrameBuffer>) {
        if let Some(fb) = frame_buf.as_ref() {
            let header = raw_frame_header(fb, self.state.frame_number());
            self.viz.log_frame(&header, &fb.rgb);
            for entry in self.crop_frames_pending.drain(..) {
                if let Some(crop) = entry.crop_frame {
                    self.viz.log_crop_frame(&entry.model, &header, crop);
                }
            }
        }
    }

    fn drain_ingest_counters(&mut self) {
        let c = self.ingest.drain_ingest_counters();
        self.metrics.tick_ingest(
            c.pframes_dropped,
            c.keyframes_dup,
            c.keyframes_seen,
            c.keyframes_dropped,
        );
        if let Some(r) = c.retina {
            self.metrics.tick_retina_counters(
                r.timeouts,
                r.ssrc_changes,
                r.rtp_errors,
                r.stream_ends,
                r.reconnect_attempts,
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

fn depth_summary(
    depth: Option<&DepthMap>,
    fallback_width: u32,
    fallback_height: u32,
) -> (u32, u32, u64, Option<f32>, Option<f32>) {
    let Some(depth) = depth else {
        return (fallback_width, fallback_height, 0, None, None);
    };
    let (width, height) = crate::depth::map_dims(depth);
    let mut valid_pixels = 0;
    let mut min_depth = f32::INFINITY;
    let mut max_depth: f32 = 0.0;
    for &value in &depth.data {
        if value.is_finite() && value > 0.0 {
            valid_pixels += 1;
            min_depth = min_depth.min(value);
            max_depth = max_depth.max(value);
        }
    }
    if valid_pixels == 0 {
        (width, height, 0, None, None)
    } else {
        (
            width,
            height,
            valid_pixels,
            Some(min_depth),
            Some(max_depth),
        )
    }
}

fn parse_args() -> Result<PathBuf> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        println!("mana-lite v{VERSION}");
        std::process::exit(0);
    }

    if args.len() == 3 && args[1] == "--config" {
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
