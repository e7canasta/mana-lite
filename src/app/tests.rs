use super::record::wire_depth_evidence;
use super::*;
use crate::config::{MetricsTextConfig, ModelCatalog, load_app_config};
use crate::health::HealthTransition;
use crate::infer::InferenceResult;
use crate::ingest::SyntheticReader;
use crate::logger::{JsonlLevel, LogManager};
use crate::occupancy::OccupancyStateMachine;
use crate::presence::PresenceFilter;
use crate::scan::ControlPolicy;
#[cfg(feature = "rerun")]
use crate::viz::VizBridge;
use mana_control::config::{OccupancyPolicy, PresencePoiPolicy};
use mana_control::domain::LoopId;
use mana_control::{
    DepthCalibration, DepthMetric, DepthOp, DepthRegionRule, DepthRuleResult, DepthRules,
};
use ndarray::Array2;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use ultralytics_inference::DepthMap;

#[tokio::test]
async fn bootstrap_with_reader_wires_real_catalogs() {
    let config_path = Path::new("config/mana.toml");
    let mut config = load_app_config(config_path).expect("mana.toml loads");
    // Avoid side effects from real deployment paths while keeping catalogs real.
    config.viz.enabled = false;
    config.output.save_dir = None;
    config.output.snapshot_dir = None;

    let mut app = App::bootstrap_with_reader(&config, config_path, SyntheticReader::empty())
        .await
        .expect("bootstrap_with_reader against real catalogs");

    assert!(
        config.pipeline.track,
        "mana.toml enables tracking; pre-condition for engine presence"
    );
    assert!(config.pipeline.zones);
    assert!(config.pipeline.fsm);
    assert!(
        app.control.tracker.is_some(),
        "pipeline.track=true wires tracker"
    );
    assert!(
        app.control.zone_engine.is_some(),
        "pipeline.zones=true with zones.toml wires zone_engine"
    );
    assert!(
        app.control.fsm_engine.is_some(),
        "pipeline.fsm=true with compiled FSM wires fsm_engine"
    );
    assert_eq!(
        app.control
            .fsm_engine
            .as_ref()
            .map(|fsm| fsm.current_state()),
        Some("idle"),
        "compiled program starts at catalog initial state"
    );

    assert_eq!(app.control.policy.person_class.as_str(), "person");
    assert_eq!(
        app.control.policy.data_stale_ms,
        config.health.data_stale_ms
    );
    assert_eq!(app.control.policy.scan_period_ms, config.scan.period_ms);
    assert_eq!(
        app.control.policy.face_dwell_roi,
        Some([760, 0, 1160, 300]),
        "face_dwell ROI from zones.toml"
    );
    // person_detection_roi = first Boxes model ∩ static crop map.
    // detect-fast owns [420,0,1500,1080]; face-yolo is Boxes but dynamic-only.
    // HashMap iteration can surface either first, so None is valid today.
    let expected_person_roi = match app
        .models
        .first_with_role(crate::domain::ModelRole::Boxes)
        .map(|id| id.as_str())
    {
        Some("detect-fast") => Some([420, 0, 1500, 1080]),
        _ => None,
    };
    assert_eq!(
        app.control.policy.person_detection_roi, expected_person_roi,
        "person_detection ROI follows first Boxes model with a static crop"
    );

    assert_eq!(app.scan_timeline.period_ms(), config.scan.period_ms);
    assert_eq!(
        app.scan_timeline.loop_id().as_str(),
        LoopId::DEFAULT,
        "scan timeline uses the default loop id"
    );
    assert_eq!(app.control.loop_id.as_str(), LoopId::DEFAULT);

    assert!(
        !app.depth_rules.rules.is_empty(),
        "depth-rules.toml has rules and bootstrap keeps them"
    );

    // Health thresholds come from config.health; assert via policy + behavior.
    assert_eq!(app.control.policy.data_stale_ms, 10_000);
    let boot = app.boot_instant;
    let at = |ms: u64| boot + Duration::from_millis(ms);
    assert_eq!(
        app.control.health.evaluate_at(at(5_000)),
        HealthTransition::None
    );
    assert_eq!(
        app.control.health.evaluate_at(at(5_001)),
        HealthTransition::Stale {
            component: "ingest",
            ms_since_frame: 5_001,
        },
        "stale_warn_ms from config (5_000) gates the Stale transition"
    );
    assert_eq!(
        app.control.health.evaluate_at(at(10_001)),
        HealthTransition::Blind {
            ms_since_frame: 10_001,
        },
        "data_stale_ms from config (10_000) gates Blind"
    );
}

#[test]
fn frame_timestamp_ns_es_monotona_en_el_instante() {
    let boot_at = chrono::Utc::now();
    let boot_instant = Instant::now();
    let t1 = boot_instant + std::time::Duration::from_millis(50);
    let t2 = t1 + std::time::Duration::from_millis(250);

    let ns1 = frame_timestamp_ns(&boot_at, boot_instant, t1);
    let ns2 = frame_timestamp_ns(&boot_at, boot_instant, t2);

    assert!(ns2 >= ns1, "un instante posterior nunca produce ts menor");
    assert!(
        ns2 - ns1 >= 200_000_000,
        "dos frames a 250ms de distancia no se colapsan"
    );
}

