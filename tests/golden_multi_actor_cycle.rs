//! Golden contract for the `scan()` branches the single-actor cycle never takes.
//!
//! Regenerates with: `UPDATE_GOLDEN=1 cargo test multi_actor_cycle_matches_golden_jsonl`
//!
//! [`golden_synthetic_cycle`] pins one person, one zone, no face. That leaves
//! four branches of `scan()` with no integration coverage at all, all of them
//! load-bearing for Sprint 3's split into eight steps:
//!
//! - `PresenceState::Ambiguous` — the `tracking = &[][..]` short circuit that
//!   suppresses tracker input entirely.
//! - `RoomCardinality::Multiple` / `second_person`.
//! - the face arms of `update_context` (`face_present`, `face_confidence`,
//!   `face_in_dwell`, `at_edge`) — `update_context` is the one call Sprint 3
//!   moves, so it must be observable.
//! - two zones changing in the same tick, whose event order comes from the
//!   `BTreeMap` in `ZoneEngine` and is otherwise unverified.
//!
//! The components behind these are unit-tested in `presence.rs` / `occupancy.rs`.
//! What was missing is their *composition* through `scan()` — exactly what the
//! refactor rewrites.

use std::collections::HashMap;
use std::time::Instant;

use mana_control::FaceObservation;
use mana_control::config::{
    FsmCatalog, FsmRoles, FsmRoot, FsmState, FsmTransition, OccupancyPolicy, PresencePoiPolicy,
    ZoneCatalog, ZoneSpec,
};
use mana_control::domain::{LoopId, SignalTag};
use mana_control::signals::{Ratio, SceneSignalsSnapshot, SignalOp, SignalValue};
use mana_lite::fsm::{FsmEngine, FsmGuard, FsmProgram};
use mana_lite::health::Health;
use mana_lite::logger::{
    Event, LogSink, RecordingSink, render_events_fixed_ts, scene_events_to_log,
};
use mana_lite::occupancy::OccupancyStateMachine;
use mana_lite::presence::PresenceFilter;
use mana_lite::scan::{
    AgedEvidence, ControlPolicy, ControlState, ProcessImage, ScanInstant, ScanTimeline, SceneEvent,
    SceneObservation, SceneSample, scan,
};
use mana_lite::track::{Tracker, TrackerConfig};
use mana_lite::zones::{ZoneEngine, ZoneEvent};

const PERIOD_MS: u64 = 200;
const DATA_STALE_MS: u64 = 2_000;
const FIXED_TS: &str = "1970-01-01T00:00:00.000Z";

/// Nested zones: the bed sits inside the room, so a single track occupies both
/// on the same tick. That is deliberate — trying to occupy two disjoint zones
/// needs two tracks, and the tracker confirms them on different ticks, which
/// staggers the events and hides the ordering this fixture exists to pin.
///
/// Names matter: `ZoneEngine` keys a `BTreeMap`, so "bed" must always be
/// emitted before "room" within a tick.
fn zone_catalog() -> ZoneCatalog {
    let mut zones = HashMap::new();
    zones.insert(
        "bed".into(),
        ZoneSpec {
            x1: 100,
            y1: 200,
            x2: 500,
            y2: 800,
            label: Some("Bed A".into()),
            hysteresis_ms: 0,
        },
    );
    zones.insert(
        "room".into(),
        ZoneSpec {
            x1: 0,
            y1: 0,
            x2: 1000,
            y2: 900,
            label: Some("Room 12".into()),
            hysteresis_ms: 0,
        },
    );
    ZoneCatalog {
        zones,
        // Required by the `face_in_dwell` guard below.
        face_dwell: Some(ZoneSpec {
            x1: 150,
            y1: 250,
            x2: 450,
            y2: 500,
            label: Some("Face dwell".into()),
            hysteresis_ms: 0,
        }),
    }
}

