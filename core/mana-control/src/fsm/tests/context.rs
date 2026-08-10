use super::super::*;
use std::collections::HashMap;
use std::time::Instant;

use super::catalogs::*;
use crate::DepthRuleSnapshot;
use crate::config::{
    FsmCatalog, FsmRoles, FsmRoot, FsmState, FsmTransition, ZoneCatalog, ZoneSpec,
};
use crate::health::Health;
use crate::zones::{ZoneEngine, ZoneEvent};

#[test]
fn cardinality_guard_matches_scene_context() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("single", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "single".into(),
            guards: vec![FsmGuard::Cardinality {
                value: "single".into(),
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

    let result = engine.evaluate_with_context_at(
        &[],
        None,
        &health,
        &DepthRuleSnapshot::default(),
        &scene,
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
            guards: vec![FsmGuard::Cardinality {
                value: "single".into(),
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

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &scene,
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
