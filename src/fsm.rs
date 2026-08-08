use std::collections::HashMap;
use std::time::Instant;

use crate::config::{FsmCatalog, FsmGuard};
use crate::depth::DepthRuleSnapshot;
use crate::metrics::Health;
use crate::zones::{ZoneEngine, ZoneEvent};

/// Per-frame scene evidence consumed by blueprint-specific FSM guards.
/// Optional values distinguish unavailable evidence from a negative signal.
#[derive(Debug, Clone, Default)]
pub struct FsmSceneContext {
    pub cardinality: Option<String>,
    pub person_present: bool,
    pub face_present: bool,
    pub face_confidence: Option<f32>,
    pub face_in_dwell: Option<bool>,
    pub at_edge: bool,
    pub face_model_ran: bool,
}

#[derive(Debug, Clone)]
pub struct FsmTransitionResult {
    pub from: String,
    pub from_label: Option<String>,
    pub to: String,
    pub to_label: Option<String>,
    pub trigger: String,
    pub dwell_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FsmDwellTimerSnapshot {
    pub trigger: String,
    pub elapsed_ms: u64,
    pub required_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FsmSnapshot {
    pub state: String,
    pub state_label: Option<String>,
    pub state_dwell_ms: u64,
    pub state_dwell_required_ms: Option<u64>,
    pub face_was_inside: bool,
    pub active_timers: Vec<FsmDwellTimerSnapshot>,
}

pub struct FsmEngine {
    current_state: String,
    catalog: FsmCatalog,
    state_entered_at: Instant,
    dwell_timers: HashMap<String, Instant>,
    face_was_inside: bool,
}

impl FsmEngine {
    pub fn from_catalog(catalog: &FsmCatalog) -> Self {
        Self::from_catalog_at(catalog, Instant::now())
    }

    pub fn from_catalog_at(catalog: &FsmCatalog, now: Instant) -> Self {
        Self {
            current_state: catalog.fsm.initial.clone(),
            catalog: catalog.clone(),
            state_entered_at: now,
            dwell_timers: HashMap::new(),
            face_was_inside: false,
        }
    }

    pub fn current_state(&self) -> &str {
        &self.current_state
    }

    pub fn snapshot(&self) -> FsmSnapshot {
        self.snapshot_at(Instant::now())
    }

    pub fn snapshot_at(&self, now: Instant) -> FsmSnapshot {
        let mut active_timers: Vec<_> = self
            .dwell_timers
            .iter()
            .map(|(trigger, started_at)| {
                let required_ms = self
                    .catalog
                    .fsm
                    .transitions
                    .iter()
                    .find(|transition| transition_trigger(transition) == *trigger)
                    .map(transition_min_dwell)
                    .unwrap_or(0);
                FsmDwellTimerSnapshot {
                    trigger: trigger.clone(),
                    elapsed_ms: now.saturating_duration_since(*started_at).as_millis() as u64,
                    required_ms,
                }
            })
            .collect();
        active_timers.sort_by(|a, b| a.trigger.cmp(&b.trigger));

        FsmSnapshot {
            state: self.current_state.clone(),
            state_label: state_label(&self.catalog, &self.current_state),
            state_dwell_ms: now
                .saturating_duration_since(self.state_entered_at)
                .as_millis() as u64,
            state_dwell_required_ms: self
                .catalog
                .fsm
                .states
                .get(&self.current_state)
                .and_then(|state| state.dwell_min_ms),
            face_was_inside: self.face_was_inside,
            active_timers,
        }
    }

    pub fn evaluate(
        &mut self,
        zone_events: &[ZoneEvent],
        zone_engine: &ZoneEngine,
        health: &Health,
        depth: &DepthRuleSnapshot,
    ) -> Option<FsmTransitionResult> {
        self.evaluate_with_context(
            zone_events,
            Some(zone_engine),
            health,
            depth,
            &FsmSceneContext::default(),
        )
    }

    pub fn evaluate_with_context(
        &mut self,
        zone_events: &[ZoneEvent],
        zone_engine: Option<&ZoneEngine>,
        health: &Health,
        depth: &DepthRuleSnapshot,
        context: &FsmSceneContext,
    ) -> Option<FsmTransitionResult> {
        self.evaluate_with_context_at(
            zone_events,
            zone_engine,
            health,
            depth,
            context,
            Instant::now(),
        )
    }

    /// Evaluate only global (`from = "*"`) transitions between keyframes.
    /// State-specific scene transitions must wait for fresh frame evidence.
    pub fn evaluate_wildcard_with_context(
        &mut self,
        zone_events: &[ZoneEvent],
        zone_engine: Option<&ZoneEngine>,
        health: &Health,
        depth: &DepthRuleSnapshot,
        context: &FsmSceneContext,
    ) -> Option<FsmTransitionResult> {
        self.evaluate_wildcard_with_context_at(
            zone_events,
            zone_engine,
            health,
            depth,
            context,
            Instant::now(),
        )
    }

    pub fn evaluate_wildcard_with_context_at(
        &mut self,
        zone_events: &[ZoneEvent],
        zone_engine: Option<&ZoneEngine>,
        health: &Health,
        depth: &DepthRuleSnapshot,
        context: &FsmSceneContext,
        now: Instant,
    ) -> Option<FsmTransitionResult> {
        self.update_face_latch(context);
        if !self.state_dwell_satisfied(now) {
            return None;
        }

        for t in &self.catalog.fsm.transitions {
            if t.from != "*" {
                continue;
            }
            if let Some(result) = try_transition(
                &mut self.dwell_timers,
                t,
                &self.state_entered_at,
                &self.catalog,
                zone_events,
                zone_engine,
                health,
                depth,
                context,
                self.face_was_inside,
                now,
            ) {
                self.apply_transition(&result, context, now);
                return Some(result);
            }
        }

        None
    }

    pub fn evaluate_with_context_at(
        &mut self,
        zone_events: &[ZoneEvent],
        zone_engine: Option<&ZoneEngine>,
        health: &Health,
        depth: &DepthRuleSnapshot,
        context: &FsmSceneContext,
        now: Instant,
    ) -> Option<FsmTransitionResult> {
        self.update_face_latch(context);
        if !self.state_dwell_satisfied(now) {
            return None;
        }

        let transitions = &self.catalog.fsm.transitions;

        for t in transitions {
            if t.from == "*" {
                if let Some(result) = try_transition(
                    &mut self.dwell_timers,
                    t,
                    &self.state_entered_at,
                    &self.catalog,
                    zone_events,
                    zone_engine,
                    health,
                    depth,
                    context,
                    self.face_was_inside,
                    now,
                ) {
                    self.apply_transition(&result, context, now);
                    return Some(result);
                }
            }
        }

        for t in transitions {
            if t.from != self.current_state {
                continue;
            }
            if let Some(result) = try_transition(
                &mut self.dwell_timers,
                t,
                &self.state_entered_at,
                &self.catalog,
                zone_events,
                zone_engine,
                health,
                depth,
                context,
                self.face_was_inside,
                now,
            ) {
                self.apply_transition(&result, context, now);
                return Some(result);
            }
        }

        None
    }

    fn update_face_latch(&mut self, context: &FsmSceneContext) {
        if matches!(self.current_state.as_str(), "detected" | "in_bed") {
            self.face_was_inside = true;
        }
        if context.cardinality.as_deref() == Some("multiple") {
            self.face_was_inside = false;
        }
    }

    fn state_dwell_satisfied(&self, now: Instant) -> bool {
        self.catalog
            .fsm
            .states
            .get(&self.current_state)
            .and_then(|state| state.dwell_min_ms)
            .is_none_or(|dwell_min| {
                now.saturating_duration_since(self.state_entered_at)
                    .as_millis()
                    >= dwell_min as u128
            })
    }

    fn apply_transition(
        &mut self,
        result: &FsmTransitionResult,
        context: &FsmSceneContext,
        now: Instant,
    ) {
        self.current_state.clone_from(&result.to);
        self.state_entered_at = now;
        self.dwell_timers.clear();
        if result.to == "detected"
            || result.to == "in_bed"
            || (result.to == "edge" && context.face_present)
        {
            self.face_was_inside = true;
        }
        if result.to == "idle" {
            self.face_was_inside = false;
        }
    }

    pub fn current_models(&self) -> Vec<String> {
        self.catalog
            .fsm
            .states
            .get(&self.current_state)
            .map(|s| s.models.clone())
            .unwrap_or_default()
    }
}

fn state_label(catalog: &FsmCatalog, state: &str) -> Option<String> {
    catalog.fsm.states.get(state).and_then(|s| s.label.clone())
}

fn parse_dwell(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(num_str) = s.strip_suffix("ms") {
        return num_str.trim().parse::<f64>().ok().map(|v| v as u64);
    }
    if let Some(num_str) = s.strip_suffix('s') {
        return num_str
            .trim()
            .parse::<f64>()
            .ok()
            .map(|v| (v * 1000.0) as u64);
    }
    if let Some(num_str) = s.strip_suffix('m') {
        return num_str
            .trim()
            .parse::<f64>()
            .ok()
            .map(|v| (v * 60_000.0) as u64);
    }
    if let Some(num_str) = s.strip_suffix('h') {
        return num_str
            .trim()
            .parse::<f64>()
            .ok()
            .map(|v| (v * 3_600_000.0) as u64);
    }
    None
}

fn transition_trigger(t: &crate::config::FsmTransition) -> String {
    format!("{}→{}", t.from, t.to)
}

fn transition_min_dwell(t: &crate::config::FsmTransition) -> u64 {
    let transition_dwell = t.dwell.as_deref().and_then(parse_dwell);
    if t.guards.is_empty() {
        0
    } else {
        t.guards
            .iter()
            .filter_map(|g| match g {
                FsmGuard::ZoneOccupied {
                    min_duration_ms, ..
                } => *min_duration_ms,
                FsmGuard::ZoneVacated {
                    min_duration_ms, ..
                } => *min_duration_ms,
                FsmGuard::AllZonesVacant {
                    min_duration_ms, ..
                } => *min_duration_ms,
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .max(transition_dwell.unwrap_or(0))
    }
}

fn try_transition(
    dwell_timers: &mut HashMap<String, Instant>,
    t: &crate::config::FsmTransition,
    state_entered_at: &Instant,
    catalog: &FsmCatalog,
    zone_events: &[ZoneEvent],
    zone_engine: Option<&ZoneEngine>,
    health: &Health,
    depth: &DepthRuleSnapshot,
    context: &FsmSceneContext,
    face_was_inside: bool,
    now: Instant,
) -> Option<FsmTransitionResult> {
    let transition_dwell = t.dwell.as_deref().and_then(parse_dwell);
    if t.guards.is_empty() {
        if let Some(required_ms) = transition_dwell {
            if now.saturating_duration_since(*state_entered_at).as_millis() < required_ms as u128 {
                return None;
            }
        }
    }

    let trigger_key = transition_trigger(t);

    let all_true = t.guards.iter().all(|g| {
        eval_guard(
            g,
            zone_events,
            zone_engine,
            health,
            depth,
            context,
            face_was_inside,
        )
    });

    if !all_true {
        dwell_timers.remove(&trigger_key);
        return None;
    }

    let min_dwell = transition_min_dwell(t);

    let elapsed = if t.guards.is_empty() {
        now.saturating_duration_since(*state_entered_at).as_millis() as u64
    } else {
        let timer = dwell_timers.entry(trigger_key.clone()).or_insert(now);
        now.saturating_duration_since(*timer).as_millis() as u64
    };

    if elapsed >= min_dwell {
        dwell_timers.remove(&trigger_key);
        Some(FsmTransitionResult {
            from: t.from.clone(),
            from_label: if t.from == "*" {
                None
            } else {
                state_label(catalog, &t.from)
            },
            to: t.to.clone(),
            to_label: state_label(catalog, &t.to),
            trigger: trigger_key,
            dwell_ms: elapsed,
        })
    } else {
        None
    }
}

fn eval_guard(
    guard: &FsmGuard,
    zone_events: &[ZoneEvent],
    zone_engine: Option<&ZoneEngine>,
    health: &Health,
    depth: &DepthRuleSnapshot,
    context: &FsmSceneContext,
    face_was_inside: bool,
) -> bool {
    match guard {
        FsmGuard::ZonePresent { zone } => {
            zone_engine.is_some_and(|engine| engine.is_occupied(zone))
        }
        FsmGuard::ZoneOccupied {
            zone,
            min_confidence,
            ..
        } => zone_events.iter().any(|ev| match ev {
            ZoneEvent::Occupied {
                zone: z,
                confidence,
                ..
            } => z == zone && confidence >= min_confidence,
            _ => false,
        }),
        FsmGuard::ZoneVacated { zone, .. } => zone_events
            .iter()
            .any(|ev| matches!(ev, ZoneEvent::Vacated { zone: z, .. } if z == zone)),
        FsmGuard::AllZonesVacant { .. } => zone_engine.is_some_and(ZoneEngine::all_vacant),
        FsmGuard::DataStale => health.is_blind(),
        FsmGuard::DepthRule { rule, triggered } => depth.is_triggered(rule) == Some(*triggered),
        FsmGuard::Cardinality { value } => context.cardinality.as_deref() == Some(value),
        FsmGuard::PersonPresent => context.person_present,
        FsmGuard::PersonAbsent => !context.person_present,
        FsmGuard::FaceDetected { min_confidence } => {
            context.face_present && context.face_confidence.unwrap_or(0.0) >= *min_confidence
        }
        FsmGuard::FaceAbsent => !context.face_present,
        FsmGuard::FaceInDwell => context.face_in_dwell == Some(true),
        FsmGuard::FaceNotInDwell => context.face_in_dwell == Some(false),
        FsmGuard::FaceAtEdge => context.at_edge,
        FsmGuard::FaceNotAtEdge => !context.at_edge,
        FsmGuard::FaceWasInside => face_was_inside,
        FsmGuard::FaceWasNotInside => !face_was_inside,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FsmRoot, FsmState, FsmTransition};
    use crate::metrics::Health;

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
                },
            );
        }
        FsmCatalog {
            fsm: FsmRoot {
                initial: initial.to_string(),
                states: state_map,
                transitions,
            },
        }
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });

        let mut health = Health::new(100); // 100ms stale
        health.touch();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = health.evaluate(); // trigger blind

        let result = engine.evaluate(&[], &zones, &health, &DepthRuleSnapshot::default());
        assert!(result.is_some());
        assert_eq!(result.unwrap().to, "blind");
        assert_eq!(engine.current_state, "blind");
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
        let mut engine = FsmEngine::from_catalog_at(&catalog, start);
        let health = Health::new(10_000);
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000);

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
        assert_eq!(engine.current_state, "watching");
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000);

        let events = vec![ZoneEvent::Occupied {
            zone: "bed".into(),
            label: None,
            track_id: 1,
            class: "person".into(),
            confidence: 0.6,
        }];
        let result = engine.evaluate(&events, &zones, &health, &DepthRuleSnapshot::default());
        assert!(result.is_none());
        assert_eq!(engine.current_state, "idle");
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000);

        let events = vec![ZoneEvent::Occupied {
            zone: "bed".into(),
            label: None,
            track_id: 1,
            class: "person".into(),
            confidence: 0.9,
        }];
        let result = engine.evaluate(&events, &zones, &health, &DepthRuleSnapshot::default());
        assert!(result.is_none());
        assert_eq!(engine.current_state, "idle");
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let mut health = Health::new(100);
        health.touch();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = health.evaluate();

        assert!(
            engine
                .evaluate(&[], &zones, &health, &DepthRuleSnapshot::default())
                .is_none()
        );
        assert_eq!(engine.current_state, "idle");

        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(
            engine
                .evaluate(&[], &zones, &health, &DepthRuleSnapshot::default())
                .is_some()
        );
        assert_eq!(engine.current_state, "next");
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000);

        assert!(
            engine
                .evaluate(&[], &zones, &health, &DepthRuleSnapshot::default())
                .is_none()
        );

        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            engine
                .evaluate(&[], &zones, &health, &DepthRuleSnapshot::default())
                .is_some()
        );
        assert_eq!(engine.current_state, "blind");
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000);

        let triggered = make_snapshot("bed-approach", true);
        let result = engine.evaluate(&[], &zones, &health, &triggered);
        assert!(result.is_some());
        assert_eq!(engine.current_state, "approaching");
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000);

        // Sin evidencia de la regla (mapa, ROI o cobertura) -> guard falso.
        let empty = DepthRuleSnapshot::default();
        assert!(engine.evaluate(&[], &zones, &health, &empty).is_none());
        assert_eq!(engine.current_state, "idle");

        let not_triggered = make_snapshot("bed-approach", false);
        assert!(
            engine
                .evaluate(&[], &zones, &health, &not_triggered)
                .is_none()
        );
        assert_eq!(engine.current_state, "idle");
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000);

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
        assert_eq!(engine.current_state, "idle");
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
        let mut engine = FsmEngine::from_catalog(&catalog);
        let health = Health::new(10_000);
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
        let mut engine = FsmEngine::from_catalog_at(&catalog, start);
        let health = Health::new(10_000);
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
        let mut engine = FsmEngine::from_catalog_at(&catalog, start);
        let health = Health::new(10_000);
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
        let mut engine = FsmEngine::from_catalog_at(&catalog, start);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog {
            zones: HashMap::new(),
            face_dwell: None,
        });
        let health = Health::new(10_000);
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
        let mut engine = FsmEngine::from_catalog_at(&catalog, start);
        let health = Health::new(10_000);
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
