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

fn occupy_zone(engine: &mut ZoneEngine, zone_rect_intersects: bool, now: Instant) {
    let bbox = if zone_rect_intersects {
        [0.0, 0.0, 2.0, 2.0]
    } else {
        [10.0, 10.0, 20.0, 20.0]
    };
    let track = crate::track::Track {
        id: 1,
        source_model: "detect-fast".into(),
        class: "person".into(),
        bbox,
        confidence: 0.9,
        evidence: Vec::new(),
        kalman: crate::kalman::Kalman7::default(),
        hits: 3,
        hit_streak: 3,
        misses: 0,
        age: 3,
        time_since_update_ms: 0,
        is_confirmed: true,
    };
    let _ = engine.evaluate_at(&[&track], now);
}

#[test]
fn zone_present_guard_fires_when_engine_reports_occupied() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("watching", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "watching".into(),
            guards: vec![FsmGuard::ZonePresent { zone: "bed".into() }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let mut zones = ZoneEngine::from_catalog(&test_zones(&catalog));
    occupy_zone(&mut zones, true, start);
    assert!(zones.is_occupied("bed"));
    let health = Health::new_at(10_000, 5_000, start);

    let result = engine.evaluate_with_context_at(
        &[],
        Some(&zones),
        &health,
        &DepthRuleSnapshot::default(),
        &FsmSceneContext::default(),
        start,
    );
    assert_eq!(result.expect("zone present").to, "watching");
}

#[test]
fn zone_present_guard_rejects_when_vacant() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("watching", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "watching".into(),
            guards: vec![FsmGuard::ZonePresent { zone: "bed".into() }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&test_zones(&catalog));
    let health = Health::new_at(10_000, 5_000, start);

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &FsmSceneContext::default(),
                start,
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "idle");
}

#[test]
fn zone_vacated_guard_fires_on_vacated_event() {
    let catalog = make_catalog(
        "watching",
        vec![("watching", vec![]), ("idle", vec![])],
        vec![FsmTransition {
            from: "watching".into(),
            to: "idle".into(),
            guards: vec![FsmGuard::ZoneVacated {
                zone: "bed".into(),
                min_confidence: None,
                min_duration_ms: None,
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&test_zones(&catalog));
    let health = Health::new_at(10_000, 5_000, start);
    let events = vec![ZoneEvent::Vacated {
        zone: "bed".into(),
        label: None,
        track_id: 1,
        class: "person".into(),
    }];

    let result = engine.evaluate_with_context_at(
        &events,
        Some(&zones),
        &health,
        &DepthRuleSnapshot::default(),
        &FsmSceneContext::default(),
        start,
    );
    assert_eq!(result.expect("zone vacated").to, "idle");
}

#[test]
fn zone_vacated_guard_rejects_without_event() {
    let catalog = make_catalog(
        "watching",
        vec![("watching", vec![]), ("idle", vec![])],
        vec![FsmTransition {
            from: "watching".into(),
            to: "idle".into(),
            guards: vec![FsmGuard::ZoneVacated {
                zone: "bed".into(),
                min_confidence: None,
                min_duration_ms: None,
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&test_zones(&catalog));
    let health = Health::new_at(10_000, 5_000, start);

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &FsmSceneContext::default(),
                start,
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "watching");
}

#[test]
fn all_zones_vacant_guard_fires_when_engine_all_vacant() {
    let catalog = make_catalog(
        "watching",
        vec![("watching", vec![]), ("idle", vec![])],
        vec![FsmTransition {
            from: "watching".into(),
            to: "idle".into(),
            guards: vec![FsmGuard::AllZonesVacant {
                min_duration_ms: None,
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&ZoneCatalog {
        zones: HashMap::from([(
            "bed".into(),
            ZoneSpec {
                x1: 0,
                y1: 0,
                x2: 1,
                y2: 1,
                label: None,
                hysteresis_ms: 0,
            },
        )]),
        face_dwell: None,
    });
    assert!(zones.all_vacant());
    let health = Health::new_at(10_000, 5_000, start);

    let result = engine.evaluate_with_context_at(
        &[],
        Some(&zones),
        &health,
        &DepthRuleSnapshot::default(),
        &FsmSceneContext::default(),
        start,
    );
    assert_eq!(result.expect("all vacant").to, "idle");
}

#[test]
fn all_zones_vacant_guard_rejects_when_occupied() {
    let catalog = make_catalog(
        "watching",
        vec![("watching", vec![]), ("idle", vec![])],
        vec![FsmTransition {
            from: "watching".into(),
            to: "idle".into(),
            guards: vec![FsmGuard::AllZonesVacant {
                min_duration_ms: None,
            }],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let mut zones = ZoneEngine::from_catalog(&ZoneCatalog {
        zones: HashMap::from([(
            "bed".into(),
            ZoneSpec {
                x1: 0,
                y1: 0,
                x2: 1,
                y2: 1,
                label: None,
                hysteresis_ms: 0,
            },
        )]),
        face_dwell: None,
    });
    occupy_zone(&mut zones, true, start);
    assert!(!zones.all_vacant());
    let health = Health::new_at(10_000, 5_000, start);

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &FsmSceneContext::default(),
                start,
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "watching");
}
