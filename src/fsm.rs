use std::collections::HashMap;
use std::time::Instant;

use crate::config::{FsmCatalog, FsmGuard};
use crate::metrics::Health;
use crate::zones::{ZoneEngine, ZoneEvent};

#[derive(Debug, Clone)]
pub struct FsmTransitionResult {
    pub from: String,
    pub from_label: Option<String>,
    pub to: String,
    pub to_label: Option<String>,
    pub trigger: String,
    pub dwell_ms: u64,
}

pub struct FsmEngine {
    current_state: String,
    catalog: FsmCatalog,
    state_entered_at: Instant,
    dwell_timers: HashMap<String, Instant>,
}

impl FsmEngine {
    pub fn from_catalog(catalog: &FsmCatalog) -> Self {
        Self {
            current_state: catalog.fsm.initial.clone(),
            catalog: catalog.clone(),
            state_entered_at: Instant::now(),
            dwell_timers: HashMap::new(),
        }
    }

    pub fn evaluate(
        &mut self,
        zone_events: &[ZoneEvent],
        zone_engine: &ZoneEngine,
        health: &Health,
    ) -> Option<FsmTransitionResult> {
        let current = self.catalog.fsm.states.get(&self.current_state);
        if let Some(dwell_min) = current.and_then(|s| s.dwell_min_ms) {
            if self.state_entered_at.elapsed().as_millis() < dwell_min as u128 {
                return None;
            }
        }

        let transitions = &self.catalog.fsm.transitions;

        for t in transitions {
            if t.from == "*" {
                if let Some(result) = try_transition(&mut self.dwell_timers, t, &self.state_entered_at, &self.catalog, zone_events, zone_engine, health) {
                    self.current_state.clone_from(&result.to);
                    self.state_entered_at = Instant::now();
                    self.dwell_timers.clear();
                    return Some(result);
                }
            }
        }

        for t in transitions {
            if t.from != self.current_state {
                continue;
            }
            if let Some(result) = try_transition(&mut self.dwell_timers, t, &self.state_entered_at, &self.catalog, zone_events, zone_engine, health) {
                self.current_state.clone_from(&result.to);
                self.state_entered_at = Instant::now();
                self.dwell_timers.clear();
                return Some(result);
            }
        }

        None
    }

