//! Fixed-cadence scene control over an aged process image.

pub mod assignment;
pub mod config;
pub mod domain;
pub mod fsm;
pub mod health;
pub mod kalman;
pub mod occupancy;
pub mod presence;
pub mod scan;
pub mod timing;
pub mod track;
pub mod window;
pub mod zones;

/// Control-side depth policy vocabulary.
pub mod depth {
    pub use super::{
        DepthCalibration, DepthMetric, DepthOp, DepthRegionRule, DepthRegionStats, DepthRuleResult,
        DepthRuleSnapshot, DepthRules,
    };
}

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Narrow observation vocabulary accepted by the control loop.
///
/// Perception-specific masks, model internals, and consolidation provenance do
/// not cross this port.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneObservation {
    pub class: String,
    pub bbox: [f32; 4],
    pub confidence: f32,
    pub source_models: Vec<String>,
    pub face: Option<FaceObservation>,
}

/// Face signal attached by the application adapter when available.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceObservation {
    pub bbox: [f32; 4],
    pub confidence: f32,
}

/// Measurement sample retained by the adapter between control scans.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneSample {
    pub observations: Vec<SceneObservation>,
    pub signal_valid: bool,
    pub raw_person_count: usize,
    pub frame_number: u64,
    pub face_model_ran: bool,
}

impl SceneSample {
    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            observations: Vec::new(),
            signal_valid: false,
            raw_person_count: 0,
            frame_number: 0,
            face_model_ran: false,
        }
    }
}

/// Evidence retained with its monotonic observation time.
#[derive(Debug, Clone)]
pub struct AgedEvidence<T> {
    pub value: T,
    pub observed_at: Instant,
}

impl<T> AgedEvidence<T> {
    #[must_use]
    pub const fn new(value: T, observed_at: Instant) -> Self {
        Self { value, observed_at }
    }

    #[must_use]
    pub fn age_ms(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.observed_at).as_millis() as u64
    }
}

/// Frozen process image consumed by each fixed-cadence control decision.
#[derive(Debug, Clone)]
pub struct ProcessImage {
    pub observations: Option<AgedEvidence<SceneSample>>,
    pub depth: Option<AgedEvidence<DepthRuleSnapshot>>,
    pub measurement_pending: bool,
}

impl ProcessImage {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            observations: None,
            depth: None,
            measurement_pending: false,
        }
    }

    #[must_use]
    pub fn observations_age_ms(&self, now: Instant) -> u64 {
        self.observations
            .as_ref()
            .map_or(u64::MAX, |aged| aged.age_ms(now))
    }

    #[must_use]
    pub fn depth_age_ms(&self, now: Instant) -> Option<u64> {
        self.depth.as_ref().map(|aged| aged.age_ms(now))
    }

    /// Clears depth evidence at a new keyframe: depth is never aged.
    pub fn reset_depth(&mut self, now: Instant) {
        self.depth = Some(AgedEvidence::new(DepthRuleSnapshot::default(), now));
    }

    pub fn set_depth(&mut self, snapshot: DepthRuleSnapshot, now: Instant) {
        self.depth = Some(AgedEvidence::new(snapshot, now));
    }

    #[must_use]
    pub fn depth_snapshot(&self) -> DepthRuleSnapshot {
        self.depth
            .as_ref()
            .map(|aged| aged.value.clone())
            .unwrap_or_default()
    }
}

/// Raw region measurement produced by perception.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthRegionStats {
    pub region: [u32; 4],
    pub valid_pixels: u64,
    pub valid_ratio: Option<f32>,
    pub min_depth_m: Option<f32>,
    pub median_depth_m: Option<f32>,
    pub p10_depth_m: Option<f32>,
    pub p90_depth_m: Option<f32>,
    pub max_depth_m: Option<f32>,
}

/// Metric used by a depth policy rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthMetric {
    Min,
    Median,
    P10,
    P90,
    Max,
}

/// Comparison used by a depth policy rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthOp {
    Lt,
    Gt,
}

/// Optional one-point depth calibration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthCalibration {
    pub reference_model_m: f32,
    pub reference_scene_m: f32,
}

impl DepthCalibration {
    #[must_use]
    pub fn scale(self) -> f32 {
        self.reference_scene_m / self.reference_model_m
    }
}

