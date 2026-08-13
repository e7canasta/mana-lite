#![allow(unused_imports)]

mod app;
mod blueprint;
mod env;
mod fsm;
mod loader;
mod model_loader;
mod models;
mod observability;
mod validation;
mod zones;

pub use crate::fsm::{FsmGuard, SignalLiteral};
pub use app::{
    AppConfig, BodyPartsConfig, BodyPartsMode, CrossModelValidationConfig, DetectionConfig,
    FacePoseConfig, HealthConfig, InferenceConfig, IngestConfig, MANA_TOML_SCHEMA_VERSION,
    OccupancyPolicy, OutputConfig, PerceptionPolicyConfig, PipelineConfig, PresenceConfig,
    PresencePoiPolicy, Rotate, ScanConfigSection, SourceConfig, TrackingConfig,
    TrackingNoiseConfig, VizConfig,
};
pub use blueprint::{
    BlueprintConfig, BlueprintMetadata, CascadeConfig, CascadeRule, SemanticRegion,
};
pub use fsm::{FsmCatalog, FsmRoles, FsmRoot, FsmState, FsmTransition};
pub use loader::{
    load_app_config, load_config, load_depth_rules, load_fsm_catalog, load_metrics_log,
    load_rerun_blueprint, load_viz_data, load_zone_catalog,
};
pub use model_loader::{apply_model_overlay, load_model_catalog};
pub use models::{
    CropConfig, CropType, FallbackMode, ModelCatalog, ModelEntry, ModelTask, PostprocessConfig,
};
pub use observability::{
    MetricsInner, MetricsJsonlConfig, MetricsLogConfig, MetricsTextConfig, MetricsTextFlags,
    RerunBlueprintConfig, RerunOverride, RerunPanel, RerunRoot, RerunRow, VizDataConfig,
    VizDataInner, VizSendToggles,
};
pub use validation::validate_model_catalog;
pub use zones::{ZoneCatalog, ZoneEntry};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsm::FsmProgram;
    use std::path::{Path, PathBuf};

    fn fsm_compile_errors(
        fsm: &FsmCatalog,
        models: &ModelCatalog,
        zones: Option<&ZoneCatalog>,
        depth_rules: Option<&mana_control::DepthRules>,
    ) -> Vec<String> {
        let names = depth_rules.map(|rules| {
            rules
                .rules
                .iter()
                .map(|rule| rule.name.clone())
                .collect::<std::collections::HashSet<_>>()
        });
        let model_names: std::collections::HashSet<String> =
            models.models.keys().cloned().collect();
        FsmProgram::compile_with_references(fsm, zones, Some(&model_names), names.as_ref())
            .err()
            .unwrap_or_default()
    }

    #[test]
    fn test_load_model_catalog() {
        let catalog = load_model_catalog(Path::new("config/models.toml")).unwrap();
        assert!(catalog.models.contains_key("detect-fast"));
        let detect = &catalog.models["detect-fast"];
        assert_eq!(detect.task, ModelTask::Detect);
        assert_eq!(detect.confidence, 0.4);
        assert!(detect.is_valid());
    }

    #[test]
    fn example_model_manifest_uses_the_same_loader() {
        let catalog = load_model_catalog(Path::new("config/models.example.toml")).unwrap();
        assert_eq!(catalog.models["detect-fast"].task, ModelTask::Detect);
        assert_eq!(
            catalog.models["face-yolo"].postprocess.max_detections,
            Some(1)
        );
        assert!(catalog.models["face-yolo"].crop.is_some());
    }

    #[test]
    fn blueprint_overlay_is_relative_to_the_blueprint_file() {
        let blueprint: BlueprintConfig = load_config(Path::new(
            "config/blueprints/detect-room-face/blueprint.toml",
        ))
        .unwrap();
        assert_eq!(
            blueprint.blueprint.model_overlay,
            Some(PathBuf::from("models.toml"))
        );

        let mut catalog = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let overridden = apply_model_overlay(
            &mut catalog,
            Path::new("config/blueprints/detect-room-face/models.toml"),
            Path::new("config/models.toml"),
        )
        .unwrap();
        assert_eq!(overridden, vec!["face-yolo"]);
        assert_eq!(catalog.models["face-yolo"].confidence, 0.10);
        let face_crop = catalog.models["face-yolo"]
            .crop
            .as_ref()
            .expect("dynamic face crop");
        assert_eq!(face_crop.crop_type, CropType::LargestClass);
        assert_eq!(face_crop.class.as_deref(), Some("person"));
        assert_eq!(face_crop.square_size, Some(320));
        assert_eq!(face_crop.upper_fraction, Some(0.50));
    }

    fn model_enabled_map(catalog: &ModelCatalog) -> std::collections::HashMap<String, bool> {
        catalog
            .models
            .iter()
            .map(|(name, entry)| (name.clone(), entry.enabled))
            .collect()
    }

    #[test]
    fn configured_cascade_has_valid_pose_rule() {
        let config: CascadeConfig = load_config(Path::new("config/cascade.toml")).unwrap();
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        assert!(
            config
                .validate(&model_enabled_map(&models), "detect-fast")
                .is_empty()
        );
        // The fallback cascade mirrors the blueprint pattern (the face path is
        // the reference): one confirmed person, no region gate.
        let pose = config
            .rules
            .iter()
            .find(|r| r.model == "pose-standard")
            .expect("pose-standard rule");
        assert_eq!(pose.requires.as_deref(), Some("detect-fast"));
        assert_eq!(pose.requires_exact_count, Some(1));
        assert_eq!(pose.requires_region, None);
        assert!(!pose.same_frame);
    }

    #[test]
    fn configured_blueprints_are_valid() {
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let enabled = model_enabled_map(&models);
        for path in [
            "config/blueprints/detect-face/blueprint.toml",
            "config/blueprints/detect-pose/blueprint.toml",
            "config/blueprints/detect-seg/blueprint.toml",
            "config/blueprints/detect-face-pose/blueprint.toml",
            "config/blueprints/detect-face-pose-seg/blueprint.toml",
            "config/blueprints/detect-face-pose-seg-depth/blueprint.toml",
            "config/blueprints/detect-room-raw/blueprint.toml",
            "config/blueprints/detect-room-face/blueprint.toml",
        ] {
            let blueprint: BlueprintConfig = load_config(Path::new(path)).unwrap();
            let config = CascadeConfig {
                rules: blueprint.rules.clone(),
                regions: blueprint.regions.clone(),
            };
            assert!(
                config
                    .validate(&enabled, &blueprint.blueprint.primary_model)
                    .is_empty(),
                "invalid blueprint {path}"
            );
            assert!(
                blueprint
                    .blueprint
                    .models
                    .contains(&blueprint.blueprint.primary_model)
            );
        }
    }

    #[test]
    fn capacity_workshop_profiles_apply_cpu_overlays() {
        for (blueprint_path, overlay, expected_marker, expected_imgsz) in [
            (
                "workshop/scenarios/11-inference-capacity/blueprint-s-192.toml",
                "workshop/scenarios/11-inference-capacity/models-s-192.toml",
                "yolo26s-fp16-192.onnx",
                192,
            ),
            (
                "workshop/scenarios/11-inference-capacity/blueprint-m-192.toml",
                "workshop/scenarios/11-inference-capacity/models-m-192.toml",
                "yolo26m-fp16-192.onnx",
                192,
            ),
            (
                "workshop/scenarios/11-inference-capacity/blueprint-s-320.toml",
                "workshop/scenarios/11-inference-capacity/models-s-320.toml",
                "yolo26s-fp16-320.onnx",
                320,
            ),
            (
                "workshop/scenarios/11-inference-capacity/blueprint-m-320.toml",
                "workshop/scenarios/11-inference-capacity/models-m-320.toml",
                "yolo26m-fp16-320.onnx",
                320,
            ),
        ] {
            let blueprint: BlueprintConfig = load_config(Path::new(blueprint_path)).unwrap();
            let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
            let config = CascadeConfig {
                rules: blueprint.rules.clone(),
                regions: blueprint.regions.clone(),
            };
            assert!(
                config
                    .validate(
                        &model_enabled_map(&models),
                        &blueprint.blueprint.primary_model
                    )
                    .is_empty(),
                "invalid capacity blueprint {blueprint_path}"
            );
            assert_eq!(
                blueprint
                    .rules
                    .iter()
                    .find(|rule| rule.model == "seg-standard")
                    .map(|rule| rule.interval_min_ms),
                Some(2_000)
            );

            let mut models = load_model_catalog(Path::new("config/models.toml")).unwrap();
            let overridden = apply_model_overlay(
                &mut models,
                Path::new(overlay),
                Path::new("config/models.toml"),
            )
            .unwrap();

            assert_eq!(overridden.len(), 4);
            assert_eq!(models.models["detect-fast"].imgsz, Some(expected_imgsz));
            assert!(models.models["detect-fast"].path.ends_with(expected_marker));
            assert_eq!(models.models["pose-standard"].imgsz, Some(expected_imgsz));
            assert_eq!(models.models["seg-standard"].imgsz, Some(expected_imgsz));
        }
    }

    #[test]
    fn detect_room_face_fsm_catalog_is_valid() {
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let fsm =
            load_fsm_catalog(Path::new("config/blueprints/detect-room-face/fsm.toml")).unwrap();
        let zones = load_zone_catalog(Path::new("config/zones.toml")).unwrap();
        let errors = fsm_compile_errors(&fsm, &models, Some(&zones), None);
        assert!(errors.is_empty(), "face FSM should be valid: {errors:?}");
        assert_eq!(fsm.fsm.initial, "idle");
        for state in [
            "searching",
            "detected",
            "other",
            "in_bed",
            "edge",
            "exiting",
            "blind",
        ] {
            assert!(fsm.fsm.states.contains_key(state), "missing state {state}");
        }
        assert_eq!(fsm.fsm.roles.safe, "blind");
        assert_eq!(fsm.fsm.roles.reset, "idle");
        assert!(fsm.fsm.states["detected"].face_inside);
        assert!(fsm.fsm.states["in_bed"].face_inside);
        assert!(fsm.fsm.states["edge"].face_inside_maybe);
        for (from, to) in [
            ("searching", "in_bed"),
            ("detected", "in_bed"),
            ("edge", "in_bed"),
            ("other", "in_bed"),
            ("exiting", "in_bed"),
        ] {
            let transition = fsm
                .fsm
                .transitions
                .iter()
                .find(|transition| transition.from == from && transition.to == to)
                .unwrap_or_else(|| panic!("missing transition {from}->{to}"));
            assert!(
                transition.guards.iter().any(|guard| {
                    matches!(
                        guard,
                        FsmGuard::Signal {
                            tag,
                            op,
                            value: SignalLiteral::Bool(false),
                        } if tag == "cara.en_borde" && op == "=="
                    )
                }),
                "{from}->{to} must preserve edge priority"
            );
        }
        let edge_to_other = fsm
            .fsm
            .transitions
            .iter()
            .find(|transition| transition.from == "edge" && transition.to == "other")
            .expect("missing transition edge->other");
        assert!(edge_to_other.guards.iter().any(|guard| {
            matches!(
                guard,
                FsmGuard::Signal {
                    tag,
                    op,
                    value: SignalLiteral::Bool(false),
                } if tag == "cara.en_borde" && op == "=="
            )
        }));
    }

    #[test]
    fn test_load_fp16_benchmark_matrix() {
        let catalog = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let tasks = ["detect", "pose", "seg"];
        let sizes = ["s", "m", "l", "x"];
        let input_sizes = [192, 320, 640];

        for task in tasks {
            for size in sizes {
                for imgsz in input_sizes {
                    let key = format!("{task}-{size}-{imgsz}");
                    let entry = catalog
                        .models
                        .get(&key)
                        .unwrap_or_else(|| panic!("missing FP16 matrix entry {key}"));
                    assert!(!entry.enabled, "benchmark entry {key} must stay disabled");
                    assert!(entry.half, "benchmark entry {key} must be FP16");
                    assert_eq!(entry.imgsz, Some(imgsz));
                    assert!(entry.is_valid());
                }
            }
        }

        for size in sizes {
            for imgsz in [320, 640] {
                let key = format!("depth-{size}-{imgsz}");
                let entry = catalog
                    .models
                    .get(&key)
                    .unwrap_or_else(|| panic!("missing FP16 matrix entry {key}"));
                assert!(!entry.enabled, "benchmark entry {key} must stay disabled");
                assert!(entry.half, "benchmark entry {key} must be FP16");
                assert_eq!(entry.imgsz, Some(imgsz));
                assert!(entry.is_valid());
            }
        }
    }

    #[test]
    fn test_load_zone_catalog() {
        let catalog = load_zone_catalog(Path::new("config/zones.toml")).unwrap();
        assert!(catalog.zones.contains_key("bed"));
        assert_eq!(catalog.zones["bed"].hysteresis_ms, 500);
        assert_eq!(
            catalog.face_dwell.as_ref().map(ZoneEntry::rect),
            Some([760, 0, 1160, 300])
        );
    }

    #[test]
    fn test_load_fsm_catalog() {
        let catalog = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        assert_eq!(catalog.fsm.initial, "idle");
        assert_eq!(catalog.fsm.roles.safe, "blind");
        assert_eq!(catalog.fsm.roles.reset, "idle");
        assert!(catalog.fsm.states.contains_key("watching"));
        assert!(!catalog.fsm.transitions.is_empty());
    }

    #[test]
    fn test_load_app_config() {
        let config = load_app_config(Path::new("config/mana.toml")).unwrap();
        assert_mana_toml_core(&config);
        let model = load_model_catalog(Path::new("config/models.toml")).unwrap();
        assert_models_toml_core(&model);
    }

    /// El dedupe y la salud viven en secciones distintas de `mana.toml` y sus
    /// defaults tienen que ser coherentes entre sí: si la supresión pudiera
    /// durar tanto como la tolerancia de staleness, una escena inmóvil seguiría
    /// derivando en `data_stale` y el knob no evitaría nada.
    ///
    /// El bootstrap rechaza esa combinación; este test evita que los propios
    /// defaults del binario la produzcan al moverse por separado.
    #[test]
    fn default_dedup_window_stays_under_default_staleness() {
        let ingest = IngestConfig::default();
        let stale_ms = super::app::default_data_stale_ms_for_tests();
        assert!(
            ingest.dedup_max_suppress_ms < stale_ms,
            "dedup_max_suppress_ms ({}) debe quedar por debajo de data_stale_ms ({})",
            ingest.dedup_max_suppress_ms,
            stale_ms
        );
    }

    fn assert_mana_toml_core(config: &AppConfig) {
        assert_eq!(config.source.transport, "tcp");
        assert!(config.source.keyframes_only);
        assert!(config.presence.enabled);
        assert_eq!(config.presence.poi.off_ms, 16_000);
        assert_eq!(config.presence.occupancy.multiple_confirm_ms, 5_000);
        assert!(!config.presence.occupancy.require_confirmed_tracks);
        assert!(config.pipeline.track);
        assert!(config.pipeline.zones);
        assert!(config.pipeline.fsm);
        assert_eq!(config.detection.face_edge_margin_px, 32);
        assert!(config.perception.validation.is_valid());
        assert!(config.perception.body_parts.is_valid());
        assert_eq!(config.perception.body_parts.mode, BodyPartsMode::Validator);
        assert_eq!(config.face_pose.min_head_joints, 3);
        assert_eq!(config.face_pose.quality_joint_weight, 0.30);
        assert_eq!(config.perception.body_parts.segment_radius_ratio, 0.035);
        assert_eq!(
            config.inference.blueprint_file,
            Some(PathBuf::from(
                "config/blueprints/detect-room-face/blueprint.toml"
            ))
        );
        assert_eq!(
            config.inference.zones_file,
            Some(PathBuf::from("config/zones.toml"))
        );
        assert_eq!(
            config.inference.fsm_file,
            Some(PathBuf::from("config/blueprints/detect-room-face/fsm.toml"))
        );
        assert_eq!(config.health.data_stale_ms, 10_000);
        assert_eq!(config.health.stale_warn_ms, 5_000);
        assert_eq!(config.health.panic_window_cycles, 20);
        assert_eq!(config.health.max_panics_in_window, 3);
        assert_eq!(config.health.cycle_budget_ms, 500);
        assert_eq!(config.detection.same_class_iou, 0.5);
        assert_eq!(config.tracking.ghost_max_ms, 6_000);
        assert_eq!(config.tracking.nominal_dt_ms, 2_000);
        assert_eq!(config.tracking.noise.measurement, 1.0);
        assert_eq!(config.tracking.noise.process_position, 1.0);
        assert_eq!(config.tracking.noise.process_velocity, 0.25);
        assert!(config.health.is_valid());
        assert!(config.tracking.is_valid());
    }

    fn assert_models_toml_core(model: &ModelCatalog) {
        assert!(model.models["detect-fast"].postprocess.is_valid());
        assert_eq!(
            model.models["detect-fast"].postprocess.allow_classes,
            vec!["person", "wheelchair"]
        );
        assert_eq!(model.models["detect-fast"].postprocess.min_area_ratio, 0.01);
        assert_eq!(model.models["face-yolo"].postprocess.nms_iou, 0.005);
        assert_eq!(
            model.models["face-yolo"].postprocess.max_detections,
            Some(1)
        );
        assert_face_matrix_entries(model);
        assert_eq!(
            model.models["seg-standard"]
                .postprocess
                .min_component_area_ratio,
            0.05
        );
        assert_eq!(model.models["seg-standard"].polygon_simplify, 0.98);
        assert_eq!(model.models["seg-standard"].postprocess.mask_threshold, 0.5);
        assert_eq!(
            model.models["depth-standard"]
                .crop
                .as_ref()
                .and_then(|crop| crop.region),
            Some([560, 140, 1240, 820])
        );
        assert_eq!(
            model.models["depth-person-s-320"]
                .crop
                .as_ref()
                .map(|crop| crop.crop_type.clone()),
            Some(CropType::LargestClass)
        );
        assert_eq!(model.models["depth-person-s-320"].imgsz, Some(320));
        assert_eq!(model.models["depth-person-s-192"].imgsz, Some(192));
    }

    fn assert_face_matrix_entries(model: &ModelCatalog) {
        for version in [11, 12] {
            for size in ["s", "m", "l"] {
                for imgsz in [192, 320, 640] {
                    let key = format!("face-v{version}-{size}-{imgsz}");
                    let entry = model
                        .models
                        .get(&key)
                        .unwrap_or_else(|| panic!("missing face matrix entry {key}"));
                    assert!(!entry.enabled);
                    assert_eq!(entry.task, ModelTask::Detect);
                    assert_eq!(entry.imgsz, Some(imgsz));
                    assert!(entry.half);
                    assert_eq!(entry.postprocess.allow_classes, vec!["face".to_string()]);
                    assert!(entry.crop.is_some());
                }
            }
        }
    }

    #[test]
    fn app_config_rejects_unknown_health_fields() {
        let result = toml::from_str::<AppConfig>(
            r#"
                [source]
                url = "rtsp://example.invalid/stream"

                [inference]
                model_catalog = "config/models.toml"

                [health]
                max_consecutive_panics = 3
            "#,
        );

        assert!(
            result.is_err(),
            "unknown health fields must fail at startup"
        );
    }

    #[test]
    fn health_rejects_warning_threshold_at_or_above_blind_threshold() {
        let health: HealthConfig =
            toml::from_str("data_stale_ms = 10_000\nstale_warn_ms = 10_000").unwrap();
        assert!(!health.is_valid());
    }

    #[test]
    fn room_transition_metrics_profile_keeps_presence_and_scene_signals() {
        let config = load_metrics_log(Path::new("config/metrics-room-transition.toml")).unwrap();
        assert!(!config.metrics.jsonl.detection_events);
        assert!(config.metrics.jsonl.presence_events);
        assert!(config.metrics.jsonl.scene_signals_events);
        assert!(!config.metrics.jsonl.face_dwell_events);
        assert!(!config.metrics.jsonl.depth_events);
        assert!(!config.metrics.text.infer_summary);
    }

    #[test]
    fn face_dwell_metrics_profile_keeps_both_state_machines() {
        let config =
            load_metrics_log(Path::new("config/metrics-face-dwell-transition.toml")).unwrap();
        assert!(config.metrics.jsonl.presence_events);
        assert!(config.metrics.jsonl.face_dwell_events);
        assert!(config.metrics.jsonl.fsm_events);
        assert!(!config.metrics.jsonl.detection_events);
        assert!(!config.metrics.jsonl.depth_events);
    }

    #[test]
    fn fsm_validation_catches_unknown_model() {
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let fsm = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        let depth_rules = load_depth_rules(Path::new("config/depth-rules.toml")).unwrap();
        let errors = fsm_compile_errors(&fsm, &models, None, Some(&depth_rules));
        assert!(
            errors.is_empty(),
            "config/fsm.toml should be valid: {errors:?}"
        );
    }

    #[test]
    fn fsm_validation_requires_valid_roles_and_safe_exit() {
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let mut broken = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        broken.fsm.roles.safe = "ghost-safe".into();
        broken.fsm.roles.reset = "ghost-reset".into();

        let errors = fsm_compile_errors(&broken, &models, None, None);
        assert!(errors.iter().any(|e| e.contains("safe state 'ghost-safe'")));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("reset state 'ghost-reset'"))
        );

        let mut without_safe_exit = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        without_safe_exit.fsm.roles.safe = "idle".into();
        without_safe_exit
            .fsm
            .transitions
            .retain(|transition| transition.from != "idle");
        let errors = fsm_compile_errors(&without_safe_exit, &models, None, None);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("safe state 'idle' has no outgoing transition")),
            "missing safe exit was not rejected: {errors:?}"
        );
    }

    #[test]
    fn fsm_validation_catches_unknown_state() {
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let mut fsm = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        fsm.fsm.transitions.push(FsmTransition {
            from: "ghost".into(),
            to: "idle".into(),
            guards: vec![],
            dwell: None,
        });
        let errors = fsm_compile_errors(&fsm, &models, None, None);
        assert!(!errors.is_empty());
    }

    #[test]
    fn fsm_validation_catches_unknown_depth_rule() {
        let models = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let fsm = load_fsm_catalog(Path::new("config/fsm.toml")).unwrap();
        let rules = load_depth_rules(Path::new("config/depth-rules.toml")).unwrap();
        let mut broken = fsm.clone();
        broken.fsm.transitions.push(FsmTransition {
            from: "idle".into(),
            to: "watching".into(),
            guards: vec![FsmGuard::DepthRule {
                rule: "ghost-rule".into(),
                triggered: true,
            }],
            dwell: None,
        });
        let errors = fsm_compile_errors(&broken, &models, None, Some(&rules));
        assert!(
            errors.iter().any(|e| e.contains("depth rule 'ghost-rule'")),
            "expected unknown depth rule error: {errors:?}"
        );
    }
}
