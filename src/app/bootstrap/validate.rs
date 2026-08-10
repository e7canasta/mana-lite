//! Stage 2: validate runtime config, default model, and compile the FSM.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::config::{
    AppConfig, BlueprintConfig, FsmCatalog, MetricsLogConfig, ModelCatalog, ModelTask,
    RerunBlueprintConfig, VizDataConfig, ZoneCatalog,
};
use crate::error::{ConfigError, ManaError, Result};
use crate::fsm::FsmProgram;

use super::catalogs::LoadedCatalogs;

pub(super) struct ValidatedBootstrap {
    pub(super) runtime_catalog: ModelCatalog,
    pub(super) primary_model: String,
    pub(super) default_model_path: PathBuf,
    pub(super) default_model_task: ModelTask,
    pub(super) blueprint: Option<BlueprintConfig>,
    pub(super) zones: Option<ZoneCatalog>,
    pub(super) fsm_program: Option<FsmProgram>,
    pub(super) depth_rules: mana_control::DepthRules,
    pub(super) viz_data: VizDataConfig,
    pub(super) metrics_log: MetricsLogConfig,
    pub(super) rerun_blueprint: RerunBlueprintConfig,
}

/// Validate detection/health/tracking/presence before any catalog I/O.
pub(super) fn validate_app_config(config: &AppConfig) -> Result<()> {
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
    Ok(())
}

/// Validate the default model and compile the FSM program.
pub(super) fn validate_bootstrap(catalogs: LoadedCatalogs) -> Result<ValidatedBootstrap> {
    let LoadedCatalogs {
        runtime_catalog,
        primary_model,
        blueprint,
        zones,
        fsm,
        depth_rules,
        viz_data,
        metrics_log,
        rerun_blueprint,
    } = catalogs;

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
    let default_model_path = default_model.path.clone();
    let default_model_task = default_model.task;

    if let Some(ref z) = zones {
        log::info!("zones loaded: {} zones", z.zones.len());
    }
    let fsm_program = compile_fsm_program(&runtime_catalog, zones.as_ref(), fsm, &depth_rules)?;

    Ok(ValidatedBootstrap {
        runtime_catalog,
        primary_model,
        default_model_path,
        default_model_task,
        blueprint,
        zones,
        fsm_program,
        depth_rules,
        viz_data,
        metrics_log,
        rerun_blueprint,
    })
}

fn compile_fsm_program(
    runtime_catalog: &ModelCatalog,
    zones: Option<&ZoneCatalog>,
    fsm: Option<FsmCatalog>,
    depth_rules: &mana_control::DepthRules,
) -> Result<Option<FsmProgram>> {
    if let Some(ref f) = fsm {
        log::info!(
            "fsm loaded: {} states, {} transitions",
            f.fsm.states.len(),
            f.fsm.transitions.len()
        );
        let depth_rule_names: HashSet<String> = depth_rules
            .rules
            .iter()
            .map(|rule| rule.name.clone())
            .collect();
        let model_names: HashSet<String> = runtime_catalog.models.keys().cloned().collect();
        let program = FsmProgram::compile_with_references(
            f,
            zones,
            Some(&model_names),
            Some(&depth_rule_names),
        );
        match program {
            Ok(program) => Ok(Some(program)),
            Err(errors) => {
                for e in &errors {
                    log::error!("fsm validation: {e}");
                }
                Err(ManaError::FsmGuardError(format!(
                    "{} FSM validation errors",
                    errors.len()
                )))
            }
        }
    } else {
        Ok(None)
    }
}
