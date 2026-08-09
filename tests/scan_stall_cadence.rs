//! Cadence assertion for control-loop stalls (Fase 3).
//!
//! Expectation, not characterization: during a 3 s stall with a 200 ms period
//! the control loop emits fifteen consecutive `scan_seq` values, ages grow
//! monotonically, and the FSM enters `blind` on the first scan whose
//! `observations_age_ms` crosses `data_stale_ms`.

use std::collections::HashMap;
use std::time::Instant;

use mana_control::DepthRuleSnapshot;
use mana_lite::config::{
    FsmCatalog, FsmGuard, FsmRoles, FsmRoot, FsmState, FsmTransition, ZoneCatalog,
};
use mana_lite::fsm::{FsmEngine, FsmProgram};
use mana_lite::health::Health;
use mana_lite::scan::{AgedEvidence, ControlStamp, ProcessImage, ScanTimeline, SceneSample};
use mana_lite::zones::ZoneEngine;

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
    let mut timeline = ScanTimeline::new(start, PERIOD_MS);
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

    let mut health = Health::new_at(DATA_STALE_MS, DATA_STALE_MS / 2, start);
    let mut engine = FsmEngine::from_program_at(
        FsmProgram::compile_lenient(&stall_catalog(), &ZoneCatalog::default()).unwrap(),
        start,
    );
    let zones = ZoneEngine::from_catalog(&ZoneCatalog {
        zones: HashMap::new(),
        face_dwell: None,
    });

    let mut stamps = Vec::new();
    let mut blind_at: Option<u64> = None;

    for expected_seq in 1..=EXPECTED_SCANS {
        let now = if expected_seq == 1 {
            timeline.now()
        } else {
            timeline.advance()
        };
        let now_instant = now.as_instant();
        let observations_age_ms = process_image.observations_age_ms(now_instant);
        let depth_age_ms = process_image.depth_age_ms(now_instant);
        let evidence_frame_id = process_image
            .observations
            .as_ref()
            .map(|aged| aged.value.frame_number)
            .unwrap_or(0);

        let stamp = ControlStamp {
            scan_seq: expected_seq,
            evidence_frame_id,
            observations_age_ms,
            depth_age_ms,
        };
        stamps.push(stamp);

        let _ = health.evaluate_at(now_instant);
        if let Some(tr) = engine.evaluate(&[], &zones, &health, &DepthRuleSnapshot::default()) {
            if tr.to == "blind" && blind_at.is_none() {
                blind_at = Some(expected_seq);
            }
        }
    }

    assert_eq!(stamps.len() as u64, EXPECTED_SCANS);
    for (idx, stamp) in stamps.iter().enumerate() {
        assert_eq!(stamp.scan_seq, (idx as u64) + 1, "scan_seq must be contiguous");
        assert_eq!(stamp.evidence_frame_id, 42);
    }
    for window in stamps.windows(2) {
        assert!(
            window[1].observations_age_ms > window[0].observations_age_ms,
            "observations_age_ms must grow during a stall"
        );
    }

    let first_stale_seq = stamps
        .iter()
        .find(|s| s.observations_age_ms > DATA_STALE_MS)
        .map(|s| s.scan_seq)
        .expect("stall must cross data_stale_ms");
    assert_eq!(
        blind_at,
        Some(first_stale_seq),
        "FSM must enter blind on the first scan past data_stale_ms"
    );
    assert_eq!(engine.current_state(), "blind");
    assert!(stamps.last().unwrap().observations_age_ms >= STALL_MS - PERIOD_MS);
}

#[test]
fn scan_instant_is_injectable_via_timeline() {
    let origin = Instant::now();
    let mut timeline = ScanTimeline::new(origin, 200);
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
