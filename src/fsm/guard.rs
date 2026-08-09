//! Guard evaluation context and transition attempts.

use std::collections::HashMap;
use std::time::Instant;

use crate::config::{FsmCatalog, FsmGuard};
use crate::depth::DepthRuleSnapshot;
use crate::metrics::Health;
use crate::zones::{ZoneEngine, ZoneEvent};

use super::{FsmSceneContext, FsmTransitionResult};

/// Shared inputs for guard evaluation and transition attempts.
#[derive(Clone, Copy)]
pub struct GuardCtx<'a> {
    pub zone_events: &'a [ZoneEvent],
    pub zones: Option<&'a ZoneEngine>,
    pub health: &'a Health,
    pub depth: &'a DepthRuleSnapshot,
    pub scene: &'a FsmSceneContext,
    pub face_was_inside: bool,
}

pub(super) fn state_label(catalog: &FsmCatalog, state: &str) -> Option<String> {
    catalog.fsm.states.get(state).and_then(|s| s.label.clone())
}

pub(super) fn parse_dwell(s: &str) -> Option<u64> {
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

pub(super) fn transition_trigger(t: &crate::config::FsmTransition) -> String {
    format!("{}→{}", t.from, t.to)
}

pub(super) fn transition_min_dwell(t: &crate::config::FsmTransition) -> u64 {
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

pub(super) fn try_transition(
    dwell_timers: &mut HashMap<String, Instant>,
    t: &crate::config::FsmTransition,
    state_entered_at: &Instant,
    catalog: &FsmCatalog,
    ctx: &GuardCtx<'_>,
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

    let all_true = t.guards.iter().all(|g| eval_guard(g, ctx));

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

pub(super) fn eval_guard(guard: &FsmGuard, ctx: &GuardCtx<'_>) -> bool {
    match guard {
        FsmGuard::ZonePresent { zone } => {
            ctx.zones.is_some_and(|engine| engine.is_occupied(zone))
        }
        FsmGuard::ZoneOccupied {
            zone,
            min_confidence,
            ..
        } => ctx.zone_events.iter().any(|ev| match ev {
            ZoneEvent::Occupied {
                zone: z,
                confidence,
                ..
            } => z == zone && confidence >= min_confidence,
            _ => false,
        }),
        FsmGuard::ZoneVacated { zone, .. } => ctx
            .zone_events
            .iter()
            .any(|ev| matches!(ev, ZoneEvent::Vacated { zone: z, .. } if z == zone)),
        FsmGuard::AllZonesVacant { .. } => ctx.zones.is_some_and(ZoneEngine::all_vacant),
        FsmGuard::DataStale => ctx.health.is_blind(),
        FsmGuard::DataFresh => !ctx.health.is_blind(),
        FsmGuard::DepthRule { rule, triggered } => {
            ctx.depth.is_triggered(rule) == Some(*triggered)
        }
        FsmGuard::Cardinality { value } => ctx.scene.cardinality.as_deref() == Some(value),
        FsmGuard::PersonPresent => ctx.scene.person_present,
        FsmGuard::PersonAbsent => !ctx.scene.person_present,
        FsmGuard::FaceDetected { min_confidence } => {
            ctx.scene.face_present && ctx.scene.face_confidence.unwrap_or(0.0) >= *min_confidence
        }
        FsmGuard::FaceAbsent => !ctx.scene.face_present,
        FsmGuard::FaceInDwell => ctx.scene.face_in_dwell == Some(true),
        FsmGuard::FaceNotInDwell => ctx.scene.face_in_dwell == Some(false),
        FsmGuard::FaceAtEdge => ctx.scene.at_edge,
        FsmGuard::FaceNotAtEdge => !ctx.scene.at_edge,
        FsmGuard::FaceWasInside => ctx.face_was_inside,
        FsmGuard::FaceWasNotInside => !ctx.face_was_inside,
    }
}
