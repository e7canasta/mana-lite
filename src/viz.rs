use std::time::{Duration, Instant};

use mana_types::RawFrameV1;
use mana_viz::logging;

use crate::infer::Detection;
use crate::metrics::{MetricsReport, PerClassFrameStats};
use crate::config::{VizSendToggles, RerunRoot};

enum Inner {
    Connected {
        rec: rerun::RecordingStream,
        last_flush_warn: Instant,
    },
    Disconnected {
        next_retry: Instant,
        backoff_ms: u64,
        last_warn: Instant,
    },
    Disabled,
}

pub struct VizBridge {
    inner: Inner,
    addr: String,
    frame_w: u32,
    frame_h: u32,
    toggles: VizSendToggles,
    #[allow(dead_code)]
    blueprint: Option<RerunRoot>,
}

const INITIAL_BACKOFF_MS: u64 = 1_000;
const MAX_BACKOFF_MS: u64 = 30_000;

impl VizBridge {
    pub fn new(rerun_addr: &str, toggles: &VizSendToggles, blueprint: &RerunRoot) -> Self {
        Self {
            inner: Inner::Disconnected {
                next_retry: Instant::now(),
                backoff_ms: INITIAL_BACKOFF_MS,
                last_warn: Instant::now(),
            },
            addr: rerun_addr.to_string(),
            frame_w: 0,
            frame_h: 0,
            toggles: toggles.clone(),
            blueprint: Some(blueprint.clone()),
        }
    }

    pub fn disabled() -> Self {
        Self {
            inner: Inner::Disabled,
            addr: String::new(),
            frame_w: 0,
            frame_h: 0,
            toggles: VizSendToggles::default(),
            blueprint: None,
        }
    }

    fn try_connect(&mut self) {
        let url = format!("rerun+http://{}/proxy", self.addr);
        let rec = rerun::RecordingStreamBuilder::new("mana-lite")
            .batcher_config(rerun::log::ChunkBatcherConfig {
                max_bytes_in_flight: 32 * 1024 * 1024,
                ..Default::default()
            })
            .connect_grpc_opts(url);
        match rec {
            Ok(rec) => {
                Self::send_default_blueprint(&rec);
                log::info!("viz: connected to {}", self.addr);
                self.inner = Inner::Connected { rec, last_flush_warn: Instant::now() };
            }
            Err(e) => {
                if let Inner::Disconnected { ref mut backoff_ms, ref mut last_warn, .. } = self.inner {
                    if last_warn.elapsed().as_secs() >= 30 {
                        log::warn!("viz: connect failed (retry in {}s): {e}", *backoff_ms / 1000);
                        *last_warn = Instant::now();
                    }
                    *backoff_ms = (*backoff_ms * 2).min(MAX_BACKOFF_MS);
                }
            }
        }
    }

    fn send_default_blueprint(rec: &rerun::RecordingStream) {
        use rerun::blueprint::components::PanelState;

        let camera_view = rerun::blueprint::Spatial2DView::new("Camera")
            .with_origin("/world/camera")
            .with_contents(["+ $origin/**"]);

        let class_counts_view = rerun::blueprint::TimeSeriesView::new("Counts")
            .with_origin("/infer")
            .with_contents(["+ /infer/**/per_frame/counts/**"]);

        let class_conf_view = rerun::blueprint::TimeSeriesView::new("Confidence")
            .with_origin("/infer")
            .with_contents(["+ /infer/**/per_frame/conf/**"]);

        let class_area_view = rerun::blueprint::TimeSeriesView::new("Area")
            .with_origin("/infer")
            .with_contents(["+ /infer/**/per_frame/area/**"]);

        let latency_view = rerun::blueprint::TimeSeriesView::new("Latency")
            .with_origin("/pipeline")
            .with_contents(["+ $origin/infer/**/latency_us", "+ $origin/decode/latency_us"]);

        let signals_view = rerun::blueprint::TimeSeriesView::new("Signals")
            .with_origin("/ingest/normal")
            .with_contents(["+ /ingest/normal/gap_ms", "+ /world/signals/frame_id"]);

        let blueprint = rerun::blueprint::Blueprint::new(
            rerun::blueprint::Vertical::new([
                camera_view.into(),
                rerun::blueprint::Horizontal::new([
                    class_counts_view.into(),
                    class_conf_view.into(),
                    class_area_view.into(),
                ]).into(),
                rerun::blueprint::Horizontal::new([
                    latency_view.into(),
                    signals_view.into(),
                ]).into(),
            ])
            .with_row_shares(vec![5.0, 1.0, 1.0]),
        )
        .with_blueprint_panel(rerun::blueprint::BlueprintPanel::new().with_state(PanelState::Expanded))
        .with_selection_panel(rerun::blueprint::SelectionPanel::new().with_state(PanelState::Expanded))
        .with_time_panel(rerun::blueprint::TimePanel::new().with_state(PanelState::Expanded))
        .with_auto_views(false);

        if let Err(e) = blueprint.send(rec, Default::default()) {
            log::warn!("viz blueprint send failed: {e}");
        }
    }

