use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use image::{DynamicImage, RgbImage};
use mana_geometry::compact_mask::CompactMask;
use mana_geometry::iou::{OverlapMetric, box_overlap};
use mana_geometry::polygonize::{filter_small_components, mask_to_polygons};
use ndarray::s;
use ultralytics_inference::{DepthMap, Device, InferenceConfig, Results, YOLOModel};

use crate::config::{CropConfig, CropType, ModelCatalog, ModelEntry, PostprocessConfig};
use crate::depth_map::DepthFrame;
use crate::detection::{CropRect, Detection, DetectionMask};
use crate::error::Result;
use crate::model_runner::{ModelRunner, RunnerOutput};

mod crop;
mod segment_post;
use crop::extract_crop_frame;
pub use crop::{compute_bbox_roi, compute_largest_class_roi, compute_upper_square_roi};

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
            if !entry.enabled {
                log::info!("model {key}: disabled, skipping load");
                continue;
            }
            if !Path::new(&entry.path).exists() {
                failures.push(format!(
                    "model {key}: file not found at {}",
                    entry.path.display()
                ));
                continue;
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
                Ok(model) => {
                    let actual_task = model.task().as_str();
                    if actual_task != entry.task.as_str() {
                        failures.push(format!(
                            "model {key}: catalog task '{}' does not match ONNX task '{actual_task}'",
                            entry.task
                        ));
                        continue;
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
                    models.insert(
                        key.clone(),
                        LoadedModel {
                            model,
                            run_count: 0,
                            nms_iou: entry.postprocess.nms_iou,
                            polygon_simplify: entry.polygon_simplify,
                            postprocess: entry.postprocess.clone(),
                            crop_config: entry.crop.clone(),
                            static_roi,
                        },
                    );
                }
                Err(e) => {
                    failures.push(format!("model {key}: load failed — {e}"));
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

        let (img, offset_x, offset_y, crop_frame) = if let Some(r) = crop_rect {
            let info = extract_crop_frame(rgb, w, h, r)?;
            let img =
                DynamicImage::ImageRgb8(RgbImage::from_raw(info.w, info.h, info.rgb.clone())?);
            (img, r.x1 as f32, r.y1 as f32, Some(info))
        } else {
            let img = DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, rgb.to_vec())?);
            // Static model ROIs are applied inside ultralytics-inference. Build
            // the matching local image for Rerun without applying the ROI twice.
            let crop_frame = static_roi.and_then(|roi| extract_crop_frame(rgb, w, h, roi));
            (img, 0.0, 0.0, crop_frame)
        };

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
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        for d in &mut detections {
            d.bbox[0] += offset_x;
            d.bbox[1] += offset_y;
            d.bbox[2] += offset_x;
            d.bbox[3] += offset_y;
            if let Some(keypoints) = &mut d.keypoints {
                translate_keypoints(keypoints, offset_x, offset_y);
            }
            if let Some(mask) = &mut d.mask {
                mask.origin = [offset_x as u32, offset_y as u32];
                let fw = w.max(1) as f32;
                let fh = h.max(1) as f32;
                for poly in &mut *Arc::make_mut(&mut mask.polygons) {
                    for vertex in poly {
                        vertex[0] =
                            (vertex[0] * mask.mask_dims[0] as f32 + mask.origin[0] as f32) / fw;
                        vertex[1] =
                            (vertex[1] * mask.mask_dims[1] as f32 + mask.origin[1] as f32) / fh;
                    }
                }
            }
        }
        let roi_rejected = if let Some(roi) = loaded.static_roi {
            let before_roi = detections.len();
            detections = detections
                .into_iter()
                .filter_map(|d| {
                    let bbox = d.bbox;
                    let class = d.class.clone();
                    let confidence = d.confidence;
                    let clipped = clip_detection_to_roi(d, roi);
                    if clipped.is_none() {
                        log::info!(
                            "model {model_key}: static ROI rejected class={} confidence={:.4} bbox={:?} roi={:?}",
                            class,
                            confidence,
                            bbox,
                            roi.to_array(),
                        );
                    }
                    clipped
                })
                .collect();
            before_roi - detections.len()
        } else {
            0
        };
        let before_postprocess = detections.len();
        let (postprocess_w, postprocess_h) = crop_rect.map_or((w, h), |rect| {
            (
                rect.x2.saturating_sub(rect.x1).max(1),
                rect.y2.saturating_sub(rect.y1).max(1),
            )
        });
        detections.retain(|d| {
            let acceptance_bbox = crop_rect.map_or(d.bbox, |rect| {
                [
                    d.bbox[0] - rect.x1 as f32,
                    d.bbox[1] - rect.y1 as f32,
                    d.bbox[2] - rect.x1 as f32,
                    d.bbox[3] - rect.y1 as f32,
                ]
            });
            let reason = loaded.postprocess.rejection_reason(
                &d.class,
                d.confidence,
                acceptance_bbox,
                postprocess_w,
                postprocess_h,
            );
            if let Some(reason) = reason {
                log::debug!(
                    "model {model_key}: postprocess rejected reason={reason} class={} confidence={:.4} bbox={:?} acceptance_bbox={:?} frame={}x{}",
                    d.class,
                    d.confidence,
                    d.bbox,
                    acceptance_bbox,
                    postprocess_w,
                    postprocess_h,
                );
                false
            } else {
                true
            }
        });
        let postprocess_rejected = roi_rejected + before_postprocess - detections.len();
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

fn collect_detections(
    results: &[Results],
    polygon_simplify: f64,
    min_component_area_ratio: f32,
    mask_threshold: f32,
) -> Vec<Detection> {
    let mut dets = Vec::new();

    for r in results {
        let Some(ref boxes) = r.boxes else { continue };
        let xyxy = boxes.xyxy();
        let masks = r
            .masks
            .as_ref()
            .map(|m| (&m.data, m.data.shape()[1], m.data.shape()[2]));

        for i in 0..boxes.len() {
            if i >= xyxy.nrows() {
                break;
            }
            let cls_id_raw = boxes.cls().get(i).copied().unwrap_or(-1.0);
            if cls_id_raw < 0.0 {
                continue;
            }
            let cls = cls_id_raw as usize;
            let bbox = [xyxy[[i, 0]], xyxy[[i, 1]], xyxy[[i, 2]], xyxy[[i, 3]]];
            let keypoints = r.keypoints.as_ref().and_then(|keypoints| {
                if i >= keypoints.data.shape()[0] {
                    return None;
                }
                let count = keypoints.data.shape()[1];
                let dims = keypoints.data.shape()[2];
                Some(
                    (0..count)
                        .map(|k| {
                            [
                                keypoints.data[[i, k, 0]],
                                keypoints.data[[i, k, 1]],
                                if dims > 2 {
                                    keypoints.data[[i, k, 2]]
                                } else {
                                    1.0
                                },
                            ]
                        })
                        .collect(),
                )
            });
            let mask = masks.as_ref().and_then(|(data, mask_h, mask_w)| {
                let slice = data.slice(s![i, .., ..]);
                build_detection_mask(
                    slice,
                    bbox,
                    *mask_w as u32,
                    *mask_h as u32,
                    polygon_simplify,
                    min_component_area_ratio,
                    mask_threshold,
                )
            });
            dets.push(Detection {
                class: r
                    .names
                    .get(&cls)
                    .cloned()
                    .unwrap_or_else(|| "unknown".into()),
                confidence: boxes.conf().get(i).copied().unwrap_or(0.0),
                bbox,
                keypoints,
                mask,
            });
        }
    }

    dets
}

fn take_depth(results: &mut [Results]) -> Option<DepthMap> {
    results.iter_mut().find_map(|result| result.depth.take())
}

fn translate_keypoints(keypoints: &mut [[f32; 3]], offset_x: f32, offset_y: f32) {
    for keypoint in keypoints {
        keypoint[0] += offset_x;
        keypoint[1] += offset_y;
    }
}

/// Binarize the model output mask for detection `i` and derive the compact
/// RLE (crop-scoped) plus simplified contours, all in mask space.
///
/// Single pass over the bbox crop only: the raster (for the RLE) and the
/// float mask (for the contours) are both crop-sized, never the full mask.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn build_detection_mask(
    mask_slice: ndarray::ArrayView2<f32>,
    bbox: [f32; 4],
    mask_w: u32,
    mask_h: u32,
    polygon_simplify: f64,
    min_component_area_ratio: f32,
    mask_threshold: f32,
) -> Option<DetectionMask> {
    if mask_w == 0 || mask_h == 0 {
        return None;
    }

    let x1 = bbox[0].floor().max(0.0) as u32;
    let y1 = bbox[1].floor().max(0.0) as u32;
    let x2 = bbox[2].ceil().min(mask_w as f32) as u32;
    let y2 = bbox[3].ceil().min(mask_h as f32) as u32;
    if x2 <= x1 || y2 <= y1 {
        return None;
    }

    let crop_w = x2 - x1;
    let crop_h = y2 - y1;
    let mut crop_bits = vec![0u8; (crop_h * crop_w) as usize];
    let mut above_threshold = false;
    for row in 0..crop_h {
        for col in 0..crop_w {
            let val = mask_slice[[(y1 + row) as usize, (x1 + col) as usize]];
            if val > mask_threshold {
                crop_bits[(row * crop_w + col) as usize] = 1;
                above_threshold = true;
            }
        }
    }
    if !above_threshold {
        return None;
    }

    let crop_bits = filter_small_components(
        &crop_bits,
        crop_w as usize,
        crop_h as usize,
        min_component_area_ratio,
    );
    if !crop_bits.iter().any(|&pixel| pixel != 0) {
        return None;
    }

    let compact =
        CompactMask::from_dense(&crop_bits, crop_h, crop_w, (x1, y1), (mask_h, mask_w)).ok()?;

    // Contours over the bbox crop only: `mask_to_polygons` returns vertices
    // already normalized 0..1 over the crop, and the crop is offset by
    // (x1, y1) inside the mask output. Rescale to mask-space normalization
    // by (crop/mask) and add the normalized offset — otherwise the polygon
    // is anchored to the mask-space origin and lands far from its bbox.
    let crop_flat: Vec<f32> = crop_bits.iter().map(|&b| b as f32).collect();
    let scale_x = crop_w as f32 / mask_w as f32;
    let scale_y = crop_h as f32 / mask_h as f32;
    let off_x = x1 as f32 / mask_w as f32;
    let off_y = y1 as f32 / mask_h as f32;
    let polygons = mask_to_polygons(
        &crop_flat,
        crop_w as usize,
        crop_h as usize,
        mask_threshold,
        polygon_simplify,
        None,
        None,
    )
    .into_iter()
    .map(|poly| {
        poly.into_iter()
            .map(|[nx, ny]| [nx * scale_x + off_x, ny * scale_y + off_y])
            .collect()
    })
    .collect();

    Some(DetectionMask {
        compact: Arc::new(compact),
        polygons: Arc::new(polygons),
        origin: [0, 0],
        mask_dims: [mask_w, mask_h],
    })
}

