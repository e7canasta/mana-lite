//! Compiled FSM program with resolved state and zone identifiers.

use std::collections::{HashMap, HashSet};

use crate::config::{FsmCatalog, FsmState, ZoneCatalog, ZoneSpec};
use crate::domain::{SignalTag, StateId, ZoneId};
use crate::signals::{
    Ratio, SignalDescriptor, SignalKind, SignalOp, SignalValue, scene_signal_catalog,
};

use super::guard::{FsmGuard, SignalLiteral, parse_dwell};

fn synthesize_zones_from_guards(catalog: &FsmCatalog) -> ZoneCatalog {
    let mut zones = HashMap::new();
    for transition in &catalog.fsm.transitions {
        for guard in &transition.guards {
            let zone = match guard {
                FsmGuard::ZonePresent { zone }
                | FsmGuard::ZoneOccupied { zone, .. }
                | FsmGuard::ZoneVacated { zone, .. } => zone,
                _ => continue,
            };
            zones.entry(zone.clone()).or_insert_with(|| ZoneSpec {
                x1: 0,
                y1: 0,
                x2: 1,
                y2: 1,
                label: None,
                hysteresis_ms: 0,
            });
        }
    }
    ZoneCatalog {
        zones,
        face_dwell: None,
    }
}

#[derive(Debug, Clone)]
pub struct ProgramState {
    pub label: Option<String>,
    pub models: Vec<String>,
    pub dwell_min_ms: Option<u64>,
    pub face_inside: bool,
    pub face_inside_maybe: bool,
}

