use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::super::loader::load_config;
use super::super::models::{ModelCatalog, ModelEntry, ModelTask};
use super::patch::{ModelFile, ModelManifest, ModelPatch};
use crate::error::{ConfigError, Result};

/// Environment override for where the ONNX weights live.
///
/// The catalogs carry paths relative to the repo root
/// (`tools/model-tools/artifacts/...`), which only resolve when the process runs
/// from that root. The weights are gitignored and multi-GB, so a worktree, a CI
/// job or a deployment can't rely on that layout. Setting this repoints every
/// relative model path at a shared location; absolute paths are left alone.
pub const MODELS_HOME_ENV: &str = "MANA_MODELS_HOME";

pub fn load_model_catalog(path: &Path) -> Result<ModelCatalog> {
    let manifest: ModelManifest = load_config(path)?;
    if manifest.include.is_empty() {
        return Err(ConfigError::InvalidValue {
            field: "models.include".into(),
            msg: "the model manifest must include at least one file".into(),
        }
        .into());
    }

    let (base, base_profiles, task_files) = load_manifest_includes(path, manifest.include)?;
    let mut catalog = merge_task_files_into_catalog(base, base_profiles, task_files)?;
    rebase_model_paths(
        &mut catalog,
        std::env::var_os(MODELS_HOME_ENV).map(PathBuf::from),
    );
    validate_catalog_nonempty(&catalog)?;
    Ok(catalog)
}

/// Rebases relative model paths onto `models_home`, if set.
///
/// Absolute paths are deployment-pinned and stay untouched. With no override the
/// catalog is returned verbatim, so the default behaviour does not change.
pub(super) fn rebase_model_paths(catalog: &mut ModelCatalog, models_home: Option<PathBuf>) {
    let Some(home) = models_home else { return };
    for entry in catalog.models.values_mut() {
        if entry.path.is_relative() {
            entry.path = home.join(&entry.path);
        }
    }
}

fn load_manifest_includes(
    manifest_path: &Path,
    includes: Vec<PathBuf>,
) -> Result<(
    ModelPatch,
    HashMap<String, ModelPatch>,
    Vec<(ModelTask, ModelFile)>,
)> {
    let mut base = ModelPatch::default();
    let mut base_profiles = HashMap::new();
    let mut task_files = Vec::new();

    for include in includes {
        let include_path = resolve_include_path(manifest_path, include);
        let file: ModelFile = load_config(&include_path)?;
        if let Some(task) = file.task {
            task_files.push((task, file));
        } else {
            merge_shared_model_file(&include_path, file, &mut base, &mut base_profiles)?;
        }
    }

    Ok((base, base_profiles, task_files))
}

fn resolve_include_path(manifest_path: &Path, include: PathBuf) -> PathBuf {
    if include.is_absolute() {
        include
    } else {
        manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(include)
    }
}

fn merge_shared_model_file(
    include_path: &Path,
    file: ModelFile,
    base: &mut ModelPatch,
    base_profiles: &mut HashMap<String, ModelPatch>,
) -> Result<()> {
    if !file.models.is_empty() {
        return Err(ConfigError::InvalidValue {
            field: include_path.display().to_string(),
            msg: "a shared model file cannot define models without task".into(),
        }
        .into());
    }
    base.merge(&file.defaults);
    for (name, profile) in file.profiles {
        if base_profiles.insert(name.clone(), profile).is_some() {
            return Err(ConfigError::InvalidValue {
                field: format!("profiles.{name}"),
                msg: "duplicate shared model profile".into(),
            }
            .into());
        }
    }
    Ok(())
}

fn merge_task_files_into_catalog(
    base: ModelPatch,
    base_profiles: HashMap<String, ModelPatch>,
    task_files: Vec<(ModelTask, ModelFile)>,
) -> Result<ModelCatalog> {
    let mut catalog = ModelCatalog::default();
    let mut seen_tasks = HashMap::new();
    for (task, file) in task_files {
        if seen_tasks.insert(task, true).is_some() {
            return Err(ConfigError::InvalidValue {
                field: format!("task.{task}"),
                msg: "duplicate task file in model manifest".into(),
            }
            .into());
        }

        let mut defaults = base.clone();
        defaults.merge(&file.defaults);
        defaults.task = Some(task);

        let mut profiles = base_profiles.clone();
        profiles.extend(file.profiles);

        for (name, model) in file.models {
            let entry = resolve_model_entry(&name, task, &defaults, &profiles, &model)?;
            if catalog.models.insert(name.clone(), entry).is_some() {
                return Err(ConfigError::InvalidValue {
                    field: format!("models.{name}"),
                    msg: "duplicate model key across task files".into(),
                }
                .into());
            }
        }
    }
    Ok(catalog)
}

fn resolve_model_entry(
    name: &str,
    task: ModelTask,
    defaults: &ModelPatch,
    profiles: &HashMap<String, ModelPatch>,
    model: &ModelPatch,
) -> Result<ModelEntry> {
    let mut resolved = defaults.clone();
    if let Some(profile_name) = &model.profile {
        let Some(profile) = profiles.get(profile_name) else {
            return Err(ConfigError::InvalidValue {
                field: format!("models.{name}.profile"),
                msg: format!("unknown model profile '{profile_name}'"),
            }
            .into());
        };
        resolved.merge(profile);
    }
    resolved.merge(model);
    resolved.resolve(name, task)
}

fn validate_catalog_nonempty(catalog: &ModelCatalog) -> Result<()> {
    if catalog.models.is_empty() {
        return Err(ConfigError::InvalidValue {
            field: "models".into(),
            msg: "the model manifest resolved no models".into(),
        }
        .into());
    }
    Ok(())
}
