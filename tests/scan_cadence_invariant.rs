use std::time::Instant;

use mana_lite::config::{OccupancyPolicy, PresencePoiPolicy};
use mana_lite::detection::{ConsolidatedObservation, DetectionEvidence};
use mana_lite::occupancy::{OccupancyEvidence, OccupancyStateMachine, RoomCardinality};
use mana_lite::presence::PresenceFilter;
use mana_lite::scan::ScanTimeline;

fn person() -> ConsolidatedObservation {
    ConsolidatedObservation {
        class: "person".into(),
        confidence: 0.9,
        bbox: [0.0, 0.0, 100.0, 100.0],
        primary_model: "synthetic".into(),
        evidence: vec![DetectionEvidence {
            model: "synthetic".into(),
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 100.0, 100.0],
            mask: None,
        }],
        components: Vec::new(),
    }
}

fn single_at(start: Instant, period_ms: u64) -> Instant {
    let mut timeline = ScanTimeline::new(start, period_ms);
    let mut presence = PresenceFilter::new(
        true,
        "person",
        PresencePoiPolicy {
            on_ms: 0,
            off_ms: 500,
        },
    );
    let mut occupancy = OccupancyStateMachine::new(OccupancyPolicy {
        single_confirm_ms: 400,
        empty_confirm_ms: 500,
        multiple_confirm_ms: 300,
        multiple_exit_ms: 300,
        require_confirmed_tracks: false,
    });
    let observations = [person()];

    loop {
        let now = timeline.now();
        let (_, presence_update) = presence.update_at(&observations, true, now);
        let update = occupancy.update_at(
            OccupancyEvidence {
                signal_valid: true,
                raw_person_count: 1,
                poi_present: matches!(
                    presence_update.state,
                    mana_lite::presence::PresenceState::Present
                ),
                confirmed_person_count: 0,
            },
            now,
        );
        if update.state == RoomCardinality::Single {
            return now;
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
