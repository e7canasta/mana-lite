use std::collections::HashMap;

use serde::Deserialize;

use crate::config::ModelCatalog;
use crate::track::Track;

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
    pub fn validate(&self, models: &ModelCatalog, tracked_model: &str) -> Vec<String> {
        let mut errors = Vec::new();
        let tracked_rule = self.rules.iter().find(|rule| rule.model == tracked_model);
        if tracked_rule.is_none() {
            errors.push(format!(
                "tracked model '{}' is missing from cascade rules",
                tracked_model
            ));
        } else if tracked_rule.is_some_and(|rule| rule.requires.is_some()) {
            errors.push(format!(
                "tracked model '{}' must be a cascade root",
                tracked_model
            ));
        }

        for rule in &self.rules {
            if !models.models.contains_key(&rule.model) {
                errors.push(format!("rule references unknown model '{}'", rule.model));
            }
            if let Some(parent) = &rule.requires {
                if !models.models.contains_key(parent) {
                    errors.push(format!(
                        "model '{}' requires unknown parent '{}'",
                        rule.model, parent
                    ));
                }
                let disabled_branch = models
                    .models
                    .get(parent)
                    .is_some_and(|entry| !entry.enabled)
                    && models
                        .models
                        .get(&rule.model)
                        .is_some_and(|entry| !entry.enabled);
                if parent != tracked_model && !disabled_branch {
                    errors.push(format!(
                        "model '{}' requires '{}', but only tracked model '{}' can gate children",
                        rule.model, parent, tracked_model,
                    ));
                }
            }
            if rule.requires_exact_count == Some(0) {
                errors.push(format!(
                    "model '{}' has invalid requires_exact_count",
                    rule.model
                ));
            }
            if let Some(region) = &rule.requires_region {
                if !self.regions.contains_key(region) {
                    errors.push(format!(
                        "model '{}' references unknown region '{}'",
                        rule.model, region
                    ));
                }
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
        for (name, region) in &self.regions {
            let [x1, y1, x2, y2] = region.rect;
            if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite())
                || x2 <= x1
                || y2 <= y1
            {
                errors.push(format!("region '{}' has invalid rect", name));
            }
        }
        errors
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
    pub track_id: Option<u64>,
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
        detections: &[crate::infer::Detection],
    ) -> Option<CascadeTarget> {
        let entry = self.entries.get(model)?;
        if !entry.same_frame || entry.requires.is_none() {
            return None;
        }

        let candidates: Vec<&crate::infer::Detection> = detections
            .iter()
            .filter(|detection| {
                entry
                    .requires_class
                    .as_deref()
                    .is_none_or(|class| detection.class == class)
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
                track_id: None,
                bbox: detection.bbox,
            })
    }

    pub fn target_for(
        &self,
        model: &str,
        tracks: &[&Track],
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

        let candidates: Vec<&Track> = tracks
            .iter()
            .copied()
            .filter(|track| track.is_confirmed && track.misses == 0)
            .filter(|track| track.source_model == parent_key)
            .filter(|track| required_class.is_none_or(|class| track.class == class))
            .filter(|track| min_confidence.is_none_or(|min| track.confidence >= min))
            .filter(|track| {
                min_area_ratio
                    .is_none_or(|min| bbox_area_ratio(&track.bbox, frame_w, frame_h) >= min)
            })
            .filter(|track| {
                required_region.is_none_or(|region_name| {
                    let Some(region) = self.regions.get(region_name) else {
                        return false;
                    };
                    min_region_coverage
                        .is_none_or(|min| bbox_region_coverage(&track.bbox, &region.rect) >= min)
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

        let Some(track) = best else {
            return None;
        };

        Some(CascadeTarget {
            track_id: Some(track.id),
            bbox: track.bbox,
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
        let one_person = [crate::infer::Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: None,
            mask: None,
        }];
        let two_people = [
            crate::infer::Detection {
                class: "person".into(),
                confidence: 0.9,
                bbox: [0.0, 0.0, 100.0, 100.0],
                keypoints: None,
                mask: None,
            },
            crate::infer::Detection {
                class: "person".into(),
                confidence: 0.9,
                bbox: [120.0, 0.0, 220.0, 100.0],
                keypoints: None,
                mask: None,
            },
        ];
        assert!(
            cascade
                .target_for_detections("face-yolo", &one_person)
                .is_some()
        );
        assert!(
            cascade
                .target_for_detections("face-yolo", &two_people)
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
        let tracker = Track {
            id: 1,
            source_model: "detect-fast".into(),
            class: "chair".into(),
            bbox: [0.0, 0.0, 100.0, 100.0],
            confidence: 0.9,
            evidence: Vec::new(),
            velocity: [0.0; 4],
            hits: 2,
            hit_streak: 2,
            misses: 0,
            age: 2,
            is_confirmed: true,
        };
        assert!(
            cascade
                .target_for("pose-standard", &[&tracker], 640, 480)
                .is_none()
        );
    }

    #[test]
    fn child_runs_when_parent_has_required_class() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let tracker = Track {
            id: 1,
            source_model: "detect-fast".into(),
            class: "person".into(),
            bbox: [0.0, 0.0, 100.0, 100.0],
            confidence: 0.9,
            evidence: Vec::new(),
            velocity: [0.0; 4],
            hits: 2,
            hit_streak: 2,
            misses: 0,
            age: 2,
            is_confirmed: true,
        };
        assert!(
            cascade
                .target_for("pose-standard", &[&tracker], 640, 480)
                .is_some()
        );
    }

    #[test]
    fn unconfirmed_track_cannot_activate_child() {
        let cascade = CascadeScheduler::from_rules(&test_rules());
        let tracker = Track {
            id: 1,
            source_model: "detect-fast".into(),
            class: "person".into(),
            bbox: [0.0, 0.0, 100.0, 100.0],
            confidence: 0.9,
            evidence: Vec::new(),
            velocity: [0.0; 4],
            hits: 1,
            hit_streak: 1,
            misses: 0,
            age: 1,
            is_confirmed: false,
        };
        assert!(
            cascade
                .target_for("pose-standard", &[&tracker], 640, 480)
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
        let tracker = Track {
            id: 1,
            source_model: "detect-fast".into(),
            class: "person".into(),
            bbox: [50.0, 0.0, 150.0, 100.0],
            confidence: 0.9,
            evidence: Vec::new(),
            velocity: [0.0; 4],
            hits: 2,
            hit_streak: 2,
            misses: 0,
            age: 2,
            is_confirmed: true,
        };
        assert!(
            cascade
                .target_for("pose-standard", &[&tracker], 200, 100)
                .is_some()
        );
        assert_eq!(
            cascade
                .target_for("pose-standard", &[&tracker], 200, 100)
                .unwrap()
                .track_id,
            Some(1)
        );
    }

    #[test]
    fn configured_cascade_has_valid_pose_rule() {
        let config: CascadeConfig =
            crate::config::load_config(std::path::Path::new("config/cascade.toml")).unwrap();
        let models =
            crate::config::load_model_catalog(std::path::Path::new("config/models.toml")).unwrap();
        assert!(config.validate(&models, "detect-fast").is_empty());
        assert_eq!(
            config
                .rules
                .iter()
                .find(|r| r.model == "pose-standard")
                .and_then(|r| r.requires_region.as_deref()),
            Some("bed")
        );
    }
}
