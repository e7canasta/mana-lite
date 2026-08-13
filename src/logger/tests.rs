use super::*;

fn test_logger() -> LogManager {
    LogManager::new(JsonlLevel::Debug)
}

struct CountingHandler {
    count: std::rc::Rc<std::cell::Cell<usize>>,
}

impl LogHandler for CountingHandler {
    fn handle(&mut self, _event: Event) {
        self.count.set(self.count.get() + 1);
    }

    fn flush(&mut self) {}
}

struct RecordingHandler {
    events: std::rc::Rc<std::cell::RefCell<Vec<Event>>>,
    flushes: std::rc::Rc<std::cell::Cell<usize>>,
}

struct DroppingHandler {
    calls: std::rc::Rc<std::cell::Cell<usize>>,
}

impl LogHandler for DroppingHandler {
    fn handle(&mut self, _event: Event) {
        self.calls.set(self.calls.get() + 1);
    }

    fn flush(&mut self) {}
}

impl LogHandler for RecordingHandler {
    fn handle(&mut self, event: Event) {
        self.events.borrow_mut().push(event);
    }

    fn flush(&mut self) {
        self.flushes.set(self.flushes.get() + 1);
    }
}

#[test]
fn log_manager_fans_events_out_to_handlers() {
    let first = std::rc::Rc::new(std::cell::Cell::new(0));
    let second = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut log = LogManager::with_handlers(vec![
        Box::new(CountingHandler {
            count: first.clone(),
        }),
        Box::new(CountingHandler {
            count: second.clone(),
        }),
    ]);

    log.emit(Event::meta_startup("test", "config"));

    assert_eq!(first.get(), 1);
    assert_eq!(second.get(), 1);
}

#[test]
fn scene_signals_survive_a_degraded_fanout_handler() {
    let dropped = std::rc::Rc::new(std::cell::Cell::new(0));
    let recorded = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let flushes = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut log = LogManager::with_handlers(vec![
        Box::new(DroppingHandler {
            calls: dropped.clone(),
        }),
        Box::new(RecordingHandler {
            events: recorded.clone(),
            flushes,
        }),
    ]);

    log.emit(Event::scene_signals(
        crate::scan::ControlStamp {
            scan_seq: 3,
            evidence_frame_id: 8,
            observations_age_ms: 0,
            depth_age_ms: None,
        },
        mana_control::signals::SceneSignalsSnapshot::default(),
    ));

    assert_eq!(dropped.get(), 1);
    assert!(matches!(
        recorded.borrow().first(),
        Some(Event::SceneSignals { stamp, .. }) if stamp.scan_seq == 3
    ));
}

#[test]
fn shutdown_emits_reason_and_flushes_handlers() {
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let flushes = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut log = LogManager::with_handlers(vec![Box::new(RecordingHandler {
        events: events.clone(),
        flushes: flushes.clone(),
    })]);

    log.shutdown("signal");

    let recorded = events.borrow();
    assert!(matches!(
        recorded.last(),
        Some(Event::Meta {
            event,
            detail,
            attrs,
        }) if event == "shutdown"
            && detail == "signal"
            && attrs.iter().any(|(key, value)| key == "uptime" && value.parse::<u64>().is_ok())
    ));
    assert_eq!(flushes.get(), 1);
}

#[test]
fn dropping_log_manager_flushes_handlers() {
    let flushes = std::rc::Rc::new(std::cell::Cell::new(0));
    {
        let _log = LogManager::with_handlers(vec![Box::new(RecordingHandler {
            events: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            flushes: flushes.clone(),
        })]);
    }

    assert_eq!(flushes.get(), 1);
}

fn collect(logger: &mut LogManager) -> String {
    let mut buf = Vec::new();
    logger.flush_to_buffer(&mut buf);
    String::from_utf8(buf).unwrap()
}

#[test]
fn depth_event_v2_allows_nullable_fields() {
    let mut log = test_logger();
    log.emit(Event::depth(
        1,
        "depth-standard",
        1,
        1,
        None,
        0,
        0,
        0,
        None,
        None,
        None,
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"version\":2"));
    assert!(out.contains("\"roi\":null"));
    assert!(out.contains("\"valid_ratio\":null"));
    assert!(out.contains("\"min_depth_m\":null"));
    assert!(out.contains("\"max_depth_m\":null"));
}

