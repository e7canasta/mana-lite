use super::perception::{PerceptionConfig, PerceptionSeed};
use super::record::wire_depth_evidence;
use super::*;
use crate::cascade::{CascadeScheduler, InferenceRequest};
use crate::config::{ModelCatalog, load_app_config};
use crate::detection::{CropRect, DetectionConsolidator};
use crate::domain::ModelRegistry;
use crate::health::HealthTransition;
use crate::infer::InferEngine;
use crate::infer::InferenceResult;
use crate::ingest::SyntheticReader;
use crate::metrics::MetricsEngine;
use crate::snapshot::{FrameDecoder, SnapshotSaver};
use mana_control::domain::LoopId;
use mana_control::{
    DepthCalibration, DepthMetric, DepthOp, DepthRegionRule, DepthRuleResult, DepthRules,
};
use ndarray::Array2;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
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
    // El ROI de person_detection lo resuelve el bootstrap contra el catálogo:
    // detect-fast posee [420,0,1500,1080], y face-yolo es Boxes pero sólo
    // dinámico. El orden de un HashMap puede sacar cualquiera primero, así que
    // ambas respuestas son válidas — lo que se verifica es que la política
    // quedó cableada con una de las dos y no con basura.
    assert!(
        matches!(
            app.control.policy.person_detection_roi,
            None | Some([420, 0, 1500, 1080])
        ),
        "person_detection ROI sale del primer modelo Boxes con crop estático"
    );

    assert_eq!(app.scan_timeline.period_ms(), config.scan.period_ms);
    assert_eq!(
        app.scan_timeline.loop_id().as_str(),
        LoopId::DEFAULT,
        "scan timeline uses the default loop id"
    );
    assert_eq!(app.control.loop_id.as_str(), LoopId::DEFAULT);

    // `models` y `depth_rules` ya no se pueden observar desde acá: viven en el
    // hilo de percepción. Que el bootstrap los haya cableado bien lo prueba que
    // `perception::spawn` haya devuelto Ok — arranca o falla el arranque.
    assert!(
        app.perception.is_some(),
        "bootstrap levanta el hilo de percepción o falla"
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

/// Etapa de percepción mínima para ejercitar las reglas de profundidad.
///
/// Antes esto construía un `App` de diecinueve campos —con ingesta, control,
/// FSM y sink de JSONL— para probar una regla de profundidad. Que ahora alcance
/// con esto es la medida del corte: la etapa no necesita nada del lazo.
fn stage_for_depth_rules(
    rules: DepthRules,
    context_roi: Option<CropRect>,
) -> crate::app::perception::PerceptionStage {
    let empty_catalog = ModelCatalog {
        models: HashMap::new(),
    };
    let seed = PerceptionSeed {
        infer: InferEngine::from_catalog(&empty_catalog).expect("empty catalog loads"),
        primary_model: "detect-fast".into(),
        cascade: CascadeScheduler::from_rules(&[]),
        detection_consolidator: DetectionConsolidator::new(0.7, 0.65, 0.5),
        models: ModelRegistry::from_catalog(&empty_catalog, "detect-fast"),
        depth_context_roi: context_roi,
        depth_rules: rules,
        snapshots: SnapshotSaver::new(None, false).expect("snapshots"),
        // Un handle sobre un slot que nadie drena: los dibujos se encolan y se
        // pisan, que es justo lo que hace el sistema cuando no hay visor.
        #[cfg(feature = "rerun")]
        observer: PerceptionObserver::new(crate::app::viz_relay::VizHandle::new(Arc::new(
            crate::slot::Slot::new(),
        ))),
        #[cfg(not(feature = "rerun"))]
        observer: PerceptionObserver::new(),
        metrics: Arc::new(Mutex::new(MetricsEngine::new(0, 50))),
        boot_wall: chrono::Utc::now(),
        boot_instant: Instant::now(),
    };
    seed.grow(FrameDecoder::new().expect("ffmpeg decoder"))
}

fn depth_only_result(rows: &[&[f32]]) -> InferenceResult {
    InferenceResult {
        detections: Vec::new(),
        depth: Some(depth_map_from_rows(rows)),
        postprocess_rejected: 0,
        post_nms_suppressed: 0,
        infer_ms: 0,
        pipeline_us: 0,
        crop_frame: None,
    }
}

fn bed_approach_rules() -> DepthRules {
    DepthRules {
        rules: vec![DepthRegionRule {
            name: "bed-approach".into(),
            region: [0, 0, 2, 2],
            metric: DepthMetric::Median,
            op: DepthOp::Lt,
            threshold_m: 2.0,
            min_valid_ratio: 0.0,
            calibration: None,
        }],
    }
}

#[test]
fn evaluate_depth_rules_projects_triggered_rule_into_process_image() {
    let mut stage = stage_for_depth_rules(
        bed_approach_rules(),
        Some(CropRect::from_array([0, 0, 2, 2])),
    );
    let now = Instant::now();
    stage.image.reset_depth(now);
    assert_eq!(
        stage.image.depth_snapshot().is_triggered("bed-approach"),
        None
    );

    stage.evaluate_depth_rules(&depth_only_result(&[&[1.0, 1.0], &[1.0, 1.0]]), None, now);
    assert_eq!(
        stage.image.depth_snapshot().is_triggered("bed-approach"),
        Some(true),
        "evaluate_depth_rules debe cablear la evidencia en la imagen de proceso"
    );
}

#[test]
fn evaluate_depth_rules_without_roi_leaves_snapshot_empty() {
    // Sin crop_rect ni depth_context_roi -> salida temprana.
    let mut stage = stage_for_depth_rules(bed_approach_rules(), None);
    let now = Instant::now();
    stage.image.reset_depth(now);
    stage.evaluate_depth_rules(&depth_only_result(&[&[1.0, 1.0], &[1.0, 1.0]]), None, now);
    assert_eq!(
        stage.image.depth_snapshot().is_triggered("bed-approach"),
        None,
        "sin ROI la etapa no puede inventar evidencia de profundidad"
    );
}

#[test]
fn person_crop_depth_does_not_update_scene_depth_rules() {
    let mut stage = stage_for_depth_rules(
        bed_approach_rules(),
        Some(CropRect::from_array([0, 0, 4, 4])),
    );
    let now = Instant::now();
    stage.image.reset_depth(now);
    stage.evaluate_depth_rules(
        &depth_only_result(&[&[1.0, 1.0], &[1.0, 1.0]]),
        Some(CropRect::from_array([0, 0, 2, 2])),
        now,
    );
    assert_eq!(
        stage.image.depth_snapshot().is_triggered("bed-approach"),
        None,
        "el depth de bbox no debe reemplazar la referencia de escena"
    );
}

#[test]
fn urgent_request_is_rejected_when_the_backend_model_is_unavailable() {
    let mut stage = stage_for_depth_rules(DepthRules { rules: Vec::new() }, None);
    let config = PerceptionConfig {
        snapshot: false,
        infer: true,
        presence_class: "person".into(),
        disabled_tasks: Vec::new(),
        face_pose: crate::config::FacePoseConfig::default(),
        perception: crate::config::PerceptionPolicyConfig {
            validation: crate::config::CrossModelValidationConfig {
                relation_support_threshold: 0.50,
                source_quality_weight: 0.40,
                agreement_quality_weight: 0.60,
            },
            body_parts: crate::config::BodyPartsConfig {
                mode: crate::config::BodyPartsMode::Validator,
                frame_local_match_iou: 0.50,
                face_frame_local_coverage: 0.50,
                joint_min_confidence: 0.25,
                segment_radius_ratio: 0.035,
                head_padding_ratio: 0.06,
                torso_radius_multiplier: 1.5,
                head_face_weight: 0.65,
                head_pose_weight: 0.35,
                mask_quality_weight: 0.25,
                cross_model_quality_weight: 0.20,
                minimum_geometry_extent_px: 1.0,
                geometry_epsilon: 0.00001,
                advanced_smoothing_alpha: 0.65,
                advanced_mask_support_threshold: 0.60,
                advanced_max_gap_frames: 3,
                advanced_temporal_quality_decay: 0.85,
            },
        },
    };
    let now = Instant::now();
    let request = InferenceRequest::new(
        "detect-fast",
        "synthetic",
        1,
        now,
        now + Duration::from_secs(1),
    );

    assert!(!stage.enqueue_transient_request(request, &config, now));
    assert_eq!(stage.cascade.pending_request_count(), 0);
}
