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

/// `ZoneEvent::Vacated` carries no confidence, so a `min_confidence` on a
/// zone_vacated guard can never be evaluated. Accepting and ignoring it
/// would let a catalog advertise a gate that does not exist.
#[test]
fn min_confidence_on_zone_vacated_is_rejected() {
    let catalog = make_catalog(
        "watching",
        vec![("watching", vec![]), ("idle", vec![])],
        vec![
            FsmTransition {
                from: "watching".into(),
                to: "idle".into(),
                guards: vec![FsmGuard::ZoneVacated {
                    zone: "bed".into(),
                    min_confidence: Some(0.5),
                    min_duration_ms: None,
                }],
                dwell: None,
            },
            FsmTransition {
                from: "idle".into(),
                to: "watching".into(),
                guards: vec![],
                dwell: None,
            },
        ],
    );

    let errors = FsmProgram::compile_lenient(&catalog, &test_zones(&catalog))
        .expect_err("min_confidence on zone_vacated must be rejected");
    assert!(
        errors.iter().any(|e| e.contains("min_confidence")),
        "expected an error about min_confidence, got {errors:?}"
    );
}

#[test]
fn zone_occupied_triggers_transition() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("watching", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "watching".into(),
            guards: vec![FsmGuard::ZoneOccupied {
                zone: "bed".into(),
                min_confidence: 0.5,
                min_duration_ms: Some(0),
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

    let events = vec![ZoneEvent::Occupied {
        zone: "bed".into(),
        label: None,
        track_id: 1,
        class: "person".into(),
        confidence: 0.9,
    }];
    let result = engine.evaluate_with_context_at(
        &events,
        Some(&zones),
        &health,
        &DepthRuleSnapshot::default(),
        &FsmSceneContext::default(),
        start,
    );

    assert!(result.is_some());
    assert_eq!(result.unwrap().to, "watching");
    assert_eq!(engine.current_state(), "watching");
}

#[test]
fn low_confidence_guard_rejected() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("watching", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "watching".into(),
            guards: vec![FsmGuard::ZoneOccupied {
                zone: "bed".into(),
                min_confidence: 0.8,
                min_duration_ms: Some(0),
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

    let events = vec![ZoneEvent::Occupied {
        zone: "bed".into(),
        label: None,
        track_id: 1,
        class: "person".into(),
        confidence: 0.6,
    }];
    let result = engine.evaluate_with_context_at(
        &events,
        Some(&zones),
        &health,
        &DepthRuleSnapshot::default(),
        &FsmSceneContext::default(),
        start,
    );
    assert!(result.is_none());
    assert_eq!(engine.current_state(), "idle");
}

#[test]
fn guards_must_all_be_true() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("watching", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "watching".into(),
            guards: vec![
                FsmGuard::ZoneOccupied {
                    zone: "bed".into(),
                    min_confidence: 0.5,
                    min_duration_ms: Some(0),
                },
                FsmGuard::ZoneOccupied {
                    zone: "chair".into(),
                    min_confidence: 0.5,
                    min_duration_ms: Some(0),
                },
            ],
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

    let events = vec![ZoneEvent::Occupied {
        zone: "bed".into(),
        label: None,
        track_id: 1,
        class: "person".into(),
        confidence: 0.9,
    }];
    let result = engine.evaluate_with_context_at(
        &events,
        Some(&zones),
        &health,
        &DepthRuleSnapshot::default(),
        &FsmSceneContext::default(),
        start,
    );
    assert!(result.is_none());
    assert_eq!(engine.current_state(), "idle");
}