/// A control-side depth rule; parsing configuration is deliberately external.
#[derive(Debug, Clone)]
pub struct DepthRegionRule {
    pub name: String,
    pub region: [u32; 4],
    pub metric: DepthMetric,
    pub op: DepthOp,
    pub threshold_m: f32,
    pub min_valid_ratio: f32,
    pub calibration: Option<DepthCalibration>,
}

/// Policy result suitable for logging and FSM guards.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthRuleResult {
    pub rule: String,
    pub region: [u32; 4],
    pub metric: DepthMetric,
    pub threshold_m: f32,
    pub value: Option<f32>,
    pub triggered: bool,
    pub valid_pixels: u64,
    pub valid_ratio: Option<f32>,
    pub calibration: Option<DepthCalibration>,
}

/// Control-owned catalog of depth policies.
#[derive(Debug, Clone, Default)]
pub struct DepthRules {
    pub rules: Vec<DepthRegionRule>,
}

impl DepthRules {
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut names = HashSet::new();
        self.rules
            .iter()
            .filter_map(|rule| {
                let [x1, y1, x2, y2] = rule.region;
                if rule.name.is_empty() {
                    Some("depth rule name must not be empty".into())
                } else if x2 <= x1 || y2 <= y1 {
                    Some(format!("rule '{}' has invalid region", rule.name))
                } else if !rule.threshold_m.is_finite() || rule.threshold_m <= 0.0 {
                    Some(format!("rule '{}' has invalid threshold_m", rule.name))
                } else if !(0.0..=1.0).contains(&rule.min_valid_ratio) {
                    Some(format!("rule '{}' has invalid min_valid_ratio", rule.name))
                } else if !names.insert(rule.name.clone()) {
                    Some(format!("duplicate depth rule name '{}'", rule.name))
                } else {
                    None
                }
            })
            .collect()
    }

    #[must_use]
    pub fn evaluate(&self, measurements: &[DepthRegionStats]) -> Vec<DepthRuleResult> {
        self.rules
            .iter()
            .filter_map(|rule| {
                let stats = measurements
                    .iter()
                    .find(|stats| stats.region == rule.region)?;
                if stats
                    .valid_ratio
                    .is_some_and(|ratio| ratio < rule.min_valid_ratio)
                {
                    return None;
                }
                let raw = match rule.metric {
                    DepthMetric::Min => stats.min_depth_m,
                    DepthMetric::Median => stats.median_depth_m,
                    DepthMetric::P10 => stats.p10_depth_m,
                    DepthMetric::P90 => stats.p90_depth_m,
                    DepthMetric::Max => stats.max_depth_m,
                };
                let value =
                    raw.map(|value| value * rule.calibration.map_or(1.0, DepthCalibration::scale));
                let triggered = value.is_some_and(|value| match rule.op {
                    DepthOp::Lt => value < rule.threshold_m,
                    DepthOp::Gt => value > rule.threshold_m,
                });
                Some(DepthRuleResult {
                    rule: rule.name.clone(),
                    region: rule.region,
                    metric: rule.metric,
                    threshold_m: rule.threshold_m,
                    value,
                    triggered,
                    valid_pixels: stats.valid_pixels,
                    valid_ratio: stats.valid_ratio,
                    calibration: rule.calibration,
                })
            })
            .collect()
    }
}

/// Guard-facing snapshot of last evaluated depth policy results.
#[derive(Debug, Clone, Default)]
pub struct DepthRuleSnapshot {
    results: HashMap<String, DepthRuleResult>,
}

impl DepthRuleSnapshot {
    #[must_use]
    pub fn from_results(results: &[DepthRuleResult]) -> Self {
        Self {
            results: results
                .iter()
                .map(|result| (result.rule.clone(), result.clone()))
                .collect(),
        }
    }

    #[must_use]
    pub fn is_triggered(&self, rule: &str) -> Option<bool> {
        self.results.get(rule).map(|result| result.triggered)
    }
}

/// Injectable clock for deterministic scan tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScanInstant(Instant);

impl ScanInstant {
    #[must_use]
    pub const fn from_instant(instant: Instant) -> Self {
        Self(instant)
    }

    #[must_use]
    pub const fn as_instant(self) -> Instant {
        self.0
    }

    #[must_use]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_duration_since(earlier.0)
    }
}