#[test]
fn depth_region_event_serializes_evidence() {
    let mut log = test_logger();
    log.emit(Event::depth_region(
        42,
        "bed-approach",
        [560, 140, 1240, 820],
        "median",
        Some(1.2),
        1.5,
        true,
        462400,
        Some(1.0),
        None,
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"depth_region\""));
    assert!(out.contains("\"version\":2"));
    assert!(out.contains("\"rule\":\"bed-approach\""));
    assert!(out.contains("\"region\":[560,140,1240,820]"));
    assert!(out.contains("\"metric\":\"median\""));
    assert!(out.contains("\"value\":1.2"));
    assert!(out.contains("\"threshold_m\":1.5"));
    assert!(out.contains("\"triggered\":true"));
    assert!(out.contains("\"valid_pixels\":462400"));
    assert!(out.contains("\"valid_ratio\":1"));
}

#[test]
fn depth_region_event_allows_null_value() {
    let mut log = test_logger();
    log.emit(Event::depth_region(
        1,
        "bed-approach",
        [0, 0, 1, 1],
        "median",
        None,
        1.5,
        false,
        0,
        None,
        None,
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"value\":null"));
    assert!(out.contains("\"triggered\":false"));
    assert!(out.contains("\"valid_ratio\":null"));
}

#[test]
fn depth_region_event_serializes_calibration() {
    let mut log = test_logger();
    log.emit(Event::depth_region(
        7,
        "bed-approach",
        [560, 140, 1240, 820],
        "median",
        Some(1.0),
        1.2,
        true,
        462400,
        Some(1.0),
        Some(mana_control::DepthCalibration {
            reference_model_m: 2.0,
            reference_scene_m: 1.0,
        }),
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"version\":2"));
    assert!(out.contains("\"calibration\":{\"reference_model_m\":2,\"reference_scene_m\":1}"));
}

#[test]
fn depth_event_v2_has_version_roi_and_map_dims() {
    let mut log = test_logger();
    log.emit(Event::depth(
        123,
        "depth-standard",
        268,
        281,
        Some([560, 140, 1240, 820]),
        680,
        680,
        462400,
        Some(1.0),
        Some(2.19),
        Some(5.16),
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"depth\""));
    assert!(out.contains("\"version\":2"));
    assert!(out.contains("\"roi\":[560,140,1240,820]"));
    assert!(out.contains("\"map_width\":680"));
    assert!(out.contains("\"map_height\":680"));
    assert!(out.contains("\"valid_pixels\":462400"));
    assert!(out.contains("\"valid_ratio\":1"));
    assert!(out.contains("\"min_depth_m\":2.19"));
    assert!(out.contains("\"max_depth_m\":5.16"));
    assert!(!out.contains("\"width\":680"));
}

#[test]
fn detection_emits_class_and_bbox() {
    let det = vec![DetRecord {
        class: "person".into(),
        confidence: 0.87,
        bbox: [100.0, 200.0, 300.0, 500.0],
        area_px: 60_000.0,
        area_ratio: 0.1,
        keypoints: None,
        mask: None,
    }];
    let mut log = test_logger();
    log.emit(Event::detection(
        1,
        "detect-fast",
        52,
        60,
        det,
        0,
        0,
        None,
        None,
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"detection\""));
    assert!(out.contains("\"pipeline_ms\":60"));
    assert!(out.contains("\"class\":\"person\""));
    assert!(out.contains("\"confidence\":0.87"));
    assert!(out.contains("\"area_px\":60000"));
    assert!(out.contains("\"area_ratio\":0.1"));
    assert!(out.contains("\"bbox\":[100,200,300,500]"));
}

#[test]
fn det_record_carries_frame_relative_area() {
    let detection = crate::detection::Detection {
        class: "person".into(),
        confidence: 0.9,
        bbox: [10.0, 20.0, 110.0, 220.0],
        keypoints: None,
        mask: None,
    };
    let record = DetRecord::from_detection(&detection, 1_000, 1_000);
    assert_eq!(record.area_px, 20_000.0);
    assert!((record.area_ratio - 0.02).abs() < f32::EPSILON);
}

