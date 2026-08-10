//! Fixed-cadence scene decisions over a frozen process image.
use std::time::{Duration, Instant};

pub use crate::SceneSample as ClinicalSample;
use crate::domain::{ClassName, LoopId, SignalTag};
use crate::fsm::{FsmEngine, FsmSceneContext, FsmTransitionResult};
use crate::health::{Health, HealthTransition};
use crate::occupancy::{
    self, OccupancyStateMachine, OccupancyUpdate, RoomCardinality, SecondPersonState,
    SignalValidity,
};
use crate::presence::{PresenceFilter, PresenceState, PresenceUpdate};
use crate::signals::{Ratio, SceneSignalsSnapshot, SignalTable, SignalValue, scene_signal_catalog};
use crate::track::{Track, TrackEvent, Tracker};
use crate::zones::{ZoneEngine, ZoneEvent};
pub use crate::{AgedEvidence, ProcessImage, SceneObservation, SceneSample};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanInstant(Instant);
impl ScanInstant {
    /// Private on purpose (ADR-029): a `ScanInstant` may only originate from a
    /// [`ScanTimeline`], so control time can never be read off the wall clock.
    const fn from_instant(v: Instant) -> Self {
        Self(v)
    }
    pub const fn as_instant(self) -> Instant {
        self.0
    }
    pub fn elapsed_ms_since(self, earlier: Self) -> u64 {
        crate::timing::elapsed_ms(self.0, earlier.0)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlStamp {
    pub scan_seq: u64,
    pub evidence_frame_id: u64,
    pub observations_age_ms: u64,
    pub depth_age_ms: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanConfig {
    pub period_ms: u64,
}
impl Default for ScanConfig {
    fn default() -> Self {
        Self { period_ms: 200 }
    }
}
impl ScanConfig {
    pub fn period(self) -> Duration {
        Duration::from_millis(self.period_ms.max(1))
    }
}
#[derive(Debug, Clone)]
pub struct ControlPolicy {
    pub person_class: ClassName,
    pub presence_enabled: bool,
    pub data_stale_ms: u64,
    pub scan_period_ms: u64,
    pub face_dwell_roi: Option<[u32; 4]>,
    pub person_detection_roi: Option<[u32; 4]>,
    pub face_edge_margin_px: u32,
}
pub struct ControlState {
    pub loop_id: LoopId,
    pub tracker: Option<Tracker>,
    pub presence: PresenceFilter,
    pub occupancy: OccupancyStateMachine,
    pub zone_engine: Option<ZoneEngine>,
    pub fsm_engine: Option<FsmEngine>,
    pub health: Health,
    pub fsm_context: FsmSceneContext,
    /// Signals produced by the most recent control cycle.
    pub signal_snapshot: SceneSignalsSnapshot,
    pub last_scan_at: Instant,
    pub scan_seq: u64,
    pub policy: ControlPolicy,
}
#[derive(Debug, Clone)]
pub enum SceneEvent {
    Track {
        event: TrackEvent,
        stamp: ControlStamp,
    },
    Presence {
        stamp: ControlStamp,
        state: RoomCardinality,
        presence: PresenceState,
        second_person: SecondPersonState,
        signal: SignalValidity,
        raw_person_count: usize,
        confirmed_person_count: usize,
        held: bool,
        positive_ms: u64,
        empty_ms: u64,
        single_timer_ms: u64,
        empty_timer_ms: u64,
        multiple_candidate_timer_ms: u64,
        multiple_exit_timer_ms: u64,
    },
    Occupancy {
        state: RoomCardinality,
        second_person: SecondPersonState,
        signal: SignalValidity,
    },
    EntityBoxes(Vec<Track>),
    Zone {
        event: ZoneEvent,
        stamp: ControlStamp,
    },
    FsmTransition(FsmTransitionResult),
    FsmState(String),
    Health(HealthTransition),
}
/// Advances one control tick against a frozen process image.
///
/// Takes the loop's own [`ScanTimeline`] rather than a bare instant: control
/// time can only come from the timeline (ADR-029), and the loop identity of the
/// clock is checked against the state it drives, so a multi-loop host cannot
/// tick one loop with another's clock.
pub fn scan(
    state: &mut ControlState,
    image: &ProcessImage,
    timeline: &ScanTimeline,
) -> Vec<SceneEvent> {
    debug_assert_eq!(
        state.loop_id,
        *timeline.loop_id(),
        "ScanTimeline belongs to a different control loop than ControlState"
    );
    let now = timeline.now().as_instant();
    let mut events = Vec::new();
    let dt = predict(state, now);
    let input = age_input(state, image, now);
    let presence = update_presence(state, &input, now);
    let confirmed = update_tracking(state, image, &input, &presence, dt, &mut events);
    let occupancy = update_occupancy(state, &input, &presence, confirmed, now, &mut events);
    let zones = update_zones(state, input.stamp, now, &mut events);
    let mut signals = SignalTable::new();
    evaluate_fsm(
        state,
        image,
        &input,
        occupancy.state,
        &zones,
        now,
        &mut signals,
        &mut events,
    );
    evaluate_health(state, now, &mut events);
    events
}

struct ScanInput {
    sample: SceneSample,
    stamp: ControlStamp,
    valid: bool,
}

struct PresenceOutcome {
    observations: Vec<SceneObservation>,
    update: PresenceUpdate,
}

/// Advance cadence clocks and predict tracks to `now`.
fn predict(state: &mut ControlState, now: Instant) -> u64 {
    let dt = crate::timing::elapsed_ms(now, state.last_scan_at);
    let dt = dt.max(state.policy.scan_period_ms.max(1));
    state.last_scan_at = now;
    state.scan_seq = state.scan_seq.saturating_add(1);
    if let Some(t) = state.tracker.as_mut() {
        t.predict_at(dt)
    }
    dt
}

fn age_input(state: &ControlState, image: &ProcessImage, now: Instant) -> ScanInput {
    let age = image.observations_age_ms(now);
    let sample = image
        .observations
        .as_ref()
        .map(|x| x.value.clone())
        .unwrap_or_else(SceneSample::unavailable);
    let stamp = ControlStamp {
        scan_seq: state.scan_seq,
        evidence_frame_id: sample.frame_number,
        observations_age_ms: age,
        depth_age_ms: image.depth_age_ms(now),
    };
    let valid = sample.signal_valid && age <= state.policy.data_stale_ms;
    ScanInput {
        sample,
        stamp,
        valid,
    }
}

fn update_presence(state: &mut ControlState, input: &ScanInput, now: Instant) -> PresenceOutcome {
    let (observations, update) =
        state
            .presence
            .update_at(&input.sample.observations, input.valid, now);
    PresenceOutcome {
        observations,
        update,
    }
}

fn update_tracking(
    state: &mut ControlState,
    image: &ProcessImage,
    input: &ScanInput,
    presence: &PresenceOutcome,
    dt: u64,
    events: &mut Vec<SceneEvent>,
) -> usize {
    let tracking = if presence.update.state == PresenceState::Ambiguous {
        &[][..]
    } else {
        &presence.observations
    };
    if let Some(t) = state.tracker.as_mut() {
        let allow = input.sample.raw_person_count == 1
            && tracking.len() == 1
            && tracking[0].class == state.policy.person_class;
        let input_obs = if image.measurement_pending {
            tracking
        } else {
            &[]
        };
        for event in t.associate_observations(input_obs, allow && image.measurement_pending, dt) {
            events.push(SceneEvent::Track {
                event,
                stamp: input.stamp,
            })
        }
    }
    state.tracker.as_ref().map_or(0, |t| {
        t.current_tracks()
            .into_iter()
            .filter(|x| x.class == state.policy.person_class)
            .count()
    })
}

fn update_occupancy(
    state: &mut ControlState,
    input: &ScanInput,
    presence: &PresenceOutcome,
    confirmed: usize,
    now: Instant,
    events: &mut Vec<SceneEvent>,
) -> OccupancyUpdate {
    let update = state.occupancy.update_at(
        occupancy::build_evidence(occupancy::OccupancyEvidenceInputs {
            tracking_enabled: state.tracker.is_some(),
            presence_enabled: state.policy.presence_enabled,
            signal_valid: input.valid,
            raw_person_count: input.sample.raw_person_count,
            confirmed_person_count: confirmed,
            presence_is_present: presence.update.state == PresenceState::Present,
            presence_held: presence.update.held,
        }),
        now,
    );
    events.push(SceneEvent::Occupancy {
        state: update.state,
        second_person: update.second_person,
        signal: SignalValidity::from_bool(input.valid),
    });
    events.push(SceneEvent::Presence {
        stamp: input.stamp,
        state: update.state,
        presence: presence.update.state,
        second_person: update.second_person,
        signal: SignalValidity::from_bool(input.valid),
        raw_person_count: input.sample.raw_person_count,
        confirmed_person_count: confirmed,
        held: presence.update.held,
        positive_ms: presence.update.positive_ms,
        empty_ms: presence.update.empty_ms,
        single_timer_ms: update.single_timer_ms,
        empty_timer_ms: update.empty_timer_ms,
        multiple_candidate_timer_ms: update.multiple_candidate_timer_ms,
        multiple_exit_timer_ms: update.multiple_exit_timer_ms,
    });
    update
}

fn update_zones(
    state: &mut ControlState,
    stamp: ControlStamp,
    now: Instant,
    events: &mut Vec<SceneEvent>,
) -> Vec<ZoneEvent> {
    let zones = if let Some(z) = state.zone_engine.as_mut() {
        let tracks = state
            .tracker
            .as_ref()
            .map_or_else(Vec::new, |t| t.current_tracks());
        let es = z.evaluate_at(&tracks, now);
        for event in &es {
            events.push(SceneEvent::Zone {
                event: event.clone(),
                stamp,
            })
        }
        es
    } else {
        Vec::new()
    };
    if let Some(t) = state.tracker.as_ref() {
        events.push(SceneEvent::EntityBoxes(
            t.current_tracks().into_iter().cloned().collect(),
        ))
    }
    zones
}

fn evaluate_fsm(
    state: &mut ControlState,
    image: &ProcessImage,
    input: &ScanInput,
    cardinality: RoomCardinality,
    zones: &[ZoneEvent],
    now: Instant,
    signals: &mut SignalTable,
    events: &mut Vec<SceneEvent>,
) {
    // Context refresh moved here from between occupancy and zones: nothing between
    // those steps reads fsm_context, and App only reads it after the full scan batch.
    update_context(
        &mut state.fsm_context,
        &state.policy,
        cardinality,
        input.sample.raw_person_count,
        input.sample.face_model_ran,
        &input.sample.observations,
        signals,
    );
    if let Some(f) = state.fsm_engine.as_mut() {
        f.update_face_latch(&state.fsm_context);
        insert_signal(
            signals,
            "cara.estuvo_dentro",
            SignalValue::Bool(f.face_was_inside()),
        );
    }
    state.signal_snapshot = signals.snapshot(scene_signal_catalog());
    if let Some(f) = state.fsm_engine.as_mut() {
        let depth = image.depth_snapshot();
        if let Some(x) = f.evaluate_with_signals_at(
            zones,
            state.zone_engine.as_ref(),
            &state.health,
            &depth,
            &state.fsm_context,
            &state.signal_snapshot,
            now,
        ) {
            events.push(SceneEvent::FsmTransition(x))
        }
        events.push(SceneEvent::FsmState(f.snapshot_at(now).state));
        if let Some(x) = f.evaluate_wildcard_with_signals_at(
            &[],
            state.zone_engine.as_ref(),
            &state.health,
            &depth,
            &state.fsm_context,
            &state.signal_snapshot,
            now,
        ) {
            events.push(SceneEvent::FsmTransition(x));
            events.push(SceneEvent::FsmState(f.snapshot_at(now).state))
        }
    }
}

fn evaluate_health(state: &mut ControlState, now: Instant, events: &mut Vec<SceneEvent>) {
    let h = state.health.evaluate_at(now);
    if !matches!(h, HealthTransition::None) {
        events.push(SceneEvent::Health(h))
    }
}

fn update_context(
    ctx: &mut FsmSceneContext,
    policy: &ControlPolicy,
    cardinality: RoomCardinality,
    raw: usize,
    face_model_ran: bool,
    obs: &[SceneObservation],
    signals: &mut SignalTable,
) {
    let person = obs
        .iter()
        .filter(|x| x.class == policy.person_class)
        .max_by(|a, b| a.confidence.total_cmp(&b.confidence));
    let face = person.and_then(|x| x.face);
    ctx.cardinality = Some(cardinality.as_str().into());
    ctx.person_present = raw > 0;
    ctx.face_present = face.is_some();
    ctx.face_confidence = face.map(|x| x.confidence);
    ctx.face_in_dwell = policy
        .face_dwell_roi
        .map(|r| face.is_some_and(|f| intersects(f.bbox, r)));
    ctx.at_edge = person.is_some_and(|p| {
        near(
            p.bbox,
            policy.person_detection_roi,
            policy.face_edge_margin_px,
        )
    });
    ctx.face_model_ran = face_model_ran;

    insert_signal(
        signals,
        "persona.presente",
        SignalValue::Bool(ctx.person_present),
    );
    insert_signal(signals, "persona.cantidad", SignalValue::Count(raw as u64));
    insert_signal(
        signals,
        "cara.presente",
        SignalValue::Bool(ctx.face_present),
    );
    if let Some(confidence) = ctx.face_confidence {
        let ratio = Ratio::new(confidence)
            .unwrap_or_else(|error| panic!("invalid face confidence for scene signal: {error:?}"));
        insert_signal(signals, "cara.confianza", SignalValue::Ratio(ratio));
    }
    if let Some(in_dwell) = ctx.face_in_dwell {
        insert_signal(signals, "cara.en_dwell", SignalValue::Bool(in_dwell));
    }
    insert_signal(signals, "cara.en_borde", SignalValue::Bool(ctx.at_edge));
    insert_signal(
        signals,
        "cara.modelo_corrio",
        SignalValue::Bool(ctx.face_model_ran),
    );
    insert_signal(
        signals,
        "ocupacion.cardinalidad",
        SignalValue::Label(cardinality.as_str().into()),
    );
}

fn insert_signal(table: &mut SignalTable, tag: &str, value: SignalValue) {
    table
        .insert(scene_signal_catalog(), SignalTag::new(tag), value)
        .unwrap_or_else(|error| panic!("scene signal producer/catalog mismatch: {error:?}"));
}
fn intersects(b: [f32; 4], r: [u32; 4]) -> bool {
    b[0] < r[2] as f32 && b[2] > r[0] as f32 && b[1] < r[3] as f32 && b[3] > r[1] as f32
}
fn near(b: [f32; 4], r: Option<[u32; 4]>, m: u32) -> bool {
    let Some(r) = r else { return false };
    let m = m as f32;
    b[0] <= r[0] as f32 + m
        || b[1] <= r[1] as f32 + m
        || b[2] >= r[2] as f32 - m
        || b[3] >= r[3] as f32 - m
}
#[derive(Debug, Clone)]
pub struct ScanTimeline {
    loop_id: LoopId,
    start: Instant,
    period_ms: u64,
    tick: u64,
}
impl ScanTimeline {
    pub fn new(loop_id: LoopId, start: Instant, period_ms: u64) -> Self {
        Self {
            loop_id,
            start,
            period_ms: period_ms.max(1),
            tick: 0,
        }
    }
    pub fn loop_id(&self) -> &LoopId {
        &self.loop_id
    }
    pub fn period_ms(&self) -> u64 {
        self.period_ms
    }
    pub fn now(&self) -> ScanInstant {
        ScanInstant::from_instant(self.start + Duration::from_millis(self.tick * self.period_ms))
    }
    pub fn advance(&mut self) -> ScanInstant {
        self.tick += 1;
        self.now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        FsmCatalog, FsmRoles, FsmRoot, FsmState, OccupancyPolicy, PresencePoiPolicy, ZoneCatalog,
    };

    fn person() -> SceneObservation {
        SceneObservation {
            class: "person".into(),
            bbox: [0.0, 0.0, 100.0, 100.0],
            confidence: 0.9,
            source_models: vec!["synthetic".into()],
            face: None,
        }
    }

    fn person_with_face() -> SceneObservation {
        SceneObservation {
            face: Some(crate::FaceObservation {
                bbox: [10.0, 10.0, 20.0, 20.0],
                confidence: 0.8,
            }),
            ..person()
        }
    }

    fn signal<'a>(snapshot: &'a SceneSignalsSnapshot, name: &str) -> &'a SignalValue {
        snapshot
            .get(&SignalTag::new(name))
            .unwrap_or_else(|| panic!("expected signal {name}"))
    }

    fn assert_bool(snapshot: &SceneSignalsSnapshot, name: &str, expected: bool) {
        assert!(matches!(signal(snapshot, name), SignalValue::Bool(value) if *value == expected));
    }

    fn control_state(start: Instant, data_stale_ms: u64) -> ControlState {
        ControlState {
            loop_id: LoopId::default_loop(),
            tracker: None,
            presence: PresenceFilter::new(
                true,
                "person",
                PresencePoiPolicy {
                    on_ms: 0,
                    off_ms: 500,
                },
            ),
            occupancy: OccupancyStateMachine::new(OccupancyPolicy {
                single_confirm_ms: 0,
                empty_confirm_ms: 500,
                multiple_confirm_ms: 300,
                multiple_exit_ms: 300,
                require_confirmed_tracks: false,
            }),
            zone_engine: None,
            fsm_engine: None,
            health: Health::new_at(10_000, 5_000, start),
            fsm_context: Default::default(),
            signal_snapshot: Default::default(),
            last_scan_at: start,
            scan_seq: 0,
            policy: ControlPolicy {
                person_class: "person".into(),
                presence_enabled: true,
                data_stale_ms,
                scan_period_ms: 200,
                face_dwell_roi: None,
                person_detection_roi: None,
                face_edge_margin_px: 0,
            },
        }
    }

    #[test]
    fn scan_emits_presence_and_occupancy_on_valid_evidence() {
        let start = Instant::now(); // cfg(test)
        let timeline = ScanTimeline::new(LoopId::default_loop(), start, 200);
        let mut state = control_state(start, 10_000);
        let image = ProcessImage {
            observations: Some(AgedEvidence::new(
                SceneSample {
                    observations: vec![person()],
                    signal_valid: true,
                    raw_person_count: 1,
                    frame_number: 1,
                    face_model_ran: false,
                },
                start,
            )),
            depth: None,
            measurement_pending: true,
        };

        let events = scan(&mut state, &image, &timeline);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SceneEvent::Presence { .. })),
            "expected Presence event"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SceneEvent::Occupancy { .. })),
            "expected Occupancy event"
        );
    }

    #[test]
    fn scan_produces_base_signals_and_rebuilds_them_each_cycle() {
        let start = Instant::now(); // cfg(test)
        let mut timeline = ScanTimeline::new(LoopId::default_loop(), start, 200);
        let mut state = control_state(start, 10_000);
        state.policy.face_dwell_roi = Some([0, 0, 50, 50]);
        state.policy.person_detection_roi = Some([0, 0, 100, 100]);

        let mut image = ProcessImage {
            observations: Some(AgedEvidence::new(
                SceneSample {
                    observations: vec![person_with_face()],
                    signal_valid: true,
                    raw_person_count: 1,
                    frame_number: 1,
                    face_model_ran: true,
                },
                start,
            )),
            depth: None,
            measurement_pending: true,
        };

        scan(&mut state, &image, &timeline);
        let first = state.signal_snapshot.clone();
        assert_eq!(first.catalog_version(), 1);
        assert_eq!(
            first.iter().filter(|(_, value)| value.is_some()).count(),
            8,
            "the first cycle must publish the eight base signals"
        );
        assert_bool(&first, "persona.presente", true);
        assert!(matches!(
            signal(&first, "persona.cantidad"),
            SignalValue::Count(1)
        ));
        assert_bool(&first, "cara.presente", true);
        let SignalValue::Ratio(confidence) = signal(&first, "cara.confianza") else {
            panic!("face confidence must be a ratio");
        };
        assert!((confidence.get() - 0.8).abs() < f32::EPSILON);
        assert_bool(&first, "cara.en_dwell", true);
        assert_bool(&first, "cara.en_borde", true);
        assert_bool(&first, "cara.modelo_corrio", true);
        assert!(matches!(
            signal(&first, "ocupacion.cardinalidad"),
            SignalValue::Label(value) if value == "single"
        ));
        assert!(first.is_absent(&SignalTag::new("cara.estuvo_dentro")));

        timeline.advance();
        image.observations = Some(AgedEvidence::new(
            SceneSample {
                observations: Vec::new(),
                signal_valid: true,
                raw_person_count: 0,
                frame_number: 2,
                face_model_ran: false,
            },
            start + Duration::from_millis(200),
        ));
        scan(&mut state, &image, &timeline);
        let second = &state.signal_snapshot;
        assert_bool(second, "persona.presente", false);
        assert!(matches!(
            signal(second, "persona.cantidad"),
            SignalValue::Count(0)
        ));
        assert_bool(second, "cara.presente", false);
        assert!(second.is_absent(&SignalTag::new("cara.confianza")));
        assert_bool(second, "cara.en_dwell", false);
        assert_bool(second, "cara.modelo_corrio", false);
    }

    #[test]
    fn scan_publishes_latch_after_fsm_applies_its_rules() {
        let start = Instant::now(); // cfg(test)
        let catalog = FsmCatalog {
            fsm: FsmRoot {
                initial: "inside".into(),
                states: [(
                    "inside".into(),
                    FsmState {
                        label: None,
                        models: Vec::new(),
                        dwell_min_ms: None,
                        face_inside: true,
                        face_inside_maybe: false,
                    },
                )]
                .into_iter()
                .collect(),
                roles: FsmRoles {
                    safe: "inside".into(),
                    reset: "inside".into(),
                },
                transitions: Vec::new(),
            },
        };
        let program = crate::fsm::FsmProgram::compile_lenient(&catalog, &ZoneCatalog::default())
            .expect("compile latch fixture");
        let mut state = control_state(start, 10_000);
        state.fsm_engine = Some(FsmEngine::from_program_at(program, start));
        let timeline = ScanTimeline::new(LoopId::default_loop(), start, 200);
        let image = ProcessImage::empty();

        scan(&mut state, &image, &timeline);

        assert!(
            state
                .fsm_engine
                .as_ref()
                .is_some_and(FsmEngine::face_was_inside)
        );
        assert_bool(&state.signal_snapshot, "cara.estuvo_dentro", true);
    }

    /// Integration goldens always wire tracker/zones/FSM. This catches a lost
    /// `if let Some(...)` when those three Option engines are split out of `scan()`.
    #[test]
    fn scan_with_null_engines_emits_only_occupancy_and_presence() {
        let start = Instant::now(); // cfg(test)
        let timeline = ScanTimeline::new(LoopId::default_loop(), start, 200);
        let mut state = control_state(start, 10_000);
        assert!(state.tracker.is_none());
        assert!(state.zone_engine.is_none());
        assert!(state.fsm_engine.is_none());

        let image = ProcessImage {
            observations: Some(AgedEvidence::new(
                SceneSample {
                    observations: vec![person()],
                    signal_valid: true,
                    raw_person_count: 1,
                    frame_number: 1,
                    face_model_ran: false,
                },
                start,
            )),
            depth: None,
            measurement_pending: true,
        };

        let events = scan(&mut state, &image, &timeline);
        assert_eq!(
            events.len(),
            2,
            "null engines must not emit Track/Zone/EntityBoxes/Fsm/Health: {events:?}"
        );
        assert!(
            matches!(events[0], SceneEvent::Occupancy { .. }),
            "first event must be Occupancy, got {:?}",
            events[0]
        );
        assert!(
            matches!(events[1], SceneEvent::Presence { .. }),
            "second event must be Presence, got {:?}",
            events[1]
        );
    }

    /// The clock of one control loop must never tick another loop's state.
    /// Cheap to check and impossible to hit at N=1, but this is the invariant a
    /// multi-stream host (mana-os) would otherwise violate silently.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "different control loop")]
    fn scanning_with_another_loops_timeline_panics() {
        let start = Instant::now(); // cfg(test)
        let mut state = control_state(start, 10_000);
        state.loop_id = LoopId::new("cam-a");
        let timeline = ScanTimeline::new(LoopId::new("cam-b"), start, 200);

        let _ = scan(&mut state, &ProcessImage::empty(), &timeline);
    }

    #[test]
    fn scan_marks_signal_invalid_when_evidence_is_stale() {
        let start = Instant::now(); // cfg(test)
        let data_stale_ms = 500;
        let mut state = control_state(start, data_stale_ms);
        let image = ProcessImage {
            observations: Some(AgedEvidence::new(
                SceneSample {
                    observations: vec![person()],
                    signal_valid: true,
                    raw_person_count: 1,
                    frame_number: 1,
                    face_model_ran: false,
                },
                start,
            )),
            depth: None,
            measurement_pending: true,
        };

        // Age beyond data_stale_ms while keeping the aged evidence frozen.
        // One tick of a timeline whose period exceeds the staleness budget.
        let mut timeline = ScanTimeline::new(LoopId::default_loop(), start, data_stale_ms + 1);
        timeline.advance();
        let events = scan(&mut state, &image, &timeline);

        let presence_invalid = events.iter().any(|e| {
            matches!(
                e,
                SceneEvent::Presence {
                    signal: SignalValidity::Invalid,
                    ..
                }
            )
        });
        let occupancy_invalid = events.iter().any(|e| {
            matches!(
                e,
                SceneEvent::Occupancy {
                    signal: SignalValidity::Invalid,
                    ..
                }
            )
        });
        assert!(
            presence_invalid,
            "stale evidence must mark Presence Invalid"
        );
        assert!(
            occupancy_invalid,
            "stale evidence must mark Occupancy Invalid"
        );
    }
}
