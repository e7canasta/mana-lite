use super::fsm::{FsmCatalog, FsmGuard};
use super::models::ModelCatalog;
use super::zones::ZoneCatalog;
use crate::depth::DepthRules;
use std::collections::HashSet;

pub fn validate_fsm(
    fsm: &FsmCatalog,
    models: &ModelCatalog,
    zones: &Option<ZoneCatalog>,
    depth_rules: &Option<DepthRules>,
) -> Vec<String> {
    let mut errors = Vec::new();

    if !fsm.fsm.states.contains_key(&fsm.fsm.initial) {
        errors.push(format!(
            "initial state '{}' not found in states",
            fsm.fsm.initial
        ));
    }

    for t in &fsm.fsm.transitions {
        if t.from != "*" && !fsm.fsm.states.contains_key(&t.from) {
            errors.push(format!("transition from unknown state '{}'", t.from));
        }
        if !fsm.fsm.states.contains_key(&t.to) {
            errors.push(format!("transition to unknown state '{}'", t.to));
        }
    }

    let with_exit: HashSet<&str> = fsm
        .fsm
        .transitions
        .iter()
        .filter(|t| t.from != "*")
        .map(|t| t.from.as_str())
        .collect();
    for name in fsm.fsm.states.keys() {
        if !with_exit.contains(name.as_str()) {
            errors.push(format!("state '{name}' has no outgoing transition (sink)"));
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
                FsmGuard::ZonePresent { zone }
                | FsmGuard::ZoneOccupied { zone, .. }
                | FsmGuard::ZoneVacated { zone, .. } => {
                    if let Some(zc) = zones {
                        if !zc.zones.contains_key(zone) {
                            errors.push(format!(
                                "transition {}→{} references zone '{}' not found in zone catalog",
                                t.from, t.to, zone
                            ));
                        }
                    }
                }
                FsmGuard::DepthRule { rule, .. } => {
                    if let Some(rules) = depth_rules {
                        if !rules.rules.iter().any(|r| r.name == *rule) {
                            errors.push(format!(
                                "transition {}→{} references depth rule '{}' not found in depth-rules",
                                t.from, t.to, rule
                            ));
                        }
                    }
                }
                FsmGuard::FaceInDwell | FsmGuard::FaceNotInDwell => {
                    if let Some(zc) = zones {
                        if zc.face_dwell.is_none() {
                            errors.push(format!(
                                "transition {}→{} requires face_dwell in zone catalog",
                                t.from, t.to
                            ));
                        }
                    }
                }
                FsmGuard::Cardinality { value } => {
                    if !matches!(value.as_str(), "empty" | "single" | "multiple") {
                        errors.push(format!(
                            "transition {}→{} has invalid cardinality '{}'",
                            t.from, t.to, value
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    errors
}

pub fn validate_model_catalog(models: &ModelCatalog) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, entry) in &models.models {
        if !entry.is_valid() {
            errors.push(format!(
                "models.{name}: confidence, iou, max_det and imgsz must be valid positive model settings"
            ));
        }
        if !entry.postprocess.is_valid() {
            errors.push(format!(
                "models.{name}.postprocess: confidence and area thresholds must be finite and ordered"
            ));
        }
        if entry.crop.as_ref().is_some_and(|crop| !crop.is_valid()) {
            errors.push(format!(
                "models.{name}.crop: square_size must be positive and upper_fraction must be within 0..=1"
            ));
        }
    }
    errors
}
