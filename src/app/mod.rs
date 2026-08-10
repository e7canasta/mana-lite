//! Orquestador del pipeline: bootstrap, superloop y etapas por keyframe.

mod bootstrap;
mod cycle;
mod observer;

pub use cycle::CycleContext;
pub use observer::{FanoutObserver, NullObserver, PipelineObserver};

use crate::cascade::{CascadeScheduler, CascadeTarget, GateObservation};
use mana_perception::domain::{ClassName, ModelId};
use crate::config::{AppConfig, CropType};
use crate::detection::CropRect;
use crate::detection::{ConsolidatedObservation, DetectionConsolidator, DetectionRole, ModelDetections};
use crate::domain::ModelRegistry;
use crate::error::Result;
use crate::face_dwell::FaceDwellLogStrategy;
use crate::infer::{InferEngine, InferenceResult, compute_bbox_roi, compute_upper_square_roi};
use crate::ingest::{FrameReader, IngestEngine, RawKeyframe, RetinaReader};
use crate::logger::{DetRecord, Event, scene_events_to_log};
use crate::metrics::{MetricsEngine, PerClassFrameStats};
use crate::pipeline::PipelineState;
use crate::scan::{ControlStamp, ControlState, SceneEvent, ScanTimeline};
use mana_control::domain::LoopId;
use crate::snapshot::{FrameBuffer, FrameDecoder, SnapshotSaver};
use mana_types::RawFrameV1;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;
use tokio::signal::unix::{SignalKind, signal};

pub(crate) static VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
struct ClinicalSample { observations: Vec<ConsolidatedObservation>, signal_valid: bool, raw_person_count: usize, frame_number: u64, face_model_ran: bool }

pub struct App<R: FrameReader = RetinaReader> {
    pub(crate) infer: InferEngine,
    pub(crate) primary_model: String,
    pub(crate) control: ControlState,
    pub(crate) scan_timeline: ScanTimeline,
    pub(crate) cascade: CascadeScheduler,
    pub(crate) detection_consolidator: DetectionConsolidator,
    pub(crate) models: ModelRegistry,
    pub(crate) ingest: IngestEngine<R>,
    pub(crate) metrics: MetricsEngine,
    pub(crate) depth_context_roi: Option<CropRect>,
    pub(crate) depth_rules: mana_control::DepthRules,
    pub(crate) decoder: FrameDecoder,
    pub(crate) snapshots: SnapshotSaver,
    pub(crate) observer: FanoutObserver,
    pub(crate) state: PipelineState,
    pub(crate) boot_wall: chrono::DateTime<chrono::Utc>,
    pub(crate) boot_instant: Instant,
    pub(crate) crop_frames_pending: Vec<CropFrameQueue>,
    pub(crate) face_dwell_logger: FaceDwellLogStrategy,
    /// Control's vocabulary mirror. The legacy scan state remains temporarily
    /// available for compatibility with the existing integration-test facade.
    control_image: mana_control::ProcessImage,
}

pub(crate) struct CropFrameQueue {
    pub(crate) model: String,
    pub(crate) crop_frame: Option<crate::infer::CropFrameInfo>,
}

struct PendingModelOutput {
    model_key: String,
    output: InferenceResult,
    crop_frame: Option<crate::infer::CropFrameInfo>,
    crop_rect: Option<CropRect>,
}