#[test]
fn wired_depth_results_reach_control_snapshot() {
    let results = [DepthRuleResult {
        rule: "bed-approach".into(),
        region: [10, 20, 30, 40],
        metric: DepthMetric::Median,
        threshold_m: 1.5,
        value: Some(1.0),
        triggered: true,
        valid_pixels: 8,
        valid_ratio: Some(0.9),
        calibration: Some(DepthCalibration {
            reference_model_m: 1.0,
            reference_scene_m: 2.0,
        }),
    }];

    let mut image = mana_control::ProcessImage::empty();
    let now = Instant::now();
    // Keyframe start clears stale depth; wiring must then install evidence.
    image.reset_depth(now);
    assert_eq!(image.depth_snapshot().is_triggered("bed-approach"), None);

    wire_depth_evidence(&mut image, &results, now);
    assert_eq!(
        image.depth_snapshot().is_triggered("bed-approach"),
        Some(true),
        "evaluate_depth_rules must wire results into ProcessImage, not reset them"
    );
}

fn depth_map_from_rows(rows: &[&[f32]]) -> crate::depth_map::DepthFrame {
    let data = Array2::from_shape_fn((rows.len(), rows[0].len()), |(y, x)| rows[y][x]);
    crate::depth_map::DepthFrame::from_ultralytics(DepthMap::new(
        data,
        (rows.len() as u32, rows[0].len() as u32),
    ))
}

fn app_for_depth_rules(rules: DepthRules, context_roi: Option<CropRect>) -> App<SyntheticReader> {
    let start = Instant::now();
    let empty_catalog = ModelCatalog {
        models: HashMap::new(),
    };
    App {
        infer: InferEngine::from_catalog(&empty_catalog).expect("empty catalog loads"),
        primary_model: "detect-fast".into(),
        control: ControlState {
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
            fsm_engine: None,
            health: mana_control::health::Health::new_at(10_000, 5_000, start),
            signal_snapshot: Default::default(),
            last_scan_at: start,
            scan_seq: 0,
            policy: ControlPolicy {
                person_class: "person".into(),
                presence_enabled: false,
                data_stale_ms: 10_000,
                scan_period_ms: 200,
                face_dwell_roi: None,
                person_detection_roi: None,
                face_edge_margin_px: 0,
            },
        },
        scan_timeline: ScanTimeline::new(LoopId::default_loop(), start, 200),
        cascade: CascadeScheduler::from_rules(&[]),
        detection_consolidator: DetectionConsolidator::new(0.7, 0.65, 0.5),
        models: ModelRegistry::from_catalog(&empty_catalog, "detect-fast"),
        ingest: IngestEngine::new(SyntheticReader::empty()),
        metrics: MetricsEngine::new(0, 50),
        depth_context_roi: context_roi,
        depth_rules: rules,
        decoder: FrameDecoder::new().expect("ffmpeg decoder"),
        snapshots: SnapshotSaver::new(None, false).expect("snapshots"),
        #[cfg(feature = "rerun")]
        observer: FanoutObserver::new(
            VizBridge::disabled(),
            Box::new(LogManager::new(JsonlLevel::Info)),
        ),
        #[cfg(not(feature = "rerun"))]
        observer: FanoutObserver::new(Box::new(LogManager::new(JsonlLevel::Info))),
        state: PipelineState::new(MetricsTextConfig::default(), 20, 3),
        boot_wall: chrono::Utc::now(),
        boot_instant: start,
        crop_frames_pending: Vec::new(),
        face_dwell_logger: FaceDwellLogStrategy,
        control_image: mana_control::ProcessImage::empty(),
    }
}

#[test]
fn evaluate_depth_rules_projects_triggered_rule_into_process_image() {
    let rules = DepthRules {
        rules: vec![DepthRegionRule {
            name: "bed-approach".into(),
            region: [0, 0, 2, 2],
            metric: DepthMetric::Median,
            op: DepthOp::Lt,
            threshold_m: 2.0,
            min_valid_ratio: 0.0,
            calibration: None,
        }],
    };
    let mut app = app_for_depth_rules(rules, Some(CropRect::from_array([0, 0, 2, 2])));
    let now = Instant::now();
    app.control_image.reset_depth(now);
    assert_eq!(
        app.control_image
            .depth_snapshot()
            .is_triggered("bed-approach"),
        None
    );

    let output = InferenceResult {
        detections: Vec::new(),
        depth: Some(depth_map_from_rows(&[&[1.0, 1.0], &[1.0, 1.0]])),
        postprocess_rejected: 0,
        post_nms_suppressed: 0,
        infer_ms: 0,
        pipeline_us: 0,
        crop_frame: None,
    };
    app.evaluate_depth_rules(&output, None, now);
    assert_eq!(
        app.control_image
            .depth_snapshot()
            .is_triggered("bed-approach"),
        Some(true),
        "App::evaluate_depth_rules must wire depth evidence into control_image"
    );
}

#[test]
fn evaluate_depth_rules_without_roi_leaves_snapshot_empty() {
    let rules = DepthRules {
        rules: vec![DepthRegionRule {
            name: "bed-approach".into(),
            region: [0, 0, 2, 2],
            metric: DepthMetric::Median,
            op: DepthOp::Lt,
            threshold_m: 2.0,
            min_valid_ratio: 0.0,
            calibration: None,
        }],
    };
    // No crop_rect and no depth_context_roi → early return.
    let mut app = app_for_depth_rules(rules, None);
    let now = Instant::now();
    app.control_image.reset_depth(now);
    let output = InferenceResult {
        detections: Vec::new(),
        depth: Some(depth_map_from_rows(&[&[1.0, 1.0], &[1.0, 1.0]])),
        postprocess_rejected: 0,
        post_nms_suppressed: 0,
        infer_ms: 0,
        pipeline_us: 0,
        crop_frame: None,
    };
    app.evaluate_depth_rules(&output, None, now);
    assert_eq!(
        app.control_image
            .depth_snapshot()
            .is_triggered("bed-approach"),
        None,
        "without ROI the adapter must not invent depth evidence"
    );
}
