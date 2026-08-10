//! Stage 3: build perception engines (ROIs, infer, track, zones, cascade).

use std::collections::HashMap;

use crate::cascade::{CascadeRule, CascadeScheduler};
use crate::config::{AppConfig, CropType, load_config};
use crate::detection::CropRect;
use crate::domain::{ModelRegistry, ModelRole};
use crate::error::{ConfigError, ManaError, Result};
use crate::infer::InferEngine;
use crate::track::{Tracker, TrackerConfig};
#[cfg(feature = "rerun")]
use crate::viz::FixedRoi;
use crate::zones::ZoneEngine;

use super::validate::ValidatedBootstrap;

pub(super) struct PerceptionEngines {
    pub(super) infer: InferEngine,
    pub(super) models: ModelRegistry,
    pub(super) cascade: CascadeScheduler,
    pub(super) tracker: Option<Tracker>,
    pub(super) zone_engine: Option<ZoneEngine>,
    pub(super) depth_context_roi: Option<CropRect>,
    pub(super) face_dwell_roi: Option<CropRect>,
    pub(super) person_detection_roi: Option<CropRect>,
    #[cfg(feature = "rerun")]
    pub(super) fixed_rois: Vec<FixedRoi>,
}

/// Build inference, tracking, zones, cascade, and static ROI maps.
pub(super) fn build_perception_engines(
    config: &AppConfig,
    validated: &ValidatedBootstrap,
) -> Result<PerceptionEngines> {
    let rois = build_static_rois(validated);
    let infer = InferEngine::from_catalog(&validated.runtime_catalog)?;
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
        .then(|| validated.zones.as_ref().map(ZoneEngine::from_catalog))
        .flatten();
    let cascade = build_cascade(config, validated)?;
    let models =
        ModelRegistry::from_catalog(&validated.runtime_catalog, validated.primary_model.as_str());
    let person_detection_roi = models
        .first_with_role(ModelRole::Boxes)
        .and_then(|id| rois.static_roi_map.get(id.as_str()).copied());

    Ok(PerceptionEngines {
        infer,
        models,
        cascade,
        tracker,
        zone_engine,
        depth_context_roi: rois.depth_context_roi,
        face_dwell_roi: rois.face_dwell_roi,
        person_detection_roi,
        #[cfg(feature = "rerun")]
        fixed_rois: rois.fixed_rois,
    })
}

struct StaticRois {
    depth_context_roi: Option<CropRect>,
    face_dwell_roi: Option<CropRect>,
    static_roi_map: HashMap<String, CropRect>,
    #[cfg(feature = "rerun")]
    fixed_rois: Vec<FixedRoi>,
}

fn build_static_rois(validated: &ValidatedBootstrap) -> StaticRois {
    let depth_model =
        ModelRegistry::from_catalog(&validated.runtime_catalog, validated.primary_model.as_str())
            .first_with_role(ModelRole::DepthMap)
            .map(|id| id.as_str().to_owned());
    let depth_context_roi = depth_model
        .as_deref()
        .and_then(|key| validated.runtime_catalog.models.get(key))
        .and_then(|entry| entry.crop.as_ref())
        .filter(|crop| crop.crop_type == CropType::Static)
        .and_then(|crop| crop.region)
        .map(CropRect::from_array);

    let face_dwell_roi = validated
        .zones
        .as_ref()
        .and_then(|catalog| catalog.face_dwell.as_ref())
        .map(|entry| CropRect::from_array(entry.rect()));

    let mut static_roi_map: HashMap<String, CropRect> = validated
        .runtime_catalog
        .models
        .iter()
        .filter(|(_, entry)| entry.enabled)
        .filter_map(|(model, entry)| {
            entry
                .crop
                .as_ref()
                .filter(|crop| crop.crop_type == CropType::Static)
                .and_then(|crop| crop.region)
                .map(|region| (model.clone(), CropRect::from_array(region)))
        })
        .collect();
    if let Some(rect) = face_dwell_roi {
        static_roi_map.insert("face-dwell".into(), rect);
    }
    #[cfg(feature = "rerun")]
    let fixed_rois: Vec<FixedRoi> = {
        let mut fixed_rois: Vec<FixedRoi> = static_roi_map
            .iter()
            .map(|(model, rect)| FixedRoi {
                model: model.clone(),
                rect: *rect,
            })
            .collect();
        fixed_rois.sort_by(|a, b| a.model.cmp(&b.model));
        fixed_rois
    };

    StaticRois {
        depth_context_roi,
        face_dwell_roi,
        static_roi_map,
        #[cfg(feature = "rerun")]
        fixed_rois,
    }
}

fn build_cascade(config: &AppConfig, validated: &ValidatedBootstrap) -> Result<CascadeScheduler> {
    let model_enabled: HashMap<String, bool> = validated
        .runtime_catalog
        .models
        .iter()
        .map(|(name, entry)| (name.clone(), entry.enabled))
        .collect();
    let primary_model = &validated.primary_model;
    let (cascade_rules, cascade_regions) = if let Some(ref bp) = validated.blueprint {
        let cfg = crate::cascade::CascadeConfig {
            rules: bp.rules.clone(),
            regions: bp.regions.clone(),
        };
        let errors = cfg.validate(&model_enabled, primary_model);
        if !errors.is_empty() {
            return Err(ManaError::Config(ConfigError::InvalidValue {
                field: "inference.blueprint_file".into(),
                msg: errors.join("; "),
            }));
        }
        (cfg.rules, cfg.regions)
    } else if let Some(ref path) = config.inference.cascade_file {
        let cfg: crate::cascade::CascadeConfig = load_config(path)?;
        let errors = cfg.validate(&model_enabled, primary_model);
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
    Ok(CascadeScheduler::from_rules_and_regions(
        &cascade_rules,
        cascade_regions,
    ))
}
