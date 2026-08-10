//! Guard evaluation context and transition attempts.

use std::collections::HashMap;
use std::time::Instant;

use crate::DepthRuleSnapshot;
use crate::health::Health;
use crate::zones::{ZoneEngine, ZoneEvent};

use serde::Deserialize;

use super::program::{ProgramGuard, ProgramTransition};
use super::{FsmProgram, FsmSceneContext, FsmTransitionResult};

/// Clinical predicates that drive FSM transitions.
///
/// This is deserialized directly from the FSM TOML, then resolved into
/// [`ProgramGuard`] when an [`FsmProgram`] is compiled.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum FsmGuard {
    #[serde(rename = "zone_present")]
    ZonePresent { zone: String },
    #[serde(rename = "zone_occupied")]
    ZoneOccupied {
        zone: String,
        #[serde(default = "default_guard_confidence")]
        min_confidence: f32,
        #[serde(default)]
        min_duration_ms: Option<u64>,
    },
    #[serde(rename = "zone_vacated")]
    ZoneVacated {
        zone: String,
        /// Accepted only to be rejected at compile time: [`ZoneEvent::Vacated`]
        /// carries no confidence, so this threshold can never be evaluated.
        /// Silently ignoring it would let a catalog claim a gate it never had.
        #[serde(default)]
        min_confidence: Option<f32>,
        #[serde(default)]
        min_duration_ms: Option<u64>,
    },
    #[serde(rename = "all_zones_vacant")]
    AllZonesVacant {
        #[serde(default)]
        min_duration_ms: Option<u64>,
    },
    #[serde(rename = "data_stale")]
    DataStale,
    #[serde(rename = "data_fresh")]
    DataFresh,
    #[serde(rename = "depth_rule")]
    DepthRule {
        rule: String,
        #[serde(default = "default_guard_triggered")]
        triggered: bool,
    },
    #[serde(rename = "cardinality")]
    Cardinality { value: String },
    #[serde(rename = "person_present")]
    PersonPresent,
    #[serde(rename = "person_absent")]
    PersonAbsent,
    #[serde(rename = "face_detected")]
    FaceDetected {
        #[serde(default = "default_guard_confidence")]
        min_confidence: f32,
    },
    #[serde(rename = "face_absent")]
    FaceAbsent,
    #[serde(rename = "face_in_dwell")]
    FaceInDwell,
    #[serde(rename = "face_not_in_dwell")]
    FaceNotInDwell,
    #[serde(rename = "face_at_edge")]
    FaceAtEdge,
    #[serde(rename = "face_not_at_edge")]
    FaceNotAtEdge,
    #[serde(rename = "face_was_inside")]
    FaceWasInside,
    #[serde(rename = "face_was_not_inside")]
    FaceWasNotInside,
}

fn default_guard_triggered() -> bool {
    true
}

fn default_guard_confidence() -> f32 {
    0.5
}

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

pub(super) fn state_label(program: &FsmProgram, state: &str) -> Option<String> {
    program.state(state).and_then(|s| s.label.clone())
}

/// Convierte `"500ms"`, `"5s"`, `"5m"`, `"1h"` a milisegundos.
///
/// Rechaza valores negativos y no finitos en vez de aceptarlos. El `as u64`
/// sobre `f64` satura por definición del lenguaje, así que `"-5s"` daba `0`:
/// un dwell negativo se aceptaba y significaba "sin espera". Es la misma clase
/// de mentira que un knob que se ignora — el catálogo declaraba una condición
/// que el programa no aplicaba.
pub(super) fn parse_dwell(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_str, factor) = if let Some(rest) = s.strip_suffix("ms") {
        (rest, 1.0)
    } else if let Some(rest) = s.strip_suffix('s') {
        (rest, 1_000.0)
    } else if let Some(rest) = s.strip_suffix('m') {
        (rest, 60_000.0)
    } else if let Some(rest) = s.strip_suffix('h') {
        (rest, 3_600_000.0)
    } else {
        return None;
    };

    let value = num_str.trim().parse::<f64>().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let millis = value * factor;
    // `as u64` satura en ambos extremos; el guardia de arriba ya descartó lo
    // que no tiene sentido como duración.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(millis as u64)
}

