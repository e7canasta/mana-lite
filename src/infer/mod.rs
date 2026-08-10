use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use image::{DynamicImage, RgbImage};
use ultralytics_inference::{Device, InferenceConfig, YOLOModel};

use crate::config::{CropConfig, CropType, ModelCatalog, ModelEntry, PostprocessConfig};
use crate::depth_map::DepthFrame;
use crate::detection::{CropRect, Detection};
use crate::error::Result;
use crate::model_runner::{ModelRunner, RunnerOutput};

mod crop;
mod run_helpers;
mod segment_post;
use crop::extract_crop_frame;
pub use crop::{compute_bbox_roi, compute_largest_class_roi, compute_upper_square_roi};
use run_helpers::{
    apply_max_detections, apply_nms, apply_postprocess_filter, apply_static_roi_filter,
    collect_detections, take_depth, translate_detections_to_frame,
};

#[cfg(test)]
use run_helpers::{build_detection_mask, clip_detection_to_roi, translate_keypoints};
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use ultralytics_inference::{DepthMap, Results};

pub struct CropFrameInfo {
    pub rgb: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

pub struct InferenceResult {
    pub detections: Vec<Detection>,
    pub depth: Option<DepthFrame>,
    pub postprocess_rejected: usize,
    pub post_nms_suppressed: usize,
    pub infer_ms: u64,
    pub pipeline_us: u64,
    pub crop_frame: Option<CropFrameInfo>,
}

pub struct InferEngine {
    models: HashMap<String, LoadedModel>,
}

struct LoadedModel {
    model: YOLOModel,
    run_count: u64,
    nms_iou: f32,
    polygon_simplify: f64,
    postprocess: PostprocessConfig,
    crop_config: Option<CropConfig>,
    static_roi: Option<CropRect>,
}

impl InferEngine {
    pub fn from_catalog(catalog: &ModelCatalog) -> Result<Self> {
        let mut models = HashMap::new();
        let mut failures = Vec::new();
        for (key, entry) in &catalog.models {
            match try_load_model(key, entry) {
                LoadAttempt::Skip => {}
                LoadAttempt::Failed(message) => failures.push(message),
                LoadAttempt::Loaded(loaded) => {
                    models.insert(key.clone(), loaded);
                }
            }
        }
        if !failures.is_empty() {
            return Err(crate::error::ManaError::Inference(failures.join("; ")));
        }
        Ok(Self { models })
    }

    pub fn crop_info(&self, model_key: &str) -> Option<&CropConfig> {
        self.models.get(model_key)?.crop_config.as_ref()
    }

    pub fn run(
        &mut self,
        model_key: &str,
        rgb: &[u8],
        w: u32,
        h: u32,
        crop_rect: Option<CropRect>,
    ) -> Option<InferenceResult> {
        let started_at = Instant::now();
        let loaded = self.models.get_mut(model_key)?;
        let static_roi = loaded.static_roi;

        let (img, offset_x, offset_y, crop_frame) =
            prepare_infer_image(rgb, w, h, crop_rect, static_roi)?;
        let mut results = loaded.model.predict_image(&img, String::new()).ok()?;

        let infer_ms = results
            .first()
            .and_then(|r| r.speed.inference)
            .map(|ms| ms as u64)
            .unwrap_or(0);

        // Depth is a parallel output. Take ownership before detection parsing so
        // the map is not copied and never enters the detection pipeline.
        let depth = take_depth(&mut results).map(DepthFrame::from_ultralytics);
        let mut detections = collect_detections(
            &results,
            loaded.polygon_simplify,
            loaded.postprocess.min_component_area_ratio,
            loaded.postprocess.mask_threshold,
        );
        translate_detections_to_frame(&mut detections, offset_x, offset_y, w, h);
        let roi_rejected = apply_static_roi_filter(&mut detections, loaded.static_roi, model_key);
        let postprocess_rejected = roi_rejected
            + apply_postprocess_filter(
                &mut detections,
                &loaded.postprocess,
                crop_rect,
                w,
                h,
                model_key,
            );
        let (mut detections, mut post_nms_suppressed) = apply_nms(detections, loaded.nms_iou);
        post_nms_suppressed +=
            apply_max_detections(&mut detections, loaded.postprocess.max_detections);
        loaded.run_count += 1;

        Some(InferenceResult {
            detections,
            depth,
            postprocess_rejected,
            post_nms_suppressed,
            infer_ms,
            pipeline_us: started_at.elapsed().as_micros() as u64,
            crop_frame,
        })
    }

