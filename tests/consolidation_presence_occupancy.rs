//! Integration test: consolidation → presence → occupancy without ONNX/RTSP.

use std::time::{Duration, Instant};

use mana_control::config::{OccupancyPolicy, PresencePoiPolicy};
use mana_lite::detection::Detection;
use mana_lite::detection::{DetectionConsolidator, DetectionRole, ModelDetections};
use mana_lite::occupancy::{OccupancyEvidence, OccupancyStateMachine, RoomCardinality};
use mana_lite::presence::{PresenceFilter, PresenceState};
use mana_lite::scan::SceneObservation;

fn detection(class: &str, bbox: [f32; 4], confidence: f32) -> Detection {
    Detection {
        class: class.into(),
        confidence,
        bbox,
        keypoints: None,
        mask: None,
    }
}

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
fn consolidation_presence_occupancy_confirms_single_person() {
    let consolidator = DetectionConsolidator::new(0.70, 0.65, 0.5);
    let person = [detection("person", [40.0, 40.0, 200.0, 400.0], 0.92)];
    let face = [detection("face", [80.0, 60.0, 140.0, 130.0], 0.88)];

    let observations = consolidator.consolidate(&[
        ModelDetections {
            model: "detect-fast",
            role: DetectionRole::Primary,
            detections: &person,
        },
        ModelDetections {
            model: "face-yolo",
            role: DetectionRole::Secondary,
            detections: &face,
        },
    ]);

    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].class, "person");
    assert_eq!(observations[0].components.len(), 1);
    assert_eq!(observations[0].components[0].class, "face");

    let scene = to_scene(&observations);
    let mut presence = PresenceFilter::new(
        true,
        "person",
        PresencePoiPolicy {
            on_ms: 100,
            off_ms: 500,
        },
    );
    let mut occupancy = OccupancyStateMachine::new(OccupancyPolicy {
        single_confirm_ms: 200,
        empty_confirm_ms: 500,
        multiple_confirm_ms: 300,
        multiple_exit_ms: 300,
        require_confirmed_tracks: false,
    });

    let t0 = Instant::now();
    presence.update_at(&scene, true, t0);
    let (effective, presence_update) =
        presence.update_at(&scene, true, t0 + Duration::from_millis(120));
    assert_eq!(presence_update.state, PresenceState::Present);
    assert!(!presence_update.held);
    assert_eq!(effective.len(), 1);

    let raw_person_count = observations
        .iter()
        .filter(|observation| observation.class == "person")
        .count();
    let update = occupancy.update_at(
        OccupancyEvidence {
            signal_valid: true,
            raw_person_count,
            poi_present: true,
            confirmed_person_count: 0,
        },
        t0,
    );
    let confirmed = occupancy.update_at(
        OccupancyEvidence {
            signal_valid: true,
            raw_person_count,
            poi_present: true,
            confirmed_person_count: 0,
        },
        t0 + Duration::from_millis(250),
    );
    assert_eq!(update.state, RoomCardinality::Empty);
    assert_eq!(confirmed.state, RoomCardinality::Single);
}
