use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

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

#[derive(Debug, Clone, Deserialize)]
pub struct CascadeRule {
    pub model: String,
    pub requires: Option<String>,
    pub requires_class: Option<String>,
    #[serde(default)]
    pub requires_exact_count: Option<usize>,
    #[serde(default)]
    pub same_frame: bool,
    #[serde(default)]
    pub requires_min_confidence: Option<f32>,
    #[serde(default)]
    pub requires_min_area_ratio: Option<f32>,
    #[serde(default)]
    pub requires_region: Option<String>,
    #[serde(default)]
    pub requires_region_coverage: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SemanticRegion {
    pub rect: [f32; 4],
    #[serde(default)]
    #[allow(dead_code)]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CascadeConfig {
    pub rules: Vec<CascadeRule>,
    #[serde(default)]
    pub regions: HashMap<String, SemanticRegion>,
}
