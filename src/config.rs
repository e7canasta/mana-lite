use crate::error::{ConfigError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub source: SourceConfig,
    #[serde(default)]
    pub ingest: IngestConfig,
    pub inference: InferenceConfig,
    pub health: HealthConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub viz: VizConfig,
    #[serde(default)]
    pub pipeline: PipelineConfig,
    #[serde(default)]
    pub detection: DetectionConfig,
    #[serde(default)]
    pub tracking: TrackingConfig,
    #[serde(default)]
    pub metrics_file: Option<PathBuf>,
    #[serde(default)]
    pub viz_file: Option<PathBuf>,
    #[serde(default)]
    pub rerun_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct SourceConfig {
    pub url: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default)]
    pub keyframes_only: bool,
}

fn default_transport() -> String {
    "tcp".into()
}

#[derive(Debug, Deserialize)]
pub struct VizConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_rerun_addr")]
    pub rerun_addr: String,
}

impl Default for VizConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rerun_addr: default_rerun_addr(),
        }
    }
}

fn default_rerun_addr() -> String {
    "0.0.0.0:9876".into()
}

#[derive(Debug, Deserialize)]
pub struct PipelineConfig {
    #[serde(default = "default_true")]
    pub infer: bool,
    #[serde(default = "default_true")]
    pub track: bool,
    #[serde(default = "default_true")]
    pub zones: bool,
    #[serde(default = "default_true")]
    pub fsm: bool,
    #[serde(default = "default_true")]
    pub snapshot: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            infer: true,
            track: true,
            zones: true,
            fsm: true,
            snapshot: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct DetectionConfig {
    #[serde(default = "default_face_component_coverage")]
    pub face_component_coverage: f32,
    #[serde(default = "default_face_max_center_y_ratio")]
    pub face_max_center_y_ratio: f32,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            face_component_coverage: default_face_component_coverage(),
            face_max_center_y_ratio: default_face_max_center_y_ratio(),
        }
    }
}

fn default_face_component_coverage() -> f32 {
    0.70
}
fn default_face_max_center_y_ratio() -> f32 {
    0.65
}

#[derive(Debug, Deserialize)]
pub struct TrackingConfig {
    #[serde(default = "default_tracking_min_hits")]
    pub min_hits: u32,
    #[serde(default = "default_tracking_max_age")]
    pub max_age: u32,
    #[serde(default = "default_tracking_tentative_max_age")]
    pub tentative_max_age: u32,
    #[serde(default = "default_tracking_iou")]
    pub iou_threshold: f32,
}

impl Default for TrackingConfig {
    fn default() -> Self {
        Self {
            min_hits: default_tracking_min_hits(),
            max_age: default_tracking_max_age(),
            tentative_max_age: default_tracking_tentative_max_age(),
            iou_threshold: default_tracking_iou(),
        }
    }
}

fn default_tracking_min_hits() -> u32 {
    2
}
fn default_tracking_max_age() -> u32 {
    20
}
fn default_tracking_tentative_max_age() -> u32 {
    3
}
fn default_tracking_iou() -> f32 {
    0.2
}

#[derive(Debug, Deserialize)]
pub struct IngestConfig {
    #[serde(default = "default_poll_timeout_ms")]
    pub poll_timeout_ms: u64,
    #[serde(default = "default_error_window_size")]
    pub error_window_size: usize,
    #[serde(default = "default_error_window_threshold")]
    pub error_window_threshold: u32,
    #[serde(default = "default_backoff_initial_ms")]
    pub reconnect_backoff_initial_ms: u64,
    #[serde(default = "default_backoff_max_ms")]
    pub reconnect_backoff_max_ms: u64,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            poll_timeout_ms: default_poll_timeout_ms(),
            error_window_size: default_error_window_size(),
            error_window_threshold: default_error_window_threshold(),
            reconnect_backoff_initial_ms: default_backoff_initial_ms(),
            reconnect_backoff_max_ms: default_backoff_max_ms(),
        }
    }
}

fn default_poll_timeout_ms() -> u64 {
    50
}
fn default_error_window_size() -> usize {
    128
}
fn default_error_window_threshold() -> u32 {
    25
}
fn default_backoff_initial_ms() -> u64 {
    1000
}
fn default_backoff_max_ms() -> u64 {
    30_000
}

