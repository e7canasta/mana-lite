//! Golden contract for a control cycle that runs through `scan()`.
//!
//! Regenerates with: `UPDATE_GOLDEN=1 cargo test synthetic_cycle_matches_golden_jsonl`
//!
//! Covers presence, zone enter/exit, FSM transition, and health blind/recovery
//! in a single scripted run. Events are produced by `scene_events_to_log`, not
//! by hand-built `Event` values.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use mana_control::config::{
    FsmCatalog, FsmRoles, FsmRoot, FsmState, FsmTransition, OccupancyPolicy, PresencePoiPolicy,
    ZoneCatalog, ZoneSpec,
};
use mana_control::domain::LoopId;
use mana_lite::fsm::{FsmEngine, FsmGuard, FsmProgram};
use mana_lite::health::Health;
use mana_lite::logger::{
    Event, LogSink, RecordingSink, render_events_fixed_ts, scene_events_to_log,
};
use mana_lite::occupancy::OccupancyStateMachine;
use mana_lite::presence::PresenceFilter;
use mana_lite::scan::{
    AgedEvidence, ControlPolicy, ControlState, ProcessImage, ScanTimeline, SceneObservation,
    SceneSample, scan,
};
use mana_lite::track::{Tracker, TrackerConfig};
use mana_lite::zones::ZoneEngine;

const PERIOD_MS: u64 = 200;
const DATA_STALE_MS: u64 = 1_000;
const FIXED_TS: &str = "1970-01-01T00:00:00.000Z";

fn golden_catalog() -> FsmCatalog {
    let mut states = HashMap::new();
    for (name, label) in [
        ("idle", "Room Empty"),
        ("watching", "Person Present"),
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
    ZoneCatalog {
        zones,
        face_dwell: None,
    }
}

fn person_in_bed(frame: u64) -> SceneSample {
    SceneSample {
        observations: vec![SceneObservation {
            class: "person".into(),
            bbox: [150.0, 250.0, 350.0, 600.0],
            confidence: 0.9,
            source_models: vec!["detect-fast".into()],
            face: None,
        }],
        signal_valid: true,
        raw_person_count: 1,
        frame_number: frame,
        face_model_ran: false,
    }
}

fn empty_room(frame: u64) -> SceneSample {
    SceneSample {
        observations: Vec::new(),
        signal_valid: true,
        raw_person_count: 0,
        frame_number: frame,
        face_model_ran: false,
    }
}

fn control_state(start: Instant) -> ControlState {
    let zones = zone_catalog();
    let program = FsmProgram::compile_lenient(&golden_catalog(), &zones).expect("compile");
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
            multiple_confirm_ms: 300,
            multiple_exit_ms: 300,
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
            face_dwell_roi: None,
            person_detection_roi: None,
            face_edge_margin_px: 0,
        },
    }
}

fn refresh(image: &mut ProcessImage, sample: SceneSample, at: Instant) {
    image.observations = Some(AgedEvidence::new(sample, at));
    image.measurement_pending = true;
}

#[test]
fn synthetic_cycle_matches_golden_jsonl() {
    let start = Instant::now();
    let mut timeline = ScanTimeline::new(LoopId::default_loop(), start, PERIOD_MS);
    let mut state = control_state(start);
    let mut image = ProcessImage::empty();
    let mut sink = RecordingSink::default();

    sink.emit(Event::meta_startup("test", "synthetic"));

    // Phase 1 — person enters bed: presence, zone occupied, FSM → watching.
    for frame in 1..=4u64 {
        let now = if frame == 1 {
            timeline.now()
        } else {
            timeline.advance()
        };
        refresh(&mut image, person_in_bed(frame), now.as_instant());
        let events = scan(&mut state, &image, &timeline);
        image.measurement_pending = false;
        for event in scene_events_to_log(&events) {
            sink.emit(event);
        }
    }

    // Phase 2 — person leaves: zone vacated after hysteresis (0 ms), FSM → idle.
    for frame in 5..=8u64 {
        let now = timeline.advance();
        refresh(&mut image, empty_room(frame), now.as_instant());
        let events = scan(&mut state, &image, &timeline);
        image.measurement_pending = false;
        for event in scene_events_to_log(&events) {
            sink.emit(event);
        }
    }

    // Phase 3 — stall: freeze evidence age; health stale → blind; FSM → blind.
    let stall_observed_at = start + Duration::from_millis(8 * PERIOD_MS);
    image.observations = Some(AgedEvidence::new(empty_room(8), stall_observed_at));
    image.measurement_pending = false;
    for _ in 0..8 {
        timeline.advance();
        let events = scan(&mut state, &image, &timeline);
        for event in scene_events_to_log(&events) {
            sink.emit(event);
        }
    }

    // Phase 4 — recovery: touch_at (as App does on a fresh keyframe), then scan.
    let recover_at = timeline.advance();
    let recovered = state.health.touch_at(recover_at.as_instant());
    assert!(recovered, "leaving blind must report recovery via touch_at");
    sink.emit(Event::health_heartbeat(0, "ingest", 0));
    refresh(&mut image, empty_room(9), recover_at.as_instant());
    let events = scan(&mut state, &image, &timeline);
    for event in scene_events_to_log(&events) {
        sink.emit(event);
    }

    let actual = render_events_fixed_ts(&sink.events, FIXED_TS);
    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/synthetic_cycle.jsonl"
    );
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(golden_path, &actual).expect("write golden");
    }
    let expected = std::fs::read_to_string(golden_path).expect("read golden");
    assert_eq!(actual, expected);

    // Contract assertions independent of golden content: areas the sprint demands.
    let jsonl = actual.as_str();
    assert!(
        jsonl.contains(r#""type":"presence""#),
        "presence events required"
    );
    assert!(jsonl.contains(r#""type":"zone""#), "zone events required");
    assert!(jsonl.contains(r#""type":"fsm""#), "fsm transition required");
    assert!(
        jsonl.contains(r#""type":"health""#) && jsonl.contains(r#""event":"blind""#),
        "health blind required"
    );
    assert!(
        jsonl.contains(r#""event":"heartbeat""#),
        "health recovery heartbeat required"
    );
}
