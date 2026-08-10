use super::super::*;
use std::collections::HashMap;
use std::time::Instant;

use super::catalogs::*;
use crate::DepthRuleSnapshot;
use crate::config::{
    FsmCatalog, FsmRoles, FsmTransition,
};
use crate::health::Health;
use crate::zones::ZoneEngine;

#[test]
fn force_safe_state_uses_roles_in_real_catalogs() {
    let start = Instant::now(); // cfg(test)
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for rel in [
        "config/fsm.toml",
        "config/blueprints/detect-room-face/fsm.toml",
    ] {
        let path = root.join(rel);
        let catalog: FsmCatalog = toml::from_str(
            &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}")),
        )
        .unwrap_or_else(|e| panic!("parse {path:?}: {e}"));
        let mut engine = engine_at(&catalog, start);

        engine.force_safe_state(start);

        assert_eq!(engine.current_state(), catalog.fsm.roles.safe, "{rel}");
        assert!(engine.current_models().is_empty(), "{rel}");
    }
}

#[test]
fn roles_decouple_engine_from_state_names() {
    let start = Instant::now(); // cfg(test)
    let mut catalog = catalog_with_roles(FsmRoles {
        safe: "offline".into(),
        reset: "home".into(),
    });
    catalog.fsm.states.get_mut("engaged").unwrap().face_inside = true;
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let depth = DepthRuleSnapshot::default();

    engine.force_safe_state(start);
    assert_eq!(engine.current_state(), "offline");

    let face = FsmSceneContext {
        face_present: true,
        face_confidence: Some(0.9),
        ..Default::default()
    };
    let reset = engine
        .evaluate_with_context_at(&[], None, &health, &depth, &face, start)
        .expect("data fresh resets to home");
    assert_eq!(reset.to, "home");

    let engaged = engine
        .evaluate_with_context_at(&[], None, &health, &depth, &face, start)
        .expect("face enters engaged");
    assert_eq!(engaged.to, "engaged");
    assert!(engine.face_was_inside());

    let home = engine
        .evaluate_with_context_at(
            &[],
            None,
            &health,
            &depth,
            &FsmSceneContext::default(),
            start,
        )
        .expect("face absence resets to home");
    assert_eq!(home.to, "home");
    assert!(!engine.face_was_inside());
}

#[test]
fn wildcard_data_stale_triggers_from_any_state() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("blind", vec![])],
        vec![FsmTransition {
            from: "*".into(),
            to: "blind".into(),
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

    let mut health = Health::new_at(100, 50, start); // 100ms stale
    let _ = health.evaluate_at(start + std::time::Duration::from_millis(200)); // trigger blind

    let result = engine.evaluate_with_context_at(
        &[],
        Some(&zones),
        &health,
        &DepthRuleSnapshot::default(),
        &FsmSceneContext::default(),
        start,
    );
    assert!(result.is_some());
    assert_eq!(result.unwrap().to, "blind");
    assert_eq!(engine.current_state(), "blind");
}

#[test]
fn fsm_recovers_from_blind_when_data_returns() {
    let catalog = make_catalog(
        "idle",
        vec![
            ("idle", vec!["detect-fast"]),
            ("detected", vec!["detect-fast"]),
            ("blind", vec![]),
        ],
        vec![
            FsmTransition {
                from: "idle".into(),
                to: "detected".into(),
                guards: vec![FsmGuard::FaceDetected {
                    min_confidence: 0.5,
                }],
                dwell: None,
            },
            FsmTransition {
                from: "*".into(),
                to: "blind".into(),
                guards: vec![FsmGuard::DataStale],
                dwell: None,
            },
            FsmTransition {
                from: "blind".into(),
                to: "idle".into(),
                guards: vec![FsmGuard::DataFresh],
                dwell: None,
            },
        ],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
        zones: HashMap::new(),
        face_dwell: None,
    });
    let mut health = Health::new_at(100, 50, start);

    let face = FsmSceneContext {
        face_present: true,
        face_confidence: Some(0.9),
        ..Default::default()
    };
    let entered = engine
        .evaluate_with_context_at(
            &[],
            Some(&zones),
            &health,
            &DepthRuleSnapshot::default(),
            &face,
            start,
        )
        .expect("face enters detected");
    assert_eq!(entered.to, "detected");
    assert!(engine.face_was_inside(), "latch set inside detected");

    let _ = health.evaluate_at(start + std::time::Duration::from_millis(200));
    let blind = engine
        .evaluate_with_context_at(
            &[],
            Some(&zones),
            &health,
            &DepthRuleSnapshot::default(),
            &face,
            start + std::time::Duration::from_millis(200),
        )
        .expect("stale signal forces blind");
    assert_eq!(blind.to, "blind");
    assert_eq!(engine.current_state(), "blind");
    assert!(
        engine.current_models().is_empty(),
        "blind declares no models"
    );

    let recovered_from_blind = health.touch_at(start + std::time::Duration::from_millis(300));
    assert!(recovered_from_blind, "touch reports the exit from blind");
    let result = engine
        .evaluate_with_context_at(
            &[],
            Some(&zones),
            &health,
            &DepthRuleSnapshot::default(),
            &face,
            start + std::time::Duration::from_millis(300),
        )
        .expect("fresh signal exits blind");
    assert_eq!(result.to, "idle");
    assert!(
        !engine.face_was_inside(),
        "el latch no sobrevive a la ceguera"
    );
    assert!(!engine.current_models().is_empty(), "la inferencia vuelve");
}

