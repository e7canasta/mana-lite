use std::fs;
use std::path::Path;

use super::super::loader::load_config;
use super::super::models::ModelCatalog;
use super::patch::ModelOverlay;
use crate::error::{ConfigError, Result};

pub fn apply_model_overlay(
    catalog: &mut ModelCatalog,
    overlay_path: &Path,
    base_path: &Path,
) -> Result<Vec<String>> {
    let overlay: ModelOverlay = load_config(overlay_path)?;
    let declared_parent = if overlay.extends.is_absolute() {
        overlay.extends
    } else {
        overlay_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(overlay.extends)
    };
    let declared_parent = fs::canonicalize(&declared_parent)
        .map_err(|_| ConfigError::FileNotFound(declared_parent.display().to_string()))?;
    let base_path = fs::canonicalize(base_path)
        .map_err(|_| ConfigError::FileNotFound(base_path.display().to_string()))?;
    if declared_parent != base_path {
        return Err(ConfigError::InvalidValue {
            field: format!("{}::extends", overlay_path.display()),
            msg: format!(
                "overlay parent must resolve to the configured base catalog {}",
                base_path.display()
            ),
        }
        .into());
    }

    let mut applied = Vec::with_capacity(overlay.models.len());
    for (name, patch) in overlay.models {
        let Some(entry) = catalog.models.get_mut(&name) else {
            return Err(ConfigError::InvalidValue {
                field: format!("{}::models.{name}", overlay_path.display()),
                msg: "blueprint overlays may only override existing catalog models".into(),
            }
            .into());
        };
        patch.apply_overlay(&name, entry)?;
        applied.push(name);
    }
    applied.sort();
    Ok(applied)
}
