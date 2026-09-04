use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub source: SourceConfig,
    pub viz: VizConfig,
    #[serde(default)]
    pub scan: ScanConfig,
    #[serde(default)]
    pub ingest: IngestConfig,
    #[serde(default)]
    pub inference: InferenceConfig,
    #[serde(default)]
    pub output: OutputConfig,
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
}

fn default_transport() -> String {
    "tcp".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VizConfig {
    pub enabled: bool,
    #[serde(default = "default_rerun_addr")]
    pub rerun_addr: String,
    #[serde(default = "default_image_format")]
    pub image_format: String,
    #[serde(default = "default_image_quality")]
    pub image_quality: u8,
}

fn default_rerun_addr() -> String {
    "127.0.0.1:9876".into()
}

fn default_image_format() -> String {
    "jpeg".into()
}

fn default_image_quality() -> u8 {
    75
}

#[derive(Debug, Deserialize)]
pub struct ScanConfig {
    #[serde(default = "default_scan_period_ms")]
    pub period_ms: u64,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            period_ms: default_scan_period_ms(),
        }
    }
}

fn default_scan_period_ms() -> u64 {
    200
}

#[derive(Debug, Clone, Deserialize)]
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
    #[serde(default = "default_dedup_max_suppress_ms")]
    pub dedup_max_suppress_ms: u64,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            poll_timeout_ms: default_poll_timeout_ms(),
            error_window_size: default_error_window_size(),
            error_window_threshold: default_error_window_threshold(),
            reconnect_backoff_initial_ms: default_backoff_initial_ms(),
            reconnect_backoff_max_ms: default_backoff_max_ms(),
            dedup_max_suppress_ms: default_dedup_max_suppress_ms(),
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
fn default_dedup_max_suppress_ms() -> u64 {
    5_000
}

#[derive(Debug, Clone, Deserialize)]
pub struct InferenceConfig {
    /// Path to the ONNX model file.
    pub model_path: String,
    /// Name for this model (used in logs).
    #[serde(default = "default_model_name")]
    pub model_name: String,
    /// Confidence threshold (0.0 - 1.0).
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

fn default_model_name() -> String {
    "detect".into()
}

fn default_confidence() -> f32 {
    0.40
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            model_name: default_model_name(),
            confidence: default_confidence(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default)]
    pub save_dir: Option<PathBuf>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            save_dir: None,
        }
    }
}

fn default_log_level() -> String {
    "info".into()
}

pub fn load(path: &std::path::Path) -> Result<Config, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("config: cannot read {}: {e}", path.display()))?;
    toml::from_str(&content).map_err(|e| format!("config: parse error: {e}"))
}