#[test]
fn force_safe_state_purges_corrupted_state_and_recovers_to_idle() {
    let catalog = make_catalog(
        "idle",
        vec![
            ("idle", vec!["detect-fast"]),
            ("detected", vec!["detect-fast"]),
            ("blind", vec![]),
            ("edge", vec![]),
        ],
        vec![
            FsmTransition {
                from: "idle".into(),
                to: "detected".into(),
                guards: vec![FsmGuard::FaceDetected {
                    min_confidence: 0.5,
                }],
                dwell: None,
            },
            FsmTransition {
                from: "detected".into(),
                to: "edge".into(),
                guards: vec![FsmGuard::FaceInDwell],
                dwell: Some("1000ms".into()),
            },
            FsmTransition {
                from: "*".into(),
                to: "blind".into(),
                guards: vec![FsmGuard::DataStale],
                dwell: None,
            },
            FsmTransition {
                from: "blind".into(),
                to: "idle".into(),
                guards: vec![FsmGuard::DataFresh],
                dwell: None,
            },
        ],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
        zones: HashMap::new(),
        face_dwell: None,
    });
    let health = Health::new_at(100, 50, start); // nunca emborna: data siempre fresh
    let face = FsmSceneContext {
        face_present: true,
        face_confidence: Some(0.9),
        face_in_dwell: Some(true),
        ..Default::default()
    };

    // Estado torcido: detected con latch y un timer de dwell pendiente.
    let entered = engine
        .evaluate_with_context_at(
            &[],
            Some(&zones),
            &health,
            &DepthRuleSnapshot::default(),
            &face,
            start,
        )
        .expect("face entra a detected");
    assert_eq!(entered.to, "detected");
    assert!(engine.face_was_inside());
    let pending = engine.evaluate_with_context_at(
        &[],
        Some(&zones),
        &health,
        &DepthRuleSnapshot::default(),
        &face,
        start + std::time::Duration::from_millis(50),
    );
    assert!(pending.is_none(), "dwell de 1s todavia no cumple");
    assert_eq!(
        engine
            .snapshot_at(start + std::time::Duration::from_millis(50))
            .active_timers
            .len(),
        1,
        "timer de dwell armado"
    );

    // El panic fuerza el estado seguro.
    engine.force_safe_state(start + std::time::Duration::from_millis(200));
    assert_eq!(
        engine.current_state(),
        "blind",
        "blind directo, sin pasar por el evaluador"
    );
    assert!(
        engine.current_models().is_empty(),
        "blind declara cero modelos"
    );
    let snap = engine.snapshot_at(start + std::time::Duration::from_millis(200));
    assert_eq!(snap.active_timers.len(), 0, "dwell_timers purgados");
    assert_eq!(snap.state_dwell_ms, 0, "state_entered_at reiniciado");

    // Ciclo siguiente: data_fresh reconstruye desde idle, latch limpio.
    let recovered = engine
        .evaluate_with_context_at(
            &[],
            Some(&zones),
            &health,
            &DepthRuleSnapshot::default(),
            &face,
            start + std::time::Duration::from_millis(200),
        )
        .expect("data fresh sale de blind");
    assert_eq!(recovered.to, "idle");
    assert_eq!(engine.current_state(), "idle");
    assert!(!engine.face_was_inside(), "el estado torcido no sobrevive");
    assert!(!engine.current_models().is_empty(), "la inferencia vuelve");
}

#[test]
fn wildcard_evaluation_does_not_run_state_specific_transition() {
    let catalog = make_catalog(
        "idle",
        vec![("idle", vec![]), ("in_bed", vec![])],
        vec![FsmTransition {
            from: "idle".into(),
            to: "in_bed".into(),
            guards: vec![FsmGuard::FaceInDwell],
            dwell: None,
        }],
    );
    let start = Instant::now(); // cfg(test)
    let mut engine = engine_at(&catalog, start);
    let health = Health::new_at(10_000, 5_000, start);
    let inside = FsmSceneContext {
        face_in_dwell: Some(true),
        ..Default::default()
    };

    assert!(
        engine
            .evaluate_wildcard_with_context_at(
                &[],
                None,
                &health,
                &DepthRuleSnapshot::default(),
                &inside,
                start,
            )
            .is_none()
    );
    assert_eq!(engine.current_state(), "idle");
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
    assert_eq!(engine.current_state(), "in_bed");
}