#[derive(Debug, Deserialize)]
pub struct InferenceConfig {
    pub model_catalog: PathBuf,
    pub default_model: String,
    #[serde(default)]
    pub cascade_file: Option<PathBuf>,
    #[serde(default)]
    pub zones_file: Option<PathBuf>,
    #[serde(default)]
    pub fsm_file: Option<PathBuf>,
    #[serde(default)]
    pub disabled_tasks: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostprocessConfig {
    #[serde(default)]
    pub allow_classes: Vec<String>,
    #[serde(default = "default_postprocess_min_confidence")]
    pub min_confidence: f32,
    #[serde(default = "default_postprocess_min_area_ratio")]
    pub min_area_ratio: f32,
    #[serde(default = "default_postprocess_max_area_ratio")]
    pub max_area_ratio: f32,
    #[serde(default = "default_postprocess_min_component_area_ratio")]
    pub min_component_area_ratio: f32,
    #[serde(default = "default_postprocess_mask_threshold")]
    pub mask_threshold: f32,
    #[serde(default = "default_postprocess_nms_iou")]
    pub nms_iou: f32,
    #[serde(default)]
    pub max_detections: Option<usize>,
}

impl Default for PostprocessConfig {
    fn default() -> Self {
        Self {
            allow_classes: Vec::new(),
            min_confidence: default_postprocess_min_confidence(),
            min_area_ratio: default_postprocess_min_area_ratio(),
            max_area_ratio: default_postprocess_max_area_ratio(),
            min_component_area_ratio: default_postprocess_min_component_area_ratio(),
            mask_threshold: default_postprocess_mask_threshold(),
            nms_iou: default_postprocess_nms_iou(),
            max_detections: None,
        }
    }
}

impl PostprocessConfig {
    pub fn is_valid(&self) -> bool {
        self.allow_classes
            .iter()
            .all(|class| !class.trim().is_empty())
            && self.min_confidence.is_finite()
            && (0.0..=1.0).contains(&self.min_confidence)
            && self.min_area_ratio.is_finite()
            && self.min_area_ratio >= 0.0
            && self.max_area_ratio.is_finite()
            && self.max_area_ratio <= 1.0
            && self.max_area_ratio >= self.min_area_ratio
            && self.min_component_area_ratio.is_finite()
            && (0.0..=1.0).contains(&self.min_component_area_ratio)
            && self.mask_threshold.is_finite()
            && (0.0..=1.0).contains(&self.mask_threshold)
            && self.nms_iou.is_finite()
            && (0.0..=1.0).contains(&self.nms_iou)
            && self.max_detections.is_none_or(|max| max > 0)
    }

