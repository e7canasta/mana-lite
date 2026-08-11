use std::time::Instant;

use mana_control::config::{OccupancyPolicy, PresencePoiPolicy};
use mana_control::domain::LoopId;
use mana_lite::health::Health;
use mana_lite::occupancy::{OccupancyStateMachine, RoomCardinality};
use mana_lite::presence::PresenceFilter;
use mana_lite::scan::{
    AgedEvidence, ClinicalSample, ControlPolicy, ControlState, ProcessImage, ScanTimeline,
    SceneEvent, SceneObservation, scan,
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

fn single_at(start: Instant, period_ms: u64) -> Instant {
    let mut timeline = ScanTimeline::new(LoopId::default_loop(), start, period_ms);
    let mut state = ControlState {
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
            single_confirm_ms: 400,
            empty_confirm_ms: 500,
            multiple_confirm_ms: 300,
            multiple_exit_ms: 300,
            require_confirmed_tracks: false,
        }),
        zone_engine: None,
        fsm_engine: None,
        health: Health::new_at(10_000, 5_000, start),
        signal_snapshot: Default::default(),
        last_scan_at: start,
        scan_seq: 0,
        policy: ControlPolicy {
            person_class: "person".into(),
            presence_enabled: true,
            data_stale_ms: 10_000,
            scan_period_ms: period_ms,
            face_dwell_roi: None,
            person_detection_roi: None,
            face_edge_margin_px: 0,
        },
    };
    let observations = [person()];
    let process_image = ProcessImage {
        observations: Some(AgedEvidence::new(
            ClinicalSample {
                observations: observations.to_vec(),
                signal_valid: true,
                raw_person_count: 1,
                frame_number: 1,
                face_model_ran: false,
            },
            start,
        )),
        depth: None,
        measurement_pending: false,
    };

    loop {
        let now = timeline.now();
        for event in scan(&mut state, &process_image, &timeline) {
            if let SceneEvent::Presence { state: room, .. } = event
                && room == RoomCardinality::Single
            {
                return now.as_instant();
            }
        }
        timeline.advance();
    }
}

#[test]
fn scan_period_only_bounds_confirmation_quantization() {
    let start = Instant::now();
    let fast = single_at(start, 200);
    let slow = single_at(start, 2_000);

    assert!(fast.duration_since(start).as_millis() as u64 >= 400);
    assert!(slow.duration_since(start).as_millis() as u64 >= 400);
    let difference_ms = slow.duration_since(fast).as_millis() as u64;
    assert!(
        difference_ms <= 2_000,
        "confirmation differs only by the slower scan period: {difference_ms}ms"
    );
}