#[test]
fn detection_emits_mask_wire_record() {
    use crate::detection::DetectionMask;
    use mana_geometry::compact_mask::CompactMask;
    use std::sync::Arc;
    let compact = CompactMask::from_dense(&[1, 1, 1, 1], 2, 2, (3, 4), (10, 10)).unwrap();
    let mask = DetectionMask {
        compact: Arc::new(compact),
        polygons: Arc::new(vec![vec![[0.25, 0.25], [0.75, 0.25], [0.75, 0.75]]]),
        origin: [0, 0],
        mask_dims: [10, 10],
    };
    let record = MaskRecord::from_mask(&mask);
    let det = vec![DetRecord {
        class: "person".into(),
        confidence: 0.9,
        bbox: [3.0, 4.0, 5.0, 6.0],
        area_px: 4.0,
        area_ratio: 0.0,
        keypoints: None,
        mask: Some(record),
    }];
    let mut log = test_logger();
    log.emit(Event::detection(
        1,
        "seg-standard",
        52,
        60,
        det,
        0,
        0,
        None,
        None,
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"mask\":{\"rle\":["), "missing rle: {out}");
    assert!(
        out.contains("\"bbox\":[3,4,5,6]"),
        "missing mask bbox: {out}"
    );
    assert!(out.contains("\"origin\":[0,0]"));
    assert!(out.contains("\"mask_dims\":[10,10]"));
    assert!(out.contains("\"polygons\":[[[0.25,0.25],[0.75,0.25],[0.75,0.75]]]"));
}

#[test]
fn mask_wire_record_round_trips_through_rle() {
    use crate::detection::DetectionMask;
    use mana_geometry::compact_mask::CompactMask;
    use std::sync::Arc;
    let compact = CompactMask::from_dense(&[1, 1, 1, 1], 2, 2, (3, 4), (10, 10)).unwrap();
    let mask = DetectionMask {
        compact: Arc::new(compact),
        polygons: Arc::new(vec![]),
        origin: [0, 0],
        mask_dims: [10, 10],
    };
    let record = MaskRecord::from_mask(&mask);
    let rebuilt = vernier_mask::Rle::from_counts(
        (record.bbox[3] - record.bbox[1]) as u32,
        (record.bbox[2] - record.bbox[0]) as u32,
        record.rle.clone(),
    );
    let raster = rebuilt.to_raster_bytes();
    assert_eq!(
        raster,
        vec![1, 1, 1, 1],
        "lossless round-trip of mask raster"
    );
}

#[test]
fn jsonl_config_filters_optional_events() {
    let mut log = test_logger();
    log.set_jsonl_config(MetricsJsonlConfig {
        frame_events: false,
        ..MetricsJsonlConfig::default()
    });
    log.emit(Event::frame_ingest(1, true, 10, 100));
    log.emit(Event::detection(
        1,
        "detect-fast",
        10,
        12,
        Vec::new(),
        0,
        0,
        None,
        None,
    ));

    let out = collect(&mut log);
    assert!(!out.contains("\"type\":\"frame\""));
    assert!(out.contains("\"type\":\"detection\""));
}

#[test]
fn consolidated_detection_has_no_track_id() {
    let mut log = test_logger();
    log.emit(Event::consolidated_detection(
        4,
        "person",
        0.91,
        [10.0, 20.0, 110.0, 220.0],
        "detect-fast",
        vec!["detect-fast".into(), "pose-standard".into()],
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"consolidated_detection\""));
    assert!(out.contains("\"primary_model\":\"detect-fast\""));
    assert!(out.contains("\"sources\":[\"detect-fast\",\"pose-standard\"]"));
    assert!(!out.contains("track_id"));
}

#[test]
fn fsm_transition_has_from_to_trigger() {
    let mut log = test_logger();
    log.emit(Event::fsm_transition(
        "idle",
        None,
        "monitoring",
        None,
        "bed_occupied",
        0,
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"fsm\""));
    assert!(out.contains("\"from\":\"idle\""));
    assert!(out.contains("\"to\":\"monitoring\""));
    assert!(out.contains("\"trigger\":\"bed_occupied\""));
}