fn catalog() -> FsmCatalog {
    let mut states = HashMap::new();
    for (name, label) in [
        ("idle", "Room Empty"),
        ("watching", "Person Present"),
        ("engaged", "Face Engaged"),
        ("crowded", "Multiple People"),
        ("blind", "BLIND: No Camera Signal"),
    ] {
        states.insert(
            name.into(),
            FsmState {
                label: Some(label.into()),
                models: vec!["detect-fast".into()],
                dwell_min_ms: None,
                face_inside: false,
                face_inside_maybe: false,
            },
        );
    }
    FsmCatalog {
        fsm: FsmRoot {
            initial: "idle".into(),
            states,
            roles: FsmRoles {
                safe: "blind".into(),
                reset: "idle".into(),
            },
            transitions: vec![
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
                FsmTransition {
                    from: "idle".into(),
                    to: "watching".into(),
                    guards: vec![FsmGuard::ZoneOccupied {
                        zone: "bed".into(),
                        min_confidence: 0.5,
                        min_duration_ms: Some(0),
                    }],
                    dwell: None,
                },
                // Observes `face_in_dwell`, set by update_context.
                FsmTransition {
                    from: "watching".into(),
                    to: "engaged".into(),
                    guards: vec![FsmGuard::Signal {
                        tag: "cara.en_dwell".into(),
                        op: "==".into(),
                        value: mana_lite::fsm::SignalLiteral::Bool(true),
                    }],
                    dwell: None,
                },
                FsmTransition {
                    from: "engaged".into(),
                    to: "watching".into(),
                    guards: vec![FsmGuard::Signal {
                        tag: "cara.presente".into(),
                        op: "==".into(),
                        value: mana_lite::fsm::SignalLiteral::Bool(false),
                    }],
                    dwell: None,
                },
                // Observes RoomCardinality::Multiple. Reachable from both the
                // empty and the single-occupant state: a crowd can form either
                // way, and the fixture must not depend on which one we are in.
                FsmTransition {
                    from: "watching".into(),
                    to: "crowded".into(),
                    guards: vec![FsmGuard::Signal {
                        tag: "ocupacion.cardinalidad".into(),
                        op: "==".into(),
                        value: mana_lite::fsm::SignalLiteral::Text("multiple".into()),
                    }],
                    dwell: None,
                },
                FsmTransition {
                    from: "idle".into(),
                    to: "crowded".into(),
                    guards: vec![FsmGuard::Signal {
                        tag: "ocupacion.cardinalidad".into(),
                        op: "==".into(),
                        value: mana_lite::fsm::SignalLiteral::Text("multiple".into()),
                    }],
                    dwell: None,
                },
                FsmTransition {
                    from: "crowded".into(),
                    to: "idle".into(),
                    guards: vec![FsmGuard::Signal {
                        tag: "ocupacion.cardinalidad".into(),
                        op: "==".into(),
                        value: mana_lite::fsm::SignalLiteral::Text("empty".into()),
                    }],
                    dwell: None,
                },
                FsmTransition {
                    from: "watching".into(),
                    to: "idle".into(),
                    guards: vec![FsmGuard::ZoneVacated {
                        zone: "bed".into(),
                        min_confidence: None,
                        min_duration_ms: Some(0),
                    }],
                    dwell: None,
                },
            ],
        },
    }
}

fn person(bbox: [f32; 4], face: Option<FaceObservation>) -> SceneObservation {
    SceneObservation {
        class: "person".into(),
        bbox,
        confidence: 0.9,
        source_models: vec!["detect-fast".into()],
        face,
    }
}

fn sample(observations: Vec<SceneObservation>, frame: u64) -> SceneSample {
    let raw_person_count = observations
        .iter()
        .filter(|o| o.class.as_str() == "person")
        .count();
    SceneSample {
        face_model_ran: observations.iter().any(|o| o.face.is_some()),
        observations,
        signal_valid: true,
        raw_person_count,
        frame_number: frame,
        face_pose_validation: None,
    }
}

fn control_state(start: Instant) -> ControlState {
    let zones = zone_catalog();
    let program = FsmProgram::compile_lenient(&catalog(), &zones).expect("compile");
    ControlState {
        loop_id: LoopId::default_loop(),
        tracker: Some(Tracker::with_config(TrackerConfig {
            min_hits: 1,
            ..TrackerConfig::default()
        })),
        presence: PresenceFilter::new(
            true,
            "person",
            PresencePoiPolicy {
                on_ms: 0,
                off_ms: 200,
            },
        ),
        occupancy: OccupancyStateMachine::new(OccupancyPolicy {
            single_confirm_ms: 0,
            empty_confirm_ms: 200,
            multiple_confirm_ms: 0,
            multiple_exit_ms: 0,
            require_confirmed_tracks: false,
        }),
        zone_engine: Some(ZoneEngine::from_catalog(&zones)),
        fsm_engine: Some(FsmEngine::from_program_at(program, start)),
        health: Health::new_at(DATA_STALE_MS, DATA_STALE_MS / 2, start),
        signal_snapshot: Default::default(),
        last_scan_at: start,
        scan_seq: 0,
        policy: ControlPolicy {
            person_class: "person".into(),
            presence_enabled: true,
            data_stale_ms: DATA_STALE_MS,
            scan_period_ms: PERIOD_MS,
            // Both ROIs set, so face_in_dwell and at_edge evaluate to real
            // values instead of the None/false they take in the single-actor
            // golden.
            face_dwell_roi: Some([150, 250, 450, 500]),
            person_detection_roi: Some([0, 0, 1000, 900]),
            face_edge_margin_px: 20,
        },
    }
}

