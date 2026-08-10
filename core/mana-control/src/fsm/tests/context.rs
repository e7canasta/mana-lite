use super::super::*;
use std::time::Instant;

use super::catalogs::*;
use crate::DepthRuleSnapshot;
use crate::config::FsmTransition;
use crate::health::Health;
use crate::signals::{SignalTable, SignalValue, scene_signal_catalog};

fn cardinality_snapshot(value: &str) -> crate::signals::SceneSignalsSnapshot {
    let mut table = SignalTable::new();
    table
        .insert(
            scene_signal_catalog(),
            crate::domain::SignalTag::new("ocupacion.cardinalidad"),
            SignalValue::Label(value.into()),
        )
        .unwrap();
    table.snapshot(scene_signal_catalog())
}

#[test]
fn cardinality_guard_matches_scene_context() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("single", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "single".into(),
            guards: vec![FsmGuard::Signal {
                tag: "ocupacion.cardinalidad".into(),
                op: "==".into(),
                value: SignalLiteral::Text("single".into()),
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let scene = FsmSceneContext {
        cardinality: Some("single".into()),
        ..Default::default()
    };

    let snapshot = cardinality_snapshot("single");
    let result = engine.evaluate_with_signals_at(
        &[],
        None,
        &health,
        &DepthRuleSnapshot::default(),
        &scene,
        &snapshot,
        start,
    );
    assert_eq!(result.expect("cardinality match").to, "single");
}

#[test]
fn cardinality_guard_rejects_mismatch() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("single", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "single".into(),
            guards: vec![FsmGuard::Signal {
                tag: "ocupacion.cardinalidad".into(),
                op: "==".into(),
                value: SignalLiteral::Text("single".into()),
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let scene = FsmSceneContext {
        cardinality: Some("multiple".into()),
        ..Default::default()
    };

    let snapshot = cardinality_snapshot("multiple");
    assert!(
        engine
            .evaluate_with_signals_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &scene,
                &snapshot,
                start,
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "idle");
}

#[test]
fn face_at_edge_guard_requires_at_edge() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("edge", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "edge".into(),
            guards: vec![FsmGuard::FaceAtEdge],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let at_edge = FsmSceneContext {
        at_edge: true,
        ..Default::default()
    };

    let result = engine.evaluate_with_context_at(
        &[],
        None,
        &health,
        &DepthRuleSnapshot::default(),
        &at_edge,
        start,
    );
    assert_eq!(result.expect("at edge").to, "edge");
}

#[test]
fn face_at_edge_guard_rejects_when_not_at_edge() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("edge", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "edge".into(),
            guards: vec![FsmGuard::FaceAtEdge],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let not_at_edge = FsmSceneContext {
        at_edge: false,
        ..Default::default()
    };

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &not_at_edge,
                start,
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "idle");
}
