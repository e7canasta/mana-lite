use super::super::*;
use std::collections::HashMap;
use std::time::Instant;

use crate::DepthRuleSnapshot;
use crate::config::{
    FsmCatalog, FsmRoles, FsmRoot, FsmState, FsmTransition, ZoneCatalog, ZoneSpec,
};
use crate::health::Health;
use crate::zones::{ZoneEngine, ZoneEvent};

pub(super) fn make_catalog(
    initial: &str,
    states: Vec<(&str, Vec<&str>)>,
    transitions: Vec<FsmTransition>,
) -> FsmCatalog {
    make_catalog_dwell(
        initial,
        states.into_iter().map(|(n, m)| (n, m, None)).collect(),
        transitions,
    )
}

pub(super) fn make_catalog_dwell(
    initial: &str,
    states: Vec<(&str, Vec<&str>, Option<u64>)>,
    transitions: Vec<FsmTransition>,
) -> FsmCatalog {
    let mut state_map = HashMap::new();
    for (name, models, dwell_ms) in states {
        state_map.insert(
            name.to_string(),
            FsmState {
                label: None,
                models: models.iter().map(|s| s.to_string()).collect(),
                dwell_min_ms: dwell_ms,
                face_inside: false,
                face_inside_maybe: false,
            },
        );
    }
    if let Some(state) = state_map.get_mut("detected") {
        state.face_inside = true;
    }
    if let Some(state) = state_map.get_mut("in_bed") {
        state.face_inside = true;
    }
    if let Some(state) = state_map.get_mut("edge") {
        state.face_inside_maybe = true;
    }
    let safe = if state_map.contains_key("blind") {
        "blind"
    } else {
        initial
    };
    let reset = if state_map.contains_key("idle") {
        "idle"
    } else {
        initial
    };
    FsmCatalog {
        fsm: FsmRoot {
            initial: initial.to_string(),
            states: state_map,
            roles: FsmRoles {
                safe: safe.to_string(),
                reset: reset.to_string(),
            },
            transitions,
        },
    }
}

pub(super) fn catalog_with_roles(roles: FsmRoles) -> FsmCatalog {
    let mut catalog = make_catalog(
        "home",
        vec![("home", vec![]), ("offline", vec![]), ("engaged", vec![])],
        vec![
            FsmTransition {
                from: "*".into(),
                to: "offline".into(),
                guards: vec![FsmGuard::DataStale],
                dwell: None,
            },
            FsmTransition {
                from: "offline".into(),
                to: "home".into(),
                guards: vec![FsmGuard::DataFresh],
                dwell: None,
            },
            FsmTransition {
                from: "home".into(),
                to: "engaged".into(),
                guards: vec![FsmGuard::FaceDetected {
                    min_confidence: 0.5,
                }],
                dwell: None,
            },
            FsmTransition {
                from: "engaged".into(),
                to: "home".into(),
                guards: vec![FsmGuard::FaceAbsent],
                dwell: None,
            },
        ],
    );
    catalog.fsm.roles = roles;
    catalog
}

pub(super) fn test_zones(catalog: &FsmCatalog) -> ZoneCatalog {
    let mut zones = HashMap::new();
    let mut needs_face_dwell = false;
    for transition in &catalog.fsm.transitions {
        for guard in &transition.guards {
            match guard {
                FsmGuard::ZonePresent { zone }
                | FsmGuard::ZoneOccupied { zone, .. }
                | FsmGuard::ZoneVacated { zone, .. } => {
                    zones.entry(zone.clone()).or_insert(ZoneSpec {
                        x1: 0,
                        y1: 0,
                        x2: 1,
                        y2: 1,
                        label: None,
                        hysteresis_ms: 0,
                    });
                }
                FsmGuard::FaceInDwell | FsmGuard::FaceNotInDwell => {
                    needs_face_dwell = true;
                }
                _ => {}
            }
        }
    }
    ZoneCatalog {
        zones,
        face_dwell: needs_face_dwell.then_some(ZoneSpec {
            x1: 0,
            y1: 0,
            x2: 1,
            y2: 1,
            label: None,
            hysteresis_ms: 0,
        }),
    }
}

pub(super) fn engine_at(catalog: &FsmCatalog, now: Instant) -> FsmEngine {
    FsmEngine::from_program_at(
        FsmProgram::compile_lenient(catalog, &test_zones(catalog)).expect("test FSM must compile"),
        now,
    )
}

/// A state may only name models the catalog actually declares: a typo here
/// means the state runs no detector, and that must fail at boot.
#[test]
fn unknown_state_model_fails_compilation() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec!["detect-fast"]), ("busy", vec!["typo-model"])],
        vec![
            FsmTransition {
                from: "idle".into(),
                to: "busy".into(),
                guards: vec![],
                dwell: None,
            },
            FsmTransition {
                from: "busy".into(),
                to: "idle".into(),
                guards: vec![],
                dwell: None,
            },
        ],
    );
    let known: std::collections::HashSet<String> = ["detect-fast".to_string()].into();

    let errors = FsmProgram::compile_with_references(&catalog, None, Some(&known), None)
        .expect_err("unknown model must be rejected");

    assert!(
        errors.iter().any(|e| e.contains("typo-model")),
        "expected an error naming the unknown model, got {errors:?}"
    );

    // Same catalog, model names unknown to the caller: no model check runs.
    assert!(
        FsmProgram::compile_with_references(&catalog, None, None, None).is_ok(),
        "model validation must stay opt-in when names are not supplied"
    );
}

pub(super) fn make_snapshot(rule: &str, triggered: bool) -> DepthRuleSnapshot {
    use crate::DepthRuleResult;
    DepthRuleSnapshot::from_results(&[DepthRuleResult {
        rule: rule.into(),
        region: [0, 0, 2, 2],
        metric: crate::DepthMetric::Median,
        threshold_m: 1.5,
        value: Some(1.0),
        triggered,
        valid_pixels: 4,
        valid_ratio: Some(1.0),
        calibration: None,
    }])
}
