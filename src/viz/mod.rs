use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use mana_types::RawFrameV1;
use mana_viz::boxes::boxes2d_from_xyxy;
use mana_viz::logging;
use mana_viz::util::FrameSize;

use crate::config::{RerunRoot, VizSendToggles};
use crate::depth_map::DepthFrame;
use crate::detection::ConsolidatedObservation;
use crate::detection::{CropRect, Detection};
use crate::domain::{ModelRegistry, ModelRole};
use crate::infer::CropFrameInfo;
use crate::metrics::PerClassFrameStats;
use crate::occupancy::{RoomCardinality, SecondPersonState, SignalValidity};
use crate::track::Track;
use image::{Rgb, RgbImage};
use imageproc::drawing::draw_line_segment_mut;
use ultralytics_inference::visualizer::color::{Colormap, DepthViz};

mod masks;
use masks::{build_mask_overlay, frame_strip, polygon_in_roi, render_mask_debug_images};

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
    fixed_rois: Vec<FixedRoi>,
    roles: HashMap<String, ModelRole>,
    face_models: HashSet<String>,
    last_infer_at: HashMap<String, Instant>,
    last_occupancy_state: Option<RoomCardinality>,
    last_second_person_state: Option<SecondPersonState>,
    last_signal_state: Option<SignalValidity>,
    last_face_state: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FixedRoi {
    pub model: String,
    pub rect: CropRect,
}

const DEPTH_OVERLAY_ALPHA: u8 = 150;
const POSE_KEYPOINT_CONFIDENCE: f32 = 0.25;
const FRAME_NUMBER_TIMELINE: &str = "frame_nr";
const FRAME_TIME_TIMELINE: &str = "frame_time";

fn sanitize_entity_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn bbox_area_px(bbox: [f32; 4]) -> f32 {
    ((bbox[2] - bbox[0]) * (bbox[3] - bbox[1])).max(0.0)
}

fn bbox_area_ratio(bbox: [f32; 4], frame_w: u32, frame_h: u32) -> f32 {
    let frame_area = (frame_w as f32) * (frame_h as f32);
    if frame_area > 0.0 {
        bbox_area_px(bbox) / frame_area
    } else {
        0.0
    }
}

fn detection_label(
    class: &str,
    confidence: f32,
    bbox: [f32; 4],
    frame_w: u32,
    frame_h: u32,
) -> String {
    format!(
        "{class} conf={confidence:.2} area={:.0}px ratio={:.4}",
        bbox_area_px(bbox),
        bbox_area_ratio(bbox, frame_w, frame_h),
    )
}

fn bbox_in_crop(bbox: [f32; 4], rect: CropRect) -> [f32; 4] {
    [
        bbox[0] - rect.x1 as f32,
        bbox[1] - rect.y1 as f32,
        bbox[2] - rect.x1 as f32,
        bbox[3] - rect.y1 as f32,
    ]
}

fn pose_keypoint_visible(x: f32, y: f32, confidence: f32) -> bool {
    confidence >= POSE_KEYPOINT_CONFIDENCE && x.is_finite() && y.is_finite()
}

const INITIAL_BACKOFF_MS: u64 = 1_000;
const MAX_BACKOFF_MS: u64 = 30_000;

/// Instance-mask palette, keyed by (class-id − 1) mod len (ADR-022).
pub(super) const PALETTE: [[u8; 3]; 8] = [
    [230, 25, 75],
    [60, 180, 75],
    [255, 225, 25],
    [0, 130, 200],
    [245, 130, 48],
    [145, 30, 180],
    [70, 240, 240],
    [240, 50, 230],
];

impl VizBridge {
    pub fn new(
        rerun_addr: &str,
        toggles: &VizSendToggles,
        _blueprint: &RerunRoot,
        fixed_rois: Vec<FixedRoi>,
        models: &ModelRegistry,
    ) -> Self {
        Self {
            inner: Inner::Disconnected {
                next_retry: Instant::now(),
                backoff_ms: INITIAL_BACKOFF_MS,
                last_warn: Instant::now(),
            },
            addr: rerun_addr.to_string(),
            toggles: toggles.clone(),
            fixed_rois,
            roles: models
                .iter()
                .map(|(id, entry)| (id.as_str().to_owned(), entry.semantics.role))
                .collect(),
            face_models: models
                .iter()
                .filter(|(id, _)| models.is_face_model(id.as_str()))
                .map(|(id, _)| id.as_str().to_owned())
                .collect(),
            last_infer_at: HashMap::new(),
            last_occupancy_state: None,
            last_second_person_state: None,
            last_signal_state: None,
            last_face_state: None,
        }
    }

    pub fn disabled() -> Self {
        Self {
            inner: Inner::Disabled,
            addr: String::new(),
            toggles: VizSendToggles::default(),
            fixed_rois: Vec::new(),
            roles: HashMap::new(),
            face_models: HashSet::new(),
            last_infer_at: HashMap::new(),
            last_occupancy_state: None,
            last_second_person_state: None,
            last_signal_state: None,
            last_face_state: None,
        }
    }

    fn role_of(&self, model: &str) -> ModelRole {
        self.roles.get(model).copied().unwrap_or(ModelRole::Boxes)
    }

