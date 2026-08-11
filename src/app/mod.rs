//! Orquestador del pipeline: bootstrap, superloop y etapas por keyframe.

mod bootstrap;
mod cycle;
mod inference;
mod observer;
mod record;

pub use cycle::{CycleContext, FrameSize};
pub use observer::{FanoutObserver, NullObserver, PipelineObserver};

use crate::cascade::CascadeScheduler;
use crate::config::AppConfig;
use crate::detection::CropRect;
use crate::detection::DetectionConsolidator;
use crate::domain::ModelRegistry;
use crate::error::Result;
use crate::face_dwell::FaceDwellLogStrategy;
use crate::infer::InferEngine;
use crate::ingest::{FrameReader, IngestEngine, RawKeyframe, RetinaReader};
use crate::logger::scene_events_to_log;
use crate::metrics::MetricsEngine;
use crate::pipeline::PipelineState;
use crate::scan::{ControlStamp, ControlState, ScanTimeline, SceneEvent};
use crate::snapshot::{FrameBuffer, FrameDecoder, SnapshotSaver};
#[cfg(feature = "rerun")]
use mana_media::RawFrameV1;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;
use tokio::signal::unix::{SignalKind, signal};

pub(crate) static VERSION: &str = env!("CARGO_PKG_VERSION");

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
        #[cfg(feature = "rerun")]
        {
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
        }
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
        // The first tick runs at the timeline origin; every later tick advances
        // one period first, so `timeline.now()` is the instant for this scan.
        if self.control.scan_seq != 0 {
            self.scan_timeline.advance();
        }
        let scene_events =
            crate::scan::scan(&mut self.control, &self.control_image, &self.scan_timeline);
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
                    #[cfg(feature = "rerun")]
                    {
                        let track_refs: Vec<_> = tracks.iter().collect();
                        self.observer.viz.log_entity_boxes(&track_refs);
                    }
                    #[cfg(not(feature = "rerun"))]
                    let _ = tracks;
                }
                SceneEvent::FsmState(state) => {
                    #[cfg(feature = "rerun")]
                    self.observer.viz.log_face_state(state);
                    #[cfg(not(feature = "rerun"))]
                    let _ = state;
                }
                SceneEvent::Presence { stamp, .. }
                | SceneEvent::Track { stamp, .. }
                | SceneEvent::Zone { stamp, .. }
                | SceneEvent::SceneSignals { stamp, .. } => {
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
                &self.control.signal_snapshot,
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

    fn flush_viz_metrics(&mut self, frame_buf: &Option<FrameBuffer>, timestamp_ns: i64) {
        #[cfg(feature = "rerun")]
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
        #[cfg(not(feature = "rerun"))]
        {
            let _ = (frame_buf, timestamp_ns);
            self.crop_frames_pending.clear();
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

#[cfg(feature = "rerun")]
fn raw_frame_header(fb: &FrameBuffer, frame_id: u64, timestamp_ns: i64) -> RawFrameV1 {
    RawFrameV1 {
        width: fb.w,
        height: fb.h,
        frame_id,
        timestamp_ns,
        ..Default::default()
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
mod tests;
