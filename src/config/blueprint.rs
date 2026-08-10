use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

pub use mana_perception::cascade::{CascadeConfig, CascadeRule, SemanticRegion};

#[derive(Debug, Clone, Deserialize)]
pub struct BlueprintConfig {
    pub blueprint: BlueprintMetadata,
    #[serde(default)]
    pub rules: Vec<CascadeRule>,
    #[serde(default)]
    pub regions: HashMap<String, SemanticRegion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlueprintMetadata {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub primary_model: String,
    pub models: Vec<String>,
    #[serde(default)]
    pub requires_tracking: bool,
    /// Optional model-parameter overlay, resolved relative to this blueprint.
    #[serde(default)]
    pub model_overlay: Option<PathBuf>,
}
