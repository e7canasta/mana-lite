use super::super::*;
use std::collections::HashMap;
use std::time::Instant;

use super::catalogs::*;
use crate::DepthRuleSnapshot;
use crate::config::FsmTransition;
use crate::health::Health;
use crate::zones::ZoneEngine;

#[test]
fn state_dwell_min_delays_transition() {
    let catalog = make_catalog_dwell(
        "idle",
        vec![("idle", vec![], Some(500)), ("next", vec![], None)],
        vec![FsmTransition {
            from: "idle".into(),
            to: "next".into(),
            guards: vec![FsmGuard::DataStale],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
        zones: HashMap::new(),
        face_dwell: None,
    });
    let mut health = Health::new_at(100, 50, start);
    let _ = health.evaluate_at(start + std::time::Duration::from_millis(200));

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &FsmSceneContext::default(),
                start + std::time::Duration::from_millis(200),
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "idle");

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &FsmSceneContext::default(),
                start + std::time::Duration::from_millis(600),
            )
            .is_some()
    );
    assert_eq!(engine.current_state(), "next");
}

#[test]
fn transition_dwell_auto_fires() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("blind", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "blind".into(),
            guards: vec![],
            dwell: Some("100ms".into()),
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
                &DepthRuleSnapshot::default(),
                &FsmSceneContext::default(),
                start,
            )
            .is_none()
    );

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &FsmSceneContext::default(),
                start + std::time::Duration::from_millis(200),
            )
            .is_some()
    );
    assert_eq!(engine.current_state(), "blind");
}
