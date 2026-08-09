use std::time::{Duration, Instant};

use mana_control::config::{OccupancyPolicy, PresencePoiPolicy};
use mana_lite::detection::Detection;
use mana_lite::detection::{DetectionConsolidator, DetectionRole, ModelDetections};
use mana_lite::logger::{Event, LogSink, RecordingSink, render_events_fixed_ts};
use mana_lite::occupancy::{OccupancyEvidence, OccupancyStateMachine};
use mana_lite::presence::PresenceFilter;
use mana_lite::scan::{ControlStamp, SceneObservation};

fn to_scene(observations: &[mana_lite::detection::ConsolidatedObservation]) -> Vec<SceneObservation> {
    observations
        .iter()
        .map(|observation| SceneObservation {
            class: observation.class.clone(),
            bbox: observation.bbox,
            confidence: observation.confidence,
            source_models: observation
                .evidence
                .iter()
                .map(|e| e.model.clone())
                .collect(),
            face: observation
                .components
                .iter()
                .find(|c| c.class == "face")
                .map(|face| mana_control::FaceObservation {
                    bbox: face.bbox,
                    confidence: face.confidence,
                }),
        })
        .collect()
}

#[test]
fn synthetic_cycle_matches_golden_jsonl() {
    let person = [Detection {
        class: "person".into(),
        confidence: 0.9,
        bbox: [40.0, 40.0, 200.0, 400.0],
        keypoints: None,
        mask: None,
    }];
    let observations =
        DetectionConsolidator::new(0.70, 0.65, 0.5).consolidate(&[ModelDetections {
            model: "detect-fast",
            role: DetectionRole::Primary,
            detections: &person,
        }]);
    let scene = to_scene(&observations);
    let t0 = Instant::now();
    let mut presence = PresenceFilter::new(
        true,
        "person",
        PresencePoiPolicy {
            on_ms: 100,
            off_ms: 500,
        },
    );
    presence.update_at(&scene, true, t0);
    let (_, presence_update) =
        presence.update_at(&scene, true, t0 + Duration::from_millis(100));
    let mut occupancy = OccupancyStateMachine::new(OccupancyPolicy {
        single_confirm_ms: 200,
        empty_confirm_ms: 500,
        multiple_confirm_ms: 300,
        multiple_exit_ms: 300,
        require_confirmed_tracks: false,
    });
    occupancy.update_at(
        OccupancyEvidence {
            signal_valid: true,
            raw_person_count: 1,
            poi_present: true,
            confirmed_person_count: 0,
        },
        t0,
    );
    let occupancy_update = occupancy.update_at(
        OccupancyEvidence {
            signal_valid: true,
            raw_person_count: 1,
            poi_present: true,
            confirmed_person_count: 0,
        },
        t0 + Duration::from_millis(250),
    );

    let mut sink = RecordingSink::default();
    sink.emit(Event::meta_startup("test", "synthetic"));
    sink.emit(Event::frame_ingest(1, true, 3, 200));
    for observation in &observations {
        sink.emit(Event::consolidated_detection(
            1,
            &observation.class,
            observation.confidence,
            observation.bbox,
            &observation.primary_model,
            vec!["detect-fast".into()],
        ));
    }
    sink.emit(Event::presence(
        ControlStamp {
            scan_seq: 1,
            evidence_frame_id: 1,
            observations_age_ms: 0,
            depth_age_ms: None,
        },
        occupancy_update.state.as_str(),
        presence_update.state.as_str(),
        occupancy_update.second_person.as_str(),
        1,
        0,
        true,
        presence_update.held,
        presence_update.positive_ms,
        presence_update.empty_ms,
        occupancy_update.single_timer_ms,
        occupancy_update.empty_timer_ms,
        occupancy_update.multiple_candidate_timer_ms,
        occupancy_update.multiple_exit_timer_ms,
    ));
    let actual = render_events_fixed_ts(&sink.events, "1970-01-01T00:00:00.000Z");
    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/synthetic_cycle.jsonl"
    );
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(golden_path, &actual).expect("write golden");
    }
    let expected = std::fs::read_to_string(golden_path).expect("read golden");
    assert_eq!(actual, expected);
}
