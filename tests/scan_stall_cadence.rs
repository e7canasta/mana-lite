//! Cadence assertion for control-loop stalls (Fase 3).
//!
//! Expectation, not characterization: during a 3 s stall with a 200 ms period
//! the control loop emits fifteen consecutive `scan_seq` values via `scan()`,
//! ages grow monotonically, and the FSM enters `blind` on the first scan whose
//! `observations_age_ms` crosses `data_stale_ms` *after* health is already blind.

use std::collections::HashMap;
use std::time::Instant;

use mana_control::config::{
    FsmCatalog, FsmRoles, FsmRoot, FsmState, FsmTransition, OccupancyPolicy, PresencePoiPolicy,
    ZoneCatalog,
};
use mana_control::domain::LoopId;
use mana_lite::fsm::{FsmEngine, FsmGuard, FsmProgram};
use mana_lite::health::Health;
use mana_lite::occupancy::OccupancyStateMachine;
use mana_lite::presence::PresenceFilter;
use mana_lite::scan::{
    AgedEvidence, ControlPolicy, ControlState, ProcessImage, ScanTimeline, SceneEvent, SceneSample,
    scan,
};

fn stall_catalog() -> FsmCatalog {
    let mut states = HashMap::new();
    states.insert(
        "idle".into(),
        FsmState {
            label: None,
            models: vec!["detect-fast".into()],
            dwell_min_ms: None,
            face_inside: false,
            face_inside_maybe: false,
        },
    );
    states.insert(
        "blind".into(),
        FsmState {
            label: None,
            models: vec![],
            dwell_min_ms: None,
            face_inside: false,
            face_inside_maybe: false,
        },
    );
    FsmCatalog {
        fsm: FsmRoot {
            initial: "idle".into(),
            states,
            roles: FsmRoles {
                safe: "blind".into(),
                reset: "idle".into(),
            },
            transitions: vec![FsmTransition {
                from: "*".into(),
                to: "blind".into(),
                guards: vec![FsmGuard::DataStale],
                dwell: None,
            }],
        },
    }
}

#[test]
fn stall_emits_consecutive_scan_seq_and_blind_on_stale() {
    const PERIOD_MS: u64 = 200;
    const STALL_MS: u64 = 3_000;
    const DATA_STALE_MS: u64 = 1_000;
    const EXPECTED_SCANS: u64 = STALL_MS / PERIOD_MS; // 15

    let start = Instant::now();
    let mut timeline = ScanTimeline::new(LoopId::default_loop(), start, PERIOD_MS);
    let mut process_image = ProcessImage::empty();
    process_image.observations = Some(AgedEvidence::new(
        SceneSample {
            observations: Vec::new(),
            signal_valid: true,
            raw_person_count: 0,
            frame_number: 42,
            face_model_ran: false,
        },
        start,
    ));
    process_image.reset_depth(start);
    process_image.measurement_pending = false;

    let mut state = ControlState {
        loop_id: LoopId::default_loop(),
        tracker: None,
        presence: PresenceFilter::new(
            false,
            "person",
            PresencePoiPolicy {
                on_ms: 0,
                off_ms: 0,
            },
        ),
        occupancy: OccupancyStateMachine::new(OccupancyPolicy {
            single_confirm_ms: 0,
            empty_confirm_ms: 0,
            multiple_confirm_ms: 0,
            multiple_exit_ms: 0,
            require_confirmed_tracks: false,
        }),
        zone_engine: None,
        fsm_engine: Some(FsmEngine::from_program_at(
            FsmProgram::compile_lenient(&stall_catalog(), &ZoneCatalog::default()).unwrap(),
            start,
        )),
        health: Health::new_at(DATA_STALE_MS, DATA_STALE_MS / 2, start),
        fsm_context: Default::default(),
        last_scan_at: start,
        scan_seq: 0,
        policy: ControlPolicy {
            person_class: "person".into(),
            presence_enabled: false,
            data_stale_ms: DATA_STALE_MS,
            scan_period_ms: PERIOD_MS,
            face_dwell_roi: None,
            person_detection_roi: None,
            face_edge_margin_px: 0,
        },
    };

    let mut scan_seqs = Vec::new();
    let mut ages = Vec::new();
    let mut blind_at: Option<u64> = None;
    let mut health_blind_at: Option<u64> = None;

    for expected_seq in 1..=EXPECTED_SCANS {
        let now = if expected_seq == 1 {
            timeline.now()
        } else {
            timeline.advance()
        };
        let events = scan(&mut state, &process_image, &timeline);
        scan_seqs.push(state.scan_seq);
        ages.push(process_image.observations_age_ms(now.as_instant()));

        for event in &events {
            match event {
                SceneEvent::Health(mana_lite::health::HealthTransition::Blind { .. })
                    if health_blind_at.is_none() =>
                {
                    health_blind_at = Some(expected_seq);
                }
                SceneEvent::FsmTransition(tr) if tr.to == "blind" && blind_at.is_none() => {
                    blind_at = Some(expected_seq);
                }
                _ => {}
            }
        }
    }

    assert_eq!(scan_seqs.len() as u64, EXPECTED_SCANS);
    for (idx, seq) in scan_seqs.iter().enumerate() {
        assert_eq!(*seq, (idx as u64) + 1, "scan_seq must be contiguous");
    }
    for window in ages.windows(2) {
        assert!(
            window[1] > window[0],
            "observations_age_ms must grow during a stall"
        );
    }

    let first_stale_seq = ages
        .iter()
        .enumerate()
        .find(|(_, age)| **age > DATA_STALE_MS)
        .map(|(idx, _)| (idx as u64) + 1)
        .expect("stall must cross data_stale_ms");
    assert_eq!(
        health_blind_at,
        Some(first_stale_seq),
        "Health must enter blind on the first scan past data_stale_ms"
    );
    // FSM DataStale reads health.is_blind() from the previous tick, so it
    // transitions one scan after Health emits Blind.
    assert_eq!(
        blind_at,
        Some(first_stale_seq + 1),
        "FSM must enter blind on the scan after Health goes blind"
    );
    assert_eq!(
        state.fsm_engine.as_ref().map(FsmEngine::current_state),
        Some("blind")
    );
    assert!(ages.last().unwrap() >= &(STALL_MS - PERIOD_MS));
}

#[test]
fn scan_instant_is_injectable_via_timeline() {
    let origin = Instant::now();
    let mut timeline = ScanTimeline::new(LoopId::default_loop(), origin, 200);
    let t0 = timeline.now();
    assert_eq!(t0.elapsed_ms_since(t0), 0);
    let t1 = timeline.advance();
    assert_eq!(t1.elapsed_ms_since(t0), 200);
    let t15 = {
        for _ in 0..14 {
            timeline.advance();
        }
        timeline.now()
    };
    assert_eq!(t15.elapsed_ms_since(t0), 3_000);
}
