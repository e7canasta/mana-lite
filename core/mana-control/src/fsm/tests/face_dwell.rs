use super::super::*;
use std::collections::HashMap;
use std::time::Instant;

use super::catalogs::*;
use crate::DepthRuleSnapshot;
use crate::config::FsmTransition;
use crate::health::Health;
use crate::signals::SignalValue;
use crate::zones::ZoneEngine;

#[test]
fn face_dwell_guard_uses_face_context() {
    let catalog = make_catalog(
        "outside",
        vec![("outside", vec![]), ("inside", vec![])],
        vec![FsmTransition {
            from: "outside".into(),
            to: "inside".into(),
            guards: vec![FsmGuard::FaceInDwell],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let outside = FsmSceneContext {
        face_in_dwell: Some(false),
        ..Default::default()
    };
    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &outside,
                start,
            )
            .is_none()
    );

    let inside = FsmSceneContext {
        face_in_dwell: Some(true),
        ..Default::default()
    };
    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &inside,
                start,
            )
            .is_some()
    );
    assert_eq!(engine.current_state(), "inside");
}

#[test]
fn face_dwell_snapshot_reports_state_and_candidate_timer() {
    let catalog = make_catalog(
        "searching",
        vec![("searching", vec![]), ("in_bed", vec![])],
        vec![FsmTransition {
            from: "searching".into(),
            to: "in_bed".into(),
            guards: vec![FsmGuard::FaceInDwell],
            dwell: Some("1000ms".into()),
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let inside = FsmSceneContext {
        cardinality: Some("single".into()),
        person_present: true,
        face_present: true,
        face_confidence: Some(0.9),
        face_in_dwell: Some(true),
        at_edge: false,
        face_model_ran: true,
    };

    assert!(
        engine
            .evaluate_with_context_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &inside,
                start,
            )
            .is_none()
    );
    let snapshot = engine.snapshot_at(start + std::time::Duration::from_millis(500));
    assert_eq!(snapshot.state, "searching");
    assert_eq!(snapshot.state_dwell_ms, 500);
    assert_eq!(snapshot.active_timers.len(), 1);
    assert_eq!(snapshot.active_timers[0].trigger, "searching→in_bed");
    assert_eq!(snapshot.active_timers[0].elapsed_ms, 500);
    assert_eq!(snapshot.active_timers[0].required_ms, 1_000);
}

#[test]
fn edge_priority_blocks_dwell_until_face_leaves_edge() {
    let catalog = make_catalog(
        "edge",
        vec![("edge", vec![]), ("in_bed", vec![]), ("other", vec![])],
        vec![
            FsmTransition {
                from: "edge".into(),
                to: "in_bed".into(),
                guards: vec![FsmGuard::FaceInDwell, FsmGuard::FaceNotAtEdge],
                dwell: Some("1000ms".into()),
            },
            FsmTransition {
                from: "edge".into(),
                to: "other".into(),
                guards: vec![
                    FsmGuard::Signal {
                        tag: "persona.presente".into(),
                        op: "==".into(),
                        value: SignalLiteral::Bool(true),
                    },
                    FsmGuard::FaceAbsent,
                    FsmGuard::FaceNotInDwell,
                    FsmGuard::FaceNotAtEdge,
                ],
                dwell: Some("700ms".into()),
            },
        ],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let signals = snapshot_with_signals(&[("persona.presente", SignalValue::Bool(true))]);
    let at_edge = FsmSceneContext {
        person_present: true,
        face_present: true,
        face_in_dwell: Some(true),
        at_edge: true,
        ..Default::default()
    };

    assert!(
        engine
            .evaluate_with_signals_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &at_edge,
                &signals,
                start,
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "edge");

    let away_from_edge = FsmSceneContext {
        at_edge: false,
        ..at_edge
    };
    assert!(
        engine
            .evaluate_with_signals_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &away_from_edge,
                &signals,
                start + std::time::Duration::from_millis(1_000),
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "edge");
    assert_eq!(
        engine
            .evaluate_with_signals_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &away_from_edge,
                &signals,
                start + std::time::Duration::from_millis(2_000),
            )
            .expect("dwell should complete after leaving edge")
            .to,
        "in_bed"
    );
}

#[test]
fn face_dwell_and_inside_latch_drive_exiting() {
    let catalog = make_catalog(
        "searching",
        vec![
            ("searching", vec![]),
            ("detected", vec![]),
            ("exiting", vec![]),
            ("idle", vec![]),
        ],
        vec![
            FsmTransition {
                from: "searching".into(),
                to: "detected".into(),
                guards: vec![FsmGuard::FaceDetected {
                    min_confidence: 0.5,
                }],
                dwell: Some("500ms".into()),
            },
            FsmTransition {
                from: "detected".into(),
                to: "exiting".into(),
                guards: vec![
                    FsmGuard::Signal {
                        tag: "persona.presente".into(),
                        op: "==".into(),
                        value: SignalLiteral::Bool(false),
                    },
                    FsmGuard::FaceWasInside,
                ],
                dwell: Some("1s".into()),
            },
            FsmTransition {
                from: "exiting".into(),
                to: "idle".into(),
                guards: vec![FsmGuard::Signal {
                    tag: "persona.presente".into(),
                    op: "==".into(),
                    value: SignalLiteral::Bool(false),
                }],
                dwell: Some("1s".into()),
            },
        ],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
        zones: HashMap::new(),
        face_dwell: None,
    });
    let health = Health::new_at(10_000, 5_000, start);
    let present_signals = snapshot_with_signals(&[("persona.presente", SignalValue::Bool(true))]);
    let present = FsmSceneContext {
        cardinality: Some("single".into()),
        person_present: true,
        face_present: true,
        face_confidence: Some(0.9),
        face_in_dwell: Some(false),
        at_edge: false,
        face_model_ran: true,
    };

    assert!(
        engine
            .evaluate_with_signals_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &present,
                &present_signals,
                start,
            )
            .is_none()
    );
    assert!(
        engine
            .evaluate_with_signals_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &present,
                &present_signals,
                start + std::time::Duration::from_millis(500),
            )
            .is_some()
    );
    assert_eq!(engine.current_state(), "detected");

    let absent = FsmSceneContext {
        cardinality: Some("single".into()),
        person_present: false,
        face_present: false,
        face_confidence: None,
        face_in_dwell: Some(false),
        at_edge: false,
        face_model_ran: true,
    };
    let absent_signals = snapshot_with_signals(&[("persona.presente", SignalValue::Bool(false))]);
    assert!(
        engine
            .evaluate_with_signals_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &absent,
                &absent_signals,
                start + std::time::Duration::from_millis(1_000),
            )
            .is_none()
    );
    assert!(
        engine
            .evaluate_with_signals_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &absent,
                &absent_signals,
                start + std::time::Duration::from_millis(2_000),
            )
            .is_some()
    );
    assert_eq!(engine.current_state(), "exiting");
    assert!(
        engine
            .evaluate_with_signals_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &absent,
                &absent_signals,
                start + std::time::Duration::from_millis(8_000),
            )
            .is_none()
    );
    assert!(
        engine
            .evaluate_with_signals_at(
                &[],
                Some(&zones),
                &health,
                &DepthRuleSnapshot::default(),
                &absent,
                &absent_signals,
                start + std::time::Duration::from_millis(9_000),
            )
            .is_some()
    );
    assert_eq!(engine.current_state(), "idle");
}