    pub fn model_count(&self) -> usize {
        self.models.len()
    }
}

enum LoadAttempt {
    Skip,
    Failed(String),
    Loaded(LoadedModel),
}

fn try_load_model(key: &str, entry: &ModelEntry) -> LoadAttempt {
    if !entry.enabled {
        log::info!("model {key}: disabled, skipping load");
        return LoadAttempt::Skip;
    }
    if !Path::new(&entry.path).exists() {
        return LoadAttempt::Failed(format!(
            "model {key}: file not found at {}",
            entry.path.display()
        ));
    }

    let mut conf = build_config(entry);
    let static_roi = entry
        .crop
        .as_ref()
        .filter(|crop| crop.crop_type == CropType::Static)
        .and_then(|crop| crop.region)
        .map(CropRect::from_array);

    if let Some(ref crop) = entry.crop {
        if crop.crop_type == CropType::Static {
            if let Some([x1, y1, x2, y2]) = crop.region {
                conf = conf.with_roi(x1, y1, x2, y2);
                log::info!("model {key}: static ROI [{x1},{y1} {x2},{y2}]");
            }
        }
    }

    match YOLOModel::load_with_config(&entry.path, conf) {
        Ok(model) => finish_loaded_model(key, entry, model, static_roi),
        Err(e) => LoadAttempt::Failed(format!("model {key}: load failed — {e}")),
    }
}

fn finish_loaded_model(
    key: &str,
    entry: &ModelEntry,
    model: YOLOModel,
    static_roi: Option<CropRect>,
) -> LoadAttempt {
    let actual_task = model.task().as_str();
    if actual_task != entry.task.as_str() {
        return LoadAttempt::Failed(format!(
            "model {key}: catalog task '{}' does not match ONNX task '{actual_task}'",
            entry.task
        ));
    }
    log::info!("model {key}: loaded ({})", model.task());
    log::info!(
        "model {key}: postprocess classes={:?} confidence>={:.2} area=[{:.4},{:.4}] component_area>={:.4} mask_threshold>={:.2} nms_iou<={:.2} max_detections={}",
        entry.postprocess.allow_classes,
        entry.postprocess.min_confidence,
        entry.postprocess.min_area_ratio,
        entry.postprocess.max_area_ratio,
        entry.postprocess.min_component_area_ratio,
        entry.postprocess.mask_threshold,
        entry.postprocess.nms_iou,
        entry
            .postprocess
            .max_detections
            .map_or_else(|| "all".into(), |max| max.to_string()),
    );
    LoadAttempt::Loaded(LoadedModel {
        model,
        run_count: 0,
        nms_iou: entry.postprocess.nms_iou,
        polygon_simplify: entry.polygon_simplify,
        postprocess: entry.postprocess.clone(),
        crop_config: entry.crop.clone(),
        static_roi,
    })
}

fn prepare_infer_image(
    rgb: &[u8],
    w: u32,
    h: u32,
    crop_rect: Option<CropRect>,
    static_roi: Option<CropRect>,
) -> Option<(DynamicImage, f32, f32, Option<CropFrameInfo>)> {
    if let Some(r) = crop_rect {
        let info = extract_crop_frame(rgb, w, h, r)?;
        let img = DynamicImage::ImageRgb8(RgbImage::from_raw(info.w, info.h, info.rgb.clone())?);
        Some((img, r.x1 as f32, r.y1 as f32, Some(info)))
    } else {
        let img = DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, rgb.to_vec())?);
        // Static model ROIs are applied inside ultralytics-inference. Build
        // the matching local image for Rerun without applying the ROI twice.
        let crop_frame = static_roi.and_then(|roi| extract_crop_frame(rgb, w, h, roi));
        Some((img, 0.0, 0.0, crop_frame))
    }
}

impl ModelRunner for InferEngine {
    fn run(
        &mut self,
        model_key: &str,
        rgb: &[u8],
        width: u32,
        height: u32,
        crop: Option<CropRect>,
    ) -> Option<RunnerOutput> {
        let result = InferEngine::run(self, model_key, rgb, width, height, crop)?;
        Some(RunnerOutput {
            detections: result.detections,
            depth: result.depth,
            infer_ms: result.infer_ms,
        })
    }

    fn model_count(&self) -> usize {
        InferEngine::model_count(self)
    }
}

fn build_config(entry: &ModelEntry) -> InferenceConfig {
    let mut c = InferenceConfig::default()
        .with_confidence(entry.confidence)
        .with_iou(entry.iou)
        .with_max_det(entry.max_det as usize)
        .with_rect(entry.rect);

    if entry.half {
        c = c.with_half(true);
    }

    if let Some(imgsz) = entry.imgsz {
        c = c.with_imgsz(imgsz as usize, imgsz as usize);
    }

    if entry.device != "cpu" {
        match entry.device.parse::<Device>() {
            Ok(dev) => c = c.with_device(dev),
            Err(e) => log::warn!("invalid device '{}': {e}, using cpu", entry.device),
        }
    }

    c
}

#[allow(dead_code)]
#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::manual_midpoint
)]
mod tests;
