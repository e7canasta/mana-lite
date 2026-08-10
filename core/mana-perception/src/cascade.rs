use std::collections::HashMap;

use serde::Deserialize;

use crate::domain::{ClassName, ModelId};

/// Narrow port for cascade gating — no SORT filter state.
#[derive(Debug, Clone, PartialEq)]
pub struct GateObservation {
    pub id: u64,
    pub bbox: [f32; 4],
    pub class: ClassName,
    pub confidence: f32,
    pub source_model: ModelId,
    pub is_confirmed: bool,
    pub misses: u32,
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

impl CascadeConfig {
    /// Validate cascade rules against a name→enabled map (no ModelCatalog).
    pub fn validate(&self, models: &HashMap<String, bool>, primary_model: &str) -> Vec<String> {
        let mut errors = Vec::new();
        self.validate_primary(models, primary_model, &mut errors);
        for rule in &self.rules {
            self.validate_rule_refs(rule, models, primary_model, &mut errors);
            self.validate_rule_thresholds(rule, &mut errors);
        }
        self.validate_regions(&mut errors);
        errors
    }

    fn validate_primary(
        &self,
        _models: &HashMap<String, bool>,
        primary_model: &str,
        errors: &mut Vec<String>,
    ) {
        let primary_rule = self.rules.iter().find(|rule| rule.model == primary_model);
        if primary_rule.is_none() {
            errors.push(format!(
                "primary model '{}' is missing from cascade rules",
                primary_model
            ));
        } else if primary_rule.is_some_and(|rule| rule.requires.is_some()) {
            errors.push(format!(
                "primary model '{}' must be a cascade root",
                primary_model
            ));
        }
    }

    fn validate_rule_refs(
        &self,
        rule: &CascadeRule,
        models: &HashMap<String, bool>,
        primary_model: &str,
        errors: &mut Vec<String>,
    ) {
        if !models.contains_key(&rule.model) {
            errors.push(format!("rule references unknown model '{}'", rule.model));
        }
        if let Some(parent) = &rule.requires {
            if !models.contains_key(parent) {
                errors.push(format!(
                    "model '{}' requires unknown parent '{}'",
                    rule.model, parent
                ));
            }
            let disabled_branch = models.get(parent).is_some_and(|enabled| !*enabled)
                && models.get(&rule.model).is_some_and(|enabled| !*enabled);
            if parent != primary_model && !disabled_branch {
                errors.push(format!(
                    "model '{}' requires '{}', but only primary model '{}' can gate children",
                    rule.model, parent, primary_model,
                ));
            }
        }
        if let Some(region) = &rule.requires_region {
            if !self.regions.contains_key(region) {
                errors.push(format!(
                    "model '{}' references unknown region '{}'",
                    rule.model, region
                ));
            }
        }
    }

    fn validate_rule_thresholds(&self, rule: &CascadeRule, errors: &mut Vec<String>) {
        if rule.requires_exact_count == Some(0) {
            errors.push(format!(
                "model '{}' has invalid requires_exact_count",
                rule.model
            ));
        }
        if rule
            .requires_min_confidence
            .is_some_and(|v| !(0.0..=1.0).contains(&v))
        {
            errors.push(format!(
                "model '{}' has invalid requires_min_confidence",
                rule.model
            ));
        }
        if rule
            .requires_min_area_ratio
            .is_some_and(|v| !(0.0..=1.0).contains(&v))
        {
            errors.push(format!(
                "model '{}' has invalid requires_min_area_ratio",
                rule.model
            ));
        }
        if rule
            .requires_region_coverage
            .is_some_and(|v| !(0.0..=1.0).contains(&v))
        {
            errors.push(format!(
                "model '{}' has invalid requires_region_coverage",
                rule.model
            ));
        }
    }

    fn validate_regions(&self, errors: &mut Vec<String>) {
        for (name, region) in &self.regions {
            let [x1, y1, x2, y2] = region.rect;
            if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite())
                || x2 <= x1
                || y2 <= y1
            {
                errors.push(format!("region '{}' has invalid rect", name));
            }
        }
    }
}

struct CascadeEntry {
    requires: Option<String>,
    requires_class: Option<String>,
    requires_exact_count: Option<usize>,
    same_frame: bool,
    requires_min_confidence: Option<f32>,
    requires_min_area_ratio: Option<f32>,
    requires_region: Option<String>,
    requires_region_coverage: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CascadeTarget {
    pub id: Option<u64>,
    pub bbox: [f32; 4],
}

pub struct CascadeScheduler {
    entries: HashMap<String, CascadeEntry>,
    order: Vec<String>,
    regions: HashMap<String, SemanticRegion>,
}

impl CascadeScheduler {
    #[allow(dead_code)]
    pub fn from_rules(rules: &[CascadeRule]) -> Self {
        Self::from_rules_and_regions(rules, HashMap::new())
    }