    pub fn tick(&mut self) {
        match &mut self.inner {
            Inner::Connected { rec, last_flush_warn } => {
                if let Err(e) = rec.flush_with_timeout(Duration::from_millis(100)) {
                    if last_flush_warn.elapsed().as_secs() >= 30 {
                        log::warn!("viz disconnected — viewer may be offline: {e}");
                        *last_flush_warn = Instant::now();
                    }
                    self.inner = Inner::Disconnected {
                        next_retry: Instant::now() + Duration::from_millis(INITIAL_BACKOFF_MS),
                        backoff_ms: INITIAL_BACKOFF_MS * 2,
                        last_warn: Instant::now(),
                    };
                }
            }
            Inner::Disconnected { next_retry, .. } => {
                if Instant::now() >= *next_retry {
                    self.try_connect();
                }
            }
            Inner::Disabled => {}
        }
    }

    pub fn log_frame(&mut self, header: &RawFrameV1, rgb: &[u8], loop_latency_us: u64) {
        self.frame_w = header.width;
        self.frame_h = header.height;
        if let Inner::Connected { ref rec, .. } = self.inner {
            if self.toggles.frames {
                if let Err(e) = logging::frame::log_frame_rgb24(rec, "/world/camera/bgr", header, rgb) {
                    log::warn!("viz frame log failed: {e}");
                }
            }
            if self.toggles.frame_id {
                self.log_scalar_inner(rec, "/world/signals/frame_id", header.frame_id as f64);
            }
            if self.toggles.loop_latency {
                self.log_scalar_inner(rec, "/world/signals/latency/viewer_loop_s", loop_latency_us as f64 / 1_000_000.0);
                self.log_scalar_inner(rec, "/pipeline/loop_latency_us", loop_latency_us as f64);
            }
        }
    }

    pub fn log_detection_boxes(&self, model: &str, detections: &[Detection]) {
        if !self.toggles.boxes { return; }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = format!("/world/camera/detections/{model}");
        rec.log(path.as_str(), &rerun::Clear::recursive()).ok();

        for (i, det) in detections.iter().enumerate() {
            let class = sanitize_entity_name(&det.class);
            let entity = format!("{path}/{class}/{i}");
            let x1 = det.bbox[0]; let y1 = det.bbox[1];
            let x2 = det.bbox[2]; let y2 = det.bbox[3];
            let cx = (x1 + x2) / 2.0;
            let cy = (y1 + y2) / 2.0;
            let hw = (x2 - x1).abs() / 2.0;
            let hh = (y2 - y1).abs() / 2.0;
            let label = format!("{} {:.2}", det.class, det.confidence);
            let [r, g, b] = class_color(&det.class);

            let bbox = rerun::Boxes2D::from_centers_and_half_sizes(
                [rerun::datatypes::Vec2D([cx, cy])],
                [rerun::datatypes::Vec2D([hw, hh])],
            )
            .with_labels([label.as_str()])
            .with_colors([rerun::Color::from_unmultiplied_rgba(r, g, b, 220)])
            .with_radii([2.0]);

            if let Err(e) = rec.log(entity.as_str(), &bbox) {
                log::warn!("viz bbox {entity} failed: {e}");
            }
        }
    }

    pub fn log_decode_latency(&self, us: u64) {
        if !self.toggles.decode_latency { return; }
        if let Inner::Connected { ref rec, .. } = self.inner {
            self.log_scalar_inner(rec, "/pipeline/decode/latency_us", us as f64);
        }
    }

