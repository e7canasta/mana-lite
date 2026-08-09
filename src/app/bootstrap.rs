//! Application bootstrap: load catalogs, validate, wire engines.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::cascade::{BlueprintConfig, CascadeRule, CascadeScheduler};
use crate::config::{
    AppConfig, CropType, MetricsLogConfig, RerunBlueprintConfig, apply_model_overlay, load_config,
    load_depth_rules, load_fsm_catalog, load_metrics_log, load_model_catalog, load_rerun_blueprint,
    load_viz_data, load_zone_catalog, validate_model_catalog, ZoneCatalog,
};
use crate::detection::CropRect;
use crate::detection::DetectionConsolidator;
use crate::domain::{ModelRegistry, ModelRole};
use crate::error::{ConfigError, ManaError, Result};
use crate::face_dwell::FaceDwellLogStrategy;
use crate::fsm::{FsmEngine, FsmProgram, FsmSceneContext};
use crate::health::Health;
use crate::infer::InferEngine;
use crate::ingest::{FrameReader, IngestEngine, RetinaReader};
use crate::logger::{Event, JsonlLevel, LogManager, LogSink};
use crate::metrics::MetricsEngine;
use crate::pipeline::PipelineState;
use crate::scan::{ControlPolicy, ControlState};
use crate::snapshot::{FrameDecoder, SnapshotSaver};
use crate::track::{Tracker, TrackerConfig};
use crate::viz::{FixedRoi, VizBridge};
use crate::zones::ZoneEngine;

use super::{App, FanoutObserver, VERSION};

impl App<RetinaReader> {
    pub async fn bootstrap(config: &AppConfig, config_path: &std::path::Path) -> Result<Self> {
        let reader = RetinaReader::connect(
            &config.source.url,
            config.source.username.as_deref(),
            config.source.password.as_deref(),
            &config.source.transport,
            &config.ingest,
        )
        .await?;
        Self::bootstrap_with_reader(config, config_path, reader).await
    }
}

