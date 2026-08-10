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
fn depth_rule_guard_fires_transition() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("approaching", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "approaching".into(),
            guards: vec![FsmGuard::DepthRule {
                rule: "bed-approach".into(),
                triggered: true,
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
        zones: HashMap::new(),
        face_dwell: None,
    });
    let health = Health::new_at(10_000, 5_000, start);

    let triggered = make_snapshot("bed-approach", true);
    let result = engine.evaluate_with_context_at(
        &[],
        Some(&zones),
        &health,
        &triggered,
        &FsmSceneContext::default(),
        start,
    );
    assert!(result.is_some());
    assert_eq!(engine.current_state(), "approaching");
}

#[test]
fn depth_rule_guard_requires_evidence() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("approaching", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "approaching".into(),
            guards: vec![FsmGuard::DepthRule {
                rule: "bed-approach".into(),
                triggered: true,
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
        zones: HashMap::new(),
        face_dwell: None,
    });
    let health = Health::new_at(10_000, 5_000, start);

    // Sin evidencia de la regla (mapa, ROI o cobertura) -> guard falso.
    let empty = DepthRuleSnapshot::default();
    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &empty,
                &FsmSceneContext::default(),
                start
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "idle");

    let not_triggered = make_snapshot("bed-approach", false);
    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &not_triggered,
                &FsmSceneContext::default(),
                start
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "idle");
}

#[test]
fn depth_rule_guard_supports_inverted_trigger() {
    let catalog = make_catalog(
        "approaching",
        vec![("approaching", vec![]), ("idle", vec![])],
        vec![FsmTransition {
            from: "approaching".into(),
            to: "idle".into(),
            guards: vec![FsmGuard::DepthRule {
                rule: "bed-approach".into(),
                triggered: false,
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
        zones: HashMap::new(),
        face_dwell: None,
    });
    let health = Health::new_at(10_000, 5_000, start);

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &make_snapshot("bed-approach", true),
                &FsmSceneContext::default(),
                start
            )
            .is_none()
    );
    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &make_snapshot("bed-approach", false),
                &FsmSceneContext::default(),
                start
            )
            .is_some()
    );
    assert_eq!(engine.current_state(), "idle");
}
