//! Depth evidence in the process image must reach FSM guards (Sprint 0 residual).
//!
//! Characterizes the control-side contract: when `ProcessImage.depth` carries a
//! triggered `bed-approach` snapshot, `scan()` must emit the matching FSM
//! transition. The App adapter that fills that snapshot is covered separately.

use std::collections::HashMap;
use std::time::Instant;

use mana_control::config::{OccupancyPolicy, PresencePoiPolicy};
use mana_control::domain::LoopId;
use mana_control::{DepthMetric, DepthRuleResult, DepthRuleSnapshot};
use mana_lite::config::{
    FsmCatalog, FsmGuard, FsmRoles, FsmRoot, FsmState, FsmTransition, ZoneCatalog,
};
use mana_lite::fsm::{FsmEngine, FsmProgram};
use mana_lite::health::Health;
use mana_lite::occupancy::OccupancyStateMachine;
use mana_lite::presence::PresenceFilter;
use mana_lite::scan::{
    AgedEvidence, ControlPolicy, ControlState, ProcessImage, ScanTimeline, SceneEvent, SceneSample,
};

fn depth_catalog() -> FsmCatalog {
    let mut states = HashMap::new();
    states.insert(
        "watching".into(),
        FsmState {
            label: None,
            models: vec!["detect-fast".into()],
            dwell_min_ms: None,
            face_inside: false,
            face_inside_maybe: false,
        },
    );
    states.insert(
        "bed_approaching".into(),
        FsmState {
            label: None,
            models: vec!["detect-fast".into()],
            dwell_min_ms: None,
            face_inside: false,
            face_inside_maybe: false,
        },
    );
    FsmCatalog {
        fsm: FsmRoot {
            initial: "watching".into(),
            states,
            roles: FsmRoles {
                safe: "watching".into(),
                reset: "watching".into(),
            },
            transitions: vec![FsmTransition {
                from: "watching".into(),
                to: "bed_approaching".into(),
                guards: vec![FsmGuard::DepthRule {
                    rule: "bed-approach".into(),
                    triggered: true,
                }],
                dwell: None,
            }],
        },
    }
}

fn triggered_snapshot(rule: &str) -> DepthRuleSnapshot {
    DepthRuleSnapshot::from_results(&[DepthRuleResult {
        rule: rule.into(),
        region: [0, 0, 2, 2],
        metric: DepthMetric::Median,
        threshold_m: 1.5,
        value: Some(1.0),
        triggered: true,
        valid_pixels: 4,
        valid_ratio: Some(1.0),
        calibration: None,
    }])
}

#[test]
fn depth_evidence_in_process_image_fires_fsm_guard() {
    const PERIOD_MS: u64 = 200;
    let start = Instant::now();
    let timeline = ScanTimeline::new(LoopId::default_loop(), start, PERIOD_MS);

    let mut image = ProcessImage::empty();
    image.observations = Some(AgedEvidence::new(
        SceneSample {
            observations: Vec::new(),
            signal_valid: true,
            raw_person_count: 0,
            frame_number: 1,
            face_model_ran: false,
        },
        start,
    ));
    image.set_depth(triggered_snapshot("bed-approach"), start);

    let program =
        FsmProgram::compile_lenient(&depth_catalog(), &ZoneCatalog::default()).expect("compile");
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
        fsm_engine: Some(FsmEngine::from_program_at(program, start)),
        health: Health::new_at(10_000, 5_000, start),
        fsm_context: Default::default(),
        signal_snapshot: Default::default(),
        last_scan_at: start,
        scan_seq: 0,
        policy: ControlPolicy {
            person_class: "person".into(),
            presence_enabled: false,
            data_stale_ms: 10_000,
            scan_period_ms: PERIOD_MS,
            face_dwell_roi: None,
            person_detection_roi: None,
            face_edge_margin_px: 0,
        },
    };

    let events = mana_lite::scan::scan(&mut state, &image, &timeline);
    let transitioned = events.iter().any(|event| {
        matches!(
            event,
            SceneEvent::FsmTransition(tr) if tr.to == "bed_approaching"
        )
    });
    assert!(
        transitioned,
        "scan must emit watching→bed_approaching when ProcessImage carries triggered bed-approach"
    );
    assert_eq!(
        state.fsm_engine.as_ref().map(FsmEngine::current_state),
        Some("bed_approaching")
    );
}

#[test]
fn empty_depth_snapshot_does_not_fire_depth_guard() {
    const PERIOD_MS: u64 = 200;
    let start = Instant::now();
    let timeline = ScanTimeline::new(LoopId::default_loop(), start, PERIOD_MS);

    let mut image = ProcessImage::empty();
    image.observations = Some(AgedEvidence::new(
        SceneSample {
            observations: Vec::new(),
            signal_valid: true,
            raw_person_count: 0,
            frame_number: 1,
            face_model_ran: false,
        },
        start,
    ));
    image.reset_depth(start);

    let program =
        FsmProgram::compile_lenient(&depth_catalog(), &ZoneCatalog::default()).expect("compile");
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
        fsm_engine: Some(FsmEngine::from_program_at(program, start)),
        health: Health::new_at(10_000, 5_000, start),
        fsm_context: Default::default(),
        signal_snapshot: Default::default(),
        last_scan_at: start,
        scan_seq: 0,
        policy: ControlPolicy {
            person_class: "person".into(),
            presence_enabled: false,
            data_stale_ms: 10_000,
            scan_period_ms: PERIOD_MS,
            face_dwell_roi: None,
            person_detection_roi: None,
            face_edge_margin_px: 0,
        },
    };

    let events = mana_lite::scan::scan(&mut state, &image, &timeline);
    let transitioned = events.iter().any(|event| {
        matches!(
            event,
            SceneEvent::FsmTransition(tr) if tr.to == "bed_approaching"
        )
    });
    assert!(
        !transitioned,
        "empty depth snapshot must not fire the depth_rule guard"
    );
    assert_eq!(
        state.fsm_engine.as_ref().map(FsmEngine::current_state),
        Some("watching")
    );
}