#[test]
fn presence_event_serializes_cardinality_evidence() {
    let mut log = test_logger();
    log.emit(Event::presence(
        crate::scan::ControlStamp {
            scan_seq: 12,
            evidence_frame_id: 10,
            observations_age_ms: 2_500,
            depth_age_ms: None,
        },
        "single",
        "present",
        "candidate",
        2,
        1,
        true,
        false,
        3,
        0,
        1_000,
        0,
        500,
        0,
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"presence\""));
    assert!(out.contains("\"scan_seq\":12"));
    assert!(out.contains("\"evidence_frame_id\":10"));
    assert!(out.contains("\"observations_age_ms\":2500"));
    assert!(out.contains("\"depth_age_ms\":null"));
    assert!(out.contains("\"state\":\"single\""));
    assert!(out.contains("\"poi_state\":\"present\""));
    assert!(out.contains("\"poi_positive_ms\":3"));
    assert!(out.contains("\"single_timer_ms\":1000"));
    assert!(out.contains("\"second_person\":\"candidate\""));
    assert!(out.contains("\"raw_count\":2"));
    assert!(out.contains("\"confirmed_count\":1"));
}

#[test]
fn scene_signals_event_serializes_values_absences_and_stable_order() {
    let mut table = mana_control::signals::SignalTable::new();
    let catalog = mana_control::signals::scene_signal_catalog();
    table
        .insert(
            catalog,
            mana_control::domain::SignalTag::new("persona.presente"),
            mana_control::signals::SignalValue::Bool(true),
        )
        .unwrap();
    table
        .insert(
            catalog,
            mana_control::domain::SignalTag::new("cara.confianza"),
            mana_control::signals::SignalValue::Ratio(
                mana_control::signals::Ratio::new(0.87).unwrap(),
            ),
        )
        .unwrap();
    let snapshot = table.snapshot(catalog);

    let mut log = test_logger();
    log.emit(Event::scene_signals(
        crate::scan::ControlStamp {
            scan_seq: 12,
            evidence_frame_id: 42,
            observations_age_ms: 3,
            depth_age_ms: Some(5),
        },
        snapshot,
    ));
    let out = collect(&mut log);

    assert!(out.contains("\"type\":\"scene_signals\""));
    assert!(out.contains("\"catalog_version\":1"));
    assert!(out.contains("\"scan_seq\":12"));
    assert!(out.contains("\"evidence_frame_id\":42"));
    assert!(out.contains("\"tag\":\"cara.confianza\",\"kind\":\"ratio\",\"value\":0.87"));
    assert!(out.contains("\"tag\":\"cara.en_borde\",\"kind\":\"bool\",\"absent\":true"));
    assert!(out.contains("\"tag\":\"persona.presente\",\"kind\":\"bool\",\"value\":true"));
    assert_eq!(out.matches("\"tag\":").count(), 9);
    assert!(
        out.find("\"tag\":\"cara.confianza\"").unwrap()
            < out.find("\"tag\":\"persona.presente\"").unwrap()
    );
}

#[test]
fn scene_signals_event_can_be_filtered_without_affecting_default_persistence() {
    let table = mana_control::signals::SignalTable::new();
    let snapshot = table.snapshot(mana_control::signals::scene_signal_catalog());
    let event = Event::scene_signals(
        crate::scan::ControlStamp {
            scan_seq: 1,
            evidence_frame_id: 1,
            observations_age_ms: 0,
            depth_age_ms: None,
        },
        snapshot.clone(),
    );

    let mut default_log = test_logger();
    default_log.emit(event.clone());
    assert!(collect(&mut default_log).contains("\"type\":\"scene_signals\""));

    let mut filtered_log = test_logger();
    filtered_log.set_jsonl_config(MetricsJsonlConfig {
        scene_signals_events: false,
        ..MetricsJsonlConfig::default()
    });
    filtered_log.emit(event);
    assert!(!collect(&mut filtered_log).contains("\"type\":\"scene_signals\""));
}