impl<R: FrameReader> App<R> {
    pub async fn bootstrap_with_reader(
        config: &AppConfig,
        config_path: &std::path::Path,
        reader: R,
    ) -> Result<Self> {
        if !config.detection.face_component_coverage.is_finite()
            || !config.detection.face_max_center_y_ratio.is_finite()
            || !config.detection.same_class_iou.is_finite()
            || !(0.0..=1.0).contains(&config.detection.face_component_coverage)
            || !(0.0..=1.0).contains(&config.detection.face_max_center_y_ratio)
            || !(0.0..=1.0).contains(&config.detection.same_class_iou)
        {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: "detection".into(),
                msg: "association ratios must be within 0..=1".into(),
            }));
        }
        if !config.health.is_valid() {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: "health.stale_warn_ms".into(),
                msg: "must be less than health.data_stale_ms".into(),
            }));
        }
        if !config.tracking.is_valid() {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: "tracking".into(),
                msg: "ghost/nominal intervals and noise scales must be positive and finite".into(),
            }));
        }
        if !config.presence.is_valid() {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: "presence".into(),
                msg: "class must be non-empty and presence policies must be positive".into(),
            }));
        }
        let model_catalog = load_model_catalog(&config.inference.model_catalog)?;
        let model_errors = validate_model_catalog(&model_catalog);
        if !model_errors.is_empty() {
            return Err(ManaError::Config(ConfigError::ValidationError(
                model_errors.join("; "),
            )));
        }
        let blueprint = config
            .inference
            .blueprint_file
            .as_ref()
            .map(|path| load_config::<BlueprintConfig>(path))
            .transpose()?;
        if let Some(ref bp) = blueprint {
            log::info!(
                "blueprint: {}{}",
                bp.blueprint.name,
                bp.blueprint
                    .description
                    .as_deref()
                    .map(|description| format!(" — {description}"))
                    .unwrap_or_default()
            );
        }

        let primary_model = blueprint
            .as_ref()
            .map(|bp| bp.blueprint.primary_model.clone())
            .or_else(|| config.inference.default_model.clone())
            .ok_or_else(|| {
                ManaError::Config(ConfigError::InvalidValue {
                    field: "inference.primary_model".into(),
                    msg: "set blueprint.primary_model or inference.default_model".into(),
                })
            })?;

        let mut runtime_catalog = model_catalog.clone();
        if let Some(ref bp) = blueprint {
            if let Some(overlay) = &bp.blueprint.model_overlay {
                let blueprint_path = config.inference.blueprint_file.as_ref().ok_or_else(|| {
                    ManaError::Config(ConfigError::InvalidValue {
                        field: "blueprint.model_overlay".into(),
                        msg: "a model overlay requires a blueprint file path".into(),
                    })
                })?;
                let overlay_path = if overlay.is_absolute() {
                    overlay.clone()
                } else {
                    blueprint_path
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .join(overlay)
                };
                let overridden = apply_model_overlay(
                    &mut runtime_catalog,
                    &overlay_path,
                    &config.inference.model_catalog,
                )?;
                log::info!(
                    "model overlay: {} extends {} (overrides: {:?})",
                    overlay_path.display(),
                    config.inference.model_catalog.display(),
                    overridden
                );
            }
            let selected: HashSet<&str> = bp.blueprint.models.iter().map(String::as_str).collect();
            if selected.is_empty() {
                return Err(ManaError::Config(ConfigError::InvalidValue {
                    field: "blueprint.models".into(),
                    msg: "a blueprint must select at least one model".into(),
                }));
            }
            if !selected.contains(primary_model.as_str()) {
                return Err(ManaError::Config(ConfigError::InvalidValue {
                    field: "blueprint.primary_model".into(),
                    msg: "primary_model must be included in blueprint.models".into(),
                }));
            }
            for model in &selected {
                if !model_catalog.models.contains_key(*model) {
                    return Err(ManaError::ModelNotFound((*model).into()));
                }
            }
            for rule in &bp.rules {
                if !selected.contains(rule.model.as_str()) {
                    return Err(ManaError::Config(ConfigError::InvalidValue {
                        field: format!("blueprint.rules.{}", rule.model),
                        msg: "rule model must be listed in blueprint.models".into(),
                    }));
                }
                if let Some(parent) = rule.requires.as_deref() {
                    if !selected.contains(parent) {
                        return Err(ManaError::Config(ConfigError::InvalidValue {
                            field: format!("blueprint.rules.{}", rule.model),
                            msg: "rule parent must be listed in blueprint.models".into(),
                        }));
                    }
                }
            }
            if bp.blueprint.requires_tracking && !config.pipeline.track {
                return Err(ManaError::Config(ConfigError::InvalidValue {
                    field: "pipeline.track".into(),
                    msg: format!("blueprint '{}' requires tracking", bp.blueprint.name),
                }));
            }
            for (key, entry) in &mut runtime_catalog.models {
                entry.enabled = selected.contains(key.as_str());
            }
        }
        let runtime_model_errors = validate_model_catalog(&runtime_catalog);
        if !runtime_model_errors.is_empty() {
            return Err(ManaError::Config(ConfigError::ValidationError(
                runtime_model_errors.join("; "),
            )));
        }

        let zones = config
            .inference
            .zones_file
            .as_ref()
            .map(|p| load_zone_catalog(p))
            .transpose()?;
        let fsm = config
            .inference
            .fsm_file
            .as_ref()
            .map(|p| load_fsm_catalog(p))
            .transpose()?;
        let depth_rules = config
            .inference
            .depth_rules_file
            .as_ref()
            .map(|p| load_depth_rules(p))
            .transpose()?
            .unwrap_or_default();
        let depth_rule_errors = depth_rules.validate();
        if !depth_rule_errors.is_empty() {
            return Err(ManaError::Config(ConfigError::ValidationError(
                depth_rule_errors.join("; "),
            )));
        }

        let viz_data = config
            .viz_file
            .as_ref()
            .map(|p| load_viz_data(p))
            .transpose()?
            .unwrap_or_default();
        let metrics_log: MetricsLogConfig = config
            .metrics_file
            .as_ref()
            .map(|p| load_metrics_log(p))
            .transpose()?
            .unwrap_or_default();
        let rerun_blueprint = config
            .rerun_file
            .as_ref()
            .map(|p| load_rerun_blueprint(p))
            .transpose()?
            .unwrap_or_else(RerunBlueprintConfig::default);

        let default_model = runtime_catalog
            .models
            .get(&primary_model)
            .ok_or_else(|| ManaError::ModelNotFound(primary_model.clone()))?;
        if !default_model.enabled {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: format!("models.{primary_model}"),
                msg: "default model must be enabled".into(),
            }));
        }
        log::info!(
            "default model: {} ({})",
            primary_model,
            default_model.path.display()
        );

        if let Some(ref z) = zones {
            log::info!("zones loaded: {} zones", z.zones.len());
        }
        let fsm_program = if let Some(ref f) = fsm {
            log::info!(
                "fsm loaded: {} states, {} transitions",
                f.fsm.states.len(),
                f.fsm.transitions.len()
            );
            let depth_rule_names: std::collections::HashSet<String> = depth_rules
                .rules
                .iter()
                .map(|rule| rule.name.clone())
                .collect();
            let program = FsmProgram::compile_with_references(
                f,
                zones.as_ref(),
                &runtime_catalog,
                Some(&depth_rule_names),
            );
            match program {
                Ok(program) => Some(program),
                Err(errors) => {
                    for e in &errors {
                        log::error!("fsm validation: {e}");
                    }
                    return Err(ManaError::FsmGuardError(format!(
                        "{} FSM validation errors",
                        errors.len()
                    )));
                }
            }
        } else {
            None
        };

        let jsonl_level = JsonlLevel::from_str(&config.output.jsonl_level);
        let mut log = if let Some(ref dir) = config.output.save_dir {
            LogManager::rotating(dir.clone(), &config.output.rotate, jsonl_level)?
        } else {
            LogManager::new(jsonl_level)
        };
        log.set_jsonl_config(metrics_log.metrics.jsonl.clone());
        let mut log: Box<dyn LogSink> = Box::new(log);

        log::info!(
            "mana-lite v{VERSION} starting (output {})",
            config.output.format
        );
        log::info!("source: {}", config.source.url);
        log.emit(Event::meta_startup(
            VERSION,
            &config_path.display().to_string(),
        ));
        log.emit(Event::meta_model_loaded(
            &primary_model,
            &default_model.path.display().to_string(),
            default_model.task.as_str(),
            0,
        ));

        let depth_model = ModelRegistry::from_catalog(&runtime_catalog, primary_model.as_str())
            .first_with_role(ModelRole::DepthMap)
            .map(|id| id.as_str().to_owned());
        let depth_context_roi = depth_model
            .as_deref()
            .and_then(|key| runtime_catalog.models.get(key))
            .and_then(|entry| entry.crop.as_ref())
            .filter(|crop| crop.crop_type == CropType::Static)
            .and_then(|crop| crop.region)
            .map(CropRect::from_array);

        let face_dwell_roi = zones
            .as_ref()
            .and_then(|catalog| catalog.face_dwell.as_ref())
            .map(|entry| CropRect::from_array(entry.rect()));

        let mut fixed_rois: Vec<FixedRoi> = runtime_catalog
            .models
            .iter()
            .filter(|(_, entry)| entry.enabled)
            .filter_map(|(model, entry)| {
                entry
                    .crop
                    .as_ref()
                    .filter(|crop| crop.crop_type == CropType::Static)
                    .and_then(|crop| crop.region)
                    .map(|region| FixedRoi {
                        model: model.clone(),
                        rect: CropRect::from_array(region),
                    })
            })
            .collect();
        if let Some(rect) = face_dwell_roi {
            fixed_rois.push(FixedRoi {
                model: "face-dwell".into(),
                rect,
            });
        }
        fixed_rois.sort_by(|a, b| a.model.cmp(&b.model));
        let static_roi_map: HashMap<String, CropRect> = fixed_rois
            .iter()
            .map(|roi| (roi.model.clone(), roi.rect))
            .collect();

        let infer = InferEngine::from_catalog(&runtime_catalog)?;
        log::info!("inference: {} model(s) loaded", infer.model_count());
        ultralytics_inference::logging::set_verbose(false);

        let tracker = Tracker::with_config(TrackerConfig {
            min_hits: config.tracking.min_hits,
            max_age_ms: config.tracking.max_age_ms,
            tentative_max_age_ms: config.tracking.tentative_max_age_ms,
            iou_threshold: config.tracking.iou_threshold,
            mahalanobis_threshold: config.tracking.mahalanobis_threshold,
            ghost_max_ms: config.tracking.ghost_max_ms,
            nominal_dt_ms: config.tracking.nominal_dt_ms,
            measurement_noise: config.tracking.noise.measurement,
            process_position_noise: config.tracking.noise.process_position,
            process_velocity_noise: config.tracking.noise.process_velocity,
        });
        let tracker = config.pipeline.track.then_some(tracker);
        let zone_engine = config
            .pipeline
            .zones
            .then(|| zones.as_ref().map(ZoneEngine::from_catalog))
            .flatten();
        let fsm_engine = config
            .pipeline
            .fsm
            .then(|| fsm_program.map(FsmEngine::from_program))
            .flatten();
        let (cascade_rules, cascade_regions) = if let Some(ref bp) = blueprint {
            let cfg = crate::cascade::CascadeConfig {
                rules: bp.rules.clone(),
                regions: bp.regions.clone(),
            };
            let errors = cfg.validate(&runtime_catalog, &primary_model);
            if !errors.is_empty() {
                return Err(ManaError::Config(ConfigError::InvalidValue {
                    field: "inference.blueprint_file".into(),
                    msg: errors.join("; "),
                }));
            }
            (cfg.rules, cfg.regions)
        } else if let Some(ref path) = config.inference.cascade_file {
            let cfg: crate::cascade::CascadeConfig = load_config(path)?;
            let errors = cfg.validate(&runtime_catalog, &primary_model);
            if !errors.is_empty() {
                return Err(ManaError::Config(ConfigError::InvalidValue {
                    field: "inference.cascade_file".into(),
                    msg: errors.join("; "),
                }));
            }
            (cfg.rules, cfg.regions)
        } else {
            (
                vec![CascadeRule {
                    model: primary_model.clone(),
                    requires: None,
                    requires_class: None,
                    requires_exact_count: None,
                    same_frame: false,
                    requires_min_confidence: None,
                    requires_min_area_ratio: None,
                    requires_region: None,
                    requires_region_coverage: None,
                }],
                HashMap::new(),
            )
        };
        let cascade = CascadeScheduler::from_rules_and_regions(&cascade_rules, cascade_regions);
        let models = ModelRegistry::from_catalog(&runtime_catalog, primary_model.as_str());

        let ingest = IngestEngine::new(reader);

        let metrics = MetricsEngine::new(
            metrics_log.metrics.report_interval_s,
            config.health.cycle_budget_ms,
        );
        let health = Health::new(config.health.data_stale_ms, config.health.stale_warn_ms);
        let decoder = FrameDecoder::new()?;
        let snapshots = SnapshotSaver::new(
            config.output.snapshot_dir.clone(),
            config.output.snapshot_verbose,
        )?;

        let viz = if config.viz.enabled {
            log::info!(
                "viz: will connect to rerun at {} when viewer opens",
                config.viz.rerun_addr
            );
            VizBridge::new(
                &config.viz.rerun_addr,
                &viz_data.viz.send,
                &rerun_blueprint.rerun,
                fixed_rois,
                &models,
            )
        } else {
            VizBridge::disabled()
        };

        let state = PipelineState::new(
            metrics_log.metrics.text.clone(),
            config.health.panic_window_cycles,
            config.health.max_panics_in_window,
        );

        // Ancla del reloj: una sola lectura de pared y una de monotónico, en
        // el mismo punto. Todo timestamp del proceso sale de aqui + delta del
        // monotónico; un salto de NTP no puede desordenar ni duplicar el JSONL.
        // Re-anclar en cada rotación horaria del log queda deliberadamente en
        // pendiente: la deriva de ppm del monotónico es despreciable frente a
        // la rotación del sistema, y un re-anclaje mal hecho reabriría el salto.
        let boot_wall = chrono::Utc::now();
        let boot_instant = Instant::now();
        let person_detection_roi = models
            .first_with_role(ModelRole::Boxes)
            .and_then(|id| static_roi_map.get(id.as_str()).copied());

        Ok(Self {
            infer,
            primary_model,
            models,
            control: ControlState {
                tracker,
                zone_engine,
                fsm_engine,
                health,
                presence: crate::presence::PresenceFilter::new(
                    config.presence.enabled,
                    config.presence.class.clone(),
                    mana_control::config::PresencePoiPolicy { on_ms: config.presence.poi.on_ms, off_ms: config.presence.poi.off_ms },
                ),
                occupancy: crate::occupancy::OccupancyStateMachine::new(
                    mana_control::config::OccupancyPolicy { single_confirm_ms: config.presence.occupancy.single_confirm_ms, empty_confirm_ms: config.presence.occupancy.empty_confirm_ms, multiple_confirm_ms: config.presence.occupancy.multiple_confirm_ms, multiple_exit_ms: config.presence.occupancy.multiple_exit_ms, require_confirmed_tracks: config.presence.occupancy.require_confirmed_tracks },
                ),
                fsm_context: FsmSceneContext::default(),
                last_scan_at: boot_instant,
                scan_seq: 0,
                policy: ControlPolicy {
                    person_class: config.presence.class.clone(),
                    presence_enabled: config.presence.enabled,
                    data_stale_ms: config.health.data_stale_ms,
                    scan_period_ms: config.scan.period_ms,
                    face_dwell_roi: face_dwell_roi.map(|x| x.to_array()),
                    person_detection_roi: person_detection_roi.map(|x| x.to_array()),
                    face_edge_margin_px: config.detection.face_edge_margin_px,
                },
            },
            cascade,
            detection_consolidator: DetectionConsolidator::new(
                config.detection.face_component_coverage,
                config.detection.face_max_center_y_ratio,
                config.detection.same_class_iou,
            ),
            ingest,
            metrics,
            depth_context_roi,
            depth_rules,
            decoder,
            snapshots,
            observer: FanoutObserver::new(viz, log),
            state,
            boot_wall,
            boot_instant,
            crop_frames_pending: Vec::new(),
            face_dwell_logger: FaceDwellLogStrategy,
            control_image: mana_control::ProcessImage::empty(),
        })
    }
}