    fn is_face_model(&self, model: &str) -> bool {
        self.face_models.contains(model)
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
                // Keep the SDK wall-clock timeline available alongside the frame timelines.
                rec.set_log_time_enabled(true);
                Self::send_default_blueprint(&rec);
                Self::send_presence_state_configuration(&rec);
                Self::send_fixed_rois(&rec, &self.fixed_rois);
                self.last_occupancy_state = None;
                self.last_second_person_state = None;
                self.last_signal_state = None;
                self.last_face_state = None;
                log::info!("viz: connected to {}", self.addr);
                self.inner = Inner::Connected {
                    rec,
                    last_flush_warn: Instant::now(),
                };
            }
            Err(e) => {
                if let Inner::Disconnected {
                    ref mut backoff_ms,
                    ref mut last_warn,
                    ..
                } = self.inner
                {
                    if last_warn.elapsed().as_secs() >= 30 {
                        log::warn!(
                            "viz: connect failed (retry in {}s): {e}",
                            *backoff_ms / 1000
                        );
                        *last_warn = Instant::now();
                    }
                    *backoff_ms = (*backoff_ms * 2).min(MAX_BACKOFF_MS);
                }
            }
        }
    }

    fn send_default_blueprint(rec: &rerun::RecordingStream) {
        use rerun::blueprint::components::PanelState;
        use rerun::blueprint::{
            ContainerLike, Horizontal, Spatial2DView, StateTimelineView, Tabs, TimeSeriesView,
            Vertical,
        };

        let main_camera = Spatial2DView::new("Main frame")
            .with_origin("/world/camera")
            .with_contents([
                "+ /world/camera/bgr",
                "+ /world/camera/entities/**",
                "+ /world/camera/observations/**",
                "+ /world/camera/detections/**",
                "+ /world/camera/rois/**",
            ]);
        let face_crop = Spatial2DView::new("Face crop")
            .with_origin("/world/camera/crops/face-yolo")
            .with_contents(["+ $origin/**"]);
        let segmentation_crop = Spatial2DView::new("Mask + border")
            .with_origin("/world/camera/crops/seg-standard")
            .with_contents(["+ $origin/**"]);
        let depth_crop = Spatial2DView::new("Depth disparity")
            .with_origin("/world/camera/crops/depth-standard/depth")
            .with_contents([
                "+ $origin/disparity",
                "+ $origin/context/seg-standard/polygon/**",
                "+ $origin/context/detect-fast/**",
                "+ $origin/context/face-yolo/**",
            ]);
        let camera_details = Vertical::new(vec![
            face_crop.into(),
            segmentation_crop.into(),
            depth_crop.into(),
        ])
        .with_row_shares([1.0, 1.0, 1.0]);
        let camera_tab = Horizontal::new(vec![
            ContainerLike::from(main_camera),
            ContainerLike::from(camera_details),
        ])
        .with_column_shares([3.0, 2.0]);

        let stream_view = TimeSeriesView::new("Stream")
            .with_origin("/ingest")
            .with_contents(["+ $origin/**"]);
        let inference_view = TimeSeriesView::new("Inference")
            .with_origin("/pipeline")
            .with_contents(["+ $origin/**"]);
        let depth_stats = TimeSeriesView::new("Depth stats")
            .with_origin("/world/camera/depth")
            .with_contents(["+ $origin/**"]);
        let class_stats = TimeSeriesView::new("Class stats")
            .with_origin("/infer")
            .with_contents(["+ $origin/**"]);
        let room_state = StateTimelineView::new("Room state")
            .with_origin("/pipeline/state/room")
            .with_contents(["+ $origin/**"]);
        let face_state = StateTimelineView::new("Face state")
            .with_origin("/pipeline/state/face")
            .with_contents(["+ $origin"]);
        let metrics_tab = Vertical::new(vec![
            stream_view.into(),
            inference_view.into(),
            depth_stats.into(),
            class_stats.into(),
            room_state.into(),
            face_state.into(),
        ])
        .with_row_shares([1.0, 1.0, 1.0, 1.0, 1.5, 1.5]);

        let viewport = Tabs::new(vec![
            ContainerLike::from(camera_tab),
            ContainerLike::from(metrics_tab),
        ]);

        let blueprint = rerun::blueprint::Blueprint::new(viewport)
            .with_blueprint_panel(
                rerun::blueprint::BlueprintPanel::new().with_state(PanelState::Expanded),
            )
            .with_selection_panel(
                rerun::blueprint::SelectionPanel::new().with_state(PanelState::Expanded),
            )
            .with_time_panel(rerun::blueprint::TimePanel::new().with_state(PanelState::Expanded))
            .with_auto_views(true);

        if let Err(e) = blueprint.send(rec, Default::default()) {
            log::warn!("viz blueprint send failed: {e}");
        }
    }

    fn send_presence_state_configuration(rec: &rerun::RecordingStream) {
        let configs = [
            (
                "/pipeline/state/room/cardinality",
                rerun::StateConfiguration::new()
                    .with_values(["empty", "single", "multiple"])
                    .with_labels(["Empty", "Single person", "Multiple people"])
                    .with_colors([0x607D8BFF, 0x4CAF50FF, 0xFF9800FF]),
            ),
            (
                "/pipeline/state/room/second_person",
                rerun::StateConfiguration::new()
                    .with_values(["none", "candidate", "confirmed"])
                    .with_labels(["No second person", "Second candidate", "Second confirmed"])
                    .with_colors([0x607D8BFF, 0xFFEB3BFF, 0xF44336FF]),
            ),
            (
                "/pipeline/state/room/signal",
                rerun::StateConfiguration::new()
                    .with_values(["valid", "invalid"])
                    .with_labels(["Valid inference", "Invalid inference"])
                    .with_colors([0x4CAF50FF, 0xF44336FF]),
            ),
            (
                "/pipeline/state/face",
                rerun::StateConfiguration::new()
                    .with_values([
                        "idle",
                        "searching",
                        "detected",
                        "other",
                        "in_bed",
                        "edge",
                        "exiting",
                    ])
                    .with_labels([
                        "Idle",
                        "Buscando cara",
                        "Cara detectada",
                        "Otra posición",
                        "En cama",
                        "En borde",
                        "Saliendo",
                    ])
                    .with_colors([
                        0x607D8BFF, 0x2196F3FF, 0x4CAF50FF, 0x9E9E9EFF, 0x9C27B0FF, 0xFF9800FF,
                        0xF44336FF,
                    ]),
            ),
        ];
        for (path, config) in configs {
            if let Err(e) = rec.log_static(path, &config) {
                log::warn!("viz presence state configuration {path} failed: {e}");
            }
        }
    }

    fn send_fixed_rois(rec: &rerun::RecordingStream, fixed_rois: &[FixedRoi]) {
        for fixed in fixed_rois {
            let model = sanitize_entity_name(&fixed.model);
            let path = format!("/world/camera/rois/fixed/{model}/roi");
            let [x1, y1, x2, y2] = fixed.rect.to_array();
            let x1 = x1 as f32;
            let y1 = y1 as f32;
            let x2 = x2 as f32;
            let y2 = y2 as f32;
            let label = format!("fixed {model} [{:.0},{:.0} {:.0},{:.0}]", x1, y1, x2, y2);
            let color = rerun::Color::from_unmultiplied_rgba(255, 200, 0, 255);
            let bbox = boxes2d_from_xyxy([x1, y1, x2, y2], color, Some(&label), 2.0);
            if let Err(e) = rec.log_static(path.as_str(), &bbox) {
                log::warn!("viz fixed ROI {model} failed: {e}");
            }
        }
    }

    pub fn tick(&mut self) {
        match &mut self.inner {
            Inner::Connected {
                rec,
                last_flush_warn,
            } => {
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

    pub fn set_frame_time(&self, frame_number: u64, timestamp_ns: i64) {
        if let Inner::Connected { ref rec, .. } = self.inner {
            let frame_number = i64::try_from(frame_number).unwrap_or(i64::MAX);
            rec.set_time_sequence(FRAME_NUMBER_TIMELINE, frame_number);
            rec.set_timestamp_nanos_since_epoch(FRAME_TIME_TIMELINE, timestamp_ns);
        }
    }

    pub fn log_occupancy_state(
        &mut self,
        state: RoomCardinality,
        second_person: SecondPersonState,
        signal: SignalValidity,
    ) {
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };

        if self.last_occupancy_state != Some(state) {
            let path = "/pipeline/state/room/cardinality";
            if let Err(e) = rec.log(path, &rerun::StateChange::single(state.as_str())) {
                log::warn!("viz occupancy state failed: {e}");
            }
            self.last_occupancy_state = Some(state);
        }
        if self.last_second_person_state != Some(second_person) {
            let path = "/pipeline/state/room/second_person";
            if let Err(e) = rec.log(path, &rerun::StateChange::single(second_person.as_str())) {
                log::warn!("viz second person state failed: {e}");
            }
            self.last_second_person_state = Some(second_person);
        }
        if self.last_signal_state != Some(signal) {
            let path = "/pipeline/state/room/signal";
            if let Err(e) = rec.log(path, &rerun::StateChange::single(signal.as_str())) {
                log::warn!("viz presence signal state failed: {e}");
            }
            self.last_signal_state = Some(signal);
        }
    }

    pub fn log_face_state(&mut self, state: &str) {
        if self.last_face_state.as_deref() == Some(state) {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        if let Err(e) = rec.log("/pipeline/state/face", &rerun::StateChange::single(state)) {
            log::warn!("viz face state failed: {e}");
        }
        self.last_face_state = Some(state.to_owned());
    }

    pub fn log_frame(&self, header: &RawFrameV1, rgb: &[u8]) {
        if !self.toggles.frames {
            return;
        }
        if let Inner::Connected { ref rec, .. } = self.inner {
            if let Err(e) = logging::frame::log_frame_rgb24(rec, "/world/camera/bgr", header, rgb) {
                log::warn!("viz frame log failed: {e}");
            }
        }
    }

    pub fn log_crop_frame(&self, model: &str, header: &RawFrameV1, crop: CropFrameInfo) {
        if !self.toggles.crop_frames {
            return;
        }
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

    pub fn log_model_depth(&self, model: &str, depth: Option<&DepthFrame>) {
        if !self.toggles.depth && !self.toggles.depth_stats {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let model = sanitize_entity_name(model);
        let base = format!("/world/camera/depth/{model}");
        let visual_path = format!("/world/camera/crops/{model}/depth");
        if self.toggles.depth {
            rec.log(base.as_str(), &rerun::Clear::recursive()).ok();
            rec.log(visual_path.as_str(), &rerun::Clear::recursive())
                .ok();
        }
        if let Some(depth) = depth {
            let (width, height) = depth.dims();
            if self.toggles.depth && width > 0 && height > 0 {
                let (viz, suffix) = match self.toggles.depth_viz.to_ascii_lowercase().as_str() {
                    "metric" => (DepthViz::Metric, "metric"),
                    "disparity" | "depthanything" => (DepthViz::Disparity, "disparity"),
                    invalid => {
                        log::warn!("viz: invalid depth_viz '{invalid}', using disparity");
                        (DepthViz::Disparity, "disparity")
                    }
                };
                let path = format!("{visual_path}/{suffix}");
                let colors = depth.colorize(Colormap::Inferno, viz);
                let rgba: Vec<u8> = depth
                    .iter_values()
                    .zip(colors.iter())
                    .flat_map(|(value, color)| {
                        let alpha = if value.is_finite() && value > 0.0 {
                            DEPTH_OVERLAY_ALPHA
                        } else {
                            0
                        };
                        [color[0], color[1], color[2], alpha]
                    })
                    .collect();
                let image = rerun::Image::from_rgba32(rgba, [width, height]);
                if let Err(e) = rec.log(path.as_str(), &image) {
                    log::warn!("viz depth {model} failed: {e}");
                }
            }

            if self.toggles.depth_stats {
                let valid_pixels = depth
                    .iter_values()
                    .filter(|&value| value.is_finite() && value > 0.0)
                    .count();
                self.log_scalar_inner(
                    rec,
                    &format!("{base}/stats/valid_pixels"),
                    valid_pixels as f64,
                );
                if let Some(min) = finite_depth_min(depth) {
                    self.log_scalar_inner(rec, &format!("{base}/stats/min_depth_m"), min as f64);
                }
                if let Some(max) = finite_depth_max(depth) {
                    self.log_scalar_inner(rec, &format!("{base}/stats/max_depth_m"), max as f64);
                }
            }
        } else if self.toggles.depth_stats {
            self.log_scalar_inner(rec, &format!("{base}/stats/valid_pixels"), 0.0);
        }
    }

    pub fn log_roi_boxes(&self, model: &str, rect: CropRect) {
        if !self.toggles.roi_rects {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = format!("/world/camera/rois/{model}");
        rec.log(path.as_str(), &rerun::Clear::recursive()).ok();

        let x1 = rect.x1 as f32;
        let y1 = rect.y1 as f32;
        let x2 = rect.x2 as f32;
        let y2 = rect.y2 as f32;
        let label = format!("ROI {:.0}x{:.0}", (x2 - x1).abs(), (y2 - y1).abs());

        let bbox = boxes2d_from_xyxy(
            [x1, y1, x2, y2],
            rerun::Color::from_unmultiplied_rgba(0, 255, 0, 255),
            Some(&label),
            2.0,
        );

        let entity = format!("{path}/roi/0");
        if let Err(e) = rec.log(entity.as_str(), &bbox) {
            log::warn!("viz roi boxes {model} failed: {e}");
        }
    }

    pub fn log_entity_boxes(&self, tracks: &[&Track]) {
        if !self.toggles.boxes {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = "/world/camera/entities";
        rec.log(path, &rerun::Clear::recursive()).ok();

        for track in tracks {
            let [x1, y1, x2, y2] = track.bbox;
            let label = format!("{} #{}", track.class, track.id);
            let color = if track.misses == 0 {
                rerun::Color::from_unmultiplied_rgba(0, 255, 0, 255)
            } else {
                rerun::Color::from_unmultiplied_rgba(255, 180, 0, 220)
            };
            let bbox = boxes2d_from_xyxy([x1, y1, x2, y2], color, Some(&label), 2.0);
            let entity = format!("{path}/{}", track.id);
            if let Err(e) = rec.log(entity.as_str(), &bbox) {
                log::warn!("viz entity bbox {entity} failed: {e}");
            }
        }
    }

    pub fn log_consolidated_observations(
        &self,
        observations: &[ConsolidatedObservation],
        frame: FrameSize,
    ) {
        if !self.toggles.boxes {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = "/world/camera/observations";
        rec.log(path, &rerun::Clear::recursive()).ok();

        for (index, observation) in observations.iter().enumerate() {
            let [x1, y1, x2, y2] = observation.bbox;
            let label = format!(
                "{} model={}",
                detection_label(
                    &observation.class,
                    observation.confidence,
                    observation.bbox,
                    frame.w,
                    frame.h,
                ),
                observation.primary_model,
            );
            let bbox = boxes2d_from_xyxy(
                [x1, y1, x2, y2],
                rerun::Color::from_unmultiplied_rgba(0, 180, 255, 255),
                Some(&label),
                2.0,
            );
            let entity = format!("{path}/{index}");
            if let Err(e) = rec.log(entity.as_str(), &bbox) {
                log::warn!("viz consolidated observation {entity} failed: {e}");
            }
        }
    }

    pub fn log_model_detections(
        &self,
        model: &str,
        detections: &[Detection],
        crop_rect: Option<CropRect>,
        frame: FrameSize,
    ) {
        if !self.toggles.boxes {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let is_face_model = self.is_face_model(model);
        let model = sanitize_entity_name(model);
        let path = format!("/world/camera/detections/{model}");
        let crop_path = format!("/world/camera/crops/{model}/detections");
        if !(is_face_model && detections.is_empty()) {
            rec.log(path.as_str(), &rerun::Clear::recursive()).ok();
            rec.log(crop_path.as_str(), &rerun::Clear::recursive()).ok();
        }

        for (index, detection) in detections.iter().enumerate() {
            let label = detection_label(
                &detection.class,
                detection.confidence,
                detection.bbox,
                frame.w,
                frame.h,
            );
            let log_box = |entity: String, [x1, y1, x2, y2]: [f32; 4]| {
                let bbox = boxes2d_from_xyxy(
                    [x1, y1, x2, y2],
                    rerun::Color::from_unmultiplied_rgba(255, 80, 80, 255),
                    Some(&label),
                    2.0,
                );
                if let Err(e) = rec.log(entity.as_str(), &bbox) {
                    log::warn!("viz raw detection {entity} failed: {e}");
                }
            };
            log_box(format!("{path}/{index}"), detection.bbox);
            if let Some(rect) = crop_rect {
                log_box(
                    format!("{crop_path}/{index}"),
                    bbox_in_crop(detection.bbox, rect),
                );
            }
        }
    }

    pub fn log_model_pose(&self, model: &str, detections: &[Detection]) {
        if !self.toggles.boxes || self.role_of(model) != ModelRole::Skeleton {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let base = format!(
            "/world/camera/detections/{}/pose",
            sanitize_entity_name(model)
        );
        rec.log(base.as_str(), &rerun::Clear::recursive()).ok();

        use ultralytics_inference::visualizer::color::POSE_COLORS;
        use ultralytics_inference::visualizer::skeleton::{
            KPT_COLOR_INDICES, LIMB_COLOR_INDICES, SKELETON,
        };

        for (person_index, detection) in detections.iter().enumerate() {
            let Some(keypoints) = detection.keypoints.as_ref() else {
                continue;
            };
            let mut points = Vec::new();
            let mut point_ids = Vec::new();
            let mut point_colors = Vec::new();
            let mut skeleton = Vec::new();
            let mut skeleton_colors = Vec::new();

            for (keypoint_index, &[x, y, confidence]) in keypoints.iter().enumerate() {
                if !pose_keypoint_visible(x, y, confidence) {
                    continue;
                }
                points.push([x, y]);
                point_ids.push(keypoint_index as u16);
                let color_index = KPT_COLOR_INDICES[keypoint_index % KPT_COLOR_INDICES.len()];
                let [r, g, b] = POSE_COLORS[color_index];
                point_colors.push(rerun::Color::from_rgb(r, g, b));
            }

            for (limb_index, &[a, b]) in SKELETON.iter().enumerate() {
                let (Some(&[x1, y1, c1]), Some(&[x2, y2, c2])) =
                    (keypoints.get(a), keypoints.get(b))
                else {
                    continue;
                };
                if !pose_keypoint_visible(x1, y1, c1) || !pose_keypoint_visible(x2, y2, c2) {
                    continue;
                }
                skeleton.push(vec![[x1, y1], [x2, y2]]);
                let color_index = LIMB_COLOR_INDICES[limb_index % LIMB_COLOR_INDICES.len()];
                let [r, g, b] = POSE_COLORS[color_index];
                skeleton_colors.push(rerun::Color::from_unmultiplied_rgba(r, g, b, 220));
            }

            if !points.is_empty() {
                let path = format!("{base}/{person_index}/keypoints");
                let points = rerun::Points2D::new(points)
                    .with_keypoint_ids(point_ids)
                    .with_colors(point_colors)
                    .with_radii([rerun::Radius::new_ui_points(5.0)]);
                if let Err(e) = rec.log(path.as_str(), &points) {
                    log::warn!("viz pose keypoints {path} failed: {e}");
                }
            }
            if !skeleton.is_empty() {
                let path = format!("{base}/{person_index}/skeleton");
                let strips = rerun::LineStrips2D::new(skeleton)
                    .with_colors(skeleton_colors)
                    .with_radii([rerun::Radius::new_ui_points(3.0)]);
                if let Err(e) = rec.log(path.as_str(), &strips) {
                    log::warn!("viz pose skeleton {path} failed: {e}");
                }
            }
        }
    }

    pub fn log_depth_context_boxes(
        &self,
        model: &str,
        detections: &[Detection],
        depth_context_roi: Option<CropRect>,
    ) {
        if !self.toggles.boxes || self.role_of(model) != ModelRole::Boxes {
            return;
        }
        let Some(depth_context_roi) = depth_context_roi else {
            return;
        };
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let is_face_model = self.is_face_model(model);
        let model = sanitize_entity_name(model);
        let path = format!("/world/camera/crops/depth-standard/depth/context/{model}");
        if !(is_face_model && detections.is_empty()) {
            rec.log(path.as_str(), &rerun::Clear::recursive()).ok();
        }

        let color = if is_face_model {
            rerun::Color::from_unmultiplied_rgba(255, 220, 0, 165)
        } else {
            rerun::Color::from_unmultiplied_rgba(0, 255, 100, 165)
        };
        let label_prefix = if is_face_model { "face" } else { "body" };

        for (index, detection) in detections.iter().enumerate() {
            let Some([x1, y1, x2, y2]) = bbox_in_roi(detection.bbox, depth_context_roi) else {
                continue;
            };
            let label = format!(
                "{label_prefix} {} {:.2}",
                detection.class, detection.confidence
            );
            let bbox = boxes2d_from_xyxy(
                [x1, y1, x2, y2],
                color,
                (!is_face_model).then_some(label.as_str()),
                3.0,
            );
            let entity = format!("{path}/{index}");
            if let Err(e) = rec.log(entity.as_str(), &bbox) {
                log::warn!("viz depth context box {entity} failed: {e}");
            }
        }
    }

    pub fn clear_depth_context_boxes(&self) {
        if !self.toggles.boxes {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        for (model, role) in &self.roles {
            if *role == ModelRole::Boxes {
                let model = sanitize_entity_name(model);
                let path = format!("/world/camera/crops/depth-standard/depth/context/{model}");
                rec.log(path.as_str(), &rerun::Clear::recursive()).ok();
            }
        }
    }

    pub fn log_depth_context_polygons(
        &self,
        model: &str,
        detections: &[Detection],
        depth_context_roi: Option<CropRect>,
        frame: FrameSize,
    ) {
        if !self.toggles.mask_polygons || self.role_of(model) != ModelRole::Mask {
            return;
        }
        let Some(depth_context_roi) = depth_context_roi else {
            return;
        };
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        if detections.is_empty() {
            return;
        }
        let base = "/world/camera/crops/depth-standard/depth/context/seg-standard/polygon";
        rec.log(base, &rerun::Clear::recursive()).ok();
        let fw = frame.w.max(1) as f32;
        let fh = frame.h.max(1) as f32;

        for (index, detection) in detections.iter().enumerate() {
            let Some(mask) = &detection.mask else {
                continue;
            };
            for (polygon_index, polygon) in mask.polygons.as_ref().iter().enumerate() {
                let points = polygon_in_roi(polygon, fw, fh, depth_context_roi);
                if points.len() < 2 {
                    continue;
                }
                let path = format!("{base}/{index}/{polygon_index}");
                let strip = rerun::LineStrips2D::new([points])
                    .with_colors([rerun::Color::from_unmultiplied_rgba(255, 255, 255, 165)])
                    .with_radii([rerun::Radius::new_ui_points(3.0)]);
                if let Err(e) = rec.log(path.as_str(), &strip) {
                    log::warn!("viz depth context polygon {path} failed: {e}");
                }
            }
        }
    }

    /// Log instance masks as a class-id overlay (RGBA) at mask resolution,
    /// plus — when `mask_debug` is enabled — standalone mask and contour
    /// images under `/world/camera/debug/{model}/...` for visual inspection.
    /// Overlay pattern imported from mana-os
    /// `mana-rerun-common::logging::segmentation::log_segmentation_overlay`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn log_model_masks(&self, model: &str, detections: &[Detection], frame: FrameSize) {
        if !self.toggles.masks {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let model = sanitize_entity_name(model);
        let mask_path = format!("/world/camera/masks/{model}");
        let crop_mask_path = format!("/world/camera/crops/{model}/mask");
        rec.log(mask_path.as_str(), &rerun::Clear::recursive()).ok();
        rec.log(crop_mask_path.as_str(), &rerun::Clear::recursive())
            .ok();

        let masked: Vec<&Detection> = detections.iter().filter(|d| d.mask.is_some()).collect();
        if masked.is_empty() {
            return;
        }
        let [mask_w, mask_h] = masked[0].mask.as_ref().unwrap().mask_dims;
        if mask_w == 0 || mask_h == 0 {
            return;
        }

        let overlay = build_mask_overlay(&masked, mask_w, mask_h, frame.w, frame.h);

        let mut rgba = Vec::with_capacity(overlay.len() * 4);
        for pixel in &overlay {
            match *pixel {
                0 => rgba.extend_from_slice(&[0, 0, 0, 0]),
                0xFF => rgba.extend_from_slice(&[255, 255, 255, 255]),
                id => {
                    let [r, g, b] = PALETTE[(id - 1) as usize];
                    rgba.extend_from_slice(&[r, g, b, 120]);
                }
            }
        }
        let image = rerun::Image::from_rgba32(rgba, [mask_w, mask_h]);
        if let Err(e) = rec.log(mask_path.as_str(), &image) {
            log::warn!("viz mask overlay {model} failed: {e}");
        }
        if let Err(e) = rec.log(crop_mask_path.as_str(), &image) {
            log::warn!("viz crop mask overlay {model} failed: {e}");
        }

        self.log_mask_debug(rec, &model, &masked, frame.w, frame.h);
        self.log_mask_polygons(rec, &model, &masked, frame.w, frame.h);
    }

    /// Log the simplified contour polygons as real 2D primitives on the
    /// camera plane (frame pixel coordinates, closed loops), so they can be
    /// inspected as a polygon in the viewer — vertex count follows the
    /// per-model `polygon_simplify` epsilon.
    fn log_mask_polygons(
        &self,
        rec: &rerun::RecordingStream,
        model: &str,
        masked: &[&Detection],
        frame_w: u32,
        frame_h: u32,
    ) {
        if !self.toggles.mask_polygons {
            return;
        }
        let base = format!("/world/camera/mask_polygons/{model}");
        rec.log(base.as_str(), &rerun::Clear::recursive()).ok();
        let fw = frame_w.max(1) as f32;
        let fh = frame_h.max(1) as f32;

        for (index, detection) in masked.iter().enumerate() {
            let Some(mask) = &detection.mask else {
                continue;
            };
            for (p_index, poly) in mask.polygons.as_ref().iter().enumerate() {
                let strip = frame_strip(poly, fw, fh);
                if strip.len() < 2 {
                    continue;
                }
                let color = PALETTE[index % PALETTE.len()];
                let path = format!("{base}/{p_index}");
                if let Err(e) = rec.log(
                    path.as_str(),
                    &rerun::LineStrips2D::new([strip])
                        .with_colors([rerun::Color::from_rgb(color[0], color[1], color[2])])
                        .with_radii([rerun::Radius::new_ui_points(2.0)]),
                ) {
                    log::warn!("viz mask polygon {model} failed: {e}");
                }
            }
        }
    }

    /// Render the mask raster and the derived contour polygons as standalone
    /// images in mask space (same resolution, same origin), so the contours
    /// can be visually checked against the CompactMask they come from.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn log_mask_debug(
        &self,
        rec: &rerun::RecordingStream,
        model: &str,
        masked: &[&Detection],
        frame_w: u32,
        frame_h: u32,
    ) {
        if !self.toggles.mask_debug {
            return;
        }
        let Some((mask_img, poly_img, mask_w, mask_h)) =
            render_mask_debug_images(masked, frame_w, frame_h)
        else {
            return;
        };

        let mask_path = format!("/world/camera/debug/{model}/mask");
        let poly_path = format!("/world/camera/debug/{model}/polygon");
        rec.log(mask_path.as_str(), &rerun::Clear::recursive()).ok();
        rec.log(poly_path.as_str(), &rerun::Clear::recursive()).ok();

        let mask_img = rerun::Image::from_rgb24(mask_img.as_raw().clone(), [mask_w, mask_h]);
        if let Err(e) = rec.log(mask_path.as_str(), &mask_img) {
            log::warn!("viz mask debug {model} failed: {e}");
        }
        let poly_img = rerun::Image::from_rgb24(poly_img.as_raw().clone(), [mask_w, mask_h]);
        if let Err(e) = rec.log(poly_path.as_str(), &poly_img) {
            log::warn!("viz polygon debug {model} failed: {e}");
        }
    }

    /// Draw the mask raster and its contour polygons onto standalone images in
    /// mask space (same resolution, same origin), so contours can be visually
    /// checked against the CompactMask they derive from. Polygons arrive
    /// frame-normalized after `run()` and are mapped back via
    /// `p_mask = (p_frame * frame - origin) / mask_dims`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]

    pub fn log_decode_latency(&self, us: u64) {
        if !self.toggles.decode_latency {
            return;
        }
        if let Inner::Connected { ref rec, .. } = self.inner {
            self.log_scalar_inner(rec, "/pipeline/decode/latency_us", us as f64);
        }
    }

    pub fn log_infer_latency(&mut self, model: &str, backend_us: u64, pipeline_us: u64) {
        let now = Instant::now();
        let infer_hz = self
            .last_infer_at
            .insert(model.to_owned(), now)
            .map(|previous| {
                let seconds = now.duration_since(previous).as_secs_f64();
                if seconds > 0.0 {
                    Some(1.0 / seconds)
                } else {
                    None
                }
            })
            .flatten();
        if let Inner::Connected { ref rec, .. } = self.inner {
            if self.toggles.infer_latency {
                let path = format!("/pipeline/infer/{model}/latency_us");
                self.log_scalar_inner(rec, &path, backend_us as f64);
                let path = format!("/pipeline/infer/{model}/pipeline_us");
                self.log_scalar_inner(rec, &path, pipeline_us as f64);
            }
            if self.toggles.infer_rate {
                if let Some(hz) = infer_hz {
                    let path = format!("/pipeline/infer/{model}/hz");
                    self.log_scalar_inner(rec, &path, hz);
                }
            }
        }
    }

    pub fn log_keyframe_gap(&self, dt_ms: u64) {
        if let Inner::Connected { ref rec, .. } = self.inner {
            if self.toggles.keyframe_gap {
                self.log_scalar_inner(rec, "/ingest/normal/gap_ms", dt_ms as f64);
            }
            if self.toggles.keyframe_rate && dt_ms > 0 {
                self.log_scalar_inner(rec, "/ingest/keyframes/processed_hz", 1000.0 / dt_ms as f64);
            }
        }
    }

    pub fn log_keyframe_selection(&self, seen: u64, dropped: u64, source_window_ms: u64) {
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        if self.toggles.keyframe_rate && source_window_ms > 0 {
            let source_hz = seen as f64 * 1000.0 / source_window_ms as f64;
            self.log_scalar_inner(rec, "/ingest/keyframes/source_hz", source_hz);
        }
        if self.toggles.keyframe_drops {
            self.log_scalar_inner(rec, "/ingest/keyframes/dropped", dropped as f64);
        }
    }

    pub fn log_per_frame_class_stats(&self, model: &str, per_class: &PerClassFrameStats) {
        if !self.toggles.class_counts_per_frame
            && !self.toggles.class_confidence_per_frame
            && !self.toggles.class_area_per_frame
        {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::occupancy::{RoomCardinality, SecondPersonState};
    use std::time::Instant;

    #[test]
    fn detection_labels_include_area_and_frame_ratio() {
        let label = detection_label("person", 0.87, [100.0, 200.0, 300.0, 500.0], 1_000, 1_000);
        assert!(label.contains("conf=0.87"));
        assert!(label.contains("area=60000px"));
        assert!(label.contains("ratio=0.0600"));
    }

    #[test]
    fn crop_bbox_uses_local_coordinates() {
        let rect = CropRect {
            x1: 312,
            y1: 40,
            x2: 1608,
            y2: 540,
        };
        assert_eq!(
            bbox_in_crop([500.0, 100.0, 620.0, 220.0], rect),
            [188.0, 60.0, 308.0, 180.0]
        );
    }

    #[test]
    fn fixed_rois_use_stable_static_entities() {
        let (rec, storage) = rerun::RecordingStreamBuilder::new("mana-viz-fixed-roi-test")
            .batcher_config(rerun::log::ChunkBatcherConfig::NEVER)
            .memory()
            .expect("memory recording");
        VizBridge::send_fixed_rois(
            &rec,
            &[
                FixedRoi {
                    model: "detect-fast".into(),
                    rect: CropRect {
                        x1: 528,
                        y1: 0,
                        x2: 1392,
                        y2: 540,
                    },
                },
                FixedRoi {
                    model: "face-dwell".into(),
                    rect: CropRect {
                        x1: 760,
                        y1: 0,
                        x2: 1160,
                        y2: 300,
                    },
                },
            ],
        );

        let paths = storage
            .take()
            .into_iter()
            .filter_map(|msg| match msg {
                rerun::log::LogMsg::ArrowMsg(_, msg) => {
                    Some(rerun::log::Chunk::from_arrow_msg(&msg).expect("valid chunk"))
                }
                _ => None,
            })
            .map(|chunk| chunk.entity_path().to_string())
            .collect::<Vec<_>>();
        assert!(
            paths
                .iter()
                .any(|path| { path == "/world/camera/rois/fixed/detect-fast/roi" })
        );
        assert!(
            paths
                .iter()
                .any(|path| { path == "/world/camera/rois/fixed/face-dwell/roi" })
        );
    }

    #[test]
    fn occupancy_state_has_sequence_timestamp_and_log_time_timelines() {
        let (rec, storage) = rerun::RecordingStreamBuilder::new("mana-viz-test")
            .batcher_config(rerun::log::ChunkBatcherConfig::NEVER)
            .memory()
            .expect("memory recording");
        rec.set_log_time_enabled(true);
        let mut bridge = VizBridge {
            inner: Inner::Connected {
                rec,
                last_flush_warn: Instant::now(),
            },
            addr: String::new(),
            toggles: VizSendToggles::default(),
            fixed_rois: Vec::new(),
            roles: HashMap::new(),
            face_models: HashSet::new(),
            last_infer_at: HashMap::new(),
            last_occupancy_state: None,
            last_second_person_state: None,
            last_signal_state: None,
            last_face_state: None,
        };

        bridge.set_frame_time(1, 1_000);
        bridge.log_occupancy_state(
            RoomCardinality::Empty,
            SecondPersonState::None,
            SignalValidity::Valid,
        );
        bridge.set_frame_time(2, 2_000);
        bridge.log_occupancy_state(
            RoomCardinality::Single,
            SecondPersonState::None,
            SignalValidity::Valid,
        );

        let state_chunks = storage
            .take()
            .into_iter()
            .filter_map(|msg| match msg {
                rerun::log::LogMsg::ArrowMsg(_, msg) => {
                    Some(rerun::log::Chunk::from_arrow_msg(&msg).expect("valid chunk"))
                }
                _ => None,
            })
            .filter(|chunk| chunk.entity_path().to_string() == "/pipeline/state/room/cardinality")
            .collect::<Vec<_>>();

        let chunk = state_chunks.first().expect("state chunk");
        let timelines = chunk.timelines();
        assert_eq!(
            timelines
                .get(&rerun::TimelineName::from(FRAME_NUMBER_TIMELINE))
                .expect("frame number timeline")
                .timeline()
                .typ(),
            rerun::external::re_log_types::TimeType::Sequence
        );
        assert_eq!(
            timelines
                .get(&rerun::TimelineName::from(FRAME_TIME_TIMELINE))
                .expect("frame timestamp timeline")
                .timeline()
                .typ(),
            rerun::external::re_log_types::TimeType::TimestampNs
        );
        assert!(timelines.contains_key(&rerun::TimelineName::log_time()));
    }
}

fn finite_depth_min(depth: &DepthFrame) -> Option<f32> {
    depth
        .iter_values()
        .filter(|value| value.is_finite() && *value > 0.0)
        .reduce(f32::min)
}

fn finite_depth_max(depth: &DepthFrame) -> Option<f32> {
    depth
        .iter_values()
        .filter(|value| value.is_finite() && *value > 0.0)
        .reduce(f32::max)
}

fn bbox_in_roi(bbox: [f32; 4], roi: CropRect) -> Option<[f32; 4]> {
    let width = (roi.x2.saturating_sub(roi.x1)) as f32;
    let height = (roi.y2.saturating_sub(roi.y1)) as f32;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let x1 = (bbox[0] - roi.x1 as f32).clamp(0.0, width);
    let y1 = (bbox[1] - roi.y1 as f32).clamp(0.0, height);
    let x2 = (bbox[2] - roi.x1 as f32).clamp(0.0, width);
    let y2 = (bbox[3] - roi.y1 as f32).clamp(0.0, height);
    (x2 > x1 && y2 > y1).then_some([x1, y1, x2, y2])
}

#[cfg(test)]
mod mask_debug_tests {
    use super::*;
    use crate::detection::DetectionMask;
    use mana_geometry::compact_mask::CompactMask;
    use std::sync::Arc;

    fn detection_with_mask(
        compact: CompactMask,
        origin: [u32; 2],
        mask_dims: [u32; 2],
        polygons: Vec<Vec<[f32; 2]>>,
    ) -> Detection {
        Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 1.0, 1.0],
            keypoints: None,
            mask: Some(DetectionMask {
                compact: Arc::new(compact),
                polygons: Arc::new(polygons),
                origin,
                mask_dims,
            }),
        }
    }

    #[test]
    fn polygon_strip_is_closed_and_in_frame_pixels() {
        let poly = vec![[0.25, 0.1], [0.75, 0.5], [0.25, 0.9]];
        let strip = frame_strip(&poly, 1920.0, 1080.0);
        assert_eq!(
            strip,
            vec![
                [480.0, 108.0],
                [1440.0, 540.0],
                [480.0, 972.0],
                [480.0, 108.0]
            ]
        );
        assert_eq!(strip.first(), strip.last(), "loop closed");
    }

    #[test]
    fn overlay_paints_mask_then_polygon_on_top() {
        // 8x8 block at offset (2,2) inside a 10x10 mask space; frame == mask
        // space, so the frame-normalized polygon (3,4)-(5,4)-(5,6)-(3,6)
        // maps back to those same pixels.
        let compact = CompactMask::from_dense(&[1u8; 64], 8, 8, (2, 2), (10, 10)).unwrap();
        let poly: Vec<[f32; 2]> = vec![[0.3, 0.4], [0.5, 0.4], [0.5, 0.6], [0.3, 0.6]];
        let det = detection_with_mask(compact, [0, 0], [10, 10], vec![poly]);
        let masked: Vec<&Detection> = vec![&det];

        let overlay = build_mask_overlay(&masked, 10, 10, 10, 10);

        assert_eq!(overlay[0], 0, "background stays empty");
        assert_eq!(overlay[8 * 10 + 8], 1, "mask fill inside the block");
        assert_eq!(
            overlay[4 * 10 + 4],
            0xFF,
            "polygon contour drawn over the mask fill"
        );
        assert_eq!(
            overlay[5 * 10 + 6],
            0xFF,
            "polygon contour drawn over the mask fill"
        );
    }

    #[test]
    fn mask_and_polygon_render_in_mask_space() {
        // 8x8 block at offset (2,2) inside a 10x10 mask space.
        let compact = CompactMask::from_dense(&[1u8; 64], 8, 8, (2, 2), (10, 10)).unwrap();
        // Frame == mask space (origin 0, frame 10x10), so polygons are
        // normalized 0..1 over the same coordinates. The contour (3,4)-(5,4)-
        // (5,6)-(3,6) sits inside the block, with room for fill pixels away
        // from the thick border.
        let poly: Vec<[f32; 2]> = vec![[0.3, 0.4], [0.5, 0.4], [0.5, 0.6], [0.3, 0.6]];
        let det = detection_with_mask(compact, [0, 0], [10, 10], vec![poly]);
        let masked: Vec<&Detection> = vec![&det];

        let (mask_img, poly_img, w, h) = render_mask_debug_images(&masked, 10, 10).unwrap();
        assert_eq!((w, h), (10, 10));

        let color = Rgb(PALETTE[0]);
        assert_eq!(
            mask_img.get_pixel(8, 8),
            &color,
            "mask fill away from the border"
        );
        assert_eq!(
            mask_img.get_pixel(0, 0),
            &Rgb([0, 0, 0]),
            "background stays black"
        );

        assert_eq!(
            poly_img.get_pixel(0, 0),
            &Rgb([0, 0, 0]),
            "polygon image starts black"
        );
        let mut line_hit = false;
        for px in 3..=5 {
            if *poly_img.get_pixel(px, 4) == color {
                line_hit = true;
            }
        }
        assert!(line_hit, "polygon edge drawn between (3,4) and (5,4)");

        // The polygon border is drawn over the mask in white and is thick
        // enough to be visible on top of the colored fill.
        assert_eq!(
            mask_img.get_pixel(4, 4),
            &Rgb([255, 255, 255]),
            "white polygon border over the mask fill"
        );
        assert_eq!(
            poly_img.get_pixel(4, 3),
            &color,
            "thick stroke also paints the row above the edge"
        );
    }

    #[test]
    fn polygon_maps_back_from_frame_to_mask_space() {
        // Mask space 4x4 placed at origin (2,3) inside a 10x10 frame.
        let compact = CompactMask::from_dense(&[1, 1, 1, 1], 2, 2, (1, 1), (4, 4)).unwrap();
        // A mask-space point (0.25, 0.25) is frame-normalized as
        // (0.25*4 + 2)/10 = 0.3, (0.25*4 + 3)/10 = 0.4 (as `run()` leaves it).
        let poly: Vec<[f32; 2]> = vec![[0.3, 0.4]];
        let det = detection_with_mask(compact, [2, 3], [4, 4], vec![poly]);
        let masked: Vec<&Detection> = vec![&det];

        let (mask_img, poly_img, w, h) = render_mask_debug_images(&masked, 10, 10).unwrap();
        assert_eq!((w, h), (4, 4));
        assert_eq!(
            mask_img.get_pixel(1, 1),
            &Rgb(PALETTE[0]),
            "mask at (1,1) of mask space"
        );

        // Single-vertex polygon draws no line (len < 2) but the inverse
        // mapping itself is exercised without panicking.
        assert_eq!(poly_img.get_pixel(0, 0), &Rgb([0, 0, 0]));
    }

    #[test]
    fn depth_context_bbox_is_translated_and_clipped_to_roi() {
        let roi = CropRect {
            x1: 560,
            y1: 140,
            x2: 1240,
            y2: 820,
        };

        assert_eq!(
            bbox_in_roi([500.0, 100.0, 700.0, 300.0], roi),
            Some([0.0, 0.0, 140.0, 160.0])
        );
    }

    #[test]
    fn depth_context_bbox_is_ignored_when_outside_roi() {
        let roi = CropRect {
            x1: 560,
            y1: 140,
            x2: 1240,
            y2: 820,
        };

        assert_eq!(bbox_in_roi([0.0, 0.0, 100.0, 100.0], roi), None);
    }

    #[test]
    fn pose_keypoint_visibility_filters_confidence_and_non_finite_points() {
        assert!(pose_keypoint_visible(10.0, 20.0, 0.25));
        assert!(!pose_keypoint_visible(10.0, 20.0, 0.24));
        assert!(!pose_keypoint_visible(f32::NAN, 20.0, 0.9));
    }
}