#[test]
fn face_dwell_event_serializes_state_evidence_and_timers() {
    let mut log = test_logger();
    log.emit(Event::face_dwell(
        crate::scan::ControlStamp {
            scan_seq: 7,
            evidence_frame_id: 42,
            observations_age_ms: 0,
            depth_age_ms: Some(0),
        },
        "keyframe",
        "searching",
        Some("Buscando cara"),
        500,
        None,
        Some("single"),
        true,
        true,
        Some(0.87),
        Some(true),
        false,
        false,
        true,
        vec![FaceDwellTimerRecord {
            trigger: "searching→in_bed".into(),
            elapsed_ms: 500,
            required_ms: 1_000,
        }],
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"face_dwell\""));
    assert!(out.contains("\"scan_seq\":7"));
    assert!(out.contains("\"evidence_frame_id\":42"));
    assert!(out.contains("\"source\":\"keyframe\""));
    assert!(out.contains("\"state\":\"searching\""));
    assert!(out.contains("\"state_label\":\"Buscando cara\""));
    assert!(out.contains("\"face_in_dwell\":true"));
    assert!(out.contains("\"face_model_ran\":true"));
    assert!(out.contains("\"trigger\":\"searching→in_bed\""));
    assert!(out.contains("\"required_ms\":1000"));
}

#[test]
fn health_blind_has_message() {
    let mut log = test_logger();
    log.emit(Event::health_blind(10_000));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"health\""));
    assert!(out.contains("\"event\":\"blind\""));
    assert!(out.contains("10000ms"));
}

#[test]
fn escape_json_string_quotes_and_backslash() {
    let mut log = test_logger();
    log.emit(Event::Meta {
        event: "test".into(),
        detail: "say \"hello\"".into(),
        attrs: vec![("path".into(), "C:\\Users\\test".into())],
    });
    let out = collect(&mut log);
    assert!(out.contains("say \\\"hello\\\""));
    assert!(out.contains("C:\\\\Users\\\\test"));
}

#[test]
fn escape_json_control_chars() {
    let mut log = test_logger();
    log.emit(Event::Meta {
        event: "test".into(),
        detail: "line1\nline2".into(),
        attrs: vec![],
    });
    let out = collect(&mut log);
    assert!(out.contains("line1\\nline2"));
}

#[test]
fn negative_float_is_valid_json() {
    let det = vec![DetRecord {
        class: "x".into(),
        confidence: 0.5,
        bbox: [-10.5, 0.0, 100.0, 200.25],
        area_px: 0.0,
        area_ratio: 0.0,
        keypoints: None,
        mask: None,
    }];
    let mut log = test_logger();
    log.emit(Event::detection(1, "m", 10, 12, det, 0, 0, None, None));
    let out = collect(&mut log);
    assert!(out.contains("\"bbox\":[-10.5,0,100,200.25]"));
}

#[test]
fn detection_emits_keypoints_wire_record() {
    let det = vec![DetRecord {
        class: "person".into(),
        confidence: 0.9,
        bbox: [100.0, 200.0, 300.0, 500.0],
        area_px: 60_000.0,
        area_ratio: 0.1,
        keypoints: Some(vec![[101.5, 202.25, 0.8], [103.0, 204.0, 0.6]]),
        mask: None,
    }];
    let mut log = test_logger();
    log.emit(Event::detection(
        1,
        "pose-standard",
        52,
        60,
        det,
        0,
        0,
        None,
        None,
    ));
    let out = collect(&mut log);
    assert!(
        out.contains("\"keypoints\":[[101.5,202.25,0.8],[103,204,0.6]]"),
        "missing keypoints: {out}"
    );
}

#[test]
fn detection_without_keypoints_omits_the_field() {
    let det = vec![DetRecord {
        class: "person".into(),
        confidence: 0.9,
        bbox: [100.0, 200.0, 300.0, 500.0],
        area_px: 60_000.0,
        area_ratio: 0.1,
        keypoints: None,
        mask: None,
    }];
    let mut log = test_logger();
    log.emit(Event::detection(
        1,
        "detect-fast",
        52,
        60,
        det,
        0,
        0,
        None,
        None,
    ));
    let out = collect(&mut log);
    assert!(!out.contains("keypoints"), "unexpected keypoints: {out}");
}

