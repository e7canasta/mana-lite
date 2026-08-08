use serde::de::DeserializeOwned;
use std::path::Path;

use super::app::AppConfig;
use super::env::apply_env_overrides;
use super::fsm::FsmCatalog;
use super::observability::{MetricsLogConfig, RerunBlueprintConfig, VizDataConfig};
use super::zones::ZoneCatalog;
use crate::depth::DepthRules;
use crate::error::{ConfigError, Result};

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

pub fn load_zone_catalog(path: &Path) -> Result<ZoneCatalog> {
    load_config(path)
}

pub fn load_fsm_catalog(path: &Path) -> Result<FsmCatalog> {
    load_config(path)
}

pub fn load_depth_rules(path: &Path) -> Result<DepthRules> {
    load_config(path)
}

pub fn load_viz_data(path: &Path) -> Result<VizDataConfig> {
    load_config(path)
}

pub fn load_metrics_log(path: &Path) -> Result<MetricsLogConfig> {
    load_config(path)
}

pub fn load_rerun_blueprint(path: &Path) -> Result<RerunBlueprintConfig> {
    load_config(path)
}