fn refresh(image: &mut ProcessImage, s: SceneSample, at: Instant) {
    image.observations = Some(AgedEvidence::new(s, at));
    image.measurement_pending = true;
}

/// Drives one tick and returns the raw event batch alongside the log records,
/// so the test can assert on ordering that the JSONL flattens away.
fn tick(
    state: &mut ControlState,
    image: &mut ProcessImage,
    timeline: &mut ScanTimeline,
    sink: &mut RecordingSink,
    observations: Vec<SceneObservation>,
    frame: u64,
    first: bool,
) -> (Vec<SceneEvent>, ScanInstant) {
    let now = if first {
        timeline.now()
    } else {
        timeline.advance()
    };
    let sample = sample(observations, frame);
    refresh(image, sample.clone(), now.as_instant());
    let events = scan(state, image, timeline);
    let signal_events: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            SceneEvent::SceneSignals { stamp, snapshot } => Some((*stamp, snapshot)),
            _ => None,
        })
        .collect();
    assert_eq!(signal_events.len(), 1, "one SceneSignals event per scan");
    let (stamp, snapshot) = signal_events[0];
    assert_eq!(stamp.scan_seq, state.scan_seq);
    assert_eq!(stamp.evidence_frame_id, sample.frame_number);
    assert_eq!(stamp.observations_age_ms, 0);
    assert_eq!(snapshot.catalog_version(), 1);
    assert_eq!(snapshot.len(), 11);
    assert_snapshots_equal(snapshot, &state.signal_snapshot);
    assert_base_signal_parity(state, &sample);
    image.measurement_pending = false;
    for event in scene_events_to_log(&events) {
        sink.emit(event);
    }
    (events, now)
}

fn signal<'a>(snapshot: &'a SceneSignalsSnapshot, name: &str) -> &'a SignalValue {
    snapshot
        .get(&SignalTag::new(name))
        .unwrap_or_else(|| panic!("expected signal {name}"))
}

fn assert_bool_signal(snapshot: &SceneSignalsSnapshot, name: &str, expected: bool) {
    assert!(matches!(signal(snapshot, name), SignalValue::Bool(value) if *value == expected));
}

fn assert_ratio_signal(snapshot: &SceneSignalsSnapshot, name: &str, expected: f32) {
    let expected = SignalValue::Ratio(Ratio::new(expected).expect("fixture ratio"));
    let actual = signal(snapshot, name);
    assert!(actual.compare(SignalOp::Gte, &expected).unwrap());
    assert!(expected.compare(SignalOp::Gte, actual).unwrap());
}

