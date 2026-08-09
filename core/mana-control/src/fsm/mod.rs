//! Finite state machine engine for clinical scene logic.

mod dwell;
mod engine;
mod guard;
mod program;

pub use engine::{
    FsmDwellTimerSnapshot, FsmEngine, FsmSceneContext, FsmSnapshot, FsmTransitionResult,
};
pub use guard::{FsmGuard, GuardCtx};
pub use program::FsmProgram;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Instant;

    use crate::config::{
        FsmCatalog, FsmRoles, FsmRoot, FsmState, FsmTransition, ZoneCatalog, ZoneSpec,
    };
    use crate::depth::DepthRuleSnapshot;
    use crate::health::Health;
    use crate::zones::{ZoneEngine, ZoneEvent};

    fn make_catalog(
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

    fn make_catalog_dwell(
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

    fn catalog_with_roles(roles: FsmRoles) -> FsmCatalog {
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
                    guards: vec![FsmGuard::FaceDetected {
                        min_confidence: 0.5,
                    }],
                    dwell: None,
                },
                FsmTransition {
                    from: "engaged".into(),
                    to: "home".into(),
                    guards: vec![FsmGuard::FaceAbsent],
                    dwell: None,
                },
            ],
        );
        catalog.fsm.roles = roles;
        catalog
    }

    fn test_zones(catalog: &FsmCatalog) -> ZoneCatalog {
        let mut zones = HashMap::new();
        let mut needs_face_dwell = false;
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
                    FsmGuard::FaceInDwell | FsmGuard::FaceNotInDwell => {
                        needs_face_dwell = true;
                    }
                    _ => {}
                }
            }
        }
        ZoneCatalog {
            zones,
            face_dwell: needs_face_dwell.then_some(ZoneSpec {
                x1: 0,
                y1: 0,
                x2: 1,
                y2: 1,
                label: None,
                hysteresis_ms: 0,
            }),
        }
    }

    fn engine(catalog: &FsmCatalog) -> FsmEngine {
        engine_at(catalog, Instant::now())
    }

    fn engine_at(catalog: &FsmCatalog, now: Instant) -> FsmEngine {
        FsmEngine::from_program_at(
            FsmProgram::compile_lenient(catalog, &test_zones(catalog))
                .expect("test FSM must compile"),
            now,
        )
    }

    #[test]
    fn force_safe_state_uses_roles_in_real_catalogs() {
        let start = Instant::now();
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
        let start = Instant::now();
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
        let mut engine = engine(&catalog);
        let start = Instant::now();
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });

        let mut health = Health::new_at(100, 50, start); // 100ms stale
        let _ = health.evaluate_at(start + std::time::Duration::from_millis(200)); // trigger blind

        let result = engine.evaluate(&[], &zones, &health, &DepthRuleSnapshot::default());
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
        let start = Instant::now();
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
        let start = Instant::now();
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
        let start = Instant::now();
        let mut engine = engine_at(&catalog, start);
        let health = Health::new(10_000, 5_000);
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
        let mut engine = engine(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000, 5_000);

        let events = vec![ZoneEvent::Occupied {
            zone: "bed".into(),
            label: None,
            track_id: 1,
            class: "person".into(),
            confidence: 0.9,
        }];
        let result = engine.evaluate(&events, &zones, &health, &DepthRuleSnapshot::default());

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
        let mut engine = engine(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000, 5_000);

        let events = vec![ZoneEvent::Occupied {
            zone: "bed".into(),
            label: None,
            track_id: 1,
            class: "person".into(),
            confidence: 0.6,
        }];
        let result = engine.evaluate(&events, &zones, &health, &DepthRuleSnapshot::default());
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
        let mut engine = engine(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000, 5_000);

        let events = vec![ZoneEvent::Occupied {
            zone: "bed".into(),
            label: None,
            track_id: 1,
            class: "person".into(),
            confidence: 0.9,
        }];
        let result = engine.evaluate(&events, &zones, &health, &DepthRuleSnapshot::default());
        assert!(result.is_none());
        assert_eq!(engine.current_state(), "idle");
    }

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
        let start = Instant::now();
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
        let start = Instant::now();
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
        let mut engine = engine(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000, 5_000);

        let triggered = make_snapshot("bed-approach", true);
        let result = engine.evaluate(&[], &zones, &health, &triggered);
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
        let mut engine = engine(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000, 5_000);

        // Sin evidencia de la regla (mapa, ROI o cobertura) -> guard falso.
        let empty = DepthRuleSnapshot::default();
        assert!(engine.evaluate(&[], &zones, &health, &empty).is_none());
        assert_eq!(engine.current_state(), "idle");

        let not_triggered = make_snapshot("bed-approach", false);
        assert!(
            engine
                .evaluate(&[], &zones, &health, &not_triggered)
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
        let mut engine = engine(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000, 5_000);

        assert!(
            engine
                .evaluate(&[], &zones, &health, &make_snapshot("bed-approach", true))
                .is_none()
        );
        assert!(
            engine
                .evaluate(&[], &zones, &health, &make_snapshot("bed-approach", false))
                .is_some()
        );
        assert_eq!(engine.current_state(), "idle");
    }

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
        let mut engine = engine(&catalog);
        let health = Health::new(10_000, 5_000);
        let outside = FsmSceneContext {
            face_in_dwell: Some(false),
            ..Default::default()
        };
        assert!(
            engine
                .evaluate_with_context(&[], None, &health, &DepthRuleSnapshot::default(), &outside,)
                .is_none()
        );

        let inside = FsmSceneContext {
            face_in_dwell: Some(true),
            ..Default::default()
        };
        assert!(
            engine
                .evaluate_with_context(&[], None, &health, &DepthRuleSnapshot::default(), &inside,)
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
        let start = Instant::now();
        let mut engine = engine_at(&catalog, start);
        let health = Health::new(10_000, 5_000);
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
                        FsmGuard::PersonPresent,
                        FsmGuard::FaceAbsent,
                        FsmGuard::FaceNotInDwell,
                        FsmGuard::FaceNotAtEdge,
                    ],
                    dwell: Some("700ms".into()),
                },
            ],
        );
        let start = Instant::now();
        let mut engine = engine_at(&catalog, start);
        let health = Health::new(10_000, 5_000);
        let at_edge = FsmSceneContext {
            person_present: true,
            face_present: true,
            face_in_dwell: Some(true),
            at_edge: true,
            ..Default::default()
        };

        assert!(
            engine
                .evaluate_with_context_at(
                    &[],
                    None,
                    &health,
                    &DepthRuleSnapshot::default(),
                    &at_edge,
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
                .evaluate_with_context_at(
                    &[],
                    None,
                    &health,
                    &DepthRuleSnapshot::default(),
                    &away_from_edge,
                    start + std::time::Duration::from_millis(1_000),
                )
                .is_none()
        );
        assert_eq!(engine.current_state(), "edge");
        assert_eq!(
            engine
                .evaluate_with_context_at(
                    &[],
                    None,
                    &health,
                    &DepthRuleSnapshot::default(),
                    &away_from_edge,
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
                    guards: vec![FsmGuard::PersonAbsent, FsmGuard::FaceWasInside],
                    dwell: Some("1s".into()),
                },
                FsmTransition {
                    from: "exiting".into(),
                    to: "idle".into(),
                    guards: vec![FsmGuard::PersonAbsent],
                    dwell: Some("1s".into()),
                },
            ],
        );
        let start = Instant::now();
        let mut engine = engine_at(&catalog, start);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000, 5_000);
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
                .evaluate_with_context_at(
                    &[],
                    Some(&zones),
                    &health,
                    &DepthRuleSnapshot::default(),
                    &present,
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
                    &present,
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
        assert!(
            engine
                .evaluate_with_context_at(
                    &[],
                    Some(&zones),
                    &health,
                    &DepthRuleSnapshot::default(),
                    &absent,
                    start + std::time::Duration::from_millis(1_000),
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
                    &absent,
                    start + std::time::Duration::from_millis(2_000),
                )
                .is_some()
        );
        assert_eq!(engine.current_state(), "exiting");
        assert!(
            engine
                .evaluate_with_context_at(
                    &[],
                    Some(&zones),
                    &health,
                    &DepthRuleSnapshot::default(),
                    &absent,
                    start + std::time::Duration::from_millis(8_000),
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
                    &absent,
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
                    guards: vec![FsmGuard::PersonAbsent, FsmGuard::FaceWasInside],
                    dwell: None,
                },
                FsmTransition {
                    from: "other".into(),
                    to: "idle".into(),
                    guards: vec![FsmGuard::PersonAbsent, FsmGuard::FaceWasNotInside],
                    dwell: None,
                },
            ],
        );
        let start = Instant::now();
        let mut engine = engine_at(&catalog, start);
        let health = Health::new(10_000, 5_000);
        let absent = FsmSceneContext {
            cardinality: Some("single".into()),
            person_present: false,
            ..Default::default()
        };
        let result = engine.evaluate_with_context_at(
            &[],
            None,
            &health,
            &DepthRuleSnapshot::default(),
            &absent,
            start,
        );
        assert_eq!(result.expect("fallback to idle").to, "idle");
    }

    fn make_snapshot(rule: &str, triggered: bool) -> DepthRuleSnapshot {
        use crate::depth::DepthRuleResult;
        DepthRuleSnapshot::from_results(&[DepthRuleResult {
            rule: rule.into(),
            region: [0, 0, 2, 2],
            metric: crate::depth::DepthMetric::Median,
            threshold_m: 1.5,
            value: Some(1.0),
            triggered,
            valid_pixels: 4,
            valid_ratio: Some(1.0),
            calibration: None,
        }])
    }
}
