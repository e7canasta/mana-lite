use super::super::*;
use std::collections::HashMap;
use std::time::Instant;

use crate::DepthRuleSnapshot;
use crate::config::{
    FsmCatalog, FsmRoles, FsmRoot, FsmState, FsmTransition, ZoneCatalog, ZoneSpec,
};
use crate::signals::{SceneSignalsSnapshot, SignalTable, SignalValue, scene_signal_catalog};

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

pub(super) fn snapshot_with_signals(entries: &[(&str, SignalValue)]) -> SceneSignalsSnapshot {
    let mut table = SignalTable::new();
    for (tag, value) in entries {
        table
            .insert(
                scene_signal_catalog(),
                crate::domain::SignalTag::new(tag),
                value.clone(),
            )
            .unwrap();
    }
    table.snapshot(scene_signal_catalog())
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
                guards: vec![FsmGuard::Signal {
                    tag: "cara.confianza".into(),
                    op: ">=".into(),
                    value: SignalLiteral::Float(0.5),
                }],
                dwell: None,
            },
            FsmTransition {
                from: "engaged".into(),
                to: "home".into(),
                guards: vec![FsmGuard::Signal {
                    tag: "cara.presente".into(),
                    op: "==".into(),
                    value: SignalLiteral::Bool(false),
                }],
                dwell: None,
            },
        ],
    );
    catalog.fsm.roles = roles;
    catalog
}

pub(super) fn test_zones(catalog: &FsmCatalog) -> ZoneCatalog {
    let mut zones = HashMap::new();
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
                _ => {}
            }
        }
    }
    ZoneCatalog {
        zones,
        face_dwell: None,
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

fn signal_errors(guard: FsmGuard, from: &str) -> Vec<String> {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("target", vec![])],
        vec![FsmTransition {
            from: from.into(),
            to: "target".into(),
            guards: vec![guard],
            dwell: None,
        }],
    );
    FsmProgram::compile_lenient(&catalog, &test_zones(&catalog))
        .expect_err("invalid signal guard must be rejected")
}

#[test]
fn signal_guard_compiles_and_reads_the_frozen_snapshot() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("target", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "target".into(),
            guards: vec![FsmGuard::Signal {
                tag: "persona.presente".into(),
                op: "==".into(),
                value: SignalLiteral::Bool(true),
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let mut table = SignalTable::new();
    table
        .insert(
            scene_signal_catalog(),
            crate::domain::SignalTag::new("persona.presente"),
            SignalValue::Bool(true),
        )
        .unwrap();
    let snapshot = table.snapshot(scene_signal_catalog());
    let health = crate::health::Health::new_at(10_000, 5_000, start);

    let result = engine.evaluate_with_signals_at(
        &[],
        None,
        &health,
        &DepthRuleSnapshot::default(),
        &crate::fsm::FsmSceneContext::default(),
        &snapshot,
        start,
    );

    assert_eq!(result.expect("signal should match").to, "target");
}

#[test]
fn signal_literals_deserialize_without_losing_numeric_shape() {
    let catalog: FsmCatalog = toml::from_str(
        r#"
            [fsm]
            initial = "idle"

            [fsm.states.idle]
            [fsm.states.target]

            [fsm.roles]
            safe = "idle"
            reset = "idle"

            [[fsm.transitions]]
            from = "idle"
            to = "target"
            guards = [
                { type = "signal", tag = "cara.confianza", op = ">=", value = 0.8 },
                { type = "signal", tag = "persona.cantidad", op = ">", value = 1 },
            ]
        "#,
    )
    .expect("signal TOML should deserialize");

    assert!(matches!(
        &catalog.fsm.transitions[0].guards[0],
        FsmGuard::Signal {
            tag,
            op,
            value: SignalLiteral::Float(value),
        } if tag == "cara.confianza" && op == ">=" && (*value - 0.8).abs() < f64::EPSILON
    ));
    assert!(matches!(
        &catalog.fsm.transitions[0].guards[1],
        FsmGuard::Signal {
            tag,
            op,
            value: SignalLiteral::Integer(1),
        } if tag == "persona.cantidad" && op == ">"
    ));
}

#[test]
fn unknown_signal_tag_reports_transition_guard_and_expectation() {
    let errors = signal_errors(
        FsmGuard::Signal {
            tag: "unknown.tag".into(),
            op: "==".into(),
            value: SignalLiteral::Bool(true),
        },
        "idle",
    );
    assert!(
        errors.iter().any(|error| {
            error.contains("transition idle→target")
                && error.contains("guard 0")
                && error.contains("unknown.tag")
                && error.contains("not declared")
        }),
        "unexpected errors: {errors:?}"
    );
}

#[test]
fn incompatible_signal_operator_reports_expected_kind() {
    let ratio_errors = signal_errors(
        FsmGuard::Signal {
            tag: "cara.confianza".into(),
            op: "==".into(),
            value: SignalLiteral::Float(0.5),
        },
        "idle",
    );
    assert!(
        ratio_errors.iter().any(|error| {
            error.contains("cara.confianza") && error.contains("==") && error.contains("Ratio")
        }),
        "unexpected ratio errors: {ratio_errors:?}"
    );

    let bool_errors = signal_errors(
        FsmGuard::Signal {
            tag: "persona.presente".into(),
            op: ">=".into(),
            value: SignalLiteral::Bool(true),
        },
        "idle",
    );
    assert!(
        bool_errors.iter().any(|error| {
            error.contains("persona.presente") && error.contains(">=") && error.contains("Bool")
        }),
        "unexpected bool errors: {bool_errors:?}"
    );
}

#[test]
fn invalid_signal_values_report_type_and_label_expectations() {
    let count_errors = signal_errors(
        FsmGuard::Signal {
            tag: "persona.cantidad".into(),
            op: ">=".into(),
            value: SignalLiteral::Integer(-1),
        },
        "idle",
    );
    assert!(
        count_errors
            .iter()
            .any(|error| { error.contains("persona.cantidad") && error.contains("non-negative") }),
        "unexpected count errors: {count_errors:?}"
    );

    let label_errors = signal_errors(
        FsmGuard::Signal {
            tag: "ocupacion.cardinalidad".into(),
            op: "==".into(),
            value: SignalLiteral::Text("full".into()),
        },
        "idle",
    );
    assert!(
        label_errors.iter().any(|error| {
            error.contains("ocupacion.cardinalidad")
                && error.contains("full")
                && error.contains("empty")
        }),
        "unexpected label errors: {label_errors:?}"
    );
}

#[test]
fn signal_guard_is_rejected_on_wildcard_transition() {
    let errors = signal_errors(
        FsmGuard::Signal {
            tag: "persona.presente".into(),
            op: "==".into(),
            value: SignalLiteral::Bool(true),
        },
        "*",
    );
    assert!(
        errors
            .iter()
            .any(|error| { error.contains("persona.presente") && error.contains("wildcard") }),
        "unexpected wildcard errors: {errors:?}"
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