    pub fn from_rules_and_regions(
        rules: &[CascadeRule],
        regions: HashMap<String, SemanticRegion>,
    ) -> Self {
        let mut entries = HashMap::new();
        let mut order = Vec::new();
        for rule in rules {
            order.push(rule.model.clone());
            entries.insert(
                rule.model.clone(),
                CascadeEntry {
                    requires: rule.requires.clone(),
                    requires_class: rule.requires_class.clone(),
                    requires_exact_count: rule.requires_exact_count,
                    same_frame: rule.same_frame,
                    requires_min_confidence: rule.requires_min_confidence,
                    requires_min_area_ratio: rule.requires_min_area_ratio,
                    requires_region: rule.requires_region.clone(),
                    requires_region_coverage: rule.requires_region_coverage,
                },
            );
        }
        Self {
            entries,
            order,
            regions,
        }
    }

    pub fn all_models(&self) -> &[String] {
        &self.order
    }

    /// Topo-ordered view of `requested` without owning the names:
    /// roots (no `requires`) first, then children, preserving the
    /// configured order inside each group.
    pub fn ordered<'a>(&self, requested: &'a [String]) -> Vec<&'a str> {
        let mut roots: Vec<&'a str> = Vec::new();
        let mut children: Vec<&'a str> = Vec::new();

        for name in requested {
            match self.entries.get(name.as_str()) {
                Some(entry) if entry.requires.is_none() => roots.push(name.as_str()),
                _ => children.push(name.as_str()),
            }
        }

