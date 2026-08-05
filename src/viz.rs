use std::time::{Duration, Instant};

use mana_types::RawFrameV1;
use mana_viz::logging;

use crate::infer::{CropRect, Detection, CropFrameInfo};
use crate::metrics::PerClassFrameStats;
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
    toggles: VizSendToggles,
}

const INITIAL_BACKOFF_MS: u64 = 1_000;
const MAX_BACKOFF_MS: u64 = 30_000;

impl VizBridge {
    pub fn new(rerun_addr: &str, toggles: &VizSendToggles, _blueprint: &RerunRoot) -> Self {
        Self {
            inner: Inner::Disconnected {
                next_retry: Instant::now(),
                backoff_ms: INITIAL_BACKOFF_MS,
                last_warn: Instant::now(),
            },
            addr: rerun_addr.to_string(),
            toggles: toggles.clone(),
        }
    }

    pub fn disabled() -> Self {
        Self {
            inner: Inner::Disabled,
            addr: String::new(),
            toggles: VizSendToggles::default(),
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

        let blueprint = rerun::blueprint::Blueprint::new(camera_view)
            .with_blueprint_panel(rerun::blueprint::BlueprintPanel::new().with_state(PanelState::Expanded))
            .with_selection_panel(rerun::blueprint::SelectionPanel::new().with_state(PanelState::Expanded))
            .with_time_panel(rerun::blueprint::TimePanel::new().with_state(PanelState::Expanded))
            .with_auto_views(true);

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

    pub fn set_frame_time(&self) {
        if let Inner::Connected { ref rec, .. } = self.inner {
            let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
            rec.set_time_sequence("frame_ns", ts);
        }
    }

    pub fn log_frame(&self, header: &RawFrameV1, rgb: &[u8]) {
        if !self.toggles.frames { return; }
        if let Inner::Connected { ref rec, .. } = self.inner {
            if let Err(e) = logging::frame::log_frame_rgb24(rec, "/world/camera/bgr", header, rgb) {
                log::warn!("viz frame log failed: {e}");
            }
        }
    }

    pub fn log_crop_frame(&self, model: &str, header: &RawFrameV1, crop: CropFrameInfo) {
        if !self.toggles.crop_frames { return; }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = format!("/world/camera/crops/{model}/bgr");
        if let Err(e) = logging::frame::log_frame_rgb24_owned(
            rec,
            &path,
            &RawFrameV1 {
                width: crop.w,
                height: crop.h,
                ..header.clone()
            },
            crop.rgb,
        ) {
            log::warn!("viz crop frame {model} failed: {e}");
        }
    }

    pub fn log_roi_boxes(&self, model: &str, rect: CropRect) {
        if !self.toggles.roi_rects { return; }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = format!("/world/camera/rois/{model}");
        rec.log(path.as_str(), &rerun::Clear::recursive()).ok();

        let x1 = rect.x1 as f32; let y1 = rect.y1 as f32;
        let x2 = rect.x2 as f32; let y2 = rect.y2 as f32;
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2) / 2.0;
        let hw = ((x2 - x1).abs()) / 2.0;
        let hh = ((y2 - y1).abs()) / 2.0;
        let label = format!("ROI {:.0}x{:.0}", (x2 - x1).abs(), (y2 - y1).abs());

        let bbox = rerun::Boxes2D::from_centers_and_half_sizes(
            [rerun::datatypes::Vec2D([cx, cy])],
            [rerun::datatypes::Vec2D([hw, hh])],
        )
        .with_labels([label.as_str()])
        .with_colors([rerun::Color::from_unmultiplied_rgba(0, 255, 0, 255)])
        .with_radii([2.0]);

        let entity = format!("{path}/roi/0");
        if let Err(e) = rec.log(entity.as_str(), &bbox) {
            log::warn!("viz roi boxes {model} failed: {e}");
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