impl From<&FsmState> for ProgramState {
    fn from(state: &FsmState) -> Self {
        Self {
            label: state.label.clone(),
            models: state.models.clone(),
            dwell_min_ms: state.dwell_min_ms,
            face_inside: state.face_inside,
            face_inside_maybe: state.face_inside_maybe,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ProgramFrom {
    Wildcard,
    State(StateId),
}

impl ProgramFrom {
    pub fn is_wildcard(&self) -> bool {
        matches!(self, Self::Wildcard)
    }

    pub fn matches(&self, state: &StateId) -> bool {
        matches!(self, Self::Wildcard) || matches!(self, Self::State(from) if from == state)
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Wildcard => "*",
            Self::State(id) => id.as_str(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgramTransition {
    pub(super) from: ProgramFrom,
    pub(super) to: StateId,
    pub(super) guards: Vec<ProgramGuard>,
    pub(super) dwell_ms: Option<u64>,
}

impl ProgramTransition {
    pub(super) fn from_name(&self) -> &str {
        self.from.name()
    }
}

#[derive(Debug, Clone)]
pub enum ProgramGuard {
    ZonePresent {
        zone: ZoneId,
    },
    ZoneOccupied {
        zone: ZoneId,
        min_confidence: f32,
        min_duration_ms: Option<u64>,
    },
    ZoneVacated {
        zone: ZoneId,
        min_duration_ms: Option<u64>,
    },
    AllZonesVacant {
        min_duration_ms: Option<u64>,
    },
    DataStale,
    DataFresh,
    DepthRule {
        rule: String,
        triggered: bool,
    },
    Signal {
        tag: SignalTag,
        op: SignalOp,
        value: SignalValue,
    },
    FaceNotInDwell,
    FaceAtEdge,
    FaceNotAtEdge,
    FaceWasInside,
    FaceWasNotInside,
}

/// Validated runtime representation of an FSM catalog.
#[derive(Debug, Clone)]
pub struct FsmProgram {
    initial: StateId,
    safe: StateId,
    reset: StateId,
    states: HashMap<StateId, ProgramState>,
    transitions: Vec<ProgramTransition>,
}

impl FsmProgram {
    /// Resolves every state and zone name required by the state machine.
    ///
    /// A program is only produced if its control-flow and zone references are
    /// valid, so the engine never needs to re-check catalog strings at runtime.
    pub fn compile(catalog: &FsmCatalog, zones: &ZoneCatalog) -> Result<Self, Vec<String>> {
        Self::compile_inner(catalog, zones, true)
    }

    /// Resolves identifiers without requiring every state to have a non-wildcard exit.
    ///
    /// Unit tests build incomplete catalogs; production always uses [`Self::compile`].
    pub fn compile_lenient(catalog: &FsmCatalog, zones: &ZoneCatalog) -> Result<Self, Vec<String>> {
        Self::compile_inner(catalog, zones, false)
    }

    fn compile_inner(
        catalog: &FsmCatalog,
        zones: &ZoneCatalog,
        require_exits: bool,
    ) -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        let states: HashMap<_, _> = catalog
            .fsm
            .states
            .iter()
            .map(|(name, state)| (StateId::new(name), ProgramState::from(state)))
            .collect();
        let resolve_state = |name: &str, role: &str, errors: &mut Vec<String>| {
            if states.contains_key(name) {
                Some(StateId::new(name))
            } else {
                errors.push(format!("{role} state '{name}' not found in states"));
                None
            }
        };

        let initial = resolve_state(&catalog.fsm.initial, "initial", &mut errors);
        let safe = resolve_state(&catalog.fsm.roles.safe, "safe", &mut errors);
        let reset = resolve_state(&catalog.fsm.roles.reset, "reset", &mut errors);

        let mut transitions = Vec::with_capacity(catalog.fsm.transitions.len());
        for transition in &catalog.fsm.transitions {
            let from = if transition.from == "*" {
                Some(ProgramFrom::Wildcard)
            } else {
                resolve_state(&transition.from, "transition from unknown", &mut errors)
                    .map(ProgramFrom::State)
            };
            let to = resolve_state(&transition.to, "transition to unknown", &mut errors);
            let guards = transition
                .guards
                .iter()
                .enumerate()
                .filter_map(|(index, guard)| {
                    Self::resolve_guard(
                        guard,
                        zones,
                        &transition.from,
                        &transition.to,
                        index,
                        &mut errors,
                    )
                })
                .collect();
            if let (Some(from), Some(to)) = (from, to) {
                transitions.push(ProgramTransition {
                    from,
                    to,
                    guards,
                    dwell_ms: transition.dwell.as_deref().and_then(parse_dwell),
                });
            }
        }

        // Wildcard exits are emergency paths (data_stale), not normal operation —
        // a state reachable only via wildcard is still a sink.
        let with_exit: HashSet<_> = transitions
            .iter()
            .filter_map(|transition| match &transition.from {
                ProgramFrom::State(state) => Some(state.clone()),
                ProgramFrom::Wildcard => None,
            })
            .collect();
        if require_exits {
            for state in states.keys() {
                if !with_exit.contains(state) {
                    errors.push(format!("state '{state}' has no outgoing transition (sink)"));
                }
            }
            if let Some(safe) = &safe {
                if !with_exit.contains(safe) {
                    errors.push(format!("safe state '{safe}' has no outgoing transition"));
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            initial: initial.expect("validated above"),
            safe: safe.expect("validated above"),
            reset: reset.expect("validated above"),
            states,
            transitions,
        })
    }

    /// Compiles a program after validating references owned by other catalogs.
    ///
    /// Zone and depth-rule checks apply only when those catalogs are provided
    /// (matching the former `validate_fsm` contract). When zones are omitted,
    /// zone names are still resolved into [`ZoneId`]s so the program can be
    /// built for model/depth-only checks.
    ///
    /// `model_names` and `depth_rule_names` are the known catalog keys; the
    /// control crate only needs names at compile time, never the model catalog
    /// or the measurement policy behind them.
    pub fn compile_with_references(
        catalog: &FsmCatalog,
        zones: Option<&ZoneCatalog>,
        model_names: Option<&HashSet<String>>,
        depth_rule_names: Option<&HashSet<String>>,
    ) -> Result<Self, Vec<String>> {
        let synthesized;
        let zones_for_resolve = match zones {
            Some(zones) => zones,
            None => {
                synthesized = synthesize_zones_from_guards(catalog);
                &synthesized
            }
        };
        let mut errors = Self::validate_references(catalog, zones, model_names, depth_rule_names);
        match Self::compile(catalog, zones_for_resolve) {
            Ok(program) if errors.is_empty() => Ok(program),
            Ok(_) => Err(errors),
            Err(compile_errors) => {
                errors.extend(compile_errors);
                Err(errors)
            }
        }
    }

    fn validate_references(
        catalog: &FsmCatalog,
        zones: Option<&ZoneCatalog>,
        model_names: Option<&HashSet<String>>,
        depth_rule_names: Option<&HashSet<String>>,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        // A state whose model key is absent from the catalog runs no detector
        // at all: it must fail at boot, not degrade silently at runtime.
        if let Some(known) = model_names {
            // Sorted: `states` is a HashMap, and boot diagnostics must be
            // byte-identical across runs for the same catalogs.
            let mut names: Vec<&String> = catalog.fsm.states.keys().collect();
            names.sort_unstable();
            for name in names {
                for model_key in &catalog.fsm.states[name].models {
                    if !known.contains(model_key.as_str()) {
                        errors.push(format!(
                            "state '{name}' references model '{model_key}' not found in model catalog"
                        ));
                    }
                }
            }
        }
        for transition in &catalog.fsm.transitions {
            for guard in &transition.guards {
                match guard {
                    FsmGuard::ZonePresent { zone }
                    | FsmGuard::ZoneOccupied { zone, .. }
                    | FsmGuard::ZoneVacated { zone, .. } => {
                        if let Some(zc) = zones {
                            if !zc.zones.contains_key(zone) {
                                errors.push(format!(
                                    "transition {}→{} references zone '{}' not found in zone catalog",
                                    transition.from, transition.to, zone
                                ));
                            }
                        }
                    }
                    FsmGuard::DepthRule { rule, .. } => {
                        if let Some(names) = depth_rule_names {
                            if !names.contains(rule) {
                                errors.push(format!(
                                    "transition {}→{} references depth rule '{}' not found in depth-rules",
                                    transition.from, transition.to, rule
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        errors
    }

    fn resolve_guard(
        guard: &FsmGuard,
        zones: &ZoneCatalog,
        from: &str,
        to: &str,
        index: usize,
        errors: &mut Vec<String>,
    ) -> Option<ProgramGuard> {
        let resolve_zone = |zone: &str, errors: &mut Vec<String>| {
            if zones.zones.contains_key(zone) {
                Some(ZoneId::new(zone))
            } else {
                errors.push(format!(
                    "transition {from}→{to} references zone '{zone}' not found in zone catalog"
                ));
                None
            }
        };
        Some(match guard {
            FsmGuard::ZonePresent { zone } => ProgramGuard::ZonePresent {
                zone: resolve_zone(zone, errors)?,
            },
            FsmGuard::ZoneOccupied {
                zone,
                min_confidence,
                min_duration_ms,
            } => ProgramGuard::ZoneOccupied {
                zone: resolve_zone(zone, errors)?,
                min_confidence: *min_confidence,
                min_duration_ms: *min_duration_ms,
            },
            FsmGuard::ZoneVacated {
                zone,
                min_confidence,
                min_duration_ms,
            } => {
                if min_confidence.is_some() {
                    errors.push(format!(
                        "transition {from}→{to} sets min_confidence on zone_vacated, \
                         which carries no confidence: remove it or use zone_occupied"
                    ));
                    return None;
                }
                ProgramGuard::ZoneVacated {
                    zone: resolve_zone(zone, errors)?,
                    min_duration_ms: *min_duration_ms,
                }
            }
            FsmGuard::AllZonesVacant { min_duration_ms } => ProgramGuard::AllZonesVacant {
                min_duration_ms: *min_duration_ms,
            },
            FsmGuard::DataStale => ProgramGuard::DataStale,
            FsmGuard::DataFresh => ProgramGuard::DataFresh,
            FsmGuard::DepthRule { rule, triggered } => ProgramGuard::DepthRule {
                rule: rule.clone(),
                triggered: *triggered,
            },
            FsmGuard::Signal { tag, op, value } => {
                return Self::resolve_signal_guard(tag, op, value, from, to, index, errors);
            }
            FsmGuard::FaceNotInDwell => ProgramGuard::FaceNotInDwell,
            FsmGuard::FaceAtEdge => ProgramGuard::FaceAtEdge,
            FsmGuard::FaceNotAtEdge => ProgramGuard::FaceNotAtEdge,
            FsmGuard::FaceWasInside => ProgramGuard::FaceWasInside,
            FsmGuard::FaceWasNotInside => ProgramGuard::FaceWasNotInside,
        })
    }

    fn resolve_signal_guard(
        tag_name: &str,
        op_name: &str,
        literal: &SignalLiteral,
        from: &str,
        to: &str,
        index: usize,
        errors: &mut Vec<String>,
    ) -> Option<ProgramGuard> {
        let context = format!("transition {from}→{to}, guard {index}, signal tag '{tag_name}'");
        if from == "*" {
            errors.push(format!(
                "{context}: Signal guards are not allowed on wildcard transitions; expected a state-specific transition"
            ));
            return None;
        }
        let tag = SignalTag::new(tag_name);
        let catalog = scene_signal_catalog();
        let Some(descriptor) = catalog.get(&tag) else {
            errors.push(format!(
                "{context}: tag is not declared by the signal catalog"
            ));
            return None;
        };

        let Some(op) = SignalOp::parse(op_name) else {
            errors.push(format!(
                "{context}: unknown operator '{op_name}', expected one of ==, !=, >=, <=, >, <"
            ));
            return None;
        };
        if let Err(error) = op.require_compatible(descriptor.kind()) {
            errors.push(format!(
                "{context}: operator '{}' is incompatible with {:?}; expected an operator valid for {:?}",
                op.symbol(), error.kind, descriptor.kind()
            ));
            return None;
        }

        let value =
            match Self::compile_signal_literal(literal, descriptor.kind(), descriptor, &context) {
                Ok(value) => value,
                Err(error) => {
                    errors.push(error);
                    return None;
                }
            };

        Some(ProgramGuard::Signal { tag, op, value })
    }

    fn compile_signal_literal(
        literal: &SignalLiteral,
        kind: SignalKind,
        descriptor: &SignalDescriptor,
        context: &str,
    ) -> Result<SignalValue, String> {
        match (kind, literal) {
            (SignalKind::Bool, SignalLiteral::Bool(value)) => Ok(SignalValue::Bool(*value)),
            (SignalKind::Bool, other) => Err(format!(
                "{context}: expected a Bool literal, got {}",
                Self::literal_kind(other)
            )),
            (SignalKind::Count, SignalLiteral::Integer(value)) => {
                let count = u64::try_from(*value).map_err(|_| {
                    format!("{context}: expected a non-negative Count literal, got {value}")
                })?;
                Ok(SignalValue::Count(count))
            }
            (SignalKind::Count, other) => Err(format!(
                "{context}: expected a non-negative integer Count literal, got {}",
                Self::literal_kind(other)
            )),
            (SignalKind::Ratio, SignalLiteral::Integer(value)) => {
                #[allow(clippy::cast_precision_loss)]
                let value = *value as f64;
                Self::compile_ratio_literal(value, context)
            }
            (SignalKind::Ratio, SignalLiteral::Float(value)) => {
                Self::compile_ratio_literal(*value, context)
            }
            (SignalKind::Ratio, other) => Err(format!(
                "{context}: expected a finite Ratio literal in [0, 1], got {}",
                Self::literal_kind(other)
            )),
            (SignalKind::Label, SignalLiteral::Text(value)) => {
                if descriptor.allows_label(value) {
                    Ok(SignalValue::Label(value.clone()))
                } else {
                    let expected = descriptor
                        .allowed_labels()
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(", ");
                    Err(format!(
                        "{context}: label '{value}' is not emitible; expected one of [{expected}]"
                    ))
                }
            }
            (SignalKind::Label, other) => Err(format!(
                "{context}: expected a Label string literal, got {}",
                Self::literal_kind(other)
            )),
        }
    }

    fn compile_ratio_literal(value: f64, context: &str) -> Result<SignalValue, String> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(format!(
                "{context}: expected a finite Ratio literal in [0, 1], got {value}"
            ));
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let value = value as f32;
        Ratio::new(value).map(SignalValue::Ratio).map_err(|_| {
            format!("{context}: expected a finite Ratio literal in [0, 1], got {value}")
        })
    }

    fn literal_kind(literal: &SignalLiteral) -> &'static str {
        match literal {
            SignalLiteral::Bool(_) => "Bool",
            SignalLiteral::Integer(_) => "Integer",
            SignalLiteral::Float(_) => "Float",
            SignalLiteral::Text(_) => "Text",
        }
    }

    pub(super) fn initial(&self) -> &StateId {
        &self.initial
    }

    pub(super) fn safe(&self) -> &StateId {
        &self.safe
    }

    pub(super) fn reset(&self) -> &StateId {
        &self.reset
    }

    pub(super) fn state(&self, id: &str) -> Option<&ProgramState> {
        self.states.get(id)
    }

    pub(super) fn transitions(&self) -> &[ProgramTransition] {
        &self.transitions
    }
}
