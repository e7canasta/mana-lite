//! Offline posture-profile loading and validation.
//!
//! This module loads validated posture profiles and scores offline diagnostic
//! reports. It does not run models or publish runtime decisions; the CLI owns
//! file IO and the runtime remains isolated from this evaluator.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

use crate::error::{ConfigError, ManaError, Result};
use crate::surface_calibration::{SurfaceCalibration, SurfaceZone};
use serde_json::Value;

pub const POSTURE_ANALYSIS_SCHEMA_VERSION: u32 = 1;
const VALID_COMPONENT_COUNT: usize = 5;
const VALID_SOURCES: [&str; 7] = [
    "geometry",
    "keypoint",
    "keypoint_group",
    "face",
    "segment",
    "body_part",
    "surface",
];
const HEAD_KEYPOINTS: [&str; 5] = ["nose", "left_eye", "right_eye", "left_ear", "right_ear"];
const SHOULDER_KEYPOINTS: [&str; 2] = ["left_shoulder", "right_shoulder"];
const HIP_KEYPOINTS: [&str; 2] = ["left_hip", "right_hip"];
const VALID_BASE_POSTURES: [&str; 4] = [
    "acostado",
    "sentado-in-bed",
    "sentado-aside",
    "standby-aside",
];
const VALID_PLANES: [&str; 2] = ["in-bed", "aside-bed"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostureMaster {
    pub schema_version: u32,
    pub engine: String,
    pub model_key: String,
    pub depth_semantics: String,
    pub surface_calibration: String,
    pub posture_dir: String,
    pub min_observed_components: usize,
    pub min_total_score: f32,
    #[serde(default = "default_semantic_min_total_score")]
    pub semantic_min_total_score: f32,
    pub ambiguity_margin: f32,
    #[serde(default = "default_surface_spatial_padding_px")]
    pub surface_spatial_padding_px: f32,
    #[serde(default = "default_surface_depth_padding_m")]
    pub surface_depth_padding_m: f32,
    #[serde(default = "default_master_policy")]
    pub policy: MasterPolicy,
    pub postures: Vec<PostureReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MasterPolicy {
    #[serde(default)]
    pub missing_is_conflict: bool,
    #[serde(default = "default_true")]
    pub allow_partial_parts: bool,
    #[serde(default = "default_true")]
    pub require_same_model_context: bool,
    #[serde(default = "default_true")]
    pub require_same_roi: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostureReference {
    pub id: String,
    pub label: String,
    pub file: String,
    #[serde(default = "default_weight")]
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostureProfile {
    pub schema_version: u32,
    pub posture_id: String,
    pub label: String,
    pub training_sample: String,
    pub base_posture: String,
    pub plane: String,
    #[serde(default = "default_posture_policy")]
    pub policy: PosturePolicy,
    pub features: Vec<PostureFeature>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PosturePolicy {
    #[serde(default = "default_min_observed_features")]
    pub min_observed_features: usize,
    #[serde(default = "default_min_observed_components")]
    pub min_observed_components: usize,
    #[serde(default = "default_true")]
    pub allow_partial: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostureFeature {
    pub id: String,
    pub source: String,
    pub field: String,
    #[serde(default)]
    pub part: Option<String>,
    #[serde(default)]
    pub center: Option<f32>,
    #[serde(default)]
    pub tolerance: Option<f32>,
    #[serde(default)]
    pub allowed: Vec<String>,
    #[serde(default = "default_weight")]
    pub weight: f32,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedPostureProfiles {
    pub master: PostureMaster,
    pub surface_calibration: SurfaceCalibration,
    pub profiles: Vec<PostureProfile>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnalysisReport {
    pub schema_version: u32,
    pub engine: String,
    pub image: String,
    pub model_key: String,
    pub depth_semantics: String,
    pub calibration: AnalysisCalibration,
    pub decision: DecisionReport,
    pub semantic_decision: SemanticDecisionReport,
    pub candidates: Vec<CandidateReport>,
    pub semantic_candidates: Vec<SemanticCandidateReport>,
    pub observations: BTreeMap<String, FeatureObservation>,
    pub missing: Vec<String>,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnalysisCalibration {
    pub session: String,
    pub model_key: String,
    pub roi: [u32; 4],
    pub surface_spatial_padding_px: f32,
    pub surface_depth_padding_m: f32,
    pub compatible: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Classified,
    Ambiguous,
    Unknown,
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionReport {
    pub status: DecisionStatus,
    pub posture_id: Option<String>,
    pub label: Option<String>,
    pub base_posture: Option<String>,
    pub plane: Option<String>,
    pub score: Option<f32>,
    pub margin: Option<f32>,
    pub observed_components: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SemanticDecisionReport {
    pub status: DecisionStatus,
    pub base_posture: Option<String>,
    pub plane: Option<String>,
    pub score: Option<f32>,
    pub margin: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CandidateReport {
    pub posture_id: String,
    pub label: String,
    pub base_posture: String,
    pub plane: String,
    pub score: f32,
    pub observed_features: usize,
    pub observed_components: usize,
    pub quorum: bool,
    pub missing: Vec<String>,
    pub conflicts: Vec<String>,
    pub attention: BTreeMap<String, AttentionReport>,
    pub components: BTreeMap<String, ComponentReport>,
    pub features: Vec<FeatureScoreReport>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AttentionReport {
    pub status: ObservationStatus,
    pub score: Option<f32>,
    pub observed_features: usize,
    pub effective_weight: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SemanticCandidateReport {
    pub base_posture: String,
    pub plane: String,
    pub score: f32,
    pub quorum: bool,
    pub source_postures: Vec<String>,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComponentReport {
    pub status: ObservationStatus,
    pub score: Option<f32>,
    pub observed_features: usize,
    pub effective_weight: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureScoreReport {
    pub id: String,
    pub status: ObservationStatus,
    pub observed: Option<FeatureValue>,
    pub support: Option<f32>,
    pub quality: Option<f32>,
    pub weight: f32,
    pub effective_weight: f32,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Observed,
    Partial,
    Missing,
    Invalid,
    Stale,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum FeatureValue {
    Number(f32),
    Category(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureObservation {
    pub component: String,
    pub status: ObservationStatus,
    pub value: Option<FeatureValue>,
    pub quality: f32,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
struct NormalizedObservation {
    image: String,
    compatible: bool,
    compatibility_reasons: Vec<String>,
    features: BTreeMap<String, FeatureObservation>,
}

#[derive(Debug, Default)]
struct ComponentAccumulator {
    numerator: f32,
    denominator: f32,
    effective_weight: f32,
    observed_features: usize,
    partial: bool,
    conflict: bool,
}

#[derive(Clone)]
struct KeypointObservation {
    point: [f32; 2],
    normalized: [f32; 2],
    confidence: f32,
    depth_m: Option<f32>,
    zone: Option<String>,
    delta_m: Option<f32>,
}

#[derive(Clone)]
struct SurfacePoint {
    point: [f32; 2],
    quality: f32,
    weight: f32,
    zone: Option<String>,
    delta_m: Option<f32>,
}

#[derive(Default)]
struct SurfaceAnchorEvidence {
    bed_support: f32,
    floor_support: f32,
    zone_index: f32,
    depth_fit: f32,
    quality: f32,
    zone: Option<String>,
}

impl PostureMaster {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.schema_version != POSTURE_ANALYSIS_SCHEMA_VERSION {
            return Err(format!(
                "unsupported schema_version {}; expected {}",
                self.schema_version, POSTURE_ANALYSIS_SCHEMA_VERSION
            ));
        }
        if self.engine != "posture-analysis" {
            return Err(format!(
                "engine must be 'posture-analysis', got '{}'",
                self.engine
            ));
        }
        validate_identifier("model_key", &self.model_key)?;
        if self.depth_semantics != "model-relative" {
            return Err(format!(
                "depth_semantics must be 'model-relative', got '{}'",
                self.depth_semantics
            ));
        }
        validate_relative_path("surface_calibration", &self.surface_calibration)?;
        validate_relative_path("posture_dir", &self.posture_dir)?;
        validate_component_quorum("min_observed_components", self.min_observed_components)?;
        validate_unit_interval("min_total_score", self.min_total_score)?;
        validate_unit_interval("semantic_min_total_score", self.semantic_min_total_score)?;
        validate_unit_interval("ambiguity_margin", self.ambiguity_margin)?;
        validate_positive_finite(
            "surface_spatial_padding_px",
            self.surface_spatial_padding_px,
        )?;
        validate_positive_finite("surface_depth_padding_m", self.surface_depth_padding_m)?;
        if self.postures.is_empty() {
            return Err("postures must not be empty".into());
        }

        let mut ids = HashSet::new();
        let mut files = HashSet::new();
        for posture in &self.postures {
            validate_identifier("posture id", &posture.id)?;
            validate_non_empty("posture label", &posture.label)?;
            validate_profile_file(&posture.file)?;
            validate_positive_finite("posture weight", posture.weight)?;
            if !ids.insert(&posture.id) {
                return Err(format!("duplicate posture id '{}'", posture.id));
            }
            if !files.insert(&posture.file) {
                return Err(format!("duplicate posture file '{}'", posture.file));
            }
        }
        Ok(())
    }
}

impl PostureProfile {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.schema_version != POSTURE_ANALYSIS_SCHEMA_VERSION {
            return Err(format!(
                "unsupported schema_version {}; expected {}",
                self.schema_version, POSTURE_ANALYSIS_SCHEMA_VERSION
            ));
        }
        validate_identifier("posture_id", &self.posture_id)?;
        validate_non_empty("posture label", &self.label)?;
        validate_non_empty("training_sample", &self.training_sample)?;
        if !VALID_BASE_POSTURES.contains(&self.base_posture.as_str()) {
            return Err(format!(
                "posture '{}' has unsupported base_posture '{}'; expected one of {}",
                self.posture_id,
                self.base_posture,
                VALID_BASE_POSTURES.join(", ")
            ));
        }
        if !VALID_PLANES.contains(&self.plane.as_str()) {
            return Err(format!(
                "posture '{}' has unsupported plane '{}'; expected one of {}",
                self.posture_id,
                self.plane,
                VALID_PLANES.join(", ")
            ));
        }
        if self.policy.min_observed_features == 0 {
            return Err("policy.min_observed_features must be positive".into());
        }
        validate_component_quorum(
            "policy.min_observed_components",
            self.policy.min_observed_components,
        )?;
        if self.features.is_empty() {
            return Err("features must not be empty".into());
        }
        if self.policy.min_observed_features > self.features.len() {
            return Err(format!(
                "policy.min_observed_features {} exceeds feature count {}",
                self.policy.min_observed_features,
                self.features.len()
            ));
        }

        let mut ids = HashSet::new();
        for feature in &self.features {
            validate_non_empty("feature id", &feature.id)?;
            validate_non_empty("feature field", &feature.field)?;
            if !ids.insert(&feature.id) {
                return Err(format!("duplicate feature id '{}'", feature.id));
            }
            if !VALID_SOURCES.contains(&feature.source.as_str()) {
                return Err(format!(
                    "feature '{}' has unsupported source '{}'",
                    feature.id, feature.source
                ));
            }
            if feature.source == "body_part" {
                let Some(part) = feature.part.as_deref() else {
                    return Err(format!(
                        "feature '{}' from body_part requires part",
                        feature.id
                    ));
                };
                validate_non_empty("body_part feature part", part)?;
            }
            validate_positive_finite("feature weight", feature.weight)?;
            validate_feature_value(feature)?;
        }
        Ok(())
    }
}

impl LoadedPostureProfiles {
    #[must_use]
    pub fn profile(&self, posture_id: &str) -> Option<&PostureProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.posture_id == posture_id)
    }
}

pub fn parse_master(contents: &str) -> std::result::Result<PostureMaster, String> {
    let master: PostureMaster = toml::from_str(contents).map_err(|error| error.to_string())?;
    master.validate()?;
    Ok(master)
}

pub fn parse_profile(contents: &str) -> std::result::Result<PostureProfile, String> {
    let profile: PostureProfile = toml::from_str(contents).map_err(|error| error.to_string())?;
    profile.validate()?;
    Ok(profile)
}

pub fn load_master(path: &Path) -> Result<PostureMaster> {
    let contents = read_toml(path)?;
    parse_master(&contents).map_err(|message| validation_error(path, message))
}

pub fn load_profile_set(master_path: &Path) -> Result<LoadedPostureProfiles> {
    let master = load_master(master_path)?;
    let master_dir = master_path.parent().unwrap_or_else(|| Path::new("."));
    let calibration_path = master_dir.join(&master.surface_calibration);
    let surface_calibration = crate::config::load_surface_calibration(&calibration_path)?;
    if surface_calibration.model_key != master.model_key {
        return Err(validation_error(
            master_path,
            format!(
                "surface calibration model_key '{}' does not match master '{}'",
                surface_calibration.model_key, master.model_key
            ),
        ));
    }
    let profile_dir = master_dir.join(&master.posture_dir);
    let mut profiles = Vec::with_capacity(master.postures.len());

    for reference in &master.postures {
        let profile_path = profile_dir.join(&reference.file);
        let profile = load_profile(&profile_path)?;
        if profile.posture_id != reference.id {
            return Err(validation_error(
                &profile_path,
                format!(
                    "posture_id '{}' does not match master reference '{}'",
                    profile.posture_id, reference.id
                ),
            ));
        }
        if profile.label != reference.label {
            return Err(validation_error(
                &profile_path,
                format!(
                    "label '{}' does not match master reference '{}'",
                    profile.label, reference.label
                ),
            ));
        }
        profiles.push(profile);
    }

    Ok(LoadedPostureProfiles {
        master,
        surface_calibration,
        profiles,
    })
}

fn load_profile(path: &Path) -> Result<PostureProfile> {
    let contents = read_toml(path)?;
    parse_profile(&contents).map_err(|message| validation_error(path, message))
}

pub fn analyze_reports(
    master_path: &Path,
    radio_path: &Path,
    parts_path: &Path,
) -> Result<AnalysisReport> {
    let loaded = load_profile_set(master_path)?;
    let observation = normalize_reports(&loaded, radio_path, parts_path)?;
    let mut candidates = loaded
        .profiles
        .iter()
        .map(|profile| score_profile(profile, &loaded.master, &observation))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.posture_id.cmp(&right.posture_id))
    });

    let decision = decide(&loaded.master, &observation, &candidates);
    let semantic_candidates = aggregate_semantic_candidates(&candidates);
    let semantic_decision = decide_semantic(&loaded.master, &observation, &semantic_candidates);
    let missing = candidates
        .first()
        .map_or_else(Vec::new, |candidate| candidate.missing.clone());
    let conflicts = candidates
        .first()
        .map_or_else(Vec::new, |candidate| candidate.conflicts.clone());

    Ok(AnalysisReport {
        schema_version: loaded.master.schema_version,
        engine: loaded.master.engine.clone(),
        image: observation.image,
        model_key: loaded.master.model_key.clone(),
        depth_semantics: loaded.master.depth_semantics.clone(),
        calibration: AnalysisCalibration {
            session: master_path.display().to_string(),
            model_key: loaded.surface_calibration.model_key.clone(),
            roi: loaded.surface_calibration.roi,
            surface_spatial_padding_px: loaded.master.surface_spatial_padding_px,
            surface_depth_padding_m: loaded.master.surface_depth_padding_m,
            compatible: observation.compatible,
            reasons: observation.compatibility_reasons,
        },
        decision,
        semantic_decision,
        candidates,
        semantic_candidates,
        observations: observation.features,
        missing,
        conflicts,
    })
}

fn normalize_reports(
    loaded: &LoadedPostureProfiles,
    radio_path: &Path,
    parts_path: &Path,
) -> Result<NormalizedObservation> {
    let radio = read_json(radio_path)?;
    let parts = read_json(parts_path)?;
    let model_key = required_string(&radio, "model_key", radio_path)?;
    let parts_model_key = required_string(&parts, "model_key", parts_path)?;
    let depth_roi = required_roi(&radio, "depth_roi", radio_path)?;
    let image = radio
        .get("image")
        .and_then(Value::as_str)
        .map_or_else(|| radio_path.display().to_string(), str::to_owned);
    let mut compatibility_reasons = Vec::new();

    if model_key != loaded.master.model_key {
        compatibility_reasons.push(format!(
            "radio model_key '{}' does not match master '{}'",
            model_key, loaded.master.model_key
        ));
    }
    if parts_model_key != loaded.master.model_key {
        compatibility_reasons.push(format!(
            "parts model_key '{}' does not match master '{}'",
            parts_model_key, loaded.master.model_key
        ));
    }
    if depth_roi != loaded.surface_calibration.roi {
        compatibility_reasons.push(format!(
            "radio ROI {depth_roi:?} does not match calibration ROI {:?}",
            loaded.surface_calibration.roi
        ));
    }

    let mut features = BTreeMap::new();
    let person_bbox = required_bbox(&radio, "person_bbox", radio_path)?;
    add_geometry_features(
        &mut features,
        &radio,
        person_bbox,
        &loaded.surface_calibration,
        loaded.master.surface_spatial_padding_px,
        loaded.master.surface_depth_padding_m,
    )?;
    add_face_features(&mut features, &radio, person_bbox);
    add_segment_features(&mut features, &radio);
    add_body_part_features(&mut features, &parts, depth_roi, &mut compatibility_reasons);

    Ok(NormalizedObservation {
        image,
        compatible: compatibility_reasons.is_empty(),
        compatibility_reasons,
        features,
    })
}

fn score_profile(
    profile: &PostureProfile,
    master: &PostureMaster,
    observation: &NormalizedObservation,
) -> CandidateReport {
    let mut accumulators = BTreeMap::<String, ComponentAccumulator>::new();
    let mut features = Vec::with_capacity(profile.features.len());
    let mut missing = Vec::new();
    let mut conflicts = Vec::new();
    let mut reasons = Vec::new();
    let mut required_unavailable = false;
    let mut total_numerator = 0.0;
    let mut total_denominator = 0.0;
    let mut observed_features = 0;

    for feature in &profile.features {
        let component = feature_component(feature).to_string();
        let accumulator = accumulators.entry(component).or_default();
        let Some(observed) = observation.features.get(&feature.id) else {
            missing.push(feature.id.clone());
            if feature.required {
                required_unavailable = true;
                reasons.push(format!("required feature missing: {}", feature.id));
            }
            features.push(FeatureScoreReport {
                id: feature.id.clone(),
                status: ObservationStatus::Missing,
                observed: None,
                support: None,
                quality: None,
                weight: feature.weight,
                effective_weight: 0.0,
                reason: Some("feature not observed".into()),
            });
            continue;
        };

        if observed.status == ObservationStatus::Conflict {
            accumulator.conflict = true;
            required_unavailable |= feature.required;
            conflicts.push(format!("feature conflict: {}", feature.id));
            features.push(FeatureScoreReport {
                id: feature.id.clone(),
                status: observed.status.clone(),
                observed: observed.value.clone(),
                support: None,
                quality: Some(observed.quality),
                weight: feature.weight,
                effective_weight: 0.0,
                reason: observed.reason.clone(),
            });
            continue;
        }

        if matches!(
            observed.status,
            ObservationStatus::Invalid | ObservationStatus::Stale
        ) {
            accumulator.partial = true;
            missing.push(feature.id.clone());
            if feature.required {
                required_unavailable = true;
                reasons.push(format!("required feature unavailable: {}", feature.id));
            }
            features.push(FeatureScoreReport {
                id: feature.id.clone(),
                status: observed.status.clone(),
                observed: observed.value.clone(),
                support: None,
                quality: Some(observed.quality),
                weight: feature.weight,
                effective_weight: 0.0,
                reason: observed.reason.clone(),
            });
            continue;
        }

        let Some((support, support_reason)) = feature_support(feature, observed) else {
            accumulator.partial = true;
            missing.push(feature.id.clone());
            required_unavailable |= feature.required;
            reasons.push(format!("feature type mismatch: {}", feature.id));
            features.push(FeatureScoreReport {
                id: feature.id.clone(),
                status: ObservationStatus::Invalid,
                observed: observed.value.clone(),
                support: None,
                quality: Some(observed.quality),
                weight: feature.weight,
                effective_weight: 0.0,
                reason: Some("observed value type does not match profile".into()),
            });
            continue;
        };

        let quality = observed.quality.clamp(0.0, 1.0);
        let attention_weight = attention_weight(&feature.id);
        let effective_weight = feature.weight * quality * attention_weight;
        let weighted_support = feature.weight * support * quality * attention_weight;
        accumulator.numerator += weighted_support;
        accumulator.denominator += feature.weight * attention_weight;
        accumulator.effective_weight += effective_weight;
        accumulator.observed_features += 1;
        accumulator.partial |= observed.status == ObservationStatus::Partial;
        total_numerator += weighted_support;
        total_denominator += feature.weight * attention_weight;
        observed_features += 1;
        if let Some(reason) = support_reason.as_ref() {
            reasons.push(format!("{}: {reason}", feature.id));
        }
        features.push(FeatureScoreReport {
            id: feature.id.clone(),
            status: observed.status.clone(),
            observed: observed.value.clone(),
            support: Some(support),
            quality: Some(quality),
            weight: feature.weight,
            effective_weight,
            reason: support_reason.or_else(|| observed.reason.clone()),
        });
    }

    let components = accumulators
        .into_iter()
        .map(|(name, accumulator)| {
            let status = if accumulator.conflict {
                ObservationStatus::Conflict
            } else if accumulator.observed_features == 0 {
                ObservationStatus::Missing
            } else if accumulator.partial {
                ObservationStatus::Partial
            } else {
                ObservationStatus::Observed
            };
            let score = (accumulator.denominator > 0.0)
                .then(|| accumulator.numerator / accumulator.denominator);
            (
                name,
                ComponentReport {
                    status,
                    score,
                    observed_features: accumulator.observed_features,
                    effective_weight: accumulator.effective_weight,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let observed_components = components
        .values()
        .filter(|component| component.observed_features > 0)
        .count();
    let score = if total_denominator > 0.0 {
        total_numerator / total_denominator
    } else {
        0.0
    };
    let quorum = observed_features >= profile.policy.min_observed_features
        && observed_components >= profile.policy.min_observed_components
        && observed_components >= master.min_observed_components
        && !required_unavailable;
    if !quorum {
        reasons.push(format!(
            "quorum not met: features {observed_features}/{}, components {observed_components}/{}",
            profile.policy.min_observed_features.max(1),
            profile
                .policy
                .min_observed_components
                .max(master.min_observed_components)
        ));
    }
    if score < master.min_total_score {
        reasons.push(format!(
            "score {:.3} below minimum {:.3}",
            score, master.min_total_score
        ));
    }

    CandidateReport {
        posture_id: profile.posture_id.clone(),
        label: profile.label.clone(),
        base_posture: profile.base_posture.clone(),
        plane: profile.plane.clone(),
        score,
        observed_features,
        observed_components,
        quorum,
        missing,
        conflicts,
        attention: build_attention_reports(&features),
        components,
        features,
        reasons,
    }
}

fn decide(
    master: &PostureMaster,
    observation: &NormalizedObservation,
    candidates: &[CandidateReport],
) -> DecisionReport {
    let Some(best) = candidates.first() else {
        return DecisionReport {
            status: DecisionStatus::Unknown,
            posture_id: None,
            label: None,
            base_posture: None,
            plane: None,
            score: None,
            margin: None,
            observed_components: 0,
        };
    };
    let margin = candidates
        .get(1)
        .map_or(best.score, |second| (best.score - second.score).max(0.0));
    if !observation.compatible {
        return DecisionReport {
            status: DecisionStatus::Incompatible,
            posture_id: None,
            label: None,
            base_posture: None,
            plane: None,
            score: Some(best.score),
            margin: Some(margin),
            observed_components: best.observed_components,
        };
    }
    let status =
        if !best.quorum || best.score < master.min_total_score || !best.conflicts.is_empty() {
            DecisionStatus::Unknown
        } else if margin < master.ambiguity_margin {
            DecisionStatus::Ambiguous
        } else {
            DecisionStatus::Classified
        };
    let classified = status == DecisionStatus::Classified;
    DecisionReport {
        status,
        posture_id: classified.then(|| best.posture_id.clone()),
        label: classified.then(|| best.label.clone()),
        base_posture: classified.then(|| best.base_posture.clone()),
        plane: classified.then(|| best.plane.clone()),
        score: Some(best.score),
        margin: Some(margin),
        observed_components: best.observed_components,
    }
}

#[derive(Default)]
struct SemanticAccumulator {
    score: f32,
    quorum: bool,
    source_postures: Vec<String>,
    conflicts: Vec<String>,
}

fn aggregate_semantic_candidates(candidates: &[CandidateReport]) -> Vec<SemanticCandidateReport> {
    let mut grouped = BTreeMap::<(String, String), SemanticAccumulator>::new();
    for candidate in candidates {
        let key = (candidate.base_posture.clone(), candidate.plane.clone());
        let entry = grouped.entry(key).or_default();
        entry.score = entry.score.max(candidate.score);
        entry.quorum |= candidate.quorum;
        entry.source_postures.push(candidate.posture_id.clone());
        if candidate.score >= entry.score {
            entry.conflicts = candidate.conflicts.clone();
        }
    }

    let mut output = grouped
        .into_iter()
        .map(|((base_posture, plane), mut accumulator)| {
            accumulator.source_postures.sort();
            accumulator.source_postures.dedup();
            accumulator.conflicts.sort();
            accumulator.conflicts.dedup();
            SemanticCandidateReport {
                base_posture,
                plane,
                score: accumulator.score,
                quorum: accumulator.quorum,
                source_postures: accumulator.source_postures,
                conflicts: accumulator.conflicts,
            }
        })
        .collect::<Vec<_>>();
    output.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.base_posture.cmp(&right.base_posture))
            .then_with(|| left.plane.cmp(&right.plane))
    });
    output
}

fn decide_semantic(
    master: &PostureMaster,
    observation: &NormalizedObservation,
    candidates: &[SemanticCandidateReport],
) -> SemanticDecisionReport {
    let Some(best) = candidates.first() else {
        return SemanticDecisionReport {
            status: DecisionStatus::Unknown,
            base_posture: None,
            plane: None,
            score: None,
            margin: None,
        };
    };
    let margin = candidates
        .get(1)
        .map_or(best.score, |second| (best.score - second.score).max(0.0));
    if !observation.compatible {
        return SemanticDecisionReport {
            status: DecisionStatus::Incompatible,
            base_posture: None,
            plane: None,
            score: Some(best.score),
            margin: Some(margin),
        };
    }
    let status = if !best.quorum
        || best.score < master.semantic_min_total_score
        || !best.conflicts.is_empty()
    {
        DecisionStatus::Unknown
    } else if margin < master.ambiguity_margin {
        DecisionStatus::Ambiguous
    } else {
        DecisionStatus::Classified
    };
    let classified = status == DecisionStatus::Classified;
    SemanticDecisionReport {
        status,
        base_posture: classified.then(|| best.base_posture.clone()),
        plane: classified.then(|| best.plane.clone()),
        score: Some(best.score),
        margin: Some(margin),
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let contents = std::fs::read_to_string(path).map_err(|error| {
        ManaError::Config(ConfigError::FileNotFound(format!(
            "{} ({error})",
            path.display()
        )))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        ManaError::Config(ConfigError::ParseError {
            file: path.display().to_string(),
            msg: error.to_string(),
        })
        .into()
    })
}

fn required_string(value: &Value, field: &str, path: &Path) -> Result<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| validation_error(path, format!("missing string field '{field}'")))
}

fn required_bbox(value: &Value, field: &str, path: &Path) -> Result<[f32; 4]> {
    let Some(bbox) = value.get(field).and_then(Value::as_array) else {
        return Err(validation_error(
            path,
            format!("missing array field '{field}'"),
        ));
    };
    let Some(bbox) = array_as_f32::<4>(bbox) else {
        return Err(validation_error(
            path,
            format!("field '{field}' must contain four finite numbers"),
        ));
    };
    if bbox[2] <= bbox[0] || bbox[3] <= bbox[1] {
        return Err(validation_error(
            path,
            format!("field '{field}' has an invalid rectangle"),
        ));
    }
    Ok(bbox)
}

fn required_roi(value: &Value, field: &str, path: &Path) -> Result<[u32; 4]> {
    let Some(roi) = value.get(field).and_then(Value::as_array) else {
        return Err(validation_error(
            path,
            format!("missing array field '{field}'"),
        ));
    };
    let Some(roi) = array_as_u32::<4>(roi) else {
        return Err(validation_error(
            path,
            format!("field '{field}' must contain four unsigned integers"),
        ));
    };
    if roi[0] >= roi[2] || roi[1] >= roi[3] {
        return Err(validation_error(
            path,
            format!("field '{field}' has an invalid rectangle"),
        ));
    }
    Ok(roi)
}

fn add_geometry_features(
    features: &mut BTreeMap<String, FeatureObservation>,
    radio: &Value,
    person_bbox: [f32; 4],
    calibration: &SurfaceCalibration,
    surface_spatial_padding_px: f32,
    surface_depth_padding_m: f32,
) -> Result<()> {
    let width = person_bbox[2] - person_bbox[0];
    let height = person_bbox[3] - person_bbox[1];
    insert_feature(
        features,
        "geometry.person_bbox_aspect".into(),
        FeatureObservation {
            component: "geometry".into(),
            status: ObservationStatus::Observed,
            value: Some(FeatureValue::Number(width / height)),
            quality: 1.0,
            reason: None,
        },
    );

    let mut keypoints = BTreeMap::<String, KeypointObservation>::new();
    if let Some(items) = radio.get("keypoints").and_then(Value::as_array) {
        for item in items {
            let Some(name) = item.get("part").and_then(Value::as_str) else {
                continue;
            };
            let Some(point) = item.get("point").and_then(Value::as_array) else {
                continue;
            };
            let Some(point) = array_as_f32::<2>(point) else {
                continue;
            };
            let confidence = item.get("conf").and_then(as_f32).map_or(0.0, clamp_quality);
            let depth_m = item.get("depth_m").and_then(as_f32);
            keypoints.insert(
                name.to_string(),
                KeypointObservation {
                    point,
                    normalized: [
                        (point[0] - person_bbox[0]) / width,
                        (point[1] - person_bbox[1]) / height,
                    ],
                    confidence,
                    depth_m,
                    zone: item.get("zone").and_then(Value::as_str).map(str::to_owned),
                    delta_m: item.get("delta_m").and_then(as_f32),
                },
            );
        }
    }

    let shoulders = average_keypoints(&keypoints, &SHOULDER_KEYPOINTS);
    let hips = average_keypoints(&keypoints, &HIP_KEYPOINTS);
    if let (Some((shoulders, shoulder_quality)), Some((hips, hip_quality))) = (shoulders, hips) {
        let quality = (shoulder_quality + hip_quality) * 0.5;
        let tilt = (hips[0] - shoulders[0])
            .abs()
            .atan2((hips[1] - shoulders[1]).abs())
            .to_degrees();
        insert_feature(
            features,
            "geometry.torso_tilt_deg".into(),
            FeatureObservation {
                component: "geometry".into(),
                status: status_for_quality(quality),
                value: Some(FeatureValue::Number(tilt)),
                quality,
                reason: None,
            },
        );

        // Vertical shoulder-to-hip span is a direct pose geometry signal. It
        // complements tilt because a person can rotate without changing the
        // overall camera-relative body extent.
        insert_feature(
            features,
            "keypoint_group.shoulder_hip_vertical_span".into(),
            FeatureObservation {
                component: "geometry".into(),
                status: status_for_quality(quality),
                value: Some(FeatureValue::Number(hips[1] - shoulders[1])),
                quality,
                reason: None,
            },
        );
    }

    let shoulder_depth = average_keypoint_depth(&keypoints, &SHOULDER_KEYPOINTS);
    let hip_depth = average_keypoint_depth(&keypoints, &HIP_KEYPOINTS);
    if let (Some((shoulder_depth, shoulder_quality)), Some((hip_depth, hip_quality))) =
        (shoulder_depth, hip_depth)
    {
        let quality = (shoulder_quality + hip_quality) * 0.5;
        insert_feature(
            features,
            "keypoint_group.shoulder_hip_depth_delta".into(),
            FeatureObservation {
                component: "depth".into(),
                status: status_for_quality(quality),
                value: Some(FeatureValue::Number(shoulder_depth - hip_depth)),
                quality,
                reason: None,
            },
        );

        if let Some((head_depth, head_quality, head_count)) =
            average_head_depth(&keypoints, &HEAD_KEYPOINTS)
        {
            let head_hip_delta = head_depth - hip_depth;
            let relation_quality =
                head_quality * (0.5 + 0.5 * head_count as f32 / HEAD_KEYPOINTS.len() as f32);
            insert_feature(
                features,
                "keypoint_group.head_hip_depth_delta".into(),
                FeatureObservation {
                    component: "depth".into(),
                    status: status_for_quality(relation_quality),
                    value: Some(FeatureValue::Number(head_hip_delta)),
                    quality: relation_quality,
                    reason: None,
                },
            );
            insert_feature(
                features,
                "keypoint_group.head_hip_depth_relation".into(),
                FeatureObservation {
                    component: "depth".into(),
                    status: status_for_quality(relation_quality),
                    value: Some(FeatureValue::Category(
                        if head_hip_delta >= 0.18 {
                            "head_farther_than_hips"
                        } else {
                            "head_near_hips"
                        }
                        .into(),
                    )),
                    quality: relation_quality,
                    reason: None,
                },
            );
        }
    }
    add_surface_features(
        features,
        radio,
        &keypoints,
        calibration,
        surface_spatial_padding_px,
        surface_depth_padding_m,
    );
    Ok(())
}

fn add_surface_features(
    features: &mut BTreeMap<String, FeatureObservation>,
    radio: &Value,
    keypoints: &BTreeMap<String, KeypointObservation>,
    calibration: &SurfaceCalibration,
    spatial_padding_px: f32,
    depth_padding_m: f32,
) {
    let mut head_points = surface_points(keypoints, &HEAD_KEYPOINTS);
    if let Some(face) = radio.get("face").and_then(Value::as_object) {
        if let Some(bbox) = face.get("bbox").and_then(Value::as_array) {
            if let Some(bbox) = array_as_f32::<4>(bbox) {
                head_points.push(SurfacePoint {
                    point: [(bbox[0] + bbox[2]) * 0.5, (bbox[1] + bbox[3]) * 0.5],
                    quality: face.get("conf").and_then(as_f32).map_or(0.5, clamp_quality),
                    weight: 0.75,
                    zone: face.get("zone").and_then(Value::as_str).map(str::to_owned),
                    delta_m: None,
                });
            }
        }
    }

    let torso_points = surface_points(
        keypoints,
        &["left_shoulder", "right_shoulder", "left_hip", "right_hip"],
    );
    let hip_points = surface_points(keypoints, &HIP_KEYPOINTS);

    for (anchor, points) in [
        ("head", head_points),
        ("torso", torso_points),
        ("hips", hip_points),
    ] {
        let Some(evidence) =
            surface_anchor_evidence(&points, calibration, spatial_padding_px, depth_padding_m)
        else {
            continue;
        };
        insert_surface_number(
            features,
            format!("surface.{anchor}_bed_support"),
            evidence.bed_support,
            evidence.quality,
        );
        insert_surface_number(
            features,
            format!("surface.{anchor}_floor_support"),
            evidence.floor_support,
            evidence.quality,
        );
        insert_surface_number(
            features,
            format!("surface.{anchor}_zone_index"),
            evidence.zone_index,
            evidence.quality,
        );
        insert_surface_number(
            features,
            format!("surface.{anchor}_depth_fit"),
            evidence.depth_fit,
            evidence.quality,
        );
        if let Some(zone) = evidence.zone {
            insert_feature(
                features,
                format!("surface.{anchor}_zone"),
                FeatureObservation {
                    component: "surface".into(),
                    status: status_for_quality(evidence.quality),
                    value: Some(FeatureValue::Category(zone)),
                    quality: evidence.quality,
                    reason: None,
                },
            );
        }
    }
}

fn surface_points(
    keypoints: &BTreeMap<String, KeypointObservation>,
    names: &[&str],
) -> Vec<SurfacePoint> {
    names
        .iter()
        .filter_map(|name| keypoints.get(*name))
        .map(|keypoint| SurfacePoint {
            point: keypoint.point,
            quality: keypoint.confidence,
            weight: 1.0,
            zone: keypoint.zone.clone(),
            delta_m: keypoint.delta_m,
        })
        .collect()
}

fn insert_surface_number(
    features: &mut BTreeMap<String, FeatureObservation>,
    id: String,
    value: f32,
    quality: f32,
) {
    insert_feature(
        features,
        id,
        FeatureObservation {
            component: "surface".into(),
            status: status_for_quality(quality),
            value: Some(FeatureValue::Number(value)),
            quality,
            reason: None,
        },
    );
}

fn surface_anchor_evidence(
    points: &[SurfacePoint],
    calibration: &SurfaceCalibration,
    spatial_padding_px: f32,
    depth_padding_m: f32,
) -> Option<SurfaceAnchorEvidence> {
    if points.is_empty() {
        return None;
    }

    let mut total_weight = 0.0;
    let mut bed_support = 0.0;
    let mut floor_support = 0.0;
    let mut zone_index = 0.0;
    let mut depth_fit = 0.0;
    let mut quality = 0.0;
    let mut zones = BTreeMap::<String, f32>::new();

    for point in points {
        let weight = point.weight * point.quality.clamp(0.0, 1.0).max(0.1);
        let point_depth_fit = point
            .delta_m
            .map(|delta| (1.0 - delta.abs() / depth_padding_m.max(f32::EPSILON)).clamp(0.0, 1.0))
            .unwrap_or(0.5);
        let bed = best_surface_zone_support(
            point,
            &calibration.bed,
            "bed",
            spatial_padding_px,
            point_depth_fit,
        );
        let floor = best_surface_zone_support(
            point,
            &calibration.floor,
            "floor",
            spatial_padding_px,
            point_depth_fit,
        );

        total_weight += weight;
        bed_support += bed.support * weight;
        floor_support += floor.support * weight;
        depth_fit += point_depth_fit * weight;
        quality += point.quality.clamp(0.0, 1.0) * weight;

        let selected = if bed.support >= floor.support {
            bed
        } else {
            floor
        };
        zone_index += selected.index * weight;
        if let Some(zone) = selected.zone {
            *zones.entry(zone).or_default() += selected.support * weight;
        }
    }

    if total_weight <= 0.0 {
        return None;
    }
    let zone = zones
        .into_iter()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(zone, _)| zone);
    Some(SurfaceAnchorEvidence {
        bed_support: bed_support / total_weight,
        floor_support: floor_support / total_weight,
        zone_index: zone_index / total_weight,
        depth_fit: depth_fit / total_weight,
        quality: (quality / total_weight).clamp(0.0, 1.0),
        zone,
    })
}

struct SurfaceZoneSupport {
    zone: Option<String>,
    support: f32,
    index: f32,
}

fn best_surface_zone_support(
    point: &SurfacePoint,
    zones: &[SurfaceZone],
    layer: &str,
    spatial_padding_px: f32,
    depth_fit: f32,
) -> SurfaceZoneSupport {
    let Some((zone, support)) = zones
        .iter()
        .map(|zone| {
            let spatial = padded_zone_support(zone, point.point, spatial_padding_px);
            let label = format!("{layer}/{}", zone.name);
            let zone_hint = point.zone.as_deref().map_or(0.0, |observed| {
                if observed == label {
                    1.0
                } else if observed.starts_with(&format!("{layer}/")) {
                    0.35
                } else {
                    0.0
                }
            });
            let location_support = (0.75 * spatial + 0.25 * zone_hint).clamp(0.0, 1.0);
            (zone, location_support * (0.7 + 0.3 * depth_fit))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
    else {
        return SurfaceZoneSupport {
            zone: None,
            support: 0.0,
            index: 0.5,
        };
    };
    SurfaceZoneSupport {
        zone: (support > 0.0).then_some(format!("{layer}/{}", zone.name)),
        support,
        index: zone_index(&zone.name),
    }
}

fn zone_index(name: &str) -> f32 {
    match name {
        "head" => 0.0,
        "body" => 0.5,
        "feet" => 1.0,
        _ => 0.5,
    }
}

fn padded_zone_support(zone: &SurfaceZone, point: [f32; 2], padding: f32) -> f32 {
    if zone.contains(point) {
        return 1.0;
    }
    if padding <= 0.0 {
        return 0.0;
    }
    let distance = polygon_distance(point, &zone.polygon);
    (1.0 - distance / padding).clamp(0.0, 1.0)
}

fn polygon_distance(point: [f32; 2], polygon: &[[f32; 2]]) -> f32 {
    if polygon.len() < 2 {
        return f32::INFINITY;
    }
    polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
        .map(|(&start, &end)| point_segment_distance(point, start, end))
        .fold(f32::INFINITY, f32::min)
}

fn point_segment_distance(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> f32 {
    let [dx, dy] = [end[0] - start[0], end[1] - start[1]];
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return ((point[0] - start[0]).powi(2) + (point[1] - start[1]).powi(2)).sqrt();
    }
    let projection = (((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / length_squared)
        .clamp(0.0, 1.0);
    let closest = [start[0] + projection * dx, start[1] + projection * dy];
    ((point[0] - closest[0]).powi(2) + (point[1] - closest[1]).powi(2)).sqrt()
}

fn add_face_features(
    features: &mut BTreeMap<String, FeatureObservation>,
    radio: &Value,
    person_bbox: [f32; 4],
) {
    let Some(face) = radio.get("face").and_then(Value::as_object) else {
        return;
    };
    let quality = face.get("conf").and_then(as_f32).map_or(0.5, clamp_quality);
    if let Some(bbox) = face.get("bbox").and_then(Value::as_array) {
        if let Some(bbox) = array_as_f32::<4>(bbox) {
            let person_height = (person_bbox[3] - person_bbox[1]).max(f32::EPSILON);
            insert_feature(
                features,
                "face.bbox_height_ratio".into(),
                FeatureObservation {
                    component: "face".into(),
                    status: status_for_quality(quality),
                    value: Some(FeatureValue::Number((bbox[3] - bbox[1]) / person_height)),
                    quality,
                    reason: None,
                },
            );
            insert_feature(
                features,
                "face.center_y_ratio".into(),
                FeatureObservation {
                    component: "face".into(),
                    status: status_for_quality(quality),
                    value: Some(FeatureValue::Number(
                        (((bbox[1] + bbox[3]) * 0.5) - person_bbox[1]) / person_height,
                    )),
                    quality,
                    reason: None,
                },
            );
        }
    }
    if let Some(zone) = face.get("zone").and_then(Value::as_str) {
        insert_feature(
            features,
            "face.zone".into(),
            FeatureObservation {
                component: "face".into(),
                status: status_for_quality(quality),
                value: Some(FeatureValue::Category(zone.to_string())),
                quality,
                reason: None,
            },
        );
    }
}

fn add_segment_features(features: &mut BTreeMap<String, FeatureObservation>, radio: &Value) {
    let Some(segment) = radio.get("segment").and_then(Value::as_object) else {
        return;
    };
    let Some(area_ratio) = segment.get("area_ratio").and_then(as_f32) else {
        return;
    };
    insert_feature(
        features,
        "segment.area_ratio".into(),
        FeatureObservation {
            component: "segment".into(),
            status: ObservationStatus::Observed,
            value: Some(FeatureValue::Number(area_ratio)),
            quality: 1.0,
            reason: None,
        },
    );
}

fn add_body_part_features(
    features: &mut BTreeMap<String, FeatureObservation>,
    parts_report: &Value,
    expected_roi: [u32; 4],
    compatibility_reasons: &mut Vec<String>,
) {
    let Some(actor) = parts_report
        .get("actors")
        .and_then(Value::as_array)
        .and_then(|actors| actors.first())
    else {
        return;
    };
    let Some(parts) = actor.get("parts").and_then(Value::as_array) else {
        return;
    };
    for part in parts {
        let Some(name) = part.get("part").and_then(Value::as_str) else {
            continue;
        };
        let part_quality = part
            .get("quality")
            .and_then(as_f32)
            .map_or(0.5, clamp_quality);
        let coverage = part
            .get("mask_coverage")
            .and_then(as_f32)
            .map(|value| value.clamp(0.0, 1.0));
        let coverage_factor = coverage.map_or(0.75, |value| 0.5 + 0.5 * value);
        let quality = (part_quality * coverage_factor).clamp(0.0, 1.0);

        if let Some(coverage) = coverage {
            insert_feature(
                features,
                format!("body_part.{name}.mask_coverage"),
                FeatureObservation {
                    component: "body_parts".into(),
                    status: status_for_quality(quality),
                    value: Some(FeatureValue::Number(coverage)),
                    quality,
                    reason: (coverage < 0.75).then_some("low mask coverage".into()),
                },
            );
        }

        let Some(depth) = part.get("depth").and_then(Value::as_object) else {
            continue;
        };
        if let Some(roi) = depth.get("roi").and_then(as_roi) {
            if roi != expected_roi {
                compatibility_reasons.push(format!(
                    "body part '{name}' ROI {roi:?} does not match radio ROI {expected_roi:?}"
                ));
            }
        }
        if let Some(relative) = depth.get("relative_to_torso_m").and_then(as_f32) {
            insert_feature(
                features,
                format!("body_part.{name}.relative_to_torso"),
                FeatureObservation {
                    component: "body_parts".into(),
                    status: status_for_quality(quality),
                    value: Some(FeatureValue::Number(relative)),
                    quality,
                    reason: (coverage.is_some_and(|value| value < 0.75))
                        .then_some("partial mask support".into()),
                },
            );
        }
        if let Some(median) = depth.get("median_depth_m").and_then(as_f32) {
            insert_feature(
                features,
                format!("body_part.{name}.median_depth"),
                FeatureObservation {
                    component: "depth".into(),
                    status: status_for_quality(quality),
                    value: Some(FeatureValue::Number(median)),
                    quality,
                    reason: None,
                },
            );
        }
    }
}

fn build_attention_reports(
    feature_scores: &[FeatureScoreReport],
) -> BTreeMap<String, AttentionReport> {
    let mut accumulators = BTreeMap::<String, ComponentAccumulator>::new();
    for feature in feature_scores {
        let accumulator = accumulators
            .entry(attention_group(&feature.id).to_string())
            .or_default();
        if feature.status == ObservationStatus::Conflict {
            accumulator.conflict = true;
            continue;
        }
        if matches!(
            feature.status,
            ObservationStatus::Invalid | ObservationStatus::Stale
        ) {
            accumulator.partial = true;
            continue;
        }
        let Some(support) = feature.support else {
            accumulator.partial = true;
            continue;
        };
        let quality = feature.quality.unwrap_or(0.0).clamp(0.0, 1.0);
        accumulator.numerator += feature.weight * support * quality;
        accumulator.denominator += feature.weight;
        accumulator.effective_weight += feature.effective_weight;
        accumulator.observed_features += 1;
        accumulator.partial |= feature.status == ObservationStatus::Partial;
    }

    accumulators
        .into_iter()
        .map(|(group, accumulator)| {
            let status = if accumulator.conflict {
                ObservationStatus::Conflict
            } else if accumulator.observed_features == 0 {
                ObservationStatus::Missing
            } else if accumulator.partial {
                ObservationStatus::Partial
            } else {
                ObservationStatus::Observed
            };
            let score = (accumulator.denominator > 0.0)
                .then(|| accumulator.numerator / accumulator.denominator);
            (
                group,
                AttentionReport {
                    status,
                    score,
                    observed_features: accumulator.observed_features,
                    effective_weight: accumulator.effective_weight,
                },
            )
        })
        .collect()
}

fn attention_group(feature_id: &str) -> &'static str {
    if feature_id.starts_with("face.")
        || feature_id.contains("head")
        || feature_id.contains("nose")
        || feature_id.contains("eye")
    {
        "head"
    } else if feature_id.starts_with("geometry.torso.")
        || feature_id.starts_with("body_part.torso.")
        || feature_id.contains("shoulder_hip")
    {
        "torso"
    } else if feature_id.contains("left_leg")
        || feature_id.contains("right_leg")
        || feature_id.contains("ankle")
        || feature_id.contains("knee")
    {
        "legs"
    } else if feature_id.starts_with("surface.") || feature_id.starts_with("segment.") {
        "surface"
    } else {
        "geometry"
    }
}

fn attention_weight(feature_id: &str) -> f32 {
    match attention_group(feature_id) {
        "head" => 1.5,
        "torso" => 1.35,
        "legs" => 1.0,
        "surface" => 0.8,
        _ => 0.9,
    }
}

fn feature_support(
    feature: &PostureFeature,
    observed: &FeatureObservation,
) -> Option<(f32, Option<String>)> {
    match (&feature.center, &feature.tolerance, &observed.value) {
        (Some(center), Some(tolerance), Some(FeatureValue::Number(value))) => {
            let support = (1.0 - (value - center).abs() / tolerance).clamp(0.0, 1.0);
            let reason = (support == 0.0).then_some("outside numeric tolerance".into());
            Some((support, reason))
        }
        (None, None, Some(FeatureValue::Category(value))) => {
            let matches = feature.allowed.iter().any(|allowed| allowed == value);
            if matches {
                return Some((1.0, None));
            }
            let same_surface = value.contains('/')
                && feature.allowed.iter().any(|allowed| {
                    allowed.split_once('/').map(|(surface, _)| surface)
                        == value.split_once('/').map(|(surface, _)| surface)
                });
            let reason = if same_surface {
                Some("adjacent calibrated surface zone".into())
            } else {
                Some("category not allowed".into())
            };
            Some((if same_surface { 0.5 } else { 0.0 }, reason))
        }
        _ => None,
    }
}

fn feature_component(feature: &PostureFeature) -> &'static str {
    match feature.source.as_str() {
        "geometry" | "keypoint" => "geometry",
        "keypoint_group" if feature.field.contains("depth") => "depth",
        "keypoint_group" => "geometry",
        "body_part" => "body_parts",
        "face" => "face",
        "segment" => "segment",
        "surface" => "depth",
        _ => "unknown",
    }
}

fn insert_feature(
    features: &mut BTreeMap<String, FeatureObservation>,
    id: String,
    observation: FeatureObservation,
) {
    let replace = features
        .get(&id)
        .is_none_or(|current| observation.quality > current.quality);
    if replace {
        features.insert(id, observation);
    }
}

fn average_keypoints(
    keypoints: &BTreeMap<String, KeypointObservation>,
    names: &[&str],
) -> Option<([f32; 2], f32)> {
    let values = names
        .iter()
        .filter_map(|name| keypoints.get(*name))
        .collect::<Vec<_>>();
    if values.len() != names.len() {
        return None;
    }
    let count = values.len() as f32;
    Some((
        [
            values.iter().map(|value| value.normalized[0]).sum::<f32>() / count,
            values.iter().map(|value| value.normalized[1]).sum::<f32>() / count,
        ],
        values.iter().map(|value| value.confidence).sum::<f32>() / count,
    ))
}

fn average_keypoint_depth(
    keypoints: &BTreeMap<String, KeypointObservation>,
    names: &[&str],
) -> Option<(f32, f32)> {
    let values = names
        .iter()
        .filter_map(|name| {
            keypoints
                .get(*name)
                .and_then(|value| value.depth_m.map(|depth| (depth, value.confidence)))
        })
        .collect::<Vec<_>>();
    if values.len() != names.len() {
        return None;
    }
    let count = values.len() as f32;
    Some((
        values.iter().map(|value| value.0).sum::<f32>() / count,
        values.iter().map(|value| value.1).sum::<f32>() / count,
    ))
}

fn average_head_depth(
    keypoints: &BTreeMap<String, KeypointObservation>,
    names: &[&str],
) -> Option<(f32, f32, usize)> {
    let values = names
        .iter()
        .filter_map(|name| {
            keypoints
                .get(*name)
                .and_then(|value| value.depth_m.map(|depth| (depth, value.confidence)))
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    let count = values.len() as f32;
    Some((
        values.iter().map(|value| value.0).sum::<f32>() / count,
        values.iter().map(|value| value.1).sum::<f32>() / count,
        values.len(),
    ))
}

fn array_as_f32<const N: usize>(values: &[Value]) -> Option<[f32; N]> {
    if values.len() != N {
        return None;
    }
    let mut output = [0.0; N];
    for (index, value) in values.iter().enumerate() {
        output[index] = as_f32(value)?;
    }
    Some(output)
}

fn array_as_u32<const N: usize>(values: &[Value]) -> Option<[u32; N]> {
    if values.len() != N {
        return None;
    }
    let mut output = [0; N];
    for (index, value) in values.iter().enumerate() {
        output[index] = value.as_u64()?.try_into().ok()?;
    }
    Some(output)
}

fn as_roi(value: &Value) -> Option<[u32; 4]> {
    value
        .as_array()
        .and_then(|values| array_as_u32::<4>(values))
}

fn as_f32(value: &Value) -> Option<f32> {
    let value = value.as_f64()? as f32;
    value.is_finite().then_some(value)
}

fn clamp_quality(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn status_for_quality(quality: f32) -> ObservationStatus {
    if clamp_quality(quality) >= 0.75 {
        ObservationStatus::Observed
    } else {
        ObservationStatus::Partial
    }
}

fn read_toml(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|error| {
        ManaError::Config(ConfigError::FileNotFound(format!(
            "{} ({error})",
            path.display()
        )))
    })
}

fn validation_error(path: &Path, message: String) -> ManaError {
    ManaError::Config(ConfigError::ValidationError(format!(
        "{}: {message}",
        path.display()
    )))
}

fn validate_feature_value(feature: &PostureFeature) -> std::result::Result<(), String> {
    match (
        feature.center,
        feature.tolerance,
        feature.allowed.is_empty(),
    ) {
        (Some(center), Some(tolerance), true) => {
            if !center.is_finite() {
                return Err(format!("feature '{}' center is not finite", feature.id));
            }
            if !tolerance.is_finite() || tolerance <= 0.0 {
                return Err(format!(
                    "feature '{}' tolerance must be finite and positive",
                    feature.id
                ));
            }
        }
        (None, None, false) => {
            for value in &feature.allowed {
                validate_non_empty("categorical feature value", value)?;
            }
        }
        _ => {
            return Err(format!(
                "feature '{}' must define either center+tolerance or allowed",
                feature.id
            ));
        }
    }
    Ok(())
}

fn validate_identifier(field: &str, value: &str) -> std::result::Result<(), String> {
    validate_non_empty(field, value)?;
    if value.chars().any(char::is_whitespace) {
        return Err(format!("{field} must not contain whitespace"));
    }
    Ok(())
}

fn validate_non_empty(field: &str, value: &str) -> std::result::Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_relative_path(field: &str, value: &str) -> std::result::Result<(), String> {
    validate_non_empty(field, value)?;
    if Path::new(value).is_absolute() {
        return Err(format!("{field} must be relative"));
    }
    Ok(())
}

fn validate_profile_file(value: &str) -> std::result::Result<(), String> {
    validate_relative_path("posture file", value)?;
    if Path::new(value)
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(format!(
            "posture file '{value}' must stay inside posture_dir"
        ));
    }
    Ok(())
}

fn validate_component_quorum(field: &str, value: usize) -> std::result::Result<(), String> {
    if value == 0 || value > VALID_COMPONENT_COUNT {
        Err(format!(
            "{field} must be between 1 and {VALID_COMPONENT_COUNT}"
        ))
    } else {
        Ok(())
    }
}

fn validate_unit_interval(field: &str, value: f32) -> std::result::Result<(), String> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        Err(format!("{field} must be finite and in [0, 1]"))
    } else {
        Ok(())
    }
}

fn validate_positive_finite(field: &str, value: f32) -> std::result::Result<(), String> {
    if !value.is_finite() || value <= 0.0 {
        Err(format!("{field} must be finite and positive"))
    } else {
        Ok(())
    }
}

const fn default_true() -> bool {
    true
}

const fn default_weight() -> f32 {
    1.0
}

const fn default_min_observed_features() -> usize {
    1
}

const fn default_min_observed_components() -> usize {
    2
}

const fn default_semantic_min_total_score() -> f32 {
    0.40
}

const fn default_surface_spatial_padding_px() -> f32 {
    24.0
}

const fn default_surface_depth_padding_m() -> f32 {
    0.15
}

const fn default_master_policy() -> MasterPolicy {
    MasterPolicy {
        missing_is_conflict: false,
        allow_partial_parts: true,
        require_same_model_context: true,
        require_same_roi: true,
    }
}

const fn default_posture_policy() -> PosturePolicy {
    PosturePolicy {
        min_observed_features: 1,
        min_observed_components: 2,
        allow_partial: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const VALID_MASTER: &str = r#"
schema_version = 1
engine = "posture-analysis"
model_key = "depth-l-640"
depth_semantics = "model-relative"
surface_calibration = "calibration.toml"
posture_dir = "."
min_observed_components = 2
min_total_score = 0.55
ambiguity_margin = 0.10

[policy]
allow_partial_parts = true
require_same_model_context = true
require_same_roi = true

[[postures]]
id = "sentado-1"
label = "sentado"
file = "sentado-1.toml"
"#;

    const VALID_PROFILE: &str = r#"
schema_version = 1
posture_id = "sentado-1"
label = "sentado"
training_sample = "sentado-1"
base_posture = "sentado-in-bed"
plane = "in-bed"

[policy]
min_observed_features = 2
min_observed_components = 2
allow_partial = true

[[features]]
id = "geometry.torso_tilt_deg"
source = "geometry"
field = "torso_tilt_deg"
center = 4.1
tolerance = 10.0
weight = 1.0

[[features]]
id = "body_part.torso.relative_to_torso"
source = "body_part"
part = "torso"
field = "relative_to_torso"
center = 0.0
tolerance = 0.15
weight = 1.0
required = true

[[features]]
id = "face.zone"
source = "face"
field = "zone"
allowed = ["bed/head", "bed/body"]
weight = 0.4
"#;

    const VALID_CALIBRATION: &str = r#"
schema_version = 1
model_key = "depth-l-640"
frame_width = 1920
frame_height = 1080
roi = [0, 0, 1920, 1080]
"#;

    #[test]
    fn parses_valid_master_with_default_policy() {
        let master = parse_master(VALID_MASTER).expect("valid master");
        assert_eq!(master.model_key, "depth-l-640");
        assert!(master.policy.allow_partial_parts);
        assert!(master.policy.require_same_roi);
        assert_eq!(master.postures.len(), 1);
    }

    #[test]
    fn parses_numeric_and_categorical_features() {
        let profile = parse_profile(VALID_PROFILE).expect("valid profile");
        assert_eq!(profile.features.len(), 3);
        assert_eq!(profile.features[1].part.as_deref(), Some("torso"));
        assert_eq!(profile.features[2].allowed.len(), 2);
    }

    #[test]
    fn adjacent_surface_zone_is_soft_evidence() {
        let feature = PostureFeature {
            id: "face.zone".into(),
            source: "face".into(),
            field: "zone".into(),
            part: None,
            center: None,
            tolerance: None,
            allowed: vec!["bed/body".into()],
            weight: 1.0,
            required: false,
        };
        let observed = FeatureObservation {
            component: "face".into(),
            status: ObservationStatus::Observed,
            value: Some(FeatureValue::Category("bed/feet".into())),
            quality: 1.0,
            reason: None,
        };
        let (support, reason) = feature_support(&feature, &observed).expect("category support");
        assert_eq!(support, 0.5);
        assert_eq!(reason.as_deref(), Some("adjacent calibrated surface zone"));
    }

    #[test]
    fn rejects_duplicate_posture_ids() {
        let contents = VALID_MASTER.replace(
            "file = \"sentado-1.toml\"",
            "file = \"sentado-1.toml\"\n\n[[postures]]\nid = \"sentado-1\"\nlabel = \"otro\"\nfile = \"otro.toml\"",
        );
        let error = parse_master(&contents).expect_err("duplicate id must fail");
        assert!(error.contains("duplicate posture id"), "{error}");
    }

    #[test]
    fn rejects_feature_without_exactly_one_value_form() {
        let contents = VALID_PROFILE.replace(
            "allowed = [\"bed/head\", \"bed/body\"]",
            "center = 0.5\nallowed = [\"bed/head\"]",
        );
        let error = parse_profile(&contents).expect_err("mixed feature must fail");
        assert!(error.contains("must define either"), "{error}");
    }

    #[test]
    fn loads_profiles_relative_to_master_directory() {
        let directory =
            std::env::temp_dir().join(format!("mana-posture-profile-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create test directory");
        let master_path = directory.join("master.toml");
        let profile_path = directory.join("sentado-1.toml");
        let calibration_path = directory.join("calibration.toml");
        fs::write(&master_path, VALID_MASTER).expect("write master");
        fs::write(&profile_path, VALID_PROFILE).expect("write profile");
        fs::write(&calibration_path, VALID_CALIBRATION).expect("write calibration");

        let loaded = load_profile_set(&master_path).expect("load profile set");
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profile("sentado-1").unwrap().label, "sentado");
        assert_eq!(loaded.surface_calibration.model_key, "depth-l-640");

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn rejects_profile_reference_mismatch() {
        let directory = std::env::temp_dir().join(format!(
            "mana-posture-profile-mismatch-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create test directory");
        let master_path = directory.join("master.toml");
        let profile_path = directory.join("sentado-1.toml");
        let calibration_path = directory.join("calibration.toml");
        fs::write(&master_path, VALID_MASTER).expect("write master");
        fs::write(
            &profile_path,
            VALID_PROFILE.replace("posture_id = \"sentado-1\"", "posture_id = \"otro\""),
        )
        .expect("write profile");
        fs::write(&calibration_path, VALID_CALIBRATION).expect("write calibration");

        let error = load_profile_set(&master_path).expect_err("mismatch must fail");
        assert!(
            error
                .to_string()
                .contains("does not match master reference")
        );

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn repository_l640_profile_matrix_is_valid() {
        let loaded = load_profile_set(Path::new("config/posture-analysis/l-640/master.toml"))
            .expect("repository posture profile matrix");
        assert_eq!(loaded.master.model_key, "depth-l-640");
        assert_eq!(loaded.profiles.len(), 7);
        assert!(loaded.profile("acostado-1").is_some());
        assert!(loaded.profile("foots-left-bed-1").is_some());
    }

    #[test]
    fn calibrated_surface_geometry_is_reported_for_anchors() {
        let report = analyze_reports(
            Path::new("config/posture-analysis/l-640/master.toml"),
            Path::new("tests/fixtures/posture-analysis/l-640/radio/sentado-borde-1.json"),
            Path::new("tests/fixtures/posture-analysis/l-640/parts/sentado-borde-1.json"),
        )
        .expect("analyze calibrated surface sample");
        assert_eq!(report.calibration.surface_spatial_padding_px, 48.0);
        assert!(report.observations.contains_key("surface.head_zone_index"));
        assert!(
            report
                .observations
                .contains_key("surface.torso_bed_support")
        );
        assert!(report.observations.contains_key("surface.hips_depth_fit"));
    }

    #[test]
    fn repository_l640_samples_produce_deterministic_decisions() {
        let master = Path::new("config/posture-analysis/l-640/master.toml");
        for sample in [
            "acostado-1",
            "sentado-1",
            "sentado-borde-1",
            "parado-aside-1",
            "leaving-bed-aside-head-1",
            "foot-left-bed-2",
            "foots-left-bed-1",
        ] {
            let radio = Path::new("tests/fixtures/posture-analysis/l-640/radio")
                .join(format!("{sample}.json"));
            let parts = Path::new("tests/fixtures/posture-analysis/l-640/parts")
                .join(format!("{sample}.json"));
            let report = analyze_reports(master, &radio, &parts).expect("analyze sample");
            assert!(report.calibration.compatible, "{sample}: {report:?}");
            assert_eq!(report.candidates.len(), 7);
            assert_eq!(
                report.decision.status,
                DecisionStatus::Classified,
                "{sample}: {:?}",
                report.decision
            );
            assert_eq!(
                report.decision.posture_id.as_deref(),
                Some(sample),
                "{sample}: {:?}",
                report.decision
            );
            let first = serde_json::to_string(&report).expect("serialize report");
            let second = serde_json::to_string(&analyze_reports(master, &radio, &parts).unwrap())
                .expect("serialize repeated report");
            assert_eq!(first, second, "{sample} is not deterministic");
        }
    }

    #[test]
    fn missing_face_does_not_invalidate_geometry_and_parts() {
        let directory = test_directory("missing-face");
        let radio_path = directory.join("radio.json");
        let source = Path::new("tests/fixtures/posture-analysis/l-640/radio/parado-aside-1.json");
        let mut radio: Value =
            serde_json::from_str(&fs::read_to_string(source).expect("read radio fixture"))
                .expect("parse radio fixture");
        radio.as_object_mut().expect("radio object").remove("face");
        fs::write(
            &radio_path,
            serde_json::to_string(&radio).expect("serialize radio fixture"),
        )
        .expect("write radio fixture");

        let report = analyze_reports(
            Path::new("config/posture-analysis/l-640/master.toml"),
            &radio_path,
            Path::new("tests/fixtures/posture-analysis/l-640/parts/parado-aside-1.json"),
        )
        .expect("analyze without face");
        assert_eq!(report.decision.status, DecisionStatus::Classified);
        assert!(!report.observations.contains_key("face.zone"));
        assert_eq!(
            report.decision.posture_id.as_deref(),
            Some("parado-aside-1")
        );

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn keypoint_depth_semantics_can_confirm_bed_posture_without_face() {
        let directory = test_directory("missing-face-bed-semantics");
        let radio_path = directory.join("radio.json");
        let source = Path::new("tests/fixtures/posture-analysis/l-640/radio/acostado-1.json");
        let mut radio: Value =
            serde_json::from_str(&fs::read_to_string(source).expect("read radio fixture"))
                .expect("parse radio fixture");
        radio.as_object_mut().expect("radio object").remove("face");
        fs::write(
            &radio_path,
            serde_json::to_string(&radio).expect("serialize radio fixture"),
        )
        .expect("write radio fixture");

        let report = analyze_reports(
            Path::new("config/posture-analysis/l-640/master.toml"),
            &radio_path,
            Path::new("tests/fixtures/posture-analysis/l-640/parts/acostado-1.json"),
        )
        .expect("analyze bed posture without face");
        assert!(
            report
                .observations
                .contains_key("keypoint_group.shoulder_hip_vertical_span")
        );
        assert!(
            report
                .observations
                .contains_key("keypoint_group.shoulder_hip_depth_delta")
        );
        assert!(
            report
                .observations
                .contains_key("keypoint_group.head_hip_depth_relation")
        );
        assert_eq!(
            report.semantic_decision.base_posture.as_deref(),
            Some("acostado")
        );
        assert_eq!(report.semantic_decision.plane.as_deref(), Some("in-bed"));
        let acostado = report
            .candidates
            .iter()
            .find(|candidate| candidate.posture_id == "acostado-1")
            .expect("acostado candidate");
        assert!(acostado.attention.contains_key("head"));
        assert!(acostado.attention.contains_key("torso"));
        assert!(acostado.attention.contains_key("legs"));

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn missing_torso_keypoints_do_not_create_a_posture_decision() {
        let directory = test_directory("missing-torso-keypoints");
        let radio_path = directory.join("radio.json");
        let source = Path::new("tests/fixtures/posture-analysis/l-640/radio/parado-aside-1.json");
        let mut radio: Value =
            serde_json::from_str(&fs::read_to_string(source).expect("read radio fixture"))
                .expect("parse radio fixture");
        radio["keypoints"] = radio["keypoints"]
            .as_array()
            .expect("keypoints array")
            .iter()
            .filter(|point| {
                !matches!(
                    point.get("part").and_then(Value::as_str),
                    Some("left_shoulder")
                        | Some("right_shoulder")
                        | Some("left_hip")
                        | Some("right_hip")
                )
            })
            .cloned()
            .collect();
        fs::write(
            &radio_path,
            serde_json::to_string(&radio).expect("serialize radio fixture"),
        )
        .expect("write radio fixture");

        let report = analyze_reports(
            Path::new("config/posture-analysis/l-640/master.toml"),
            &radio_path,
            Path::new("tests/fixtures/posture-analysis/l-640/parts/parado-aside-1.json"),
        )
        .expect("analyze without torso keypoints");
        assert_eq!(report.decision.status, DecisionStatus::Unknown);
        assert_eq!(report.semantic_decision.status, DecisionStatus::Unknown);
        assert!(report.candidates.iter().all(|candidate| !candidate.quorum));

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn missing_leg_is_reported_as_partial_without_dropping_the_actor() {
        let directory = test_directory("missing-leg");
        let parts_path = directory.join("parts.json");
        let source = Path::new("tests/fixtures/posture-analysis/l-640/parts/foot-left-bed-2.json");
        let mut parts: Value =
            serde_json::from_str(&fs::read_to_string(source).expect("read parts fixture"))
                .expect("parse parts fixture");
        let actor = parts
            .get_mut("actors")
            .and_then(Value::as_array_mut)
            .and_then(|actors| actors.first_mut())
            .and_then(Value::as_object_mut)
            .expect("parts actor");
        actor
            .get_mut("parts")
            .and_then(Value::as_array_mut)
            .expect("parts list")
            .retain(|part| part.get("part").and_then(Value::as_str) != Some("left_leg"));
        fs::write(
            &parts_path,
            serde_json::to_string(&parts).expect("serialize parts fixture"),
        )
        .expect("write parts fixture");

        let report = analyze_reports(
            Path::new("config/posture-analysis/l-640/master.toml"),
            Path::new("tests/fixtures/posture-analysis/l-640/radio/foot-left-bed-2.json"),
            &parts_path,
        )
        .expect("analyze without left leg");
        let candidate = report
            .candidates
            .iter()
            .find(|candidate| candidate.posture_id == "foot-left-bed-2")
            .expect("foot-left candidate");
        assert!(
            candidate
                .missing
                .contains(&"body_part.left_leg.mask_coverage".into())
        );
        assert_eq!(
            candidate.components["body_parts"].status,
            ObservationStatus::Partial
        );

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn incompatible_roi_is_not_a_posture_decision() {
        let directory = test_directory("incompatible-roi");
        let radio_path = directory.join("radio.json");
        let source = Path::new("tests/fixtures/posture-analysis/l-640/radio/acostado-1.json");
        let mut radio: Value =
            serde_json::from_str(&fs::read_to_string(source).expect("read radio fixture"))
                .expect("parse radio fixture");
        radio["depth_roi"] = serde_json::json!([0, 0, 100, 100]);
        fs::write(
            &radio_path,
            serde_json::to_string(&radio).expect("serialize radio fixture"),
        )
        .expect("write radio fixture");

        let report = analyze_reports(
            Path::new("config/posture-analysis/l-640/master.toml"),
            &radio_path,
            Path::new("tests/fixtures/posture-analysis/l-640/parts/acostado-1.json"),
        )
        .expect("analyze incompatible ROI");
        assert_eq!(report.decision.status, DecisionStatus::Incompatible);
        assert!(!report.calibration.reasons.is_empty());

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    fn test_directory(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "mana-posture-analysis-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create test directory");
        directory
    }
}
