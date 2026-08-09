use serde::Deserialize;
use std::path::PathBuf;

pub const MANA_TOML_SCHEMA_VERSION: u32 = 1;

const fn default_schema_version() -> u32 {
    MANA_TOML_SCHEMA_VERSION
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
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
    /// Clinical scan clock (independent of camera I-frame cadence).
    #[serde(default)]
    pub scan: ScanConfigSection,
    #[serde(default)]
    pub metrics_file: Option<PathBuf>,
    #[serde(default)]
    pub viz_file: Option<PathBuf>,
    #[serde(default)]
    pub rerun_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

/// Scan clock section in `mana.toml` (`[scan]`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanConfigSection {
    /// Period of the clinical scan tick in milliseconds (default 200 → 5 Hz).
    #[serde(default = "default_scan_period_ms")]
    pub period_ms: u64,
}

impl Default for ScanConfigSection {
    fn default() -> Self {
        Self {
            period_ms: default_scan_period_ms(),
        }
    }
}

const fn default_scan_period_ms() -> u64 {
    200
}

impl ScanConfigSection {
    #[must_use]
    pub fn to_scan_config(&self) -> crate::scan::ScanConfig {
        crate::scan::ScanConfig {
            period_ms: self.period_ms.max(1),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectionConfig {
    #[serde(default = "default_face_component_coverage")]
    pub face_component_coverage: f32,
    #[serde(default = "default_face_max_center_y_ratio")]
    pub face_max_center_y_ratio: f32,
    #[serde(default = "default_face_edge_margin_px")]
    pub face_edge_margin_px: u32,
    #[serde(default = "default_same_class_iou")]
    pub same_class_iou: f32,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            face_component_coverage: default_face_component_coverage(),
            face_max_center_y_ratio: default_face_max_center_y_ratio(),
            face_edge_margin_px: default_face_edge_margin_px(),
            same_class_iou: default_same_class_iou(),
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

const fn default_same_class_iou() -> f32 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// Policy for retaining the person-of-interest signal before tracking
/// (real elapsed time, not keyframe counts).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresencePoiPolicy {
    #[serde(default = "default_presence_on_ms")]
    pub on_ms: u64,
    #[serde(default = "default_presence_off_ms")]
    pub off_ms: u64,
}

impl Default for PresencePoiPolicy {
    fn default() -> Self {
        Self {
            on_ms: default_presence_on_ms(),
            off_ms: default_presence_off_ms(),
        }
    }
}

/// Time-based policy for confirming and releasing room cardinality states.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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
            && self.poi.on_ms > 0
            && self.poi.off_ms > 0
            && self.occupancy.single_confirm_ms > 0
            && self.occupancy.empty_confirm_ms > 0
            && self.occupancy.multiple_confirm_ms > 0
            && self.occupancy.multiple_exit_ms > 0
    }
}

fn default_presence_class() -> String {
    "person".into()
}

fn default_presence_on_ms() -> u64 {
    200
}

fn default_presence_off_ms() -> u64 {
    800
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
#[serde(deny_unknown_fields)]
pub struct TrackingConfig {
    #[serde(default = "default_tracking_min_hits")]
    pub min_hits: u32,
    #[serde(default = "default_tracking_max_age_ms")]
    pub max_age_ms: u64,
    #[serde(default = "default_tracking_tentative_max_age_ms")]
    pub tentative_max_age_ms: u64,
    #[serde(default = "default_tracking_iou")]
    pub iou_threshold: f32,
    #[serde(default = "default_tracking_mahalanobis")]
    pub mahalanobis_threshold: f32,
    #[serde(default = "default_tracking_ghost_max_ms")]
    pub ghost_max_ms: u64,
    /// Periodo nominal medido de keyframes. Desaparece cuando el scan sea la
    /// base de tiempo del pipeline y no una parametrización de cámara.
    #[serde(default = "default_tracking_nominal_dt_ms")]
    pub nominal_dt_ms: u64,
    #[serde(default)]
    pub noise: TrackingNoiseConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingNoiseConfig {
    #[serde(default = "default_measurement_noise")]
    pub measurement: f32,
    #[serde(default = "default_process_position_noise")]
    pub process_position: f32,
    #[serde(default = "default_process_velocity_noise")]
    pub process_velocity: f32,
}

impl Default for TrackingConfig {
    fn default() -> Self {
        Self {
            min_hits: default_tracking_min_hits(),
            max_age_ms: default_tracking_max_age_ms(),
            tentative_max_age_ms: default_tracking_tentative_max_age_ms(),
            iou_threshold: default_tracking_iou(),
            mahalanobis_threshold: default_tracking_mahalanobis(),
            ghost_max_ms: default_tracking_ghost_max_ms(),
            nominal_dt_ms: default_tracking_nominal_dt_ms(),
            noise: TrackingNoiseConfig::default(),
        }
    }
}

impl Default for TrackingNoiseConfig {
    fn default() -> Self {
        Self {
            measurement: default_measurement_noise(),
            process_position: default_process_position_noise(),
            process_velocity: default_process_velocity_noise(),
        }
    }
}

impl TrackingConfig {
    pub fn is_valid(&self) -> bool {
        self.ghost_max_ms > 0
            && self.nominal_dt_ms > 0
            && self.mahalanobis_threshold.is_finite()
            && self.mahalanobis_threshold > 0.0
            && self.noise.measurement.is_finite()
            && self.noise.measurement > 0.0
            && self.noise.process_position.is_finite()
            && self.noise.process_position > 0.0
            && self.noise.process_velocity.is_finite()
            && self.noise.process_velocity > 0.0
    }
}

fn default_tracking_min_hits() -> u32 {
    2
}

fn default_tracking_max_age_ms() -> u64 {
    4_000
}

fn default_tracking_tentative_max_age_ms() -> u64 {
    600
}

fn default_tracking_iou() -> f32 {
    0.2
}

const fn default_tracking_mahalanobis() -> f32 {
    9.5
}

const fn default_tracking_ghost_max_ms() -> u64 {
    6_000
}

const fn default_tracking_nominal_dt_ms() -> u64 {
    2_000
}

const fn default_measurement_noise() -> f32 {
    1.0
}

const fn default_process_position_noise() -> f32 {
    1.0
}

const fn default_process_velocity_noise() -> f32 {
    0.25
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct HealthConfig {
    #[serde(default = "default_data_stale_ms")]
    pub data_stale_ms: u64,
    #[serde(default = "default_stale_warn_ms")]
    pub stale_warn_ms: u64,
    /// Watchdog de panics por densidad, no por racha: una racha corta
    /// (p. ej. 3 panics seguidos cada tanto) no debe tumbar el proceso si el
    /// trabajo recupera de inmediato; lo que importa es qué fracción de los
    /// últimos ciclos de keyframe están fallando. Por eso la ventana se
    /// expresa en ciclos de trabajo y no en milisegundos: un umbral por
    /// tiempo confundiría un plazo con una fracción (mismo razonamiento que
    /// `min_hits`). Sugerencia de arranque: 20 ciclos y disparar al superar 3
    /// panics. Con ciclos de ~50 ms, la ventana cubre como máximo ~1 s de
    /// trabajo degradado antes de la señal de salida.
    #[serde(default = "default_panic_window_cycles")]
    pub panic_window_cycles: usize,
    #[serde(default = "default_max_panics_in_window")]
    pub max_panics_in_window: u32,
    /// Presupuesto del scan: el periodo nominal del superloop, hoy emergente
    /// del `poll_timeout_ms` = 50 (el "techo de ~20 Hz"). Declararlo hace
    /// que `cycle_overruns` sea una señal verificable: un ciclo con trabajo
    /// real que supera el presupuesto perdió el ritmo del scan.
    #[serde(default = "default_cycle_budget_ms")]
    pub cycle_budget_ms: u64,
    #[serde(default = "default_report_interval_s")]
    pub report_interval_s: u64,
}

impl HealthConfig {
    pub fn is_valid(&self) -> bool {
        self.data_stale_ms > 0 && self.stale_warn_ms < self.data_stale_ms
    }
}

fn default_data_stale_ms() -> u64 {
    10_000
}

const fn default_stale_warn_ms() -> u64 {
    5_000
}

fn default_panic_window_cycles() -> usize {
    20
}

fn default_max_panics_in_window() -> u32 {
    3
}

fn default_cycle_budget_ms() -> u64 {
    50
}

fn default_report_interval_s() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