    pub fn log_infer_latency(&self, model: &str, us: u64) {
        if !self.toggles.infer_latency { return; }
        if let Inner::Connected { ref rec, .. } = self.inner {
            let path = format!("/pipeline/infer/{model}/latency_us");
            self.log_scalar_inner(rec, &path, us as f64);
        }
    }

    pub fn log_track_counts(&self, total: usize, active: usize) {
        if let Inner::Connected { ref rec, .. } = self.inner {
            self.log_scalar_inner(rec, "/pipeline/track/total", total as f64);
            self.log_scalar_inner(rec, "/pipeline/track/active", active as f64);
        }
    }

    pub fn log_health_ms_since_frame(&self, ms: u64) {
        if let Inner::Connected { ref rec, .. } = self.inner {
            self.log_scalar_inner(rec, "/pipeline/health/ms_since_frame", ms as f64);
        }
    }

    pub fn log_keyframe_gap(&self, dt_ms: u64) {
        if !self.toggles.keyframe_gap { return; }
        if let Inner::Connected { ref rec, .. } = self.inner {
            self.log_scalar_inner(rec, "/ingest/normal/gap_ms", dt_ms as f64);
        }
    }

    pub fn log_per_frame_class_stats(&self, model: &str, per_class: &PerClassFrameStats) {
        if !self.toggles.class_counts_per_frame && !self.toggles.class_confidence_per_frame && !self.toggles.class_area_per_frame {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let model_safe = model.replace('-', "_").replace('.', "_");
        for (class, stat) in &per_class.stats {
            let cls_safe = sanitize_entity_name(class);
            if self.toggles.class_counts_per_frame {
                let path = format!("/infer/{model_safe}/per_frame/counts/{cls_safe}");
                self.log_scalar_inner(rec, &path, stat.count as f64);
            }
            if self.toggles.class_confidence_per_frame {
                let path_min = format!("/infer/{model_safe}/per_frame/conf/{cls_safe}/min");
                let path_max = format!("/infer/{model_safe}/per_frame/conf/{cls_safe}/max");
                self.log_scalar_inner(rec, &path_min, stat.conf_min as f64);
                self.log_scalar_inner(rec, &path_max, stat.conf_max as f64);
            }
            if self.toggles.class_area_per_frame {
                let path_min = format!("/infer/{model_safe}/per_frame/area/{cls_safe}/min");
                let path_max = format!("/infer/{model_safe}/per_frame/area/{cls_safe}/max");
                self.log_scalar_inner(rec, &path_min, stat.area_min);
                self.log_scalar_inner(rec, &path_max, stat.area_max);
            }
        }
    }

    pub fn log_metrics_report(&self, report: &MetricsReport) {
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
        rec.set_time_sequence("frame_ns", ts);
        self.log_ingest_window(rec, report);
        self.log_infer_global_window(rec, report);
        self.log_infer_model_window(rec, report);
        self.log_scalar_inner(rec, "/pipeline/cycles_window", report.cycles as f64);
        self.log_scalar_inner(rec, "/pipeline/health/blind_cycles", report.blind_cycles as f64);
    }

    fn log_ingest_window(&self, rec: &rerun::RecordingStream, report: &MetricsReport) {
        let hz = if report.window_s > 0 {
            report.keyframes as f64 / report.window_s as f64
        } else { 0.0 };
        let avg_decode_ms = if report.keyframes > 0 {
            report.decode_total_ms / report.keyframes
        } else { 0 };

        self.log_scalar_inner(rec, "/ingest/normal/hz", hz);
        self.log_scalar_inner(rec, "/ingest/normal/keyframes", report.keyframes as f64);
        self.log_scalar_inner(rec, "/ingest/normal/decode_avg_ms", avg_decode_ms as f64);
        self.log_scalar_inner(rec, "/ingest/normal/pframes_dropped", report.ingest_pframes as f64);
        self.log_scalar_inner(rec, "/ingest/errors/timeouts", report.timeouts as f64);
        self.log_scalar_inner(rec, "/ingest/errors/ssrc_changes", report.ssrc_changes as f64);
        self.log_scalar_inner(rec, "/ingest/errors/rtp_errors", report.rtp_errors as f64);
        self.log_scalar_inner(rec, "/ingest/errors/reconnect_attempts", report.reconnect_attempts as f64);
        self.log_scalar_inner(rec, "/ingest/errors/dup_keyframes", report.ingest_dup_keyframes as f64);
    }

    fn log_infer_global_window(&self, rec: &rerun::RecordingStream, report: &MetricsReport) {
        let infer_hz = if report.window_s > 0 {
            report.inferences as f64 / report.window_s as f64
        } else { 0.0 };
        let infer_avg_ms = if report.inferences > 0 {
            report.infer_total_ms / report.inferences
        } else { 0 };
        let yield_avg = if report.inferences > 0 {
            report.infer_total_dets as f64 / report.inferences as f64
        } else { 0.0 };

        self.log_scalar_inner(rec, "/infer/active/hz", infer_hz);
        self.log_scalar_inner(rec, "/infer/active/avg_ms", infer_avg_ms as f64);
        self.log_scalar_inner(rec, "/infer/active/min_ms", report.infer_min_ms as f64);
        self.log_scalar_inner(rec, "/infer/active/max_ms", report.infer_max_ms as f64);
        self.log_scalar_inner(rec, "/infer/active/yield_avg", yield_avg);
        self.log_scalar_inner(rec, "/infer/warnings/skips", report.infer_skips as f64);
        self.log_scalar_inner(rec, "/infer/warnings/empty", report.infer_empty as f64);
    }

    fn log_infer_model_window(&self, rec: &rerun::RecordingStream, report: &MetricsReport) {
        for (model, m) in &report.model_metrics {
            let m_hz = if report.window_s > 0 {
                m.inferences as f64 / report.window_s as f64
            } else { 0.0 };
            let m_avg_ms = if m.inferences > 0 {
                m.infer_total_us / 1000 / m.inferences
            } else { 0 };
            let m_yield = if m.inferences > 0 {
                m.total_dets as f64 / m.inferences as f64
            } else { 0.0 };
            let m_conf_avg = if m.total_dets > 0 {
                m.conf_sum / m.total_dets as f64
            } else { 0.0 };
            let m_area_avg = if m.total_dets > 0 {
                m.bbox_area_sum / m.total_dets as f64
            } else { 0.0 };

            let model_safe = model.replace('-', "_").replace('.', "_");
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/active/hz"), m_hz);
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/active/avg_ms"), m_avg_ms as f64);
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/active/min_ms"), (m.infer_min_us / 1000) as f64);
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/active/max_ms"), (m.infer_max_us / 1000) as f64);
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/active/yield_avg"), m_yield);
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/warnings/skips"), m.skips as f64);
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/warnings/empty"), m.empty as f64);
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/detections/conf_avg"), m_conf_avg);
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/detections/conf_min"), m.conf_min);
            self.log_scalar_inner(rec, &format!("/infer/{model_safe}/detections/area_avg"), m_area_avg);
            for (cls, count) in &m.class_counts {
                let cls_safe = cls.replace(' ', "_");
                self.log_scalar_inner(rec, &format!("/infer/{model_safe}/classes/{cls_safe}"), *count as f64);
            }
        }
    }

    fn log_scalar_inner(&self, rec: &rerun::RecordingStream, path: &str, value: f64) {
        if let Err(e) = rec.log(path, &rerun::Scalars::single(value)) {
            log::warn!("viz scalar {path} failed: {e}");
        }
    }
}

fn sanitize_entity_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

fn class_color(class: &str) -> [u8; 3] {
    let h = class.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    let r = ((h.wrapping_mul(17) >> 8) & 0xFF) as u8;
    let g = ((h.wrapping_mul(13) >> 8) & 0xFF) as u8;
    let b = ((h.wrapping_mul(11) >> 8) & 0xFF) as u8;
    let max = r.max(g).max(b) as f32;
    if max < 120.0 {
        let scale = 120.0 / max.max(1.0);
        [((r as f32 * scale).min(255.0)) as u8, ((g as f32 * scale).min(255.0)) as u8, ((b as f32 * scale).min(255.0)) as u8]
    } else {
        [r, g, b]
    }
}