fn assert_base_signal_parity(state: &ControlState, sample: &SceneSample) {
    let snapshot = &state.signal_snapshot;
    assert_eq!(snapshot.len(), 11);
    assert_bool_signal(snapshot, "persona.presente", sample.raw_person_count > 0);
    assert!(matches!(
        signal(snapshot, "persona.cantidad"),
        SignalValue::Count(value) if *value == sample.raw_person_count as u64
    ));
    let person = sample
        .observations
        .iter()
        .filter(|observation| observation.class.as_str() == "person")
        .max_by(|a, b| a.confidence.total_cmp(&b.confidence));
    let face = person.and_then(|observation| observation.face);
    assert_bool_signal(snapshot, "cara.presente", face.is_some());
    let at_edge = person.is_some_and(|observation| {
        state.policy.person_detection_roi.is_some_and(|roi| {
            let margin = state.policy.face_edge_margin_px as f32;
            observation.bbox[0] <= roi[0] as f32 + margin
                || observation.bbox[1] <= roi[1] as f32 + margin
                || observation.bbox[2] >= roi[2] as f32 - margin
                || observation.bbox[3] >= roi[3] as f32 - margin
        })
    });

    match face.map(|value| value.confidence) {
        Some(confidence) => assert_ratio_signal(snapshot, "cara.confianza", confidence),
        None => assert!(snapshot.is_absent(&SignalTag::new("cara.confianza"))),
    }
    match state
        .policy
        .face_dwell_roi
        .map(|roi| face.is_some_and(|value| intersects_for_test(value.bbox, roi)))
    {
        Some(in_dwell) => assert_bool_signal(snapshot, "cara.en_dwell", in_dwell),
        None => assert!(snapshot.is_absent(&SignalTag::new("cara.en_dwell"))),
    }
    assert_bool_signal(snapshot, "cara.en_borde", at_edge);
    assert_bool_signal(snapshot, "cara.modelo_corrio", sample.face_model_ran);
    assert!(matches!(
        signal(snapshot, "ocupacion.cardinalidad"),
        SignalValue::Label(_)
    ));
    assert!(matches!(
        signal(snapshot, "cara.estuvo_dentro"),
        SignalValue::Bool(_)
    ));
}

fn assert_snapshots_equal(left: &SceneSignalsSnapshot, right: &SceneSignalsSnapshot) {
    let left_entries: Vec<_> = left.iter().collect();
    let right_entries: Vec<_> = right.iter().collect();
    assert_eq!(left_entries.len(), right_entries.len());
    for ((left_tag, left_value), (right_tag, right_value)) in
        left_entries.into_iter().zip(right_entries)
    {
        assert_eq!(left_tag, right_tag);
        match (left_value, right_value) {
            (None, None) => {}
            (Some(SignalValue::Bool(left)), Some(SignalValue::Bool(right))) => {
                assert_eq!(left, right)
            }
            (Some(SignalValue::Count(left)), Some(SignalValue::Count(right))) => {
                assert_eq!(left, right)
            }
            (Some(SignalValue::Ratio(left)), Some(SignalValue::Ratio(right))) => {
                assert!((left.get() - right.get()).abs() < f32::EPSILON)
            }
            (Some(SignalValue::Label(left)), Some(SignalValue::Label(right))) => {
                assert_eq!(left, right)
            }
            (left, right) => panic!("signal value mismatch: {left:?} vs {right:?}"),
        }
    }
}

fn intersects_for_test(b: [f32; 4], r: [u32; 4]) -> bool {
    b[0] < r[2] as f32 && b[2] > r[0] as f32 && b[1] < r[3] as f32 && b[3] > r[1] as f32
}

