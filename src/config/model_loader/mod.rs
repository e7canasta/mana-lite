use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::loader::load_config;
use super::models::{
    CropConfig, CropType, FallbackMode, ModelCatalog, ModelEntry, ModelTask, PostprocessConfig,
    default_confidence, default_crop_margin, default_device, default_iou, default_max_det,
    default_polygon_simplify, default_postprocess_mask_threshold,
    default_postprocess_max_area_ratio, default_postprocess_min_area_ratio,
    default_postprocess_min_component_area_ratio, default_postprocess_min_confidence,
    default_postprocess_nms_iou, default_rect,
};
use crate::error::{ConfigError, Result};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelManifest {
    include: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelFile {
    #[serde(default)]
    task: Option<ModelTask>,
    #[serde(default)]
    defaults: ModelPatch,
    #[serde(default)]
    profiles: HashMap<String, ModelPatch>,
    #[serde(default)]
    models: HashMap<String, ModelPatch>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelOverlay {
    extends: PathBuf,
    #[serde(default)]
    models: HashMap<String, ModelPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelPatch {
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    task: Option<ModelTask>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    iou: Option<f32>,
    #[serde(default)]
    max_det: Option<u32>,
    #[serde(default)]
    imgsz: Option<u32>,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    half: Option<bool>,
    #[serde(default)]
    rect: Option<bool>,
    #[serde(default)]
    polygon_simplify: Option<f64>,
    #[serde(default)]
    postprocess: Option<PostprocessPatch>,
    #[serde(default)]
    crop: Option<CropPatchValue>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PostprocessPatch {
    #[serde(default)]
    allow_classes: Option<Vec<String>>,
    #[serde(default)]
    min_confidence: Option<f32>,
    #[serde(default)]
    min_area_ratio: Option<f32>,
    #[serde(default)]
    max_area_ratio: Option<f32>,
    #[serde(default)]
    min_component_area_ratio: Option<f32>,
    #[serde(default)]
    mask_threshold: Option<f32>,
    #[serde(default)]
    nms_iou: Option<f32>,
    #[serde(default)]
    max_detections: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum CropPatchValue {
    Disabled(bool),
    Values(CropPatch),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CropPatch {
    #[serde(rename = "type", default)]
    crop_type: Option<CropType>,
    #[serde(default)]
    class: Option<String>,
    #[serde(default)]
    margin: Option<f32>,
    #[serde(default)]
    region: Option<[u32; 4]>,
    #[serde(default)]
    min_region: Option<[u32; 4]>,
    #[serde(default)]
    max_region: Option<[u32; 4]>,
    #[serde(default)]
    square_size: Option<u32>,
    #[serde(default)]
    upper_fraction: Option<f32>,
    #[serde(default)]
    fallback: Option<FallbackMode>,
}

impl ModelPatch {
    fn merge(&mut self, child: &Self) {
        if child.path.is_some() {
            self.path.clone_from(&child.path);
        }
        if child.task.is_some() {
            self.task = child.task;
        }
        if child.profile.is_some() {
            self.profile.clone_from(&child.profile);
        }
        if child.enabled.is_some() {
            self.enabled = child.enabled;
        }
        if child.confidence.is_some() {
            self.confidence = child.confidence;
        }
        if child.iou.is_some() {
            self.iou = child.iou;
        }
        if child.max_det.is_some() {
            self.max_det = child.max_det;
        }
        if child.imgsz.is_some() {
            self.imgsz = child.imgsz;
        }
        if child.device.is_some() {
            self.device.clone_from(&child.device);
        }
        if child.half.is_some() {
            self.half = child.half;
        }
        if child.rect.is_some() {
            self.rect = child.rect;
        }
        if child.polygon_simplify.is_some() {
            self.polygon_simplify = child.polygon_simplify;
        }
        if let Some(postprocess) = &child.postprocess {
            self.postprocess
                .get_or_insert_with(PostprocessPatch::default)
                .merge(postprocess);
        }
        if let Some(crop) = &child.crop {
            self.crop = Some(match (self.crop.take(), crop) {
                (Some(CropPatchValue::Values(mut base)), CropPatchValue::Values(child)) => {
                    base.merge(child);
                    CropPatchValue::Values(base)
                }
                (_, child) => child.clone(),
            });
        }
    }

    fn resolve(&self, name: &str, task: ModelTask) -> Result<ModelEntry> {
        let Some(path) = self.path.clone() else {
            return Err(ConfigError::InvalidValue {
                field: format!("models.{name}.path"),
                msg: "a resolved model must define path".into(),
            }
            .into());
        };
        if let Some(declared_task) = self.task
            && declared_task != task
        {
            return Err(ConfigError::InvalidValue {
                field: format!("models.{name}.task"),
                msg: format!("model task {declared_task} conflicts with task file {task}"),
            }
            .into());
        }

        let postprocess = self
            .postprocess
            .as_ref()
            .map(PostprocessPatch::resolve)
            .unwrap_or_default();
        let crop = match self.crop.as_ref() {
            None | Some(CropPatchValue::Disabled(false)) => None,
            Some(CropPatchValue::Disabled(true)) => {
                return Err(ConfigError::InvalidValue {
                    field: format!("models.{name}.crop"),
                    msg: "crop = true is invalid; use a crop table or crop = false".into(),
                }
                .into());
            }
            Some(CropPatchValue::Values(crop)) => Some(crop.resolve(name)?),
        };

        Ok(ModelEntry {
            path,
            task,
            enabled: self.enabled.unwrap_or(true),
            confidence: self.confidence.unwrap_or_else(default_confidence),
            iou: self.iou.unwrap_or_else(default_iou),
            max_det: self.max_det.unwrap_or_else(default_max_det),
            imgsz: self.imgsz,
            device: self.device.clone().unwrap_or_else(default_device),
            half: self.half.unwrap_or(false),
            rect: self.rect.unwrap_or_else(default_rect),
            polygon_simplify: self
                .polygon_simplify
                .unwrap_or_else(default_polygon_simplify),
            postprocess,
            crop,
        })
    }

    fn apply_overlay(&self, name: &str, entry: &mut ModelEntry) -> Result<()> {
        if self.task.is_some() {
            return Err(ConfigError::InvalidValue {
                field: format!("models.{name}.task"),
                msg: "a blueprint overlay cannot change the model task".into(),
            }
            .into());
        }
        if self.profile.is_some() {
            return Err(ConfigError::InvalidValue {
                field: format!("models.{name}.profile"),
                msg: "a blueprint overlay cannot change the model profile".into(),
            }
            .into());
        }
        if self.enabled.is_some() {
            return Err(ConfigError::InvalidValue {
                field: format!("models.{name}.enabled"),
                msg: "model activation belongs to blueprint.models".into(),
            }
            .into());
        }

        if let Some(path) = &self.path {
            entry.path.clone_from(path);
        }
        if let Some(confidence) = self.confidence {
            entry.confidence = confidence;
        }
        if let Some(iou) = self.iou {
            entry.iou = iou;
        }
        if let Some(max_det) = self.max_det {
            entry.max_det = max_det;
        }
        if let Some(imgsz) = self.imgsz {
            entry.imgsz = Some(imgsz);
        }
        if let Some(device) = &self.device {
            entry.device.clone_from(device);
        }
        if let Some(half) = self.half {
            entry.half = half;
        }
        if let Some(rect) = self.rect {
            entry.rect = rect;
        }
        if let Some(polygon_simplify) = self.polygon_simplify {
            entry.polygon_simplify = polygon_simplify;
        }
        if let Some(postprocess) = &self.postprocess {
            postprocess.apply_to(&mut entry.postprocess);
        }
        if let Some(crop) = &self.crop {
            match crop {
                CropPatchValue::Disabled(false) => entry.crop = None,
                CropPatchValue::Disabled(true) => {
                    return Err(ConfigError::InvalidValue {
                        field: format!("models.{name}.crop"),
                        msg: "crop = true is invalid; use a crop table or crop = false".into(),
                    }
                    .into());
                }
                CropPatchValue::Values(patch) => {
                    if let Some(existing) = entry.crop.as_mut() {
                        patch.apply_to(existing);
                    } else {
                        entry.crop = Some(patch.resolve(name)?);
                    }
                }
            }
        }
        Ok(())
    }
}

impl PostprocessPatch {
    fn merge(&mut self, child: &Self) {
        if child.allow_classes.is_some() {
            self.allow_classes.clone_from(&child.allow_classes);
        }
        if child.min_confidence.is_some() {
            self.min_confidence = child.min_confidence;
        }
        if child.min_area_ratio.is_some() {
            self.min_area_ratio = child.min_area_ratio;
        }
        if child.max_area_ratio.is_some() {
            self.max_area_ratio = child.max_area_ratio;
        }
        if child.min_component_area_ratio.is_some() {
            self.min_component_area_ratio = child.min_component_area_ratio;
        }
        if child.mask_threshold.is_some() {
            self.mask_threshold = child.mask_threshold;
        }
        if child.nms_iou.is_some() {
            self.nms_iou = child.nms_iou;
        }
        if child.max_detections.is_some() {
            self.max_detections = child.max_detections;
        }
    }

    fn resolve(&self) -> PostprocessConfig {
        PostprocessConfig {
            allow_classes: self.allow_classes.clone().unwrap_or_default(),
            min_confidence: self
                .min_confidence
                .unwrap_or_else(default_postprocess_min_confidence),
            min_area_ratio: self
                .min_area_ratio
                .unwrap_or_else(default_postprocess_min_area_ratio),
            max_area_ratio: self
                .max_area_ratio
                .unwrap_or_else(default_postprocess_max_area_ratio),
            min_component_area_ratio: self
                .min_component_area_ratio
                .unwrap_or_else(default_postprocess_min_component_area_ratio),
            mask_threshold: self
                .mask_threshold
                .unwrap_or_else(default_postprocess_mask_threshold),
            nms_iou: self.nms_iou.unwrap_or_else(default_postprocess_nms_iou),
            max_detections: self.max_detections,
        }
    }

    fn apply_to(&self, target: &mut PostprocessConfig) {
        if let Some(allow_classes) = &self.allow_classes {
            target.allow_classes.clone_from(allow_classes);
        }
        if let Some(min_confidence) = self.min_confidence {
            target.min_confidence = min_confidence;
        }
        if let Some(min_area_ratio) = self.min_area_ratio {
            target.min_area_ratio = min_area_ratio;
        }
        if let Some(max_area_ratio) = self.max_area_ratio {
            target.max_area_ratio = max_area_ratio;
        }
        if let Some(min_component_area_ratio) = self.min_component_area_ratio {
            target.min_component_area_ratio = min_component_area_ratio;
        }
        if let Some(mask_threshold) = self.mask_threshold {
            target.mask_threshold = mask_threshold;
        }
        if let Some(nms_iou) = self.nms_iou {
            target.nms_iou = nms_iou;
        }
        if let Some(max_detections) = self.max_detections {
            target.max_detections = Some(max_detections);
        }
    }
}

impl CropPatch {
    fn merge(&mut self, child: &Self) {
        if child.crop_type.is_some() {
            self.crop_type = child.crop_type.clone();
        }
        if child.class.is_some() {
            self.class.clone_from(&child.class);
        }
        if child.margin.is_some() {
            self.margin = child.margin;
        }
        if child.region.is_some() {
            self.region = child.region;
        }
        if child.min_region.is_some() {
            self.min_region = child.min_region;
        }
        if child.max_region.is_some() {
            self.max_region = child.max_region;
        }
        if child.square_size.is_some() {
            self.square_size = child.square_size;
        }
        if child.upper_fraction.is_some() {
            self.upper_fraction = child.upper_fraction;
        }
        if child.fallback.is_some() {
            self.fallback = child.fallback.clone();
        }
    }

    fn resolve(&self, name: &str) -> Result<CropConfig> {
        let Some(crop_type) = self.crop_type.clone() else {
            return Err(ConfigError::InvalidValue {
                field: format!("models.{name}.crop.type"),
                msg: "a resolved crop must define type".into(),
            }
            .into());
        };
        Ok(CropConfig {
            crop_type,
            class: self.class.clone(),
            margin: self.margin.unwrap_or_else(default_crop_margin),
            region: self.region,
            min_region: self.min_region,
            max_region: self.max_region,
            square_size: self.square_size,
            upper_fraction: self.upper_fraction,
            fallback: self.fallback.clone().unwrap_or_default(),
        })
    }

    fn apply_to(&self, target: &mut CropConfig) {
        if let Some(crop_type) = &self.crop_type {
            target.crop_type = crop_type.clone();
        }
        if let Some(class) = &self.class {
            target.class = Some(class.clone());
        }
        if let Some(margin) = self.margin {
            target.margin = margin;
        }
        if let Some(region) = self.region {
            target.region = Some(region);
        }
        if let Some(min_region) = self.min_region {
            target.min_region = Some(min_region);
        }
        if let Some(max_region) = self.max_region {
            target.max_region = Some(max_region);
        }
        if let Some(square_size) = self.square_size {
            target.square_size = Some(square_size);
        }
        if let Some(upper_fraction) = self.upper_fraction {
            target.upper_fraction = Some(upper_fraction);
        }
        if let Some(fallback) = &self.fallback {
            target.fallback = fallback.clone();
        }
    }
}

pub fn load_model_catalog(path: &Path) -> Result<ModelCatalog> {
    let manifest: ModelManifest = load_config(path)?;
    if manifest.include.is_empty() {
        return Err(ConfigError::InvalidValue {
            field: "models.include".into(),
            msg: "the model manifest must include at least one file".into(),
        }
        .into());
    }

    let mut base = ModelPatch::default();
    let mut base_profiles = HashMap::new();
    let mut task_files = Vec::new();

    for include in manifest.include {
        let include_path = if include.is_absolute() {
            include
        } else {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .join(include)
        };
        let file: ModelFile = load_config(&include_path)?;
        if let Some(task) = file.task {
            task_files.push((task, file));
        } else {
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
        }
    }

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
            resolved.merge(&model);
            let entry = resolved.resolve(&name, task)?;
            if catalog.models.insert(name.clone(), entry).is_some() {
                return Err(ConfigError::InvalidValue {
                    field: format!("models.{name}"),
                    msg: "duplicate model key across task files".into(),
                }
                .into());
            }
        }
    }

    if catalog.models.is_empty() {
        return Err(ConfigError::InvalidValue {
            field: "models".into(),
            msg: "the model manifest resolved no models".into(),
        }
        .into());
    }
    Ok(catalog)
}

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

#[cfg(test)]
mod tests;
