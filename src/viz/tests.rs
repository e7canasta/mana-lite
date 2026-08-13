use super::*;
use crate::app::body_parts::{
    ActorRef, BodyGeometry, BodyPartEstimate, BodyPartKind, BodyPartsEstimate,
};
use crate::occupancy::{RoomCardinality, SecondPersonState};
use std::time::Instant;

#[test]
fn detection_labels_include_area_and_frame_ratio() {
    let label = detection_label("person", 0.87, [100.0, 200.0, 300.0, 500.0], 1_000, 1_000);
    assert!(label.contains("conf=0.87"));
    assert!(label.contains("area=60000px"));
    assert!(label.contains("ratio=0.0600"));
}

#[test]
fn crop_bbox_uses_local_coordinates() {
    let rect = CropRect {
        x1: 312,
        y1: 40,
        x2: 1608,
        y2: 540,
    };
    assert_eq!(
        bbox_in_crop([500.0, 100.0, 620.0, 220.0], rect),
        [188.0, 60.0, 308.0, 180.0]
    );
}

#[test]
fn fixed_rois_use_stable_static_entities() {
    let (rec, storage) = rerun::RecordingStreamBuilder::new("mana-viz-fixed-roi-test")
        .batcher_config(rerun::log::ChunkBatcherConfig::NEVER)
        .memory()
        .expect("memory recording");
    VizBridge::send_fixed_rois(
        &rec,
        &[
            FixedRoi {
                model: "detect-fast".into(),
                rect: CropRect {
                    x1: 528,
                    y1: 0,
                    x2: 1392,
                    y2: 540,
                },
            },
            FixedRoi {
                model: "face-dwell".into(),
                rect: CropRect {
                    x1: 760,
                    y1: 0,
                    x2: 1160,
                    y2: 300,
                },
            },
        ],
    );

    let paths = storage
        .take()
        .into_iter()
        .filter_map(|msg| match msg {
            rerun::log::LogMsg::ArrowMsg(_, msg) => {
                Some(rerun::log::Chunk::from_arrow_msg(&msg).expect("valid chunk"))
            }
            _ => None,
        })
        .map(|chunk| chunk.entity_path().to_string())
        .collect::<Vec<_>>();
    assert!(
        paths
            .iter()
            .any(|path| { path == "/world/camera/rois/fixed/detect-fast/roi" })
    );
    assert!(
        paths
            .iter()
            .any(|path| { path == "/world/camera/rois/fixed/face-dwell/roi" })
    );
}

#[test]
fn body_parts_are_logged_under_camera_body_parts() {
    let (rec, storage) = rerun::RecordingStreamBuilder::new("mana-viz-body-parts-test")
        .batcher_config(rerun::log::ChunkBatcherConfig::NEVER)
        .memory()
        .expect("memory recording");
    let bridge = VizBridge {
        inner: Inner::Connected {
            rec,
            last_flush_warn: Instant::now(),
            flush_timeouts: 0,
        },
        addr: String::new(),
        retry_backoff_ms: INITIAL_BACKOFF_MS,
        stream_proven: true,
        image_format: VizImageFormat::Raw,
        toggles: VizSendToggles::default(),
        fixed_rois: Vec::new(),
        roles: HashMap::new(),
        face_models: HashSet::new(),
        last_infer_at: HashMap::new(),
        last_occupancy_state: None,
        last_second_person_state: None,
        last_signal_state: None,
        last_face_state: None,
    };
    let estimate = BodyPartsEstimate {
        actor_ref: ActorRef::Track(5),
        frame_number: 12,
        parts: vec![
            BodyPartEstimate {
                part: BodyPartKind::Head,
                geometry: BodyGeometry::Bbox([10.0, 20.0, 30.0, 40.0]),
                support: Vec::new(),
                source_models: Vec::new(),
                quality: 1.0,
                mask_coverage: None,
                depth: None,
                source_frame_numbers: vec![12],
                stale: false,
            },
            BodyPartEstimate {
                part: BodyPartKind::LeftArm,
                geometry: BodyGeometry::Polyline {
                    points: vec![[10.0, 40.0], [5.0, 60.0], [1.0, 80.0]],
                    radius: 3.0,
                },
                support: Vec::new(),
                source_models: Vec::new(),
                quality: 1.0,
                mask_coverage: None,
                depth: None,
                source_frame_numbers: vec![12],
                stale: false,
            },
        ],
        overall_quality: 1.0,
        mask_polygons: None,
    };

    bridge.log_body_parts(std::slice::from_ref(&estimate));
    let paths = storage
        .take()
        .into_iter()
        .filter_map(|msg| match msg {
            rerun::log::LogMsg::ArrowMsg(_, msg) => {
                Some(rerun::log::Chunk::from_arrow_msg(&msg).expect("valid chunk"))
            }
            _ => None,
        })
        .map(|chunk| chunk.entity_path().to_string())
        .collect::<Vec<_>>();

    assert!(
        paths
            .iter()
            .any(|path| { path == "/world/camera/body_parts/track/5/head" })
    );
    assert!(
        paths
            .iter()
            .any(|path| { path == "/world/camera/body_parts/track/5/left_arm" })
    );
}

#[test]
fn occupancy_state_has_sequence_timestamp_and_log_time_timelines() {
    let (rec, storage) = rerun::RecordingStreamBuilder::new("mana-viz-test")
        .batcher_config(rerun::log::ChunkBatcherConfig::NEVER)
        .memory()
        .expect("memory recording");
    rec.set_log_time_enabled(true);
    let mut bridge = VizBridge {
        inner: Inner::Connected {
            rec,
            last_flush_warn: Instant::now(),
            flush_timeouts: 0,
        },
        addr: String::new(),
        retry_backoff_ms: INITIAL_BACKOFF_MS,
        stream_proven: true,
        image_format: VizImageFormat::Raw,
        toggles: VizSendToggles::default(),
        fixed_rois: Vec::new(),
        roles: HashMap::new(),
        face_models: HashSet::new(),
        last_infer_at: HashMap::new(),
        last_occupancy_state: None,
        last_second_person_state: None,
        last_signal_state: None,
        last_face_state: None,
    };

    bridge.set_frame_time(1, 1_000);
    bridge.log_occupancy_state(
        RoomCardinality::Empty,
        SecondPersonState::None,
        SignalValidity::Valid,
    );
    bridge.set_frame_time(2, 2_000);
    bridge.log_occupancy_state(
        RoomCardinality::Single,
        SecondPersonState::None,
        SignalValidity::Valid,
    );

    let state_chunks = storage
        .take()
        .into_iter()
        .filter_map(|msg| match msg {
            rerun::log::LogMsg::ArrowMsg(_, msg) => {
                Some(rerun::log::Chunk::from_arrow_msg(&msg).expect("valid chunk"))
            }
            _ => None,
        })
        .filter(|chunk| chunk.entity_path().to_string() == "/pipeline/state/room/cardinality")
        .collect::<Vec<_>>();

    let chunk = state_chunks.first().expect("state chunk");
    let timelines = chunk.timelines();
    assert_eq!(
        timelines
            .get(&rerun::TimelineName::from(FRAME_NUMBER_TIMELINE))
            .expect("frame number timeline")
            .timeline()
            .typ(),
        rerun::external::re_log_types::TimeType::Sequence
    );
    assert_eq!(
        timelines
            .get(&rerun::TimelineName::from(FRAME_TIME_TIMELINE))
            .expect("frame timestamp timeline")
            .timeline()
            .typ(),
        rerun::external::re_log_types::TimeType::TimestampNs
    );
    assert!(timelines.contains_key(&rerun::TimelineName::log_time()));
}