impl<R: FrameReader> App<R> {
    pub async fn run(&mut self, config: &AppConfig) -> Result<()> {
        let mut term = signal(SignalKind::terminate())?;
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        let mut shutdown_reason = "signal";
        let scan_config = config.scan.to_scan_config();
        let mut scan_interval = tokio::time::interval(scan_config.period());

        loop {
            tokio::select! {
                kf = self.ingest.poll_freshest_keyframe() => {
                    if let Some(kf) = kf {
                        let cycle_now = Instant::now();
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            self.process_keyframe(kf, config, cycle_now);
                        }));
                        match result {
                            Ok(()) => {
                                let _ = self.state.on_ok();
                            }
                            Err(e) => {
                                let msg: String = e
                                    .downcast_ref::<String>()
                                    .cloned()
                                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                                    .unwrap_or_else(|| "unknown panic".into());
                                log::error!("keyframe processing panicked: {msg}");
                                // No reanudar desde estado roto: blind forzado, y que
                                // el ciclo siguiente reconstruya desde idle.
                                if let Some(fsm) = self.control.fsm_engine.as_mut() {
                                    fsm.force_safe_state(cycle_now);
                                }
                                if self.state.on_panic() {
                                    shutdown_reason = "panic";
                                    log::error!(
                                        "panic density in the last {} cycles exceeded {} — exiting",
                                        config.health.panic_window_cycles,
                                        config.health.max_panics_in_window
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
                _ = scan_interval.tick() => {
                    let now = Instant::now();
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        self.scan_tick(config, now);
                    }));
                    match result {
                        Ok(()) => {
                            let _ = self.state.on_ok();
                        }
                        Err(e) => {
                            let msg: String = e
                                .downcast_ref::<String>()
                                .cloned()
                                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                                .unwrap_or_else(|| "unknown panic".into());
                            log::error!("scan processing panicked: {msg}");
                                if let Some(fsm) = self.control.fsm_engine.as_mut() {
                                fsm.force_safe_state(now);
                            }
                            if self.state.on_panic() {
                                shutdown_reason = "panic";
                                break;
                            }
                        }
                    }
                }
                _ = term.recv() => break,
                _ = &mut ctrl_c => break,
            }
        }

        self.observer.log.shutdown(shutdown_reason);
        Ok(())
    }

    fn process_keyframe(&mut self, kf: RawKeyframe, config: &AppConfig, cycle_now: Instant) {
        let frame_timestamp_ns = frame_timestamp_ns(&self.boot_wall, self.boot_instant, cycle_now);
        let (frame_buf, decode_us) = self.decoder.decode_timed(&kf.h264);
        if frame_buf.is_some() {
            // Solo un frame decodificable es senal fresca: si el decode falla,
            // Health queda sin touch y la ceguera sigue su curso.
            self.state.mark_health_fresh(
                &mut self.control.health,
                self.observer.log.as_mut(),
                cycle_now,
            );
        }
        let dt_ms = self.state.on_keyframe(
            decode_us,
            &mut self.metrics,
            self.observer.log.as_mut(),
            cycle_now,
        );
        self.observer
            .viz
            .set_frame_time(self.state.frame_number(), frame_timestamp_ns);
        self.observer.viz.log_keyframe_selection(
            kf.keyframes_seen,
            kf.keyframes_dropped,
            kf.source_window_ms,
        );
        self.observer.viz.log_decode_latency(decode_us);
        self.observer.viz.log_keyframe_gap(dt_ms);
        self.save_snapshot_if_enabled(&kf.h264, &frame_buf, config);

        let Some(ref fb) = frame_buf else { return };

        // Depth guards require evidence from this frame, never a stale result.
        self.control_image.reset_depth(cycle_now);
        if config.pipeline.infer {
            let cycle = CycleContext::new(
                fb,
                cycle_now,
                dt_ms,
                kf.source_window_ms,
                kf.keyframes_seen,
                kf.keyframes_dropped,
                self.state.frame_number(),
                frame_timestamp_ns,
            );
            self.run_inference(cycle, config);
        }
        self.flush_viz_metrics(&frame_buf, frame_timestamp_ns);
    }