pub(super) fn transition_trigger(t: &ProgramTransition) -> String {
    format!("{}→{}", t.from_name(), t.to)
}

pub(super) fn transition_min_dwell(t: &ProgramTransition) -> u64 {
    let transition_dwell = t.dwell_ms;
    if t.guards.is_empty() {
        0
    } else {
        t.guards
            .iter()
            .filter_map(|g| match g {
                ProgramGuard::ZoneOccupied {
                    min_duration_ms, ..
                } => *min_duration_ms,
                ProgramGuard::ZoneVacated {
                    min_duration_ms, ..
                } => *min_duration_ms,
                ProgramGuard::AllZonesVacant {
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
    t: &ProgramTransition,
    state_entered_at: &Instant,
    program: &FsmProgram,
    ctx: &GuardCtx<'_>,
    now: Instant,
) -> Option<FsmTransitionResult> {
    let transition_dwell = t.dwell_ms;
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
        crate::timing::elapsed_ms(now, *state_entered_at)
    } else {
        let timer = dwell_timers.entry(trigger_key.clone()).or_insert(now);
        crate::timing::elapsed_ms(now, *timer)
    };

    if elapsed >= min_dwell {
        dwell_timers.remove(&trigger_key);
        Some(FsmTransitionResult {
            from: t.from_name().to_owned(),
            from_label: if t.from.is_wildcard() {
                None
            } else {
                state_label(program, t.from_name())
            },
            to: t.to.to_string(),
            to_label: state_label(program, t.to.as_str()),
            trigger: trigger_key,
            dwell_ms: elapsed,
        })
    } else {
        None
    }
}

pub(super) fn eval_guard(guard: &ProgramGuard, ctx: &GuardCtx<'_>) -> bool {
    match guard {
        ProgramGuard::ZonePresent { zone } => {
            ctx.zones.is_some_and(|engine| engine.is_occupied(zone))
        }
        ProgramGuard::ZoneOccupied {
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
        ProgramGuard::ZoneVacated { zone, .. } => ctx
            .zone_events
            .iter()
            .any(|ev| matches!(ev, ZoneEvent::Vacated { zone: z, .. } if z == zone)),
        ProgramGuard::AllZonesVacant { .. } => ctx.zones.is_some_and(ZoneEngine::all_vacant),
        ProgramGuard::DataStale => ctx.health.is_blind(),
        ProgramGuard::DataFresh => !ctx.health.is_blind(),
        ProgramGuard::DepthRule { rule, triggered } => {
            ctx.depth.is_triggered(rule) == Some(*triggered)
        }
        ProgramGuard::Cardinality { value } => {
            ctx.scene.cardinality.as_deref() == Some(value.as_str())
        }
        ProgramGuard::PersonPresent => ctx.scene.person_present,
        ProgramGuard::PersonAbsent => !ctx.scene.person_present,
        ProgramGuard::FaceDetected { min_confidence } => {
            ctx.scene.face_present && ctx.scene.face_confidence.unwrap_or(0.0) >= *min_confidence
        }
        ProgramGuard::FaceAbsent => !ctx.scene.face_present,
        ProgramGuard::FaceInDwell => ctx.scene.face_in_dwell == Some(true),
        ProgramGuard::FaceNotInDwell => ctx.scene.face_in_dwell == Some(false),
        ProgramGuard::FaceAtEdge => ctx.scene.at_edge,
        ProgramGuard::FaceNotAtEdge => !ctx.scene.at_edge,
        ProgramGuard::FaceWasInside => ctx.face_was_inside,
        ProgramGuard::FaceWasNotInside => !ctx.face_was_inside,
    }
}