    pub fn accepts(
        &self,
        class: &str,
        confidence: f32,
        bbox: [f32; 4],
        frame_w: u32,
        frame_h: u32,
    ) -> bool {
        let [x1, y1, x2, y2] = bbox;
        let width = x2 - x1;
        let height = y2 - y1;
        let frame_area = (frame_w as f32) * (frame_h as f32);
        let area_ratio = if frame_area > 0.0 {
            (width * height) / frame_area
        } else {
            0.0
        };

        (self.allow_classes.is_empty() || self.allow_classes.iter().any(|allowed| allowed == class))
            && confidence.is_finite()
            && confidence >= self.min_confidence
            && x1.is_finite()
            && y1.is_finite()
            && x2.is_finite()
            && y2.is_finite()
            && x1 >= 0.0
            && y1 >= 0.0
            && x2 <= frame_w as f32
            && y2 <= frame_h as f32
            && width > 0.0
            && height > 0.0
            && frame_area > 0.0
            && area_ratio >= self.min_area_ratio
            && area_ratio <= self.max_area_ratio
    }
}

fn default_postprocess_min_confidence() -> f32 {
    0.0
}
fn default_postprocess_min_area_ratio() -> f32 {
    0.0
}
fn default_postprocess_max_area_ratio() -> f32 {
    1.0
}
fn default_postprocess_min_component_area_ratio() -> f32 {
    0.0
}
fn default_postprocess_mask_threshold() -> f32 {
    0.5
}
fn default_postprocess_nms_iou() -> f32 {
    0.5
}

#[derive(Debug, Deserialize)]
pub struct HealthConfig {
    #[serde(default = "default_data_stale_ms")]
    pub data_stale_ms: u64,
    #[serde(default = "default_max_panics")]
    pub max_consecutive_panics: u32,
    #[serde(default = "default_report_interval_s")]
    pub report_interval_s: u64,
}

fn default_data_stale_ms() -> u64 {
    10_000
}
fn default_max_panics() -> u32 {
    3
}
fn default_report_interval_s() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub save_dir: Option<PathBuf>,
    #[serde(default = "default_rotate")]
    pub rotate: Rotate,
    #[serde(default = "default_snapshot_dir")]
    pub snapshot_dir: Option<PathBuf>,
    #[serde(default)]
    pub snapshot_verbose: bool,
    #[serde(default = "default_jsonl_level")]
    pub jsonl_level: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: default_format(),
            save_dir: None,
            rotate: default_rotate(),
            snapshot_dir: default_snapshot_dir(),
            snapshot_verbose: false,
            jsonl_level: default_jsonl_level(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rotate {
    Hourly,
    Daily,
    Never,
}

fn default_format() -> String {
    "jsonl".into()
}
fn default_rotate() -> Rotate {
    Rotate::Hourly
}
fn default_jsonl_level() -> String {
    "info".into()
}
fn default_snapshot_dir() -> Option<PathBuf> {
    Some("./snapshots".into())
}

#[derive(Debug, Default, Deserialize)]
pub struct ModelCatalog {
    pub models: HashMap<String, ModelEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub path: PathBuf,
    pub task: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default = "default_iou")]
    pub iou: f32,
    #[serde(default = "default_max_det")]
    pub max_det: u32,
    #[serde(default)]
    pub imgsz: Option<u32>,
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default)]
    pub half: bool,
    #[serde(default = "default_rect")]
    pub rect: bool,
    #[serde(default = "default_polygon_simplify")]
    pub polygon_simplify: f64,
    #[serde(default)]
    pub postprocess: PostprocessConfig,
    #[serde(default)]
    pub crop: Option<CropConfig>,
}