        roots.extend(children);
        roots
    }

    pub fn parent_of(&self, model: &str) -> Option<&str> {
        self.entries.get(model)?.requires.as_deref()
    }

    pub fn same_frame(&self, model: &str) -> bool {
        self.entries
            .get(model)
            .is_some_and(|entry| entry.same_frame)
    }

    pub fn target_for_detections(
        &self,
        model: &str,
        detections: &[crate::detection::Detection],
        frame_w: u32,
        frame_h: u32,
    ) -> Option<CascadeTarget> {
        let entry = self.entries.get(model)?;
        if !entry.same_frame || entry.requires.is_none() {
            return None;
        }

        let candidates: Vec<&crate::detection::Detection> = detections
            .iter()
            .filter(|detection| {
                entry
                    .requires_class
                    .as_deref()
                    .is_none_or(|class| detection.class == class)
            })
            .filter(|detection| {
                entry
                    .requires_min_confidence
                    .is_none_or(|min| detection.confidence >= min)
            })
            .filter(|detection| {
                entry
                    .requires_min_area_ratio
                    .is_none_or(|min| bbox_area_ratio(&detection.bbox, frame_w, frame_h) >= min)
            })
            .filter(|detection| {
                entry.requires_region.as_deref().is_none_or(|region_name| {
                    let Some(region) = self.regions.get(region_name) else {
                        return false;
                    };
                    entry.requires_region_coverage.is_none_or(|min| {
                        bbox_region_coverage(&detection.bbox, &region.rect) >= min
                    })
                })
            })
            .collect();
        if entry
            .requires_exact_count
            .is_some_and(|count| count != candidates.len())
        {
            return None;
        }

        candidates
            .into_iter()
            .max_by(|a, b| {
                bbox_area(&a.bbox)
                    .partial_cmp(&bbox_area(&b.bbox))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|detection| CascadeTarget {
                id: None,
                bbox: detection.bbox,
            })
    }

    pub fn target_for(
        &self,
        model: &str,
        observations: &[GateObservation],
        frame_w: u32,
        frame_h: u32,
    ) -> Option<CascadeTarget> {
        let Some(entry) = self.entries.get(model) else {
            return None;
        };

        if entry.requires.is_none() {
            return None;
        }

        let parent_key = entry.requires.as_deref().unwrap_or_default();
        let required_class = entry.requires_class.as_deref();
        let exact_count = entry.requires_exact_count;
        let min_confidence = entry.requires_min_confidence;
        let min_area_ratio = entry.requires_min_area_ratio;
        let required_region = entry.requires_region.as_deref();
        let min_region_coverage = entry.requires_region_coverage;

        let candidates: Vec<&GateObservation> = observations
            .iter()
            .filter(|obs| obs.is_confirmed && obs.misses == 0)
            .filter(|obs| obs.source_model.as_str() == parent_key)
            .filter(|obs| required_class.is_none_or(|class| obs.class.as_str() == class))
            .filter(|obs| min_confidence.is_none_or(|min| obs.confidence >= min))
            .filter(|obs| {
                min_area_ratio.is_none_or(|min| bbox_area_ratio(&obs.bbox, frame_w, frame_h) >= min)
            })
            .filter(|obs| {
                required_region.is_none_or(|region_name| {
                    let Some(region) = self.regions.get(region_name) else {
                        return false;
                    };
                    min_region_coverage
                        .is_none_or(|min| bbox_region_coverage(&obs.bbox, &region.rect) >= min)
                })
            })
            .collect();
        if exact_count.is_some_and(|count| count != candidates.len()) {
            return None;
        }

        let best = candidates.into_iter().max_by(|a, b| {
            bbox_area(&a.bbox)
                .partial_cmp(&bbox_area(&b.bbox))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let Some(obs) = best else {
            return None;
        };

        Some(CascadeTarget {
            id: Some(obs.id),
            bbox: obs.bbox,
        })
    }
}

fn bbox_area(bbox: &[f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).max(0.0) * (bbox[3] - bbox[1]).max(0.0)
}

fn bbox_area_ratio(bbox: &[f32; 4], frame_w: u32, frame_h: u32) -> f32 {
    let frame_area = (frame_w as f32) * (frame_h as f32);
    if frame_area <= 0.0 {
        0.0
    } else {
        bbox_area(bbox) / frame_area
    }
}

fn bbox_region_coverage(bbox: &[f32; 4], region: &[f32; 4]) -> f32 {
    let ix1 = bbox[0].max(region[0]);
    let iy1 = bbox[1].max(region[1]);
    let ix2 = bbox[2].min(region[2]);
    let iy2 = bbox[3].min(region[3]);
    let intersection = (ix2 - ix1).max(0.0) * (iy2 - iy1).max(0.0);
    let area = bbox_area(bbox);
    if area <= 0.0 {
        0.0
    } else {
        intersection / area
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate_obs(
        id: u64,
        source_model: &str,
        class: &str,
        bbox: [f32; 4],
        confidence: f32,
        is_confirmed: bool,
        misses: u32,
    ) -> GateObservation {
        GateObservation {
            id: id,
            bbox,
            class: ClassName::new(class),
            confidence,
            source_model: ModelId::new(source_model),
            is_confirmed,
            misses,
        }
    }

    fn test_rules() -> Vec<CascadeRule> {
        vec![
            CascadeRule {
                model: "detect-fast".into(),
                requires: None,
                requires_class: None,
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
            },
            CascadeRule {
                model: "pose-standard".into(),
                requires: Some("detect-fast".into()),
                requires_class: Some("person".into()),
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
            },
        ]
    }

    #[test]
    fn same_frame_child_requires_exact_parent_count() {
        let cascade = CascadeScheduler::from_rules(&[
            CascadeRule {
                model: "detect-fast".into(),
                requires: None,
                requires_class: None,
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
            },
            CascadeRule {
                model: "face-yolo".into(),
                requires: Some("detect-fast".into()),
                requires_class: Some("person".into()),
                requires_exact_count: Some(1),
                same_frame: true,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
            },
        ]);
        let one_person = [crate::detection::Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: None,
            mask: None,
        }];
        let two_people = [
            crate::detection::Detection {
                class: "person".into(),
                confidence: 0.9,
                bbox: [0.0, 0.0, 100.0, 100.0],
                keypoints: None,
                mask: None,
            },
            crate::detection::Detection {
                class: "person".into(),
                confidence: 0.9,
                bbox: [120.0, 0.0, 220.0, 100.0],
                keypoints: None,
                mask: None,
            },
        ];
        assert!(
            cascade
                .target_for_detections("face-yolo", &one_person, 640, 480)
                .is_some()
        );
        assert!(
            cascade
                .target_for_detections("face-yolo", &two_people, 640, 480)
                .is_none()
        );
    }

    #[test]
    fn same_frame_child_applies_confidence_and_area_gates() {
        let cascade = CascadeScheduler::from_rules(&[
            CascadeRule {
                model: "detect-fast".into(),
                requires: None,
                requires_class: None,
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
            },
            CascadeRule {
                model: "face-yolo".into(),
                requires: Some("detect-fast".into()),
                requires_class: Some("person".into()),
                requires_exact_count: Some(1),
                same_frame: true,
                requires_min_confidence: Some(0.8),
                requires_min_area_ratio: Some(0.1),
                requires_region: None,
                requires_region_coverage: None,
            },
        ]);
        let low_confidence = [crate::detection::Detection {
            class: "person".into(),
            confidence: 0.7,
            bbox: [0.0, 0.0, 640.0, 480.0],
            keypoints: None,
            mask: None,
        }];
        let small = [crate::detection::Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: None,
            mask: None,
        }];
        assert!(
            cascade
                .target_for_detections("face-yolo", &low_confidence, 640, 480)
                .is_none()
        );
        assert!(
            cascade
                .target_for_detections("face-yolo", &small, 640, 480)
                .is_none()
        );
    }

    #[test]
    fn roots_before_children() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let requested: Vec<String> = vec!["pose-standard".into(), "detect-fast".into()];
        let result = cascade.ordered(&requested);
        assert_eq!(result, vec!["detect-fast", "pose-standard"]);
    }

    #[test]
    fn root_always_runs() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        assert!(cascade.parent_of("detect-fast").is_none());
    }

    #[test]
    fn child_skipped_without_parent() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        assert!(cascade.target_for("pose-standard", &[], 640, 480).is_none());
    }

    #[test]
    fn child_skipped_when_parent_has_no_matching_class() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let obs = gate_obs(
            1,
            "detect-fast",
            "chair",
            [0.0, 0.0, 100.0, 100.0],
            0.9,
            true,
            0,
        );
        assert!(
            cascade
                .target_for("pose-standard", &[obs], 640, 480)
                .is_none()
        );
    }

    #[test]
    fn child_runs_when_parent_has_required_class() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let obs = gate_obs(
            1,
            "detect-fast",
            "person",
            [0.0, 0.0, 100.0, 100.0],
            0.9,
            true,
            0,
        );
        assert!(
            cascade
                .target_for("pose-standard", &[obs], 640, 480)
                .is_some()
        );
    }

    #[test]
    fn unconfirmed_observation_cannot_activate_child() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let obs = gate_obs(
            1,
            "detect-fast",
            "person",
            [0.0, 0.0, 100.0, 100.0],
            0.9,
            false,
            0,
        );
        assert!(
            cascade
                .target_for("pose-standard", &[obs], 640, 480)
                .is_none()
        );
    }

    #[test]
    fn unknown_model_does_not_run() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        assert!(cascade.target_for("nonexistent", &[], 640, 480).is_none());
    }

    #[test]
    fn region_coverage_filters_target() {
        let rules = vec![
            CascadeRule {
                model: "detect-fast".into(),
                requires: None,
                requires_class: None,
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: None,
                requires_min_area_ratio: None,
                requires_region: None,
                requires_region_coverage: None,
            },
            CascadeRule {
                model: "pose-standard".into(),
                requires: Some("detect-fast".into()),
                requires_class: Some("person".into()),
                requires_exact_count: None,
                same_frame: false,
                requires_min_confidence: Some(0.5),
                requires_min_area_ratio: Some(0.01),
                requires_region: Some("bed".into()),
                requires_region_coverage: Some(0.5),
            },
        ];
        let regions = HashMap::from([(
            "bed".into(),
            SemanticRegion {
                rect: [0.0, 0.0, 100.0, 100.0],
                label: None,
            },
        )]);
        let cascade = CascadeScheduler::from_rules_and_regions(&rules, regions);
        let obs = gate_obs(
            1,
            "detect-fast",
            "person",
            [50.0, 0.0, 150.0, 100.0],
            0.9,
            true,
            0,
        );
        assert!(
            cascade
                .target_for("pose-standard", &[obs.clone()], 200, 100)
                .is_some()
        );
        assert_eq!(
            cascade
                .target_for("pose-standard", &[obs], 200, 100)
                .unwrap()
                .id,
            Some(1)
        );
    }

    #[test]
    fn validate_accepts_primary_root_with_enabled_map() {
        let config = CascadeConfig {
            rules: test_rules(),
            regions: HashMap::new(),
        };
        let models = HashMap::from([("detect-fast".into(), true), ("pose-standard".into(), true)]);
        assert!(config.validate(&models, "detect-fast").is_empty());
    }

    #[test]
    fn validate_rejects_unknown_model_and_non_root_primary() {
        let config = CascadeConfig {
            rules: test_rules(),
            regions: HashMap::new(),
        };
        let models = HashMap::from([("detect-fast".into(), true)]);
        let errors = config.validate(&models, "pose-standard");
        assert!(errors.iter().any(|e| e.contains("must be a cascade root")));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("unknown model 'pose-standard'"))
        );
    }
}
