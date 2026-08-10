//! Depth rule configuration loading for the binary.
//!
//! Perception owns [`mana_perception::region_stats`]; control owns
//! [`mana_control::DepthRules`] policy evaluation. This module only parses
//! TOML into control types.

use serde::Deserialize;

use crate::error::{ConfigError, Result};

/// Version del esquema del evento `depth_region`.
pub const DEPTH_REGION_EVENT_VERSION: u8 = 2;

#[derive(Debug, Deserialize)]
struct DepthRulesFile {
    #[serde(default)]
    rules: Vec<DepthRegionRuleFile>,
}

#[derive(Debug, Deserialize)]
struct DepthRegionRuleFile {
    name: String,
    region: [u32; 4],
    #[serde(default = "default_metric")]
    metric: DepthMetricFile,
    #[serde(default = "default_op")]
    op: DepthOpFile,
    threshold_m: f32,
    #[serde(default = "default_min_valid_ratio")]
    min_valid_ratio: f32,
    #[serde(default)]
    calibration: Option<DepthCalibrationFile>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DepthMetricFile {
    Min,
    Median,
    P10,
    P90,
    Max,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DepthOpFile {
    Lt,
    Gt,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct DepthCalibrationFile {
    reference_model_m: f32,
    reference_scene_m: f32,
}

const fn default_metric() -> DepthMetricFile {
    DepthMetricFile::Median
}

const fn default_op() -> DepthOpFile {
    DepthOpFile::Lt
}

const fn default_min_valid_ratio() -> f32 {
    0.5
}

impl From<DepthMetricFile> for mana_control::DepthMetric {
    fn from(value: DepthMetricFile) -> Self {
        match value {
            DepthMetricFile::Min => Self::Min,
            DepthMetricFile::Median => Self::Median,
            DepthMetricFile::P10 => Self::P10,
            DepthMetricFile::P90 => Self::P90,
            DepthMetricFile::Max => Self::Max,
        }
    }
}

impl From<DepthOpFile> for mana_control::DepthOp {
    fn from(value: DepthOpFile) -> Self {
        match value {
            DepthOpFile::Lt => Self::Lt,
            DepthOpFile::Gt => Self::Gt,
        }
    }
}

impl From<DepthCalibrationFile> for mana_control::DepthCalibration {
    fn from(value: DepthCalibrationFile) -> Self {
        Self {
            reference_model_m: value.reference_model_m,
            reference_scene_m: value.reference_scene_m,
        }
    }
}

impl From<DepthRegionRuleFile> for mana_control::DepthRegionRule {
    fn from(value: DepthRegionRuleFile) -> Self {
        Self {
            name: value.name,
            region: value.region,
            metric: value.metric.into(),
            op: value.op.into(),
            threshold_m: value.threshold_m,
            min_valid_ratio: value.min_valid_ratio,
            calibration: value.calibration.map(Into::into),
        }
    }
}

/// Parses depth-rules TOML into control-owned [`mana_control::DepthRules`].
pub fn parse_depth_rules(content: &str) -> Result<mana_control::DepthRules> {
    let file: DepthRulesFile = toml::from_str(content).map_err(|e| ConfigError::ParseError {
        file: "depth-rules".into(),
        msg: e.to_string(),
    })?;
    Ok(mana_control::DepthRules {
        rules: file.rules.into_iter().map(Into::into).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mana_control::{DepthMetric, DepthOp};

    #[test]
    fn configured_depth_rules_file_is_valid() {
        let content = std::fs::read_to_string("config/depth-rules.toml").expect("depth rules file");
        let rules = parse_depth_rules(&content).expect("parse depth rules");
        assert!(rules.validate().is_empty());
        assert_eq!(rules.rules.len(), 1);
        assert_eq!(rules.rules[0].name, "bed-approach");
        assert_eq!(rules.rules[0].metric, DepthMetric::Median);
        assert_eq!(rules.rules[0].op, DepthOp::Lt);
    }

    #[test]
    fn parse_rejects_invalid_toml_structure() {
        let err = parse_depth_rules("rules = 1").expect_err("must fail");
        let _ = err;
    }
}