#[test]
fn multi_actor_cycle_matches_golden_jsonl() {
    let start = Instant::now();
    let mut timeline = ScanTimeline::new(LoopId::default_loop(), start, PERIOD_MS);
    let mut state = control_state(start);
    let mut image = ProcessImage::empty();
    let mut sink = RecordingSink::default();

    sink.emit(Event::meta_startup("test", "multi-actor"));

    let face = FaceObservation {
        bbox: [200.0, 300.0, 280.0, 400.0],
        confidence: 0.8,
    };
    let in_bed = [150.0, 250.0, 350.0, 600.0];

    // Phase 1 — one person with a face inside the dwell ROI: exercises the face
    // arms of update_context and drives idle → watching → engaged.
    let mut zone_batches: Vec<Vec<String>> = Vec::new();
    let mut transcript: Vec<String> = Vec::new();
    for frame in 1..=3u64 {
        let (events, _) = tick(
            &mut state,
            &mut image,
            &mut timeline,
            &mut sink,
            vec![person(in_bed, Some(face))],
            frame,
            frame == 1,
        );
        zone_batches.push(zone_names(&events));
        transcript.push(event_kinds(&events));
    }

    // Phase 2 — the room empties: bed and room vacate on the same tick.
    for frame in 4..=6u64 {
        let (events, _) = tick(
            &mut state,
            &mut image,
            &mut timeline,
            &mut sink,
            Vec::new(),
            frame,
            false,
        );
        zone_batches.push(zone_names(&events));
        transcript.push(event_kinds(&events));
    }

    // Phase 3 — two people: presence goes Ambiguous (tracker input suppressed)
    // and occupancy reaches Multiple.
    let mut saw_ambiguous = false;
    let mut saw_multiple = false;
    for frame in 7..=10u64 {
        let (events, _) = tick(
            &mut state,
            &mut image,
            &mut timeline,
            &mut sink,
            vec![
                person(in_bed, None),
                person([600.0, 250.0, 800.0, 600.0], None),
            ],
            frame,
            false,
        );
        zone_batches.push(zone_names(&events));
        transcript.push(event_kinds(&events));
        for event in &events {
            if let SceneEvent::Presence { presence, .. } = event
                && presence.as_str() == "ambiguous"
            {
                saw_ambiguous = true;
            }
            if let SceneEvent::Occupancy { state: card, .. } = event
                && card.as_str() == "multiple"
            {
                saw_multiple = true;
            }
        }
    }

    let actual = render_events_fixed_ts(&sink.events, FIXED_TS);
    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/multi_actor_cycle.jsonl"
    );
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(golden_path, &actual).expect("write golden");
    }
    let expected = std::fs::read_to_string(golden_path).expect("read golden");
    assert_eq!(actual, expected);

    // The raw event sequence, including what the logger drops. This is the
    // fixture that actually pins Sprint 3's stated invariant.
    let actual_events = format!("{}\n", transcript.join("\n"));
    let events_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/multi_actor_cycle.events.txt"
    );
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(events_path, &actual_events).expect("write event golden");
    }
    let expected_events = std::fs::read_to_string(events_path).expect("read event golden");
    assert_eq!(
        actual_events, expected_events,
        "scan() must return the same SceneEvent vector, in the same order"
    );

    // Contract assertions — these are what make the golden worth diffing.
    // Without them a regenerated fixture could silently stop covering a branch.
    assert!(
        saw_ambiguous,
        "two people must drive presence to Ambiguous; without it the \
         `tracking = &[][..]` short circuit stays unexercised"
    );
    assert!(saw_multiple, "two people must drive occupancy to Multiple");

    let multi_zone_ticks: Vec<&Vec<String>> = zone_batches.iter().filter(|b| b.len() > 1).collect();
    assert!(
        !multi_zone_ticks.is_empty(),
        "no tick emitted two zone events; multi-zone ordering is the whole \
         point of this fixture"
    );
    for batch in multi_zone_ticks {
        let mut sorted = batch.clone();
        sorted.sort();
        assert_eq!(
            batch, &sorted,
            "zone events must come out in zone-id order (ZoneEngine keys a \
             BTreeMap); got {batch:?}"
        );
    }

    let jsonl = actual.as_str();
    assert!(
        jsonl.contains(r#""state":"engaged""#) || jsonl.contains(r#""to":"engaged""#),
        "face_in_dwell must reach the FSM"
    );
    assert!(
        jsonl.contains(r#""to":"crowded""#),
        "cardinality=multiple must reach the FSM"
    );
}

/// Renders the raw `SceneEvent` sequence of one tick.
///
/// This exists because the JSONL golden **cannot** see the whole vector:
/// `scene_events_to_log` drops `Occupancy` and `FsmState` entirely
/// ([src/logger/event.rs](src/logger/event.rs)). Verified by experiment —
/// moving the `Occupancy` push past `Presence` in `scan()` leaves both JSONL
/// goldens green. So a fixture over the JSONL alone does not pin "same vector,
/// same order"; it pins "same loggable subset". This transcript closes that gap.
fn event_kinds(events: &[SceneEvent]) -> String {
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| match event {
            SceneEvent::Track { .. } => "track",
            SceneEvent::Presence { .. } => "presence",
            SceneEvent::SceneSignals { .. } => "scene_signals",
            SceneEvent::Occupancy { .. } => "occupancy",
            SceneEvent::EntityBoxes(_) => "entity_boxes",
            SceneEvent::Zone { .. } => "zone",
            SceneEvent::FsmTransition(_) => "fsm_transition",
            SceneEvent::FsmState(_) => "fsm_state",
            SceneEvent::Health(_) => "health",
        })
        .collect();
    kinds.join(",")
}

fn zone_names(events: &[SceneEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            SceneEvent::Zone { event, .. } => Some(match event {
                ZoneEvent::Occupied { zone, .. } | ZoneEvent::Vacated { zone, .. } => {
                    zone.as_str().to_owned()
                }
            }),
            _ => None,
        })
        .collect()
}