    fn scan_tick(&mut self, _config: &AppConfig, now: Instant) {
        self.metrics.tick_cycle_at(now, false);
        self.drain_ingest_counters();
        let scan_now = if self.control.scan_seq == 0 {
            self.scan_timeline.now()
        } else {
            self.scan_timeline.advance()
        };
        let scene_events = crate::scan::scan(
            &mut self.control,
            &self.control_image,
            scan_now,
        );
        self.control_image.measurement_pending = false;

        let mut last_stamp: Option<ControlStamp> = None;
        for event in &scene_events {
            match event {
                SceneEvent::Occupancy {
                    state,
                    second_person,
                    signal,
                } => self.observer.on_occupancy(*state, *second_person, *signal),
                SceneEvent::EntityBoxes(tracks) => {
                    let track_refs: Vec<_> = tracks.iter().collect();
                    self.observer.viz.log_entity_boxes(&track_refs);
                }
                SceneEvent::FsmState(state) => self.observer.viz.log_face_state(state),
                SceneEvent::Presence { stamp, .. }
                | SceneEvent::Track { stamp, .. }
                | SceneEvent::Zone { stamp, .. } => {
                    last_stamp = Some(*stamp);
                }
                SceneEvent::FsmTransition(_) | SceneEvent::Health(_) => {}
            }
        }

        for event in scene_events_to_log(&scene_events) {
            self.observer.emit(event);
        }

        // Face-dwell needs the full FsmSnapshot, which SceneEvent::FsmState does
        // not carry — emit from App-owned state after the scan batch.
        if let (Some(fsm), Some(stamp)) = (self.control.fsm_engine.as_ref(), last_stamp) {
            let snapshot = fsm.snapshot_at(now);
            self.observer.emit(self.face_dwell_logger.keyframe_event(
                stamp,
                &self.control.fsm_context,
                &snapshot,
            ));
        }

        if self.control.health.is_blind() {
            self.metrics.tick_blind();
        }
        self.state
            .emit_metrics(self.observer.log.as_mut(), &mut self.metrics);
        self.observer.flush();
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

    fn run_inference(&mut self, cycle: CycleContext<'_>, config: &AppConfig) {
        let fb = cycle.frame;
        self.observer.viz.clear_depth_context_boxes();
        let requested = self.resolve_models(config);
        let ordered = self.cascade.ordered(&requested);
        let mut pending: Vec<PendingModelOutput> = Vec::new();

        let roots: Vec<String> = ordered
            .iter()
            .filter(|model| self.cascade.parent_of(model).is_none())
            .map(|model| (*model).to_owned())
            .collect();
        let mut primary_root_valid = false;
        for model_key in roots {
            let valid = self.run_scheduled_model(&model_key, None, fb, &mut pending);
            if model_key == self.primary_model {
                primary_root_valid = valid;
            }
        }

        let children: Vec<String> = ordered
            .iter()
            .filter(|model| self.cascade.parent_of(model).is_some())
            .map(|model| (*model).to_owned())
            .collect();
        for model_key in children {
            if self.control.tracker.as_ref().map_or(0, |tracker| {
                tracker
                    .current_tracks()
                    .into_iter()
                    .filter(|track| track.class.as_str() == config.presence.class)
                    .count()
            }) != 1
            {
                self.metrics.tick_infer_skip(&model_key);
                continue;
            }
            let target = if self.cascade.same_frame(&model_key) {
                self.cascade
                    .parent_of(&model_key)
                    .and_then(|parent| {
                        pending
                            .iter()
                            .find(|item| item.model_key == parent)
                            .map(|item| item.output.detections.as_slice())
                    })
                    .and_then(|detections| {
                        self.cascade
                            .target_for_detections(&model_key, detections, fb.w, fb.h)
                    })
            } else {
                let current_tracks = self
                    .control
                    .tracker
                    .as_ref()
                    .map_or_else(Vec::new, |tracker| tracker.current_tracks());
                let observations = gate_observations(&current_tracks);
                self.cascade
                    .target_for(&model_key, &observations, fb.w, fb.h)
            };
            if target.is_none() {
                self.metrics.tick_infer_skip(&model_key);
                continue;
            }
            self.run_scheduled_model(&model_key, target, fb, &mut pending);
        }

        let mut model_outputs: Vec<ModelDetections> = Vec::new();
        model_outputs.extend(
            pending
                .iter()
                .filter(|item| !self.models.is_depth(&item.model_key))
                .map(|item| ModelDetections {
                    model: item.model_key.as_str(),
                    role: if item.model_key == self.primary_model {
                        DetectionRole::Primary
                    } else {
                        DetectionRole::Secondary
                    },
                    detections: &item.output.detections,
                }),
        );
        let observations = self.detection_consolidator.consolidate(&model_outputs);
        let face_model_ran = pending
            .iter()
            .any(|item| self.models.is_face_model(&item.model_key));
        for observation in &observations {
            let mut sources: Vec<String> = observation
                .evidence
                .iter()
                .chain(observation.components.iter())
                .map(|e| e.model.clone())
                .collect();
            sources.sort();
            sources.dedup();
            self.observer.emit(Event::consolidated_detection(
                self.state.frame_number(),
                &observation.class,
                observation.confidence,
                observation.bbox,
                &observation.primary_model,
                sources,
            ));
        }
        self.observer.viz.log_consolidated_observations(
            &observations,
            mana_viz::util::FrameSize::new(fb.w, fb.h),
        );
        for item in pending {
            self.record_model_result(
                &item.model_key,
                &item.output,
                item.crop_frame,
                item.crop_rect,
                mana_viz::util::FrameSize::new(fb.w, fb.h),
                cycle.now,
            );
        }
        let raw_person_count = observations
            .iter()
            .filter(|observation| observation.class == config.presence.class)
            .count();
        let sample = ClinicalSample {
            observations,
            signal_valid: primary_root_valid,
            raw_person_count,
            frame_number: cycle.frame_number,
            face_model_ran,
        };
        self.control_image.observations = Some(mana_control::AgedEvidence::new(
            project_scene_sample(&sample),
            cycle.now,
        ));
        self.control_image.measurement_pending = true;
    }

    fn run_scheduled_model(
        &mut self,
        model_key: &str,
        target: Option<CascadeTarget>,
        fb: &FrameBuffer,
        pending: &mut Vec<PendingModelOutput>,
    ) -> bool {
        let is_static = self
            .infer
            .crop_info(model_key)
            .is_some_and(|c| c.crop_type == CropType::Static);
        let crop_rect = self.resolve_crop_rect(model_key, target, fb);
        let manual_crop = if is_static { None } else { crop_rect };
        let Some(mut output) = self.infer.run(model_key, &fb.rgb, fb.w, fb.h, manual_crop) else {
            return false;
        };
        let crop_frame = output.crop_frame.take();
        pending.push(PendingModelOutput {
            model_key: model_key.to_owned(),
            output,
            crop_frame,
            crop_rect,
        });
        true
    }

    fn resolve_crop_rect(
        &self,
        model_key: &str,
        target: Option<CascadeTarget>,
        fb: &FrameBuffer,
    ) -> Option<crate::detection::CropRect> {
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
        self.models.enabled(name)
            && self
                .models
                .task_of(name)
                .map(|task| {
                    !config
                        .inference
                        .disabled_tasks
                        .iter()
                        .any(|disabled| disabled == task.as_str())
                })
                .unwrap_or(true)
    }

    fn resolve_models(&self, config: &AppConfig) -> Vec<String> {
        let models = self.control.fsm_engine.as_ref().map_or_else(
            || self.cascade.all_models().to_vec(),
            |fsm| fsm.current_models(),
        );
        models
            .into_iter()
            .filter(|name| self.is_model_enabled(config, name))
            .collect()
    }

    fn record_model_result(
        &mut self,
        model_key: &str,
        output: &InferenceResult,
        crop_frame: Option<crate::infer::CropFrameInfo>,
        crop_rect: Option<CropRect>,
        frame: mana_viz::util::FrameSize,
        now: Instant,
    ) {
        if self.models.is_depth(model_key) {
            self.record_depth_result(model_key, output, crop_rect, frame.w, frame.h, now);
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
        self.observer
            .viz
            .log_infer_latency(model_key, output.infer_ms * 1000, output.pipeline_us);
        if let Some(rect) = crop_rect {
            self.observer.viz.log_roi_boxes(model_key, rect);
        }
        self.observer
            .viz
            .log_per_frame_class_stats(model_key, &per_class);
        self.observer
            .viz
            .log_model_detections(model_key, &output.detections, crop_rect, frame);
        self.observer
            .viz
            .log_model_pose(model_key, &output.detections);
        self.observer.viz.log_depth_context_boxes(
            model_key,
            &output.detections,
            self.depth_context_roi,
        );
        self.observer.viz.log_depth_context_polygons(
            model_key,
            &output.detections,
            self.depth_context_roi,
            frame,
        );
        if output.detections.iter().any(|d| d.mask.is_some()) {
            self.observer
                .viz
                .log_model_masks(model_key, &output.detections, frame);
        }
        if crop_rect.is_some()
            && !(self.models.is_face_model(model_key) && output.detections.is_empty())
        {
            self.crop_frames_pending.push(CropFrameQueue {
                model: model_key.to_string(),
                crop_frame,
            });
        }
        self.observer.emit(Event::detection(
            self.state.frame_number(),
            model_key,
            output.infer_ms,
            output.pipeline_us / 1000,
            output
                .detections
                .iter()
                .map(|detection| DetRecord::from_detection(detection, frame.w, frame.h))
                .collect(),
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
        now: Instant,
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
        self.observer
            .viz
            .log_infer_latency(model_key, output.infer_ms * 1000, output.pipeline_us);
        if let Some(rect) = crop_rect {
            self.observer.viz.log_roi_boxes(model_key, rect);
        }
        self.observer
            .viz
            .log_model_depth(model_key, output.depth.as_ref());
        self.observer.emit(Event::depth(
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
        self.evaluate_depth_rules(output, crop_rect, now);
    }

    fn evaluate_depth_rules(
        &mut self,
        output: &InferenceResult,
        crop_rect: Option<CropRect>,
        now: Instant,
    ) {
        let Some(depth) = output.depth.as_ref() else {
            return;
        };
        let Some(roi) = crop_rect
            .or(self.depth_context_roi)
            .map(|rect| rect.to_array())
        else {
            return;
        };
        let measurements: Vec<mana_control::DepthRegionStats> = self
            .depth_rules
            .rules
            .iter()
            .filter_map(|rule| {
                let stats = mana_perception::region_stats(depth, roi, rule.region)?;
                Some(mana_control::DepthRegionStats {
                    region: stats.region,
                    valid_pixels: stats.valid_pixels,
                    valid_ratio: stats.valid_ratio,
                    min_depth_m: stats.min_depth_m,
                    median_depth_m: stats.median_depth_m,
                    p10_depth_m: stats.p10_depth_m,
                    p90_depth_m: stats.p90_depth_m,
                    max_depth_m: stats.max_depth_m,
                })
            })
            .collect();
        let results = self.depth_rules.evaluate(&measurements);
        wire_depth_evidence(&mut self.control_image, &results, now);
        for result in &results {
            self.observer.emit(Event::depth_region(
                self.state.frame_number(),
                &result.rule,
                result.region,
                &format!("{:?}", result.metric).to_lowercase(),
                result.value,
                result.threshold_m,
                result.triggered,
                result.valid_pixels,
                result.valid_ratio,
                result.calibration,
            ));
        }
    }

    fn flush_viz_metrics(&mut self, frame_buf: &Option<FrameBuffer>, timestamp_ns: i64) {
        if let Some(fb) = frame_buf.as_ref() {
            let header = raw_frame_header(fb, self.state.frame_number(), timestamp_ns);
            self.observer.viz.log_frame(&header, &fb.rgb);
            for entry in self.crop_frames_pending.drain(..) {
                if let Some(crop) = entry.crop_frame {
                    self.observer
                        .viz
                        .log_crop_frame(&entry.model, &header, crop);
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
}

fn gate_observations(tracks: &[&mana_control::track::Track]) -> Vec<GateObservation> {
    tracks
        .iter()
        .map(|t| GateObservation {
            id: t.id,
            bbox: t.bbox,
            class: ClassName::new(t.class.as_str()),
            confidence: t.confidence,
            source_model: ModelId::new(t.source_model.as_str()),
            is_confirmed: t.is_confirmed,
            misses: t.misses,
        })
        .collect()
}

/// Application adapter from perception's rich consolidated evidence to the
/// narrow control input port. Mask payloads and model-specific components stay
/// on the perception side; control receives only scene facts it can decide on.
fn project_scene_sample(sample: &ClinicalSample) -> mana_control::SceneSample {
    mana_control::SceneSample {
        observations: sample
            .observations
            .iter()
            .map(|observation| {
                let mut source_models: Vec<_> = observation
                    .evidence
                    .iter()
                    .chain(&observation.components)
                    .map(|evidence| mana_control::domain::ModelId::new(evidence.model.as_str()))
                    .collect();
                source_models.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                source_models.dedup();
                let face = observation
                    .components
                    .iter()
                    .filter(|component| component.class == "face")
                    .max_by(|a, b| a.confidence.total_cmp(&b.confidence))
                    .map(|face| mana_control::FaceObservation {
                        bbox: face.bbox,
                        confidence: face.confidence,
                    });
                mana_control::SceneObservation {
                    class: mana_control::domain::ClassName::new(observation.class.as_str()),
                    bbox: observation.bbox,
                    confidence: observation.confidence,
                    source_models,
                    face,
                }
            })
            .collect(),
        signal_valid: sample.signal_valid,
        raw_person_count: sample.raw_person_count,
        frame_number: sample.frame_number,
        face_model_ran: sample.face_model_ran,
    }
}

/// Installs evaluated depth policy results into the control process image.
fn wire_depth_evidence(
    image: &mut mana_control::ProcessImage,
    results: &[mana_control::DepthRuleResult],
    now: Instant,
) {
    image.set_depth(mana_control::DepthRuleSnapshot::from_results(results), now);
}

fn raw_frame_header(fb: &FrameBuffer, frame_id: u64, timestamp_ns: i64) -> RawFrameV1 {
    RawFrameV1 {
        width: fb.w,
        height: fb.h,
        frame_id,
        timestamp_ns,
        ..Default::default()
    }
}

fn depth_summary(
    depth: Option<&crate::depth_map::DepthFrame>,
    fallback_width: u32,
    fallback_height: u32,
) -> (u32, u32, u64, Option<f32>, Option<f32>) {
    let Some(depth) = depth else {
        return (fallback_width, fallback_height, 0, None, None);
    };
    let (width, height) = depth.dims();
    let mut valid_pixels = 0;
    let mut min_depth = f32::INFINITY;
    let mut max_depth: f32 = 0.0;
    for value in depth.iter_values() {
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

/// Mapea un instante monotónico del proceso a nanosegundos de pared anclados
/// en el bootstrap. El resultado nunca decrece con `now`, porque el monotónico
/// solo avanza: un salto de NTP no puede desordenar ni duplicar el JSONL.
/// Degradado: si el ancla de pared falla (epoch fuera de rango), se parte de 0
/// pero la propiedad de monotonía se conserva igual.
fn frame_timestamp_ns(
    boot_wall: &chrono::DateTime<chrono::Utc>,
    boot_instant: Instant,
    now: Instant,
) -> i64 {
    let boot_ns = boot_wall.timestamp_nanos_opt().unwrap_or(0);
    let since_boot = now.saturating_duration_since(boot_instant).as_nanos();
    let delta_ns = i64::try_from(since_boot).unwrap_or(i64::MAX);
    boot_ns.saturating_add(delta_ns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MetricsTextConfig, ModelCatalog};
    use mana_control::{DepthCalibration, DepthMetric, DepthOp, DepthRegionRule, DepthRules, DepthRuleResult};
    use crate::ingest::SyntheticReader;
    use crate::logger::{JsonlLevel, LogManager};
    use crate::occupancy::OccupancyStateMachine;
    use crate::presence::PresenceFilter;
    use crate::scan::ControlPolicy;
    use crate::viz::VizBridge;
    use mana_control::config::{OccupancyPolicy, PresencePoiPolicy};
    use ndarray::Array2;
    use std::collections::HashMap;
    use ultralytics_inference::DepthMap;

    #[test]
    fn frame_timestamp_ns_es_monotona_en_el_instante() {
        let boot_at = chrono::Utc::now();
        let boot_instant = Instant::now();
        let t1 = boot_instant + std::time::Duration::from_millis(50);
        let t2 = t1 + std::time::Duration::from_millis(250);

        let ns1 = frame_timestamp_ns(&boot_at, boot_instant, t1);
        let ns2 = frame_timestamp_ns(&boot_at, boot_instant, t2);

        assert!(ns2 >= ns1, "un instante posterior nunca produce ts menor");
        assert!(
            ns2 - ns1 >= 200_000_000,
            "dos frames a 250ms de distancia no se colapsan"
        );
    }

    #[test]
    fn wired_depth_results_reach_control_snapshot() {
        let results = [DepthRuleResult {
            rule: "bed-approach".into(),
            region: [10, 20, 30, 40],
            metric: DepthMetric::Median,
            threshold_m: 1.5,
            value: Some(1.0),
            triggered: true,
            valid_pixels: 8,
            valid_ratio: Some(0.9),
            calibration: Some(DepthCalibration {
                reference_model_m: 1.0,
                reference_scene_m: 2.0,
            }),
        }];

        let mut image = mana_control::ProcessImage::empty();
        let now = Instant::now();
        // Keyframe start clears stale depth; wiring must then install evidence.
        image.reset_depth(now);
        assert_eq!(image.depth_snapshot().is_triggered("bed-approach"), None);

        wire_depth_evidence(&mut image, &results, now);
        assert_eq!(
            image.depth_snapshot().is_triggered("bed-approach"),
            Some(true),
            "evaluate_depth_rules must wire results into ProcessImage, not reset them"
        );
    }

    fn depth_map_from_rows(rows: &[&[f32]]) -> crate::depth_map::DepthFrame {
        let data = Array2::from_shape_fn((rows.len(), rows[0].len()), |(y, x)| rows[y][x]);
        crate::depth_map::DepthFrame::from_ultralytics(DepthMap::new(
            data,
            (rows.len() as u32, rows[0].len() as u32),
        ))
    }

    fn app_for_depth_rules(rules: DepthRules, context_roi: Option<CropRect>) -> App<SyntheticReader> {
        let start = Instant::now();
        let empty_catalog = ModelCatalog {
            models: HashMap::new(),
        };
        App {
            infer: InferEngine::from_catalog(&empty_catalog).expect("empty catalog loads"),
            primary_model: "detect-fast".into(),
            control: ControlState {
                loop_id: LoopId::default_loop(),
                tracker: None,
                presence: PresenceFilter::new(
                    false,
                    "person",
                    PresencePoiPolicy {
                        on_ms: 0,
                        off_ms: 0,
                    },
                ),
                occupancy: OccupancyStateMachine::new(OccupancyPolicy {
                    single_confirm_ms: 0,
                    empty_confirm_ms: 0,
                    multiple_confirm_ms: 0,
                    multiple_exit_ms: 0,
                    require_confirmed_tracks: false,
                }),
                zone_engine: None,
                fsm_engine: None,
                health: mana_control::health::Health::new_at(10_000, 5_000, start),
                fsm_context: Default::default(),
                last_scan_at: start,
                scan_seq: 0,
                policy: ControlPolicy {
                    person_class: "person".into(),
                    presence_enabled: false,
                    data_stale_ms: 10_000,
                    scan_period_ms: 200,
                    face_dwell_roi: None,
                    person_detection_roi: None,
                    face_edge_margin_px: 0,
                },
            },
            scan_timeline: ScanTimeline::new(LoopId::default_loop(), start, 200),
            cascade: CascadeScheduler::from_rules(&[]),
            detection_consolidator: DetectionConsolidator::new(0.7, 0.65, 0.5),
            models: ModelRegistry::from_catalog(&empty_catalog, "detect-fast"),
            ingest: IngestEngine::new(SyntheticReader::empty()),
            metrics: MetricsEngine::new(0, 50),
            depth_context_roi: context_roi,
            depth_rules: rules,
            decoder: FrameDecoder::new().expect("ffmpeg decoder"),
            snapshots: SnapshotSaver::new(None, false).expect("snapshots"),
            observer: FanoutObserver::new(
                VizBridge::disabled(),
                Box::new(LogManager::new(JsonlLevel::Info)),
            ),
            state: PipelineState::new(MetricsTextConfig::default(), 20, 3),
            boot_wall: chrono::Utc::now(),
            boot_instant: start,
            crop_frames_pending: Vec::new(),
            face_dwell_logger: FaceDwellLogStrategy,
            control_image: mana_control::ProcessImage::empty(),
        }
    }

    #[test]
    fn evaluate_depth_rules_projects_triggered_rule_into_process_image() {
        let rules = DepthRules {
            rules: vec![DepthRegionRule {
                name: "bed-approach".into(),
                region: [0, 0, 2, 2],
                metric: DepthMetric::Median,
                op: DepthOp::Lt,
                threshold_m: 2.0,
                min_valid_ratio: 0.0,
                calibration: None,
            }],
        };
        let mut app = app_for_depth_rules(rules, Some(CropRect::from_array([0, 0, 2, 2])));
        let now = Instant::now();
        app.control_image.reset_depth(now);
        assert_eq!(
            app.control_image
                .depth_snapshot()
                .is_triggered("bed-approach"),
            None
        );

        let output = InferenceResult {
            detections: Vec::new(),
            depth: Some(depth_map_from_rows(&[&[1.0, 1.0], &[1.0, 1.0]])),
            postprocess_rejected: 0,
            post_nms_suppressed: 0,
            infer_ms: 0,
            pipeline_us: 0,
            crop_frame: None,
        };
        app.evaluate_depth_rules(&output, None, now);
        assert_eq!(
            app.control_image
                .depth_snapshot()
                .is_triggered("bed-approach"),
            Some(true),
            "App::evaluate_depth_rules must wire depth evidence into control_image"
        );
    }

    #[test]
    fn evaluate_depth_rules_without_roi_leaves_snapshot_empty() {
        let rules = DepthRules {
            rules: vec![DepthRegionRule {
                name: "bed-approach".into(),
                region: [0, 0, 2, 2],
                metric: DepthMetric::Median,
                op: DepthOp::Lt,
                threshold_m: 2.0,
                min_valid_ratio: 0.0,
                calibration: None,
            }],
        };
        // No crop_rect and no depth_context_roi → early return.
        let mut app = app_for_depth_rules(rules, None);
        let now = Instant::now();
        app.control_image.reset_depth(now);
        let output = InferenceResult {
            detections: Vec::new(),
            depth: Some(depth_map_from_rows(&[&[1.0, 1.0], &[1.0, 1.0]])),
            postprocess_rejected: 0,
            post_nms_suppressed: 0,
            infer_ms: 0,
            pipeline_us: 0,
            crop_frame: None,
        };
        app.evaluate_depth_rules(&output, None, now);
        assert_eq!(
            app.control_image
                .depth_snapshot()
                .is_triggered("bed-approach"),
            None,
            "without ROI the adapter must not invent depth evidence"
        );
    }
}