    pub fn current_models(&self) -> Vec<String> {
        self.catalog.fsm.states
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
    if s.is_empty() { return None; }
    if let Some(num_str) = s.strip_suffix("ms") {
        return num_str.trim().parse::<f64>().ok().map(|v| v as u64);
    }
    if let Some(num_str) = s.strip_suffix('s') {
        return num_str.trim().parse::<f64>().ok().map(|v| (v * 1000.0) as u64);
    }
    if let Some(num_str) = s.strip_suffix('m') {
        return num_str.trim().parse::<f64>().ok().map(|v| (v * 60_000.0) as u64);
    }
    if let Some(num_str) = s.strip_suffix('h') {
        return num_str.trim().parse::<f64>().ok().map(|v| (v * 3_600_000.0) as u64);
    }
    None
}

fn try_transition(
    dwell_timers: &mut HashMap<String, Instant>,
    t: &crate::config::FsmTransition,
    state_entered_at: &Instant,
    catalog: &FsmCatalog,
    zone_events: &[ZoneEvent],
    zone_engine: &ZoneEngine,
    health: &Health,
) -> Option<FsmTransitionResult> {
    if let Some(ref dwell_str) = t.dwell {
        if let Some(required_ms) = parse_dwell(dwell_str) {
            if state_entered_at.elapsed().as_millis() < required_ms as u128 {
                return None;
            }
        }
    }

    let trigger_key = format!("{}→{}", t.from, t.to);

    let all_true = t.guards.iter().all(|g| eval_guard(g, zone_events, zone_engine, health));

    if !all_true {
        dwell_timers.remove(&trigger_key);
        return None;
    }

    let min_dwell = t.guards.iter()
        .filter_map(|g| match g {
            FsmGuard::ZoneOccupied { min_duration_ms, .. } => *min_duration_ms,
            FsmGuard::ZoneVacated { min_duration_ms, .. } => *min_duration_ms,
            FsmGuard::AllZonesVacant { min_duration_ms, .. } => *min_duration_ms,
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let timer = dwell_timers
        .entry(trigger_key.clone())
        .or_insert(Instant::now());

    let elapsed = timer.elapsed().as_millis() as u64;

    if elapsed >= min_dwell {
        dwell_timers.remove(&trigger_key);
        Some(FsmTransitionResult {
            from: t.from.clone(),
            from_label: if t.from == "*" { None } else { state_label(catalog, &t.from) },
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
    zone_engine: &ZoneEngine,
    health: &Health,
) -> bool {
    match guard {
        FsmGuard::ZoneOccupied { zone, min_confidence, .. } => {
            zone_events.iter().any(|ev| match ev {
                ZoneEvent::Occupied { zone: z, confidence, .. } => {
                    z == zone && confidence >= min_confidence
                }
                _ => false,
            })
        }
        FsmGuard::ZoneVacated { zone, .. } => {
            zone_events.iter().any(|ev| matches!(ev, ZoneEvent::Vacated { zone: z, .. } if z == zone))
        }
        FsmGuard::AllZonesVacant { .. } => {
            zone_engine.all_vacant()
        }
        FsmGuard::DataStale => {
            health.is_blind()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FsmRoot, FsmState, FsmTransition};
    use crate::metrics::Health;

    fn make_catalog(initial: &str, states: Vec<(&str, Vec<&str>)>, transitions: Vec<FsmTransition>) -> FsmCatalog {
        make_catalog_dwell(initial, states.into_iter().map(|(n, m)| (n, m, None)).collect(), transitions)
    }

    fn make_catalog_dwell(initial: &str, states: Vec<(&str, Vec<&str>, Option<u64>)>, transitions: Vec<FsmTransition>) -> FsmCatalog {
        let mut state_map = HashMap::new();
        for (name, models, dwell_ms) in states {
            state_map.insert(name.to_string(), FsmState {
                label: None,
                models: models.iter().map(|s| s.to_string()).collect(),
                dwell_min_ms: dwell_ms,
            });
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
                from: "*".into(), to: "blind".into(),
                guards: vec![FsmGuard::DataStale],
                dwell: None,
            }],
        );
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog { zones: HashMap::new() });

        let mut health = Health::new(100); // 100ms stale
        health.touch();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = health.evaluate(); // trigger blind

        let result = engine.evaluate(&[], &zones, &health);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to, "blind");
        assert_eq!(engine.current_state, "blind");
    }

    #[test]
    fn zone_occupied_triggers_transition() {
        let catalog = make_catalog(
            "idle",
            vec![("idle", vec![]), ("watching", vec![])],
            vec![FsmTransition {
                from: "idle".into(), to: "watching".into(),
                guards: vec![FsmGuard::ZoneOccupied {
                    zone: "bed".into(),
                    min_confidence: 0.5,
                    min_duration_ms: Some(0),
                }],
                dwell: None,
            }],
        );
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog { zones: HashMap::new() });
        let health = Health::new(10_000);

        let events = vec![ZoneEvent::Occupied { zone: "bed".into(), label: None, track_id: 1, class: "person".into(), confidence: 0.9 }];
        let result = engine.evaluate(&events, &zones, &health);

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
                from: "idle".into(), to: "watching".into(),
                guards: vec![FsmGuard::ZoneOccupied {
                    zone: "bed".into(),
                    min_confidence: 0.8,
                    min_duration_ms: Some(0),
                }],
                dwell: None,
            }],
        );
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog { zones: HashMap::new() });
        let health = Health::new(10_000);

        let events = vec![ZoneEvent::Occupied { zone: "bed".into(), label: None, track_id: 1, class: "person".into(), confidence: 0.6 }];
        let result = engine.evaluate(&events, &zones, &health);
        assert!(result.is_none());
        assert_eq!(engine.current_state, "idle");
    }

    #[test]
    fn guards_must_all_be_true() {
        let catalog = make_catalog(
            "idle",
            vec![("idle", vec![]), ("watching", vec![])],
            vec![FsmTransition {
                from: "idle".into(), to: "watching".into(),
                guards: vec![
                    FsmGuard::ZoneOccupied { zone: "bed".into(), min_confidence: 0.5, min_duration_ms: Some(0) },
                    FsmGuard::ZoneOccupied { zone: "chair".into(), min_confidence: 0.5, min_duration_ms: Some(0) },
                ],
                dwell: None,
            }],
        );
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog { zones: HashMap::new() });
        let health = Health::new(10_000);

        let events = vec![
            ZoneEvent::Occupied { zone: "bed".into(), label: None, track_id: 1, class: "person".into(), confidence: 0.9 },
        ];
        let result = engine.evaluate(&events, &zones, &health);
        assert!(result.is_none());
        assert_eq!(engine.current_state, "idle");
    }

    #[test]
    fn state_dwell_min_delays_transition() {
        let catalog = make_catalog_dwell(
            "idle",
            vec![("idle", vec![], Some(500)), ("next", vec![], None)],
            vec![FsmTransition {
                from: "idle".into(), to: "next".into(),
                guards: vec![FsmGuard::DataStale],
                dwell: None,
            }],
        );
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog { zones: HashMap::new() });
        let mut health = Health::new(100);
        health.touch();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = health.evaluate();

        assert!(engine.evaluate(&[], &zones, &health).is_none());
        assert_eq!(engine.current_state, "idle");

        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(engine.evaluate(&[], &zones, &health).is_some());
        assert_eq!(engine.current_state, "next");
    }

    #[test]
    fn transition_dwell_auto_fires() {
        let catalog = make_catalog(
            "idle",
            vec![("idle", vec![]), ("blind", vec![])],
            vec![FsmTransition {
                from: "idle".into(), to: "blind".into(),
                guards: vec![],
                dwell: Some("100ms".into()),
            }],
        );
        let mut engine = FsmEngine::from_catalog(&catalog);
        let zones = ZoneEngine::from_catalog(&crate::config::ZoneCatalog { zones: HashMap::new() });
        let health = Health::new(10_000);

        assert!(engine.evaluate(&[], &zones, &health).is_none());

        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(engine.evaluate(&[], &zones, &health).is_some());
        assert_eq!(engine.current_state, "blind");
    }
}
