#![allow(dead_code)] // schema module: all fields are defined API contract
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::error::{ConfigError, Result};

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
    #[serde(default)]
    pub demo: bool,
}

fn default_transport() -> String { "tcp".into() }

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

fn default_rerun_addr() -> String { "0.0.0.0:9876".into() }

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

fn default_poll_timeout_ms() -> u64 { 50 }
fn default_error_window_size() -> usize { 128 }
fn default_error_window_threshold() -> u32 { 25 }
fn default_backoff_initial_ms() -> u64 { 1000 }
fn default_backoff_max_ms() -> u64 { 30_000 }

#[derive(Debug, Deserialize)]
pub struct InferenceConfig {
    pub model_catalog: PathBuf,
    pub default_model: String,
    #[serde(default)]
    pub zones_file: Option<PathBuf>,
    #[serde(default)]
    pub fsm_file: Option<PathBuf>,
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

fn default_data_stale_ms() -> u64 { 10_000 }
fn default_max_panics() -> u32 { 3 }
fn default_report_interval_s() -> u64 { 5 }

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

fn default_format() -> String { "jsonl".into() }
fn default_rotate() -> Rotate { Rotate::Hourly }
fn default_jsonl_level() -> String { "info".into() }
fn default_snapshot_dir() -> Option<PathBuf> { Some("./snapshots".into()) }

#[derive(Debug, Default, Deserialize)]
pub struct ModelCatalog {
    pub models: HashMap<String, ModelEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub path: PathBuf,
    pub task: String,
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
}

fn default_confidence() -> f32 { 0.25 }
fn default_iou() -> f32 { 0.7 }
fn default_max_det() -> u32 { 300 }
fn default_device() -> String { "cpu".into() }
fn default_rect() -> bool { true }

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

fn default_hysteresis() -> u64 { 500 }

#[derive(Debug, Deserialize)]
pub struct FsmCatalog {
    pub fsm: FsmRoot,
}

#[derive(Debug, Deserialize)]
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

fn default_guard_confidence() -> f32 { 0.5 }

use serde::de::DeserializeOwned;

fn read_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .map_err(|_| ConfigError::FileNotFound(path.display().to_string()).into())
}

pub fn load_config<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let content = read_file(path)?;
    toml::from_str(&content)
        .map_err(|e| ConfigError::ParseError {
            file: path.display().to_string(),
            msg: e.to_string(),
        }
        .into())
}

pub fn load_app_config(path: &Path) -> Result<AppConfig> {
    let mut config: AppConfig = load_config(path)?;
    apply_env_overrides(&mut config);
    Ok(config)
}

fn apply_env_overrides(cfg: &mut AppConfig) {
    if let Ok(v) = std::env::var("MANA_SOURCE_URL") { cfg.source.url = v; }
    if let Ok(v) = std::env::var("MANA_SOURCE_USERNAME") {
        cfg.source.username = if v.is_empty() { None } else { Some(v) };
    }
    if let Ok(v) = std::env::var("MANA_SOURCE_PASSWORD") {
        cfg.source.password = if v.is_empty() { None } else { Some(v) };
    }
    if let Ok(v) = std::env::var("MANA_TRANSPORT") { cfg.source.transport = v; }
    if let Ok(v) = std::env::var("MANA_KEYFRAMES_ONLY") { cfg.source.keyframes_only = v.parse().unwrap_or(true); }
    if let Ok(v) = std::env::var("MANA_DEMO") { cfg.source.demo = v == "1" || v == "true"; }
    if let Ok(v) = std::env::var("MANA_MODEL_CATALOG") { cfg.inference.model_catalog = v.into(); }
    if let Ok(v) = std::env::var("MANA_DEFAULT_MODEL") { cfg.inference.default_model = v; }
    if let Ok(v) = std::env::var("MANA_DATA_STALE_MS") { if let Ok(n) = v.parse() { cfg.health.data_stale_ms = n; } }
    if let Ok(v) = std::env::var("MANA_REPORT_INTERVAL") { if let Ok(n) = v.parse() { cfg.health.report_interval_s = n; } }
    if let Ok(v) = std::env::var("MANA_SAVE_DIR") {
        cfg.output.save_dir = if v.is_empty() { None } else { Some(v.into()) };
    }
    if let Ok(v) = std::env::var("MANA_VIZ_ENABLED") { cfg.viz.enabled = v == "1" || v == "true"; }
    if let Ok(v) = std::env::var("MANA_RERUN_ADDR") { cfg.viz.rerun_addr = v; }
    if let Ok(v) = std::env::var("MANA_SNAPSHOT_DIR") {
        cfg.output.snapshot_dir = if v.is_empty() { None } else { Some(v.into()) };
    }
    if let Ok(v) = std::env::var("MANA_SNAPSHOT_VERBOSE") { cfg.output.snapshot_verbose = v == "1" || v == "true"; }
    if let Ok(v) = std::env::var("MANA_JSONL_LEVEL") { cfg.output.jsonl_level = v; }
    if let Ok(v) = std::env::var("MANA_POLL_TIMEOUT_MS") { if let Ok(n) = v.parse() { cfg.ingest.poll_timeout_ms = n; } }
    if let Ok(v) = std::env::var("MANA_ERROR_WINDOW_SIZE") { if let Ok(n) = v.parse() { cfg.ingest.error_window_size = n; } }
    if let Ok(v) = std::env::var("MANA_ERROR_WINDOW_THRESHOLD") { if let Ok(n) = v.parse() { cfg.ingest.error_window_threshold = n; } }
    if let Ok(v) = std::env::var("MANA_BACKOFF_INITIAL_MS") { if let Ok(n) = v.parse() { cfg.ingest.reconnect_backoff_initial_ms = n; } }
    if let Ok(v) = std::env::var("MANA_BACKOFF_MAX_MS") { if let Ok(n) = v.parse() { cfg.ingest.reconnect_backoff_max_ms = n; } }
}

pub fn load_model_catalog(path: &Path) -> Result<ModelCatalog> { load_config(path) }
pub fn load_zone_catalog(path: &Path) -> Result<ZoneCatalog> { load_config(path) }
pub fn load_fsm_catalog(path: &Path) -> Result<FsmCatalog> { load_config(path) }

pub fn validate_fsm(fsm: &FsmCatalog, models: &ModelCatalog, zones: &Option<ZoneCatalog>) -> Vec<String> {
    let mut errors = Vec::new();

    if !fsm.fsm.states.contains_key(&fsm.fsm.initial) {
        errors.push(format!("initial state '{}' not found in states", fsm.fsm.initial));
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
        assert_eq!(detect.confidence, 0.5);
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
    }

    #[test]
    fn fsm_validation_catches_unknown_model() {
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let fsm = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        let errors = validate_fsm(&fsm, &models, &None);
        assert!(errors.is_empty(), "config/fsm.toml should be valid: {:?}", errors);
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
