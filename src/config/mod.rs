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

pub use app::{
    AppConfig, DetectionConfig, HealthConfig, InferenceConfig, IngestConfig,
    MANA_TOML_SCHEMA_VERSION, OccupancyPolicy, OutputConfig, PipelineConfig, PresenceConfig,
    PresencePoiPolicy, Rotate, ScanConfigSection, SourceConfig, TrackingConfig,
    TrackingNoiseConfig, VizConfig,
};
pub use blueprint::{
    BlueprintConfig, BlueprintMetadata, CascadeConfig, CascadeRule, SemanticRegion,
};
pub use fsm::{FsmCatalog, FsmRoles, FsmRoot, FsmState, FsmTransition};
pub use crate::fsm::FsmGuard;
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
        depth_rules: Option<&crate::depth::DepthRules>,
    ) -> Vec<String> {
        let names = depth_rules.map(|rules| {
            rules
                .rules
                .iter()
                .map(|rule| rule.name.clone())
                .collect::<std::collections::HashSet<_>>()
        });
        FsmProgram::compile_with_references(fsm, zones, models, names.as_ref())
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
                transition
                    .guards
                    .iter()
                    .any(|guard| matches!(guard, FsmGuard::FaceNotAtEdge)),
                "{from}->{to} must preserve edge priority"
            );
        }
        let edge_to_other = fsm
            .fsm
            .transitions
            .iter()
            .find(|transition| transition.from == "edge" && transition.to == "other")
            .expect("missing transition edge->other");
        assert!(
            edge_to_other
                .guards
                .iter()
                .any(|guard| matches!(guard, FsmGuard::FaceNotAtEdge))
        );
    }

    #[test]
    fn test_load_fp16_benchmark_matrix() {
        let catalog = load_model_catalog(Path::new("config/models.toml")).unwrap();
        let tasks = ["detect", "pose", "seg", "depth"];
        let sizes = ["s", "m", "l", "x"];
        let input_sizes = [320, 640];

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
        let model = load_model_catalog(Path::new("config/models.toml")).unwrap();
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
        for version in [11, 12] {
            for size in ["s", "m", "l"] {
                for imgsz in [320, 640] {
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
    fn room_transition_metrics_profile_only_keeps_presence_events() {
        let config = load_metrics_log(Path::new("config/metrics-room-transition.toml")).unwrap();
        assert!(!config.metrics.jsonl.detection_events);
        assert!(config.metrics.jsonl.presence_events);
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
