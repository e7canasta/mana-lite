use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::error::{ConfigError, Result};

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub source: SourceConfig,
    pub inference: InferenceConfig,
    pub health: HealthConfig,
    #[serde(default)]
    pub output: OutputConfig,
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
    #[serde(default = "default_heartbeat_cycles")]
    pub heartbeat_every_n_cycles: u64,
}

fn default_data_stale_ms() -> u64 { 10_000 }
fn default_max_panics() -> u32 { 3 }
fn default_heartbeat_cycles() -> u64 { 100 }

#[derive(Debug, Default, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub save_dir: Option<PathBuf>,
    #[serde(default)]
    pub rotate: Option<String>,
}

fn default_format() -> String { "jsonl".into() }

// ── Model Catalog ──

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

// ── Zone Catalog ──

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

// ── FSM Catalog ──

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
        #[serde(default = "default_confidence")]
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

// ── Config loading ──

pub fn load_app_config(path: &Path) -> Result<AppConfig> {
    let content = std::fs::read_to_string(path)
        .map_err(|_| ConfigError::FileNotFound(path.display().to_string()))?;
    toml::from_str(&content)
        .map_err(|e| ConfigError::ParseError { file: path.display().to_string(), msg: e.to_string() })
        .map_err(Into::into)
}

pub fn load_model_catalog(path: &Path) -> Result<ModelCatalog> {
    let content = std::fs::read_to_string(path)
        .map_err(|_| ConfigError::FileNotFound(path.display().to_string()))?;
    toml::from_str(&content)
        .map_err(|e| ConfigError::ParseError { file: path.display().to_string(), msg: e.to_string() })
        .map_err(Into::into)
}

pub fn load_zone_catalog(path: &Path) -> Result<ZoneCatalog> {
    let content = std::fs::read_to_string(path)
        .map_err(|_| ConfigError::FileNotFound(path.display().to_string()))?;
    toml::from_str(&content)
        .map_err(|e| ConfigError::ParseError { file: path.display().to_string(), msg: e.to_string() })
        .map_err(Into::into)
}

pub fn load_fsm_catalog(path: &Path) -> Result<FsmCatalog> {
    let content = std::fs::read_to_string(path)
        .map_err(|_| ConfigError::FileNotFound(path.display().to_string()))?;
    toml::from_str(&content)
        .map_err(|e| ConfigError::ParseError { file: path.display().to_string(), msg: e.to_string() })
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_model_catalog() {
        let catalog = load_model_catalog(Path::new("config/models.toml")).unwrap();
        assert!(catalog.models.contains_key("detect-fast"));
        assert!(catalog.models.contains_key("pose-standard"));

        let detect = &catalog.models["detect-fast"];
        assert_eq!(detect.task, "detect");
        assert_eq!(detect.confidence, 0.5);
        assert_eq!(detect.imgsz, Some(320));
    }

    #[test]
    fn test_load_zone_catalog() {
        let catalog = load_zone_catalog(Path::new("config/zones.toml")).unwrap();
        assert!(catalog.zones.contains_key("bed"));
        assert!(catalog.zones.contains_key("chair"));
        assert_eq!(catalog.zones["bed"].hysteresis_ms, 500);
    }

    #[test]
    fn test_load_fsm_catalog() {
        let catalog = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        assert_eq!(catalog.fsm.initial, "idle");
        assert!(catalog.fsm.states.contains_key("idle"));
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
}
