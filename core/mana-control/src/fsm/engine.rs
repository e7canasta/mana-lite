use std::collections::HashMap;
use std::time::Instant;

use crate::domain::StateId;
use crate::DepthRuleSnapshot;
use crate::health::Health;
use crate::zones::{ZoneEngine, ZoneEvent};

use super::guard::{
    GuardCtx, state_label, transition_min_dwell, transition_trigger, try_transition,
};
use super::FsmProgram;

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
    current_state: StateId,
    program: FsmProgram,
    state_entered_at: Instant,
    dwell_timers: HashMap<String, Instant>,
    face_was_inside: bool,
}

impl FsmEngine {
    pub fn from_program_at(program: FsmProgram, now: Instant) -> Self {
        Self {
            current_state: program.initial().clone(),
            program,
            state_entered_at: now,
            dwell_timers: HashMap::new(),
            face_was_inside: false,
        }
    }

    pub fn current_state(&self) -> &str {
        self.current_state.as_str()
    }

    #[must_use]
    pub fn face_was_inside(&self) -> bool {
        self.face_was_inside
    }

    pub fn snapshot_at(&self, now: Instant) -> FsmSnapshot {
        let mut active_timers: Vec<_> = self
            .dwell_timers
            .iter()
            .map(|(trigger, started_at)| {
                let required_ms = self
                    .program
                    .transitions()
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
            state: self.current_state.to_string(),
            state_label: state_label(&self.program, self.current_state.as_str()),
            state_dwell_ms: now
                .saturating_duration_since(self.state_entered_at)
                .as_millis() as u64,
            state_dwell_required_ms: self
                .program
                .state(self.current_state.as_str())
                .and_then(|state| state.dwell_min_ms),
            face_was_inside: self.face_was_inside,
            active_timers,
        }
    }

    /// Evaluate only global (`from = "*"`) transitions between keyframes.
    /// State-specific scene transitions must wait for fresh frame evidence.
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

        let ctx = GuardCtx {
            zone_events,
            zones: zone_engine,
            health,
            depth,
            scene: context,
            face_was_inside: self.face_was_inside,
        };
        for t in self.program.transitions() {
            if !t.from.is_wildcard() {
                continue;
            }
            if let Some(result) = try_transition(
                &mut self.dwell_timers,
                t,
                &self.state_entered_at,
                &self.program,
                &ctx,
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

        let transitions = self.program.transitions();
        let ctx = GuardCtx {
            zone_events,
            zones: zone_engine,
            health,
            depth,
            scene: context,
            face_was_inside: self.face_was_inside,
        };

        for t in transitions {
            if t.from.is_wildcard() {
                if let Some(result) = try_transition(
                    &mut self.dwell_timers,
                    t,
                    &self.state_entered_at,
                    &self.program,
                    &ctx,
                    now,
                ) {
                    self.apply_transition(&result, context, now);
                    return Some(result);
                }
            }
        }

        for t in transitions {
            if !t.from.matches(&self.current_state) {
                continue;
            }
            if let Some(result) = try_transition(
                &mut self.dwell_timers,
                t,
                &self.state_entered_at,
                &self.program,
                &ctx,
                now,
            ) {
                self.apply_transition(&result, context, now);
                return Some(result);
            }
        }

        None
    }

    fn update_face_latch(&mut self, context: &FsmSceneContext) {
        if self.state_sets_face_latch(&self.current_state)
            || (self.state_maybe_sets_face_latch(&self.current_state) && context.face_present)
        {
            self.face_was_inside = true;
        }
        if context.cardinality.as_deref() == Some("multiple") {
            self.face_was_inside = false;
        }
    }

    fn state_sets_face_latch(&self, state: &str) -> bool {
        self.program
            .state(state)
            .is_some_and(|entry| entry.face_inside)
    }

    fn state_maybe_sets_face_latch(&self, state: &str) -> bool {
        self.program
            .state(state)
            .is_some_and(|entry| entry.face_inside_maybe)
    }

    fn state_dwell_satisfied(&self, now: Instant) -> bool {
        self.program
            .state(self.current_state.as_str())
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
        self.current_state = StateId::new(&result.to);
        self.state_entered_at = now;
        self.dwell_timers.clear();
        if self.current_state == *self.program.reset() {
            self.face_was_inside = false;
        } else if self.state_sets_face_latch(&result.to)
            || (self.state_maybe_sets_face_latch(&result.to) && context.face_present)
        {
            self.face_was_inside = true;
        }
    }

    pub fn current_models(&self) -> Vec<String> {
        self.program
            .state(self.current_state.as_str())
            .map(|s| s.models.clone())
            .unwrap_or_default()
    }

    /// Fuerza el estado seguro configurado en `fsm.roles.safe` tras un panic
    /// del procesador de keyframe:
    /// un panic deja al motor con estado posiblemente a medio actualizar
    /// (tracker, occupancy y presence conservan su estado; el FSM puede
    /// quedar con transiciones a medias), y reanudarlo tal cual es arriesgado.
    /// En el estado seguro declara cero modelos — la máquina se detiene sobre
    /// lo que sí sabe que es estable, y al ciclo siguiente la transición
    /// `data_fresh` la lleva a `fsm.roles.reset`, donde `apply_transition`
    /// reconstruye el estado del FSM desde cero (incluido el latch
    /// `face_was_inside`).
    ///
    /// No se hace vía Health: un `mark_blind()` forzado con `stale_ms ≈ 0`
    /// limpiaría la bandera en el mismo ciclo y emitiría un `Recovered`
    /// espurio. Este método es un salto de estado directo, sin pasar por
    /// el evaluador.
    pub fn force_safe_state(&mut self, now: Instant) {
        self.current_state.clone_from(self.program.safe());
        self.state_entered_at = now;
        self.dwell_timers.clear();
    }
}