fn clip_detection_to_roi(mut detection: Detection, roi: CropRect) -> Option<Detection> {
    detection.bbox[0] = detection.bbox[0].max(roi.x1 as f32);
    detection.bbox[1] = detection.bbox[1].max(roi.y1 as f32);
    detection.bbox[2] = detection.bbox[2].min(roi.x2 as f32);
    detection.bbox[3] = detection.bbox[3].min(roi.y2 as f32);
    (detection.bbox[2] > detection.bbox[0] && detection.bbox[3] > detection.bbox[1])
        .then_some(detection)
}

fn apply_nms(mut detections: Vec<Detection>, iou_threshold: f32) -> (Vec<Detection>, usize) {
    detections.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut kept = Vec::with_capacity(detections.len());
    let mut suppressed = 0;
    for candidate in detections {
        if kept.iter().any(|accepted: &Detection| {
            accepted.class == candidate.class
                && box_overlap(
                    (
                        accepted.bbox[0],
                        accepted.bbox[1],
                        accepted.bbox[2],
                        accepted.bbox[3],
                    ),
                    (
                        candidate.bbox[0],
                        candidate.bbox[1],
                        candidate.bbox[2],
                        candidate.bbox[3],
                    ),
                    OverlapMetric::Iou,
                ) > iou_threshold
        }) {
            suppressed += 1;
        } else {
            kept.push(candidate);
        }
    }
    (kept, suppressed)
}

fn apply_max_detections(detections: &mut Vec<Detection>, max_detections: Option<usize>) -> usize {
    let Some(max_detections) = max_detections else {
        return 0;
    };
    let removed = detections.len().saturating_sub(max_detections);
    detections.truncate(max_detections);
    removed
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