#[test]
fn face_without_inside_history_cannot_enter_exiting() {
    let catalog = make_catalog(
        "other",
        vec![("other", vec![]), ("exiting", vec![]), ("idle", vec![])],
        vec![
            FsmTransition {
                from: "other".into(),
                to: "exiting".into(),
                guards: vec![
                    FsmGuard::Signal {
                        tag: "persona.presente".into(),
                        op: "==".into(),
                        value: SignalLiteral::Bool(false),
                    },
                    FsmGuard::FaceWasInside,
                ],
                dwell: None,
            },
            FsmTransition {
                from: "other".into(),
                to: "idle".into(),
                guards: vec![
                    FsmGuard::Signal {
                        tag: "persona.presente".into(),
                        op: "==".into(),
                        value: SignalLiteral::Bool(false),
                    },
                    FsmGuard::FaceWasNotInside,
                ],
                dwell: None,
            },
        ],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let absent = FsmSceneContext {
        cardinality: Some("single".into()),
        person_present: false,
        ..Default::default()
    };
    let absent_signals = snapshot_with_signals(&[("persona.presente", SignalValue::Bool(false))]);
    let result = engine.evaluate_with_signals_at(
        &[],
        None,
        &health,
        &DepthRuleSnapshot::default(),
        &absent,
        &absent_signals,
        start,
    );
    assert_eq!(result.expect("fallback to idle").to, "idle");
}