impl ModelEntry {
    pub fn is_valid(&self) -> bool {
        self.confidence.is_finite()
            && (0.0..=1.0).contains(&self.confidence)
            && self.iou.is_finite()
            && (0.0..=1.0).contains(&self.iou)
            && self.max_det > 0
            && self.imgsz.is_none_or(|size| size > 0)
            && self.polygon_simplify.is_finite()
            && self.polygon_simplify > 0.0
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CropConfig {
    #[serde(rename = "type")]
    pub crop_type: CropType,
    #[serde(default)]
    #[allow(dead_code)]
    pub class: Option<String>,
    #[serde(default = "default_crop_margin")]
    pub margin: f32,
    #[serde(default)]
    pub region: Option<[u32; 4]>,
    #[serde(default)]
    pub min_region: Option<[u32; 4]>,
    #[serde(default)]
    pub max_region: Option<[u32; 4]>,
    #[serde(default)]
    pub square_size: Option<u32>,
    #[serde(default)]
    pub upper_fraction: Option<f32>,
    #[serde(default)]
    #[allow(dead_code)]
    pub fallback: FallbackMode,
}

impl CropConfig {
    pub fn is_valid(&self) -> bool {
        self.square_size != Some(0)
            && self
                .upper_fraction
                .is_none_or(|fraction| fraction.is_finite() && (0.0..=1.0).contains(&fraction))
    }

    #[allow(dead_code)]
    pub fn always_run(&self) -> bool {
        self.min_region.is_some() || self.fallback == FallbackMode::Full
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CropType {
    Static,
    LargestClass,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FallbackMode {
    #[default]
    Skip,
    Full,
}

fn default_crop_margin() -> f32 {
    0.15
}

fn default_confidence() -> f32 {
    0.25
}
fn default_iou() -> f32 {
    0.7
}
fn default_max_det() -> u32 {
    300
}
fn default_device() -> String {
    "cpu".into()
}
fn default_rect() -> bool {
    true
}
fn default_polygon_simplify() -> f64 {
    0.75
}

#[derive(Debug, Default, Deserialize)]
pub struct ZoneCatalog {
    pub zones: HashMap<String, ZoneEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZoneEntry {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_hysteresis")]
    pub hysteresis_ms: u64,
}

fn default_hysteresis() -> u64 {
    500
}

#[derive(Debug, Deserialize, Clone)]
pub struct FsmCatalog {
    pub fsm: FsmRoot,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FsmRoot {
    pub initial: String,
    pub states: HashMap<String, FsmState>,
    #[serde(default)]
    pub transitions: Vec<FsmTransition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FsmState {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub dwell_min_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FsmTransition {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub guards: Vec<FsmGuard>,
    #[serde(default)]
    pub dwell: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum FsmGuard {
    #[serde(rename = "zone_occupied")]
    ZoneOccupied {
        zone: String,
        #[serde(default = "default_guard_confidence")]
        min_confidence: f32,
        #[serde(default)]
        min_duration_ms: Option<u64>,
    },
    #[serde(rename = "zone_vacated")]
    ZoneVacated {
        zone: String,
        #[serde(default)]
        min_duration_ms: Option<u64>,
    },
    #[serde(rename = "all_zones_vacant")]
    AllZonesVacant {
        #[serde(default)]
        min_duration_ms: Option<u64>,
    },
    #[serde(rename = "data_stale")]
    DataStale,
}

fn default_guard_confidence() -> f32 {
    0.5
}

// ── viz.toml ─────────────────────────────────────────────
#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct VizDataConfig {
    pub viz: VizDataInner,
}

impl Default for VizDataConfig {
    fn default() -> Self {
        Self {
            viz: VizDataInner::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct VizDataInner {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub rerun_addr: Option<String>,
    #[serde(default)]
    pub send: VizSendToggles,
}

impl Default for VizDataInner {
    fn default() -> Self {
        Self {
            enabled: None,
            rerun_addr: None,
            send: VizSendToggles::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct VizSendToggles {
    #[serde(default = "default_true")]
    pub frames: bool,
    #[serde(default = "default_true")]
    pub boxes: bool,
    #[serde(default)]
    pub crop_frames: bool,
    #[serde(default = "default_true")]
    pub masks: bool,
    #[serde(default)]
    pub mask_debug: bool,
    #[serde(default = "default_true")]
    pub mask_polygons: bool,
    #[serde(default)]
    pub roi_rects: bool,
    #[serde(default = "default_true")]
    pub decode_latency: bool,
    #[serde(default = "default_true")]
    pub infer_latency: bool,
    #[serde(default = "default_true")]
    pub infer_rate: bool,
    #[serde(default = "default_true")]
    pub class_counts_per_frame: bool,
    #[serde(default = "default_true")]
    pub class_confidence_per_frame: bool,
    #[serde(default = "default_true")]
    pub class_area_per_frame: bool,
    #[serde(default = "default_true")]
    pub keyframe_gap: bool,
    #[serde(default = "default_true")]
    pub keyframe_rate: bool,
    #[serde(default = "default_true")]
    pub keyframe_drops: bool,
    #[serde(default)]
    pub depth: bool,
    #[serde(default = "default_depth_viz")]
    pub depth_viz: String,
    #[serde(default = "default_true")]
    pub depth_stats: bool,
}

#[allow(dead_code)]
fn default_depth_viz() -> String {
    "disparity".into()
}

impl Default for VizSendToggles {
    fn default() -> Self {
        Self {
            frames: true,
            boxes: true,
            masks: true,
            mask_debug: false,
            mask_polygons: true,
            crop_frames: false,
            roi_rects: false,
            decode_latency: true,
            infer_latency: true,
            infer_rate: true,
            class_counts_per_frame: true,
            class_confidence_per_frame: true,
            class_area_per_frame: true,
            keyframe_gap: true,
            keyframe_rate: true,
            keyframe_drops: true,
            depth: false,
            depth_viz: default_depth_viz(),
            depth_stats: true,
        }
    }
}

// ── metrics.toml ──────────────────────────────────────────
#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct MetricsLogConfig {
    pub metrics: MetricsInner,
}

impl Default for MetricsLogConfig {
    fn default() -> Self {
        Self {
            metrics: MetricsInner::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct MetricsInner {
    #[serde(default = "default_report_interval_s")]
    pub report_interval_s: u64,
    #[serde(default)]
    pub text: MetricsTextConfig,
    #[serde(default)]
    pub jsonl: MetricsJsonlConfig,
}

impl Default for MetricsInner {
    fn default() -> Self {
        Self {
            report_interval_s: 5,
            text: MetricsTextConfig::default(),
            jsonl: MetricsJsonlConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct MetricsTextConfig {
    #[serde(default = "default_true")]
    pub ingest_line: bool,
    #[serde(default = "default_true")]
    pub infer_summary: bool,
    #[serde(default = "default_true")]
    pub per_model_lines: bool,
    #[serde(default)]
    pub flags: MetricsTextFlags,
}

impl Default for MetricsTextConfig {
    fn default() -> Self {
        Self {
            ingest_line: true,
            infer_summary: true,
            per_model_lines: true,
            flags: MetricsTextFlags::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct MetricsTextFlags {
    #[serde(default = "default_true")]
    pub ingest_pframes: bool,
    #[serde(default = "default_true")]
    pub ingest_dup: bool,
    #[serde(default = "default_true")]
    pub ingest_keyframe_drops: bool,
    #[serde(default = "default_true")]
    pub ingest_timeouts: bool,
    #[serde(default = "default_true")]
    pub ingest_reconnect: bool,
    #[serde(default = "default_true")]
    pub ingest_ssrc: bool,
    #[serde(default = "default_true")]
    pub ingest_rtp: bool,
    #[serde(default = "default_true")]
    pub infer_skips: bool,
    #[serde(default = "default_true")]
    pub infer_empty: bool,
}

impl Default for MetricsTextFlags {
    fn default() -> Self {
        Self {
            ingest_pframes: true,
            ingest_dup: true,
            ingest_keyframe_drops: true,
            ingest_timeouts: true,
            ingest_reconnect: true,
            ingest_ssrc: true,
            ingest_rtp: true,
            infer_skips: true,
            infer_empty: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct MetricsJsonlConfig {
    #[serde(default = "default_true")]
    pub frame_events: bool,
    #[serde(default = "default_true")]
    pub detection_events: bool,
    #[serde(default = "default_true")]
    pub zone_events: bool,
    #[serde(default = "default_true")]
    pub fsm_events: bool,
    #[serde(default = "default_true")]
    pub metrics_event: bool,
    #[serde(default = "default_true")]
    pub per_model_in_window: bool,
    #[serde(default = "default_true")]
    pub class_counts_in_window: bool,
    #[serde(default = "default_true")]
    pub class_per_frame_stats: bool,
    #[serde(default = "default_true")]
    pub depth_events: bool,
}

impl Default for MetricsJsonlConfig {
    fn default() -> Self {
        Self {
            frame_events: true,
            detection_events: true,
            zone_events: true,
            fsm_events: true,
            metrics_event: true,
            per_model_in_window: true,
            class_counts_in_window: true,
            class_per_frame_stats: true,
            depth_events: true,
        }
    }
}

// ── rerun.toml ────────────────────────────────────────────
#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct RerunBlueprintConfig {
    pub rerun: RerunRoot,
}

impl Default for RerunBlueprintConfig {
    fn default() -> Self {
        Self {
            rerun: RerunRoot::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RerunRoot {
    #[serde(default = "default_rerun_app")]
    pub app: String,
    #[serde(default = "default_max_bytes")]
    pub max_bytes_in_flight_mb: usize,
    #[serde(default = "default_true")]
    pub auto_views: bool,
    #[serde(default = "default_true")]
    pub panels_expanded: bool,
    #[serde(default)]
    pub rows: Vec<RerunRow>,
}

fn default_rerun_app() -> String {
    "mana-lite".into()
}
fn default_max_bytes() -> usize {
    32
}

impl Default for RerunRoot {
    fn default() -> Self {
        Self {
            app: default_rerun_app(),
            max_bytes_in_flight_mb: default_max_bytes(),
            auto_views: default_true(),
            panels_expanded: default_true(),
            rows: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RerunRow {
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub share: f32,
    #[serde(default)]
    pub overrides: Vec<RerunOverride>,
    #[serde(default)]
    pub panels: Vec<RerunPanel>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RerunOverride {
    pub path: String,
    pub interpolation: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RerunPanel {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub contents: Vec<String>,
}

use serde::de::DeserializeOwned;

fn read_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .map_err(|_| ConfigError::FileNotFound(path.display().to_string()).into())
}

pub fn load_config<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let content = read_file(path)?;
    toml::from_str(&content).map_err(|e| {
        ConfigError::ParseError {
            file: path.display().to_string(),
            msg: e.to_string(),
        }
        .into()
    })
}

pub fn load_app_config(path: &Path) -> Result<AppConfig> {
    let mut config: AppConfig = load_config(path)?;
    apply_env_overrides(&mut config);
    Ok(config)
}

macro_rules! env_str {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            $field = v;
        }
    };
}
macro_rules! env_path {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            $field = v.into();
        }
    };
}
macro_rules! env_bool {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            $field = v == "1" || v == "true";
        }
    };
}
macro_rules! env_parse {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            if let Ok(n) = v.parse() {
                $field = n;
            }
        }
    };
}
macro_rules! env_opt {
    ($var:literal => $field:expr) => {
        if let Ok(v) = std::env::var($var) {
            $field = if v.is_empty() { None } else { Some(v.into()) };
        }
    };
}

fn apply_env_overrides(cfg: &mut AppConfig) {
    env_str!("MANA_SOURCE_URL"       => cfg.source.url);
    env_opt!("MANA_SOURCE_USERNAME"   => cfg.source.username);
    env_opt!("MANA_SOURCE_PASSWORD"   => cfg.source.password);
    env_str!("MANA_TRANSPORT"         => cfg.source.transport);
    env_bool!("MANA_KEYFRAMES_ONLY"   => cfg.source.keyframes_only);
    env_path!("MANA_MODEL_CATALOG"     => cfg.inference.model_catalog);
    env_str!("MANA_DEFAULT_MODEL"     => cfg.inference.default_model);
    env_parse!("MANA_DATA_STALE_MS"   => cfg.health.data_stale_ms);
    env_parse!("MANA_REPORT_INTERVAL" => cfg.health.report_interval_s);
    env_opt!("MANA_SAVE_DIR"          => cfg.output.save_dir);
    env_bool!("MANA_VIZ_ENABLED"      => cfg.viz.enabled);
    env_str!("MANA_RERUN_ADDR"        => cfg.viz.rerun_addr);
    env_opt!("MANA_SNAPSHOT_DIR"      => cfg.output.snapshot_dir);
    env_bool!("MANA_SNAPSHOT_VERBOSE" => cfg.output.snapshot_verbose);
    env_str!("MANA_JSONL_LEVEL"       => cfg.output.jsonl_level);
    env_parse!("MANA_POLL_TIMEOUT_MS" => cfg.ingest.poll_timeout_ms);
    env_parse!("MANA_ERROR_WINDOW_SIZE" => cfg.ingest.error_window_size);
    env_parse!("MANA_ERROR_WINDOW_THRESHOLD" => cfg.ingest.error_window_threshold);
    env_parse!("MANA_BACKOFF_INITIAL_MS" => cfg.ingest.reconnect_backoff_initial_ms);
    env_parse!("MANA_BACKOFF_MAX_MS" => cfg.ingest.reconnect_backoff_max_ms);
}

macro_rules! config_loader {
    ($name:ident -> $type:ty) => {
        pub fn $name(path: &Path) -> Result<$type> {
            load_config(path)
        }
    };
}

config_loader!(load_model_catalog -> ModelCatalog);
config_loader!(load_zone_catalog -> ZoneCatalog);
config_loader!(load_fsm_catalog -> FsmCatalog);
config_loader!(load_viz_data -> VizDataConfig);
config_loader!(load_metrics_log -> MetricsLogConfig);
config_loader!(load_rerun_blueprint -> RerunBlueprintConfig);

pub fn validate_fsm(
    fsm: &FsmCatalog,
    models: &ModelCatalog,
    zones: &Option<ZoneCatalog>,
) -> Vec<String> {
    let mut errors = Vec::new();

    if !fsm.fsm.states.contains_key(&fsm.fsm.initial) {
        errors.push(format!(
            "initial state '{}' not found in states",
            fsm.fsm.initial
        ));
    }

    for t in &fsm.fsm.transitions {
        if t.from != "*" && !fsm.fsm.states.contains_key(&t.from) {
            errors.push(format!("transition from unknown state '{}'", t.from));
        }
        if !fsm.fsm.states.contains_key(&t.to) {
            errors.push(format!("transition to unknown state '{}'", t.to));
        }
    }

    for (name, state) in &fsm.fsm.states {
        for model_key in &state.models {
            if !models.models.contains_key(model_key) {
                errors.push(format!(
                    "state '{}' references model '{}' not found in model catalog",
                    name, model_key
                ));
            }
        }
    }

    for t in &fsm.fsm.transitions {
        for guard in &t.guards {
            match guard {
                FsmGuard::ZoneOccupied { zone, .. } | FsmGuard::ZoneVacated { zone, .. } => {
                    if let Some(zc) = zones {
                        if !zc.zones.contains_key(zone) {
                            errors.push(format!(
                                "transition {}→{} references zone '{}' not found in zone catalog",
                                t.from, t.to, zone
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_model_catalog() {
        let catalog = load_model_catalog(Path::new("config/models.toml")).unwrap();
        assert!(catalog.models.contains_key("detect-fast"));
        let detect = &catalog.models["detect-fast"];
        assert_eq!(detect.task, "detect");
        assert_eq!(detect.confidence, 0.2);
        assert!(detect.is_valid());
    }

    #[test]
    fn test_load_fp16_benchmark_matrix() {
        let catalog = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let tasks = ["detect", "pose", "seg", "depth"];
        let sizes = ["s", "m", "l", "x"];
        let input_sizes = [320, 640];

        for task in tasks {
            for size in sizes {
                for imgsz in input_sizes {
                    let key = format!("{task}-{size}-{imgsz}");
                    let entry = catalog
                        .models
                        .get(&key)
                        .unwrap_or_else(|| panic!("missing FP16 matrix entry {key}"));
                    assert!(!entry.enabled, "benchmark entry {key} must stay disabled");
                    assert!(entry.half, "benchmark entry {key} must be FP16");
                    assert_eq!(entry.imgsz, Some(imgsz));
                    assert!(entry.is_valid());
                }
            }
        }
    }

    #[test]
    fn test_load_zone_catalog() {
        let catalog = load_zone_catalog(Path::new("config/zones.toml")).unwrap();
        assert!(catalog.zones.contains_key("bed"));
        assert_eq!(catalog.zones["bed"].hysteresis_ms, 500);
    }

    #[test]
    fn test_load_fsm_catalog() {
        let catalog = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        assert_eq!(catalog.fsm.initial, "idle");
        assert!(catalog.fsm.states.contains_key("watching"));
        assert!(!catalog.fsm.transitions.is_empty());
    }

    #[test]
    fn test_load_app_config() {
        let config = load_app_config(Path::new("config/mana.toml")).unwrap();
        assert_eq!(config.source.transport, "tcp");
        assert!(config.source.keyframes_only);
        assert_eq!(config.health.data_stale_ms, 10_000);
        let model = load_model_catalog(Path::new("config/models.toml")).unwrap();
        assert!(model.models["detect-fast"].postprocess.is_valid());
        assert_eq!(
            model.models["detect-fast"].postprocess.allow_classes,
            vec!["person", "wheelchair"]
        );
        assert_eq!(
            model.models["detect-fast"].postprocess.min_area_ratio,
            0.001
        );
        assert_eq!(model.models["face-yolo"].postprocess.nms_iou, 0.05);
        assert_eq!(
            model.models["face-yolo"].postprocess.max_detections,
            Some(1)
        );
        assert_eq!(
            model.models["seg-standard"]
                .postprocess
                .min_component_area_ratio,
            0.05
        );
        assert_eq!(model.models["seg-standard"].polygon_simplify, 0.98);
        assert_eq!(model.models["seg-standard"].postprocess.mask_threshold, 0.5);
        assert_eq!(
            model.models["depth-standard"]
                .crop
                .as_ref()
                .and_then(|crop| crop.region),
            Some([560, 140, 1240, 820])
        );
    }

    #[test]
    fn fsm_validation_catches_unknown_model() {
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let fsm = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        let errors = validate_fsm(&fsm, &models, &None);
        assert!(
            errors.is_empty(),
            "config/fsm.toml should be valid: {:?}",
            errors
        );
    }

    #[test]
    fn fsm_validation_catches_unknown_state() {
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let mut fsm = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        fsm.fsm.transitions.push(FsmTransition {
            from: "ghost".into(),
            to: "idle".into(),
            guards: vec![],
            dwell: None,
        });
        let errors = validate_fsm(&fsm, &models, &None);
        assert!(!errors.is_empty());
    }
}