#[test]
fn cross_model_validation_emits_compact_quality_and_sources() {
    let mut log = test_logger();
    log.emit(Event::cross_model_validation(
        7,
        12,
        0.8125,
        0.75,
        1.0,
        vec!["face-yolo".into(), "pose-standard".into()],
        vec![],
        vec!["face_pose=0.750".into()],
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"cross_model_validation\""));
    assert!(out.contains("\"actor_id\":7"));
    assert!(out.contains("\"frame_id\":12"));
    assert!(out.contains("\"quality\":0.8125"));
    assert!(out.contains("\"supporting_sources\":[\"face-yolo\",\"pose-standard\"]"));
    assert!(out.contains("\"reasons\":[\"face_pose=0.750\"]"));
}

#[test]
fn body_parts_event_serializes_geometry_provenance_and_freshness() {
    let mut log = test_logger();
    log.emit(Event::body_parts(
        12,
        Some(7),
        None,
        0.72,
        vec![BodyPartRecord {
            part: "left_arm".into(),
            geometry: BodyGeometryRecord::Polyline {
                points: vec![[10.0, 20.0], [30.0, 40.0]],
                radius: 4.0,
            },
            support: vec!["pose".into(), "segment".into()],
            source_models: vec!["pose-standard".into(), "seg-standard".into()],
            quality: 0.68,
            mask_coverage: Some(0.9),
            depth: Some(BodyPartDepthRecord {
                source_model: "depth-person-s-320".into(),
                roi: [100, 100, 500, 500],
                map_width: 320,
                map_height: 320,
                sampled_pixels: 36,
                valid_pixels: 30,
                valid_ratio: Some(0.8333),
                min_depth_m: Some(1.2),
                median_depth_m: Some(1.8),
                p10_depth_m: Some(1.3),
                p90_depth_m: Some(2.2),
                max_depth_m: Some(2.4),
                relative_to_torso_m: Some(0.25),
            }),
            source_frame_numbers: vec![12],
            stale: false,
        }],
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"body_parts\""));
    assert!(out.contains("\"actor_id\":7"));
    assert!(out.contains("\"overall_quality\":0.72"));
    assert!(out.contains("\"kind\":\"polyline\""));
    assert!(out.contains("\"support\":[\"pose\",\"segment\"]"));
    assert!(out.contains("\"source_frame_numbers\":[12]"));
    assert!(out.contains("\"source_model\":\"depth-person-s-320\""));
    assert!(out.contains("\"median_depth_m\":1.8"));
    assert!(out.contains("\"relative_to_torso_m\":0.25"));
    assert!(out.contains("\"stale\":false"));
}

#[test]
fn depth_emits_dimensions_and_finite_stats() {
    let mut log = test_logger();
    log.emit(Event::depth(
        7,
        "depth-standard",
        180,
        190,
        Some([560, 140, 1240, 820]),
        680,
        680,
        462400,
        Some(0.9999),
        Some(0.42),
        Some(8.31),
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"type\":\"depth\""));
    assert!(out.contains("\"version\":2"));
    assert!(out.contains("\"roi\":[560,140,1240,820]"));
    assert!(out.contains("\"map_width\":680"));
    assert!(out.contains("\"map_height\":680"));
    assert!(out.contains("\"valid_pixels\":462400"));
    assert!(out.contains("\"valid_ratio\":0.9999"));
    assert!(out.contains("\"min_depth_m\":0.42"));
    assert!(out.contains("\"max_depth_m\":8.31"));
}

#[test]
fn depth_emits_null_stats_when_map_is_empty() {
    let mut log = test_logger();
    log.emit(Event::depth(
        8,
        "depth-standard",
        10,
        12,
        None,
        0,
        0,
        0,
        None,
        None,
        None,
    ));
    let out = collect(&mut log);
    assert!(out.contains("\"roi\":null"));
    assert!(out.contains("\"valid_pixels\":0"));
    assert!(out.contains("\"valid_ratio\":null"));
    assert!(out.contains("\"min_depth_m\":null"));
    assert!(out.contains("\"max_depth_m\":null"));
}
