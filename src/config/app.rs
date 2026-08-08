use serde::Deserialize;
use std::path::PathBuf;

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
    pub presence: PresenceConfig,
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
    #[serde(default = "default_face_edge_margin_px")]
    pub face_edge_margin_px: u32,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            face_component_coverage: default_face_component_coverage(),
            face_max_center_y_ratio: default_face_max_center_y_ratio(),
            face_edge_margin_px: default_face_edge_margin_px(),
        }
    }
}

fn default_face_component_coverage() -> f32 {
    0.70
}

fn default_face_max_center_y_ratio() -> f32 {
    0.65
}

fn default_face_edge_margin_px() -> u32 {
    32
}

#[derive(Debug, Clone, Deserialize)]
pub struct PresenceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_presence_class")]
    pub class: String,
    #[serde(default)]
    pub poi: PresencePoiPolicy,
    #[serde(default)]
    pub occupancy: OccupancyPolicy,
}

/// Policy for retaining the person-of-interest signal before tracking.
#[derive(Debug, Clone, Deserialize)]
pub struct PresencePoiPolicy {
    #[serde(default = "default_presence_on_ticks")]
    pub on_ticks: u32,
    #[serde(default = "default_presence_off_ticks")]
    pub off_ticks: u32,
}

impl Default for PresencePoiPolicy {
    fn default() -> Self {
        Self {
            on_ticks: default_presence_on_ticks(),
            off_ticks: default_presence_off_ticks(),
        }
    }
}

/// Time-based policy for confirming and releasing room cardinality states.
#[derive(Debug, Clone, Deserialize)]
pub struct OccupancyPolicy {
    #[serde(default = "default_occupancy_single_confirm_ms")]
    pub single_confirm_ms: u64,
    #[serde(default = "default_occupancy_empty_confirm_ms")]
    pub empty_confirm_ms: u64,
    #[serde(default = "default_occupancy_multiple_confirm_ms")]
    pub multiple_confirm_ms: u64,
    #[serde(default = "default_occupancy_multiple_exit_ms")]
    pub multiple_exit_ms: u64,
    #[serde(default)]
    pub require_confirmed_tracks: bool,
}

impl Default for OccupancyPolicy {
    fn default() -> Self {
        Self {
            single_confirm_ms: default_occupancy_single_confirm_ms(),
            empty_confirm_ms: default_occupancy_empty_confirm_ms(),
            multiple_confirm_ms: default_occupancy_multiple_confirm_ms(),
            multiple_exit_ms: default_occupancy_multiple_exit_ms(),
            require_confirmed_tracks: false,
        }
    }
}

impl Default for PresenceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            class: default_presence_class(),
            poi: PresencePoiPolicy::default(),
            occupancy: OccupancyPolicy::default(),
        }
    }
}

impl PresenceConfig {
    pub fn is_valid(&self) -> bool {
        !self.class.trim().is_empty()
            && self.poi.on_ticks > 0
            && self.poi.off_ticks > 0
            && self.occupancy.single_confirm_ms > 0
            && self.occupancy.empty_confirm_ms > 0
            && self.occupancy.multiple_confirm_ms > 0
            && self.occupancy.multiple_exit_ms > 0
    }
}

fn default_presence_class() -> String {
    "person".into()
}

fn default_presence_on_ticks() -> u32 {
    1
}

fn default_presence_off_ticks() -> u32 {
    4
}

fn default_occupancy_single_confirm_ms() -> u64 {
    3_000
}

fn default_occupancy_empty_confirm_ms() -> u64 {
    8_000
}

fn default_occupancy_multiple_confirm_ms() -> u64 {
    5_000
}

fn default_occupancy_multiple_exit_ms() -> u64 {
    5_000
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
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub blueprint_file: Option<PathBuf>,
    #[serde(default)]
    pub cascade_file: Option<PathBuf>,
    #[serde(default)]
    pub zones_file: Option<PathBuf>,
    #[serde(default)]
    pub fsm_file: Option<PathBuf>,
    #[serde(default)]
    pub depth_rules_file: Option<PathBuf>,
    #[serde(default)]
    pub disabled_tasks: Vec<String>,
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
