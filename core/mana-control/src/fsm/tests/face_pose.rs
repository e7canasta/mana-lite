use super::super::*;
use std::time::Instant;

use super::catalogs::{engine_at, make_catalog, snapshot_with_signals};
use crate::DepthRuleSnapshot;
use crate::config::FsmTransition;
use crate::health::Health;
use crate::signals::{Ratio, SignalValue};

fn cross_validation_catalog() -> crate::config::FsmCatalog {
    make_catalog(
        "watching",
        vec![("watching", vec![]), ("validated_alert", vec![])],
        vec![FsmTransition {
            from: "watching".into(),
            to: "validated_alert".into(),
            guards: vec![
                FsmGuard::Signal {
                    tag: "cara.presente".into(),
                    op: "==".into(),
                    value: SignalLiteral::Bool(true),
                },
                FsmGuard::Signal {
                    tag: "cara.pose_validada".into(),
                    op: "==".into(),
                    value: SignalLiteral::Bool(true),
                },
                FsmGuard::Signal {
                    tag: "cara.pose_calidad".into(),
                    op: ">=".into(),
                    value: SignalLiteral::Float(0.70),
                },
            ],
            dwell: None,
        }],
    )
}

fn evaluate(entries: &[(&str, SignalValue)]) -> Option<FsmTransitionResult> {
    let start = Instant::now(); // cfg(test)
    let catalog = cross_validation_catalog();
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    engine.evaluate_with_signals_at(
        &[],
        None,
        &health,
        &DepthRuleSnapshot::default(),
        &FsmSceneContext::default(),
        &snapshot_with_signals(entries),
        start,
    )
}

#[test]
fn validated_alert_requires_positive_face_pose_and_quality() {
    let result = evaluate(&[
        ("cara.presente", SignalValue::Bool(true)),
        ("cara.pose_validada", SignalValue::Bool(true)),
        (
            "cara.pose_calidad",
            SignalValue::Ratio(Ratio::new(0.85).unwrap()),
        ),
    ])
    .expect("positive validation should enter the alert");
    assert_eq!(result.to, "validated_alert");
}

#[test]
fn absent_false_or_low_quality_validation_does_not_alert() {
    assert!(
        evaluate(&[
            ("cara.presente", SignalValue::Bool(true)),
            (
                "cara.pose_calidad",
                SignalValue::Ratio(Ratio::new(0.85).unwrap()),
            ),
        ])
        .is_none()
    );
    assert!(
        evaluate(&[
            ("cara.presente", SignalValue::Bool(true)),
            ("cara.pose_validada", SignalValue::Bool(false)),
            (
                "cara.pose_calidad",
                SignalValue::Ratio(Ratio::new(0.85).unwrap()),
            ),
        ])
        .is_none()
    );
    assert!(
        evaluate(&[
            ("cara.presente", SignalValue::Bool(true)),
            ("cara.pose_validada", SignalValue::Bool(true)),
            (
                "cara.pose_calidad",
                SignalValue::Ratio(Ratio::new(0.69).unwrap()),
            ),
        ])
        .is_none()
    );
}
