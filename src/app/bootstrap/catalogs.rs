//! Stage 1: load catalogs from disk and resolve the runtime model set.

use std::collections::HashSet;

use crate::config::{
    AppConfig, BlueprintConfig, FsmCatalog, MetricsLogConfig, ModelCatalog, RerunBlueprintConfig,
    VizDataConfig, ZoneCatalog, apply_model_overlay, load_config, load_depth_rules,
    load_fsm_catalog, load_metrics_log, load_model_catalog, load_rerun_blueprint,
    load_surface_calibration, load_viz_data, load_zone_catalog, validate_model_catalog,
};
use crate::error::{ConfigError, ManaError, Result};

pub(super) struct LoadedCatalogs {
    pub(super) runtime_catalog: ModelCatalog,
    pub(super) primary_model: String,
    pub(super) blueprint: Option<BlueprintConfig>,
    pub(super) zones: Option<ZoneCatalog>,
    pub(super) fsm: Option<FsmCatalog>,
    pub(super) depth_rules: mana_control::DepthRules,
    pub(super) surface_calibration: Option<mana_perception::SurfaceCalibration>,
    pub(super) viz_data: VizDataConfig,
    pub(super) metrics_log: MetricsLogConfig,
    pub(super) rerun_blueprint: RerunBlueprintConfig,
}

/// Load model/blueprint/sidecar catalogs and resolve the enabled runtime set.
pub(super) fn load_catalogs(config: &AppConfig) -> Result<LoadedCatalogs> {
    let (runtime_catalog, primary_model, blueprint) = load_runtime_models(config)?;
    let (zones, fsm, depth_rules, surface_calibration) = load_control_sidecars(config)?;
    let (viz_data, metrics_log, rerun_blueprint) = load_observability_sidecars(config)?;
    Ok(LoadedCatalogs {
        runtime_catalog,
        primary_model,
        blueprint,
        zones,
        fsm,
        depth_rules,
        surface_calibration,
        viz_data,
        metrics_log,
        rerun_blueprint,
    })
}

fn load_runtime_models(
    config: &AppConfig,
) -> Result<(ModelCatalog, String, Option<BlueprintConfig>)> {
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

    let runtime_catalog =
        apply_blueprint_to_catalog(config, &model_catalog, &primary_model, blueprint.as_ref())?;
    Ok((runtime_catalog, primary_model, blueprint))
}

fn apply_blueprint_to_catalog(
    config: &AppConfig,
    model_catalog: &ModelCatalog,
    primary_model: &str,
    blueprint: Option<&BlueprintConfig>,
) -> Result<ModelCatalog> {
    let mut runtime_catalog = model_catalog.clone();
    if let Some(bp) = blueprint {
        apply_optional_overlay(config, &mut runtime_catalog, bp)?;
        let selected: HashSet<&str> = bp.blueprint.models.iter().map(String::as_str).collect();
        validate_blueprint_selection(config, model_catalog, primary_model, bp, &selected)?;
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
    Ok(runtime_catalog)
}

fn apply_optional_overlay(
    config: &AppConfig,
    runtime_catalog: &mut ModelCatalog,
    bp: &BlueprintConfig,
) -> Result<()> {
    let Some(overlay) = &bp.blueprint.model_overlay else {
        return Ok(());
    };
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
        runtime_catalog,
        &overlay_path,
        &config.inference.model_catalog,
    )?;
    log::info!(
        "model overlay: {} extends {} (overrides: {:?})",
        overlay_path.display(),
        config.inference.model_catalog.display(),
        overridden
    );
    Ok(())
}

fn validate_blueprint_selection(
    config: &AppConfig,
    model_catalog: &ModelCatalog,
    primary_model: &str,
    bp: &BlueprintConfig,
    selected: &HashSet<&str>,
) -> Result<()> {
    if selected.is_empty() {
        return Err(ManaError::Config(ConfigError::InvalidValue {
            field: "blueprint.models".into(),
            msg: "a blueprint must select at least one model".into(),
        }));
    }
    if !selected.contains(primary_model) {
        return Err(ManaError::Config(ConfigError::InvalidValue {
            field: "blueprint.primary_model".into(),
            msg: "primary_model must be included in blueprint.models".into(),
        }));
    }
    for model in selected {
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
    // Un dedupe que puede suprimir más de lo que la salud tolera no gobierna
    // nada: la escena inmóvil sigue derivando en `data_stale` y el knob mentiría
    // sobre lo que evita.
    if config.ingest.dedup_max_suppress_ms >= config.health.data_stale_ms {
        return Err(ManaError::Config(ConfigError::InvalidValue {
            field: "ingest.dedup_max_suppress_ms".into(),
            msg: format!(
                "must stay below health.data_stale_ms ({} >= {}); \
                 otherwise a still scene still reaches data_stale and the knob prevents nothing. \
                 Set it to about half of data_stale_ms",
                config.ingest.dedup_max_suppress_ms, config.health.data_stale_ms
            ),
        }));
    }
    Ok(())
}

fn load_control_sidecars(
    config: &AppConfig,
) -> Result<(
    Option<ZoneCatalog>,
    Option<FsmCatalog>,
    mana_control::DepthRules,
    Option<mana_perception::SurfaceCalibration>,
)> {
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
    let surface_calibration = config
        .inference
        .depth_calibration_file
        .as_ref()
        .map(|path| load_surface_calibration(path))
        .transpose()?;
    Ok((zones, fsm, depth_rules, surface_calibration))
}

fn load_observability_sidecars(
    config: &AppConfig,
) -> Result<(VizDataConfig, MetricsLogConfig, RerunBlueprintConfig)> {
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
    Ok((viz_data, metrics_log, rerun_blueprint))
}
