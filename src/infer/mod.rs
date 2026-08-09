use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use image::{DynamicImage, RgbImage};
use mana_geometry::compact_mask::CompactMask;
use mana_geometry::polygonize::{filter_small_components, mask_to_polygons};
use ndarray::s;
use ultralytics_inference::{DepthMap, Device, InferenceConfig, Results, YOLOModel};

use crate::config::{CropConfig, CropType, ModelCatalog, ModelEntry, PostprocessConfig};
use crate::depth_map::DepthFrame;
use crate::error::Result;
use crate::logger::{DetRecord, MaskRecord};
use crate::model_runner::{ModelRunner, RunnerOutput};

mod crop;
mod segment_post;
pub use crop::{compute_bbox_roi, compute_largest_class_roi, compute_upper_square_roi};
use crop::extract_crop_frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropRect {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

impl CropRect {
    pub fn from_array(a: [u32; 4]) -> Self {
        CropRect {
            x1: a[0],
            y1: a[1],
            x2: a[2],
            y2: a[3],
        }
    }

    pub fn to_array(self) -> [u32; 4] {
        [self.x1, self.y1, self.x2, self.y2]
    }
}

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

#[derive(Debug, Clone)]
pub struct Detection {
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
    pub keypoints: Option<Vec<[f32; 3]>>,
    pub mask: Option<DetectionMask>,
}

impl Detection {
    pub fn area_px(&self) -> f32 {
        ((self.bbox[2] - self.bbox[0]) * (self.bbox[3] - self.bbox[1])).max(0.0)
    }

    pub fn area_ratio(&self, frame_w: u32, frame_h: u32) -> f32 {
        let frame_area = (frame_w as f32) * (frame_h as f32);
        if frame_area > 0.0 {
            self.area_px() / frame_area
        } else {
            0.0
        }
    }
}

/// Instance mask attached to a detection.
///
/// The mask raster lives in *mask space* (the image passed to the model,
/// i.e. the crop for cascade models), stored as a crop-RLE `CompactMask`.
/// `origin` is the position of mask space within the original frame and
/// `mask_dims` its size, so consumers can place and rasterize the mask.
/// `polygons` are simplified contours normalized to the full frame.
///
/// `compact` and `polygons` are `Arc`-shared: the mask travels from
/// inference through consolidation and tracking without deep copies;
/// the only owned copy happens at the wire boundary (`to_wire_record`).
#[derive(Debug, Clone)]
pub struct DetectionMask {
    pub compact: Arc<CompactMask>,
    pub polygons: Arc<Vec<Vec<[f32; 2]>>>,
    pub origin: [u32; 2],
    pub mask_dims: [u32; 2],
}

impl DetectionMask {
    /// JSONL wire record (Spec-003). `rle` are column-major run-length
    /// counts of the mask crop; `bbox` is the detection box inside mask
    /// space; `origin`/`mask_dims` place mask space in the frame and
    /// `polygons` are contours normalized to the full frame.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn to_wire_record(&self) -> MaskRecord {
        let (bbox_h, bbox_w) = if let Some(rle) = self.compact.rles.first() {
            (rle.h, rle.w)
        } else {
            (0, 0)
        };
        let (off_x, off_y) = self.compact.offsets.first().copied().unwrap_or((0, 0));
        MaskRecord {
            rle: self
                .compact
                .rles
                .first()
                .map(|rle| rle.counts.to_vec())
                .unwrap_or_default(),
            bbox: [
                off_x as f32,
                off_y as f32,
                (off_x + bbox_w) as f32,
                (off_y + bbox_h) as f32,
            ],
            origin: self.origin,
            mask_dims: self.mask_dims,
            polygons: self.polygons.as_ref().clone(),
        }
    }
}

impl Detection {
    pub fn to_det_record(&self, frame_w: u32, frame_h: u32) -> DetRecord {
        DetRecord {
            class: self.class.clone(),
            confidence: self.confidence,
            bbox: self.bbox,
            area_px: self.area_px(),
            area_ratio: self.area_ratio(frame_w, frame_h),
            mask: self.mask.as_ref().map(DetectionMask::to_wire_record),
        }
    }
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
                && bbox_iou(&accepted.bbox, &candidate.bbox) > iou_threshold
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

fn bbox_iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    mana_geometry::iou::box_overlap(
        (a[0], a[1], a[2], a[3]),
        (b[0], b[1], b[2], b[3]),
        mana_geometry::iou::OverlapMetric::Iou,
    )
}

#[allow(dead_code)]
#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::manual_midpoint
)]
mod tests {
    use super::*;
    use crate::config::ModelTask;

    #[test]
    fn depth_is_extracted_without_detection_boxes() {
        let mut result = Results::new(
            ndarray::Array3::zeros((2, 2, 3)),
            "frame".into(),
            Arc::new(HashMap::new()),
            ultralytics_inference::Speed::default(),
            (2, 2),
        );
        result.depth = Some(DepthMap::new(
            ndarray::array![[1.0, 2.0], [0.0, 3.0]],
            (2, 2),
        ));
        assert!(result.boxes.is_none());

        let depth = take_depth(std::slice::from_mut(&mut result)).expect("depth map");
        assert_eq!(depth.data.shape(), &[2, 2]);
        assert_eq!(depth.min_depth(), Some(1.0));
        assert_eq!(depth.max_depth(), Some(3.0));
        assert!(result.depth.is_none());
    }

    #[test]
    fn enabled_missing_model_fails_catalog_load() {
        let entry = ModelEntry {
            path: std::path::PathBuf::from("/definitely/missing/model.onnx"),
            task: ModelTask::Depth,
            enabled: true,
            confidence: 0.0,
            iou: 0.5,
            max_det: 300,
            imgsz: Some(320),
            device: "cpu".into(),
            half: true,
            rect: true,
            polygon_simplify: 0.98,
            postprocess: PostprocessConfig::default(),
            crop: None,
        };
        let catalog = ModelCatalog {
            models: HashMap::from([(String::from("depth-standard"), entry)]),
        };

        let error = match InferEngine::from_catalog(&catalog) {
            Ok(_) => panic!("enabled missing model must fail startup"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("file not found"));
    }

    #[test]
    fn crop_frame_extracts_static_roi_in_local_pixel_order() {
        let rgb = (0u8..36).collect::<Vec<_>>();
        let crop = extract_crop_frame(
            &rgb,
            4,
            3,
            CropRect {
                x1: 1,
                y1: 1,
                x2: 3,
                y2: 3,
            },
        )
        .expect("valid crop");

        assert_eq!((crop.w, crop.h), (2, 2));
        assert_eq!(
            crop.rgb,
            vec![15, 16, 17, 18, 19, 20, 27, 28, 29, 30, 31, 32]
        );
    }

    #[test]
    fn crop_frame_rejects_roi_outside_frame() {
        assert!(
            extract_crop_frame(
                &[0; 12],
                2,
                2,
                CropRect {
                    x1: 1,
                    y1: 1,
                    x2: 3,
                    y2: 2,
                },
            )
            .is_none()
        );
    }

    fn person(x1: f32, y1: f32, x2: f32, y2: f32) -> Detection {
        Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [x1, y1, x2, y2],
            keypoints: None,
            mask: None,
        }
    }

    #[test]
    fn detection_area_and_ratio_use_frame_coordinates() {
        let detection = person(10.0, 20.0, 110.0, 220.0);
        assert_eq!(detection.area_px(), 20_000.0);
        assert!((detection.area_ratio(1_000, 1_000) - 0.02).abs() < f32::EPSILON);
        let record = detection.to_det_record(1_000, 1_000);
        assert_eq!(record.area_px, 20_000.0);
        assert!((record.area_ratio - 0.02).abs() < f32::EPSILON);
    }

    fn solid_mask(
        w: usize,
        h: usize,
        x1: usize,
        y1: usize,
        x2: usize,
        y2: usize,
    ) -> ndarray::Array2<f32> {
        let mut data = ndarray::Array2::<f32>::zeros((h, w));
        for row in y1..y2 {
            for col in x1..x2 {
                data[[row, col]] = 1.0;
            }
        }
        data
    }

    #[test]
    fn mask_builder_produces_compact_rle_and_polygons() {
        let mask = solid_mask(8, 8, 2, 2, 6, 6);
        let built =
            build_detection_mask(mask.view(), [2.0, 2.0, 6.0, 6.0], 8, 8, 0.75, 0.0, 0.5).unwrap();
        assert_eq!(built.compact.offsets, vec![(2, 2)]);
        assert_eq!(built.compact.image_shape, (8, 8));
        assert_eq!(built.compact.area(0).unwrap(), 16);
        assert_eq!(built.compact.rles[0].h, 4);
        assert_eq!(built.compact.rles[0].w, 4);
        assert!(!built.compact.rles[0].counts.is_empty());
        assert!(!built.polygons.is_empty(), "polygons must be derived");
        let rect = &built.polygons[0];
        let shoelace = (0..rect.len())
            .map(|i| {
                let [x1, y1] = rect[i];
                let [x2, y2] = rect[(i + 1) % rect.len()];
                x1 * y2 - x2 * y1
            })
            .sum::<f32>()
            .abs();
        assert!(shoelace > 0.0, "polygon must enclose area, got {shoelace}");
    }

    #[test]
    fn mask_builder_removes_small_components_from_compact_and_polygons() {
        let mut mask = ndarray::Array2::<f32>::zeros((20, 20));
        for row in 2..10 {
            for col in 2..10 {
                mask[[row, col]] = 1.0;
            }
        }
        mask[[17, 17]] = 1.0;

        let built =
            build_detection_mask(mask.view(), [0.0, 0.0, 20.0, 20.0], 20, 20, 0.75, 0.02, 0.5)
                .unwrap();

        assert_eq!(built.compact.area(0).unwrap(), 64);
        assert_eq!(built.polygons.len(), 1);
    }

    #[test]
    fn mask_builder_keeps_multiple_large_components() {
        let mut mask = ndarray::Array2::<f32>::zeros((20, 20));
        for row in 2..8 {
            for col in 2..8 {
                mask[[row, col]] = 1.0;
            }
        }
        for row in 12..18 {
            for col in 12..18 {
                mask[[row, col]] = 1.0;
            }
        }

        let built =
            build_detection_mask(mask.view(), [0.0, 0.0, 20.0, 20.0], 20, 20, 0.75, 0.02, 0.5)
                .unwrap();

        assert_eq!(built.compact.area(0).unwrap(), 72);
        assert_eq!(built.polygons.len(), 2);
    }

    #[test]
    fn mask_builder_omits_mask_when_all_components_are_too_small() {
        let mut mask = ndarray::Array2::<f32>::zeros((20, 20));
        mask[[10, 10]] = 1.0;

        let built =
            build_detection_mask(mask.view(), [0.0, 0.0, 20.0, 20.0], 20, 20, 0.75, 0.02, 0.5);

        assert!(built.is_none());
    }

    #[test]
    fn polygon_keeps_bbox_offset_in_mask_space() {
        // Solid 4x4 block at (2,2) inside an 8x8 mask: the bbox crop starts
        // at (2,2), so mask_to_polygons' crop-normalized vertices (0..0.75)
        // must land in mask space scaled by 0.5 plus normalized offset 0.25,
        // i.e. exactly [(0.25,0.25)..(0.625,0.625)]. Before the fix the
        // polygon was anchored at the mask-space origin, missing the offset
        // entirely (the margin between crop origin and bbox).
        let mask = solid_mask(8, 8, 2, 2, 6, 6);
        let built =
            build_detection_mask(mask.view(), [2.0, 2.0, 6.0, 6.0], 8, 8, 0.75, 0.0, 0.5).unwrap();
        let rect = &built.polygons[0];
        let min_x = rect.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
        let min_y = rect.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
        let max_x = rect.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        let max_y = rect.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
        assert!(
            (min_x - 0.25).abs() < 0.05 && (min_y - 0.25).abs() < 0.05,
            "polygon must start at the bbox offset, got min ({min_x}, {min_y})"
        );
        assert!(
            (max_x - 0.625).abs() < 0.05 && (max_y - 0.625).abs() < 0.05,
            "polygon must end at the bbox extent, got max ({max_x}, {max_y})"
        );
        assert!(
            (max_x - min_x) < 0.45,
            "polygon scale must stay in range, got {}",
            max_x - min_x
        );
    }

    #[test]
    fn mask_builder_ignores_below_threshold_pixels() {
        let mut mask = ndarray::Array2::<f32>::zeros((8, 8));
        mask[[4, 4]] = 0.4;
        let built = build_detection_mask(mask.view(), [3.0, 3.0, 5.0, 5.0], 8, 8, 0.75, 0.0, 0.5);
        assert!(built.is_none(), "no pixel above threshold -> no mask");
    }

    #[test]
    fn mask_builder_bbox_outside_mask_is_none() {
        let mask = solid_mask(8, 8, 2, 2, 6, 6);
        let built = build_detection_mask(mask.view(), [9.0, 9.0, 10.0, 10.0], 8, 8, 0.75, 0.0, 0.5);
        assert!(built.is_none());
    }

    #[test]
    fn mask_builder_respects_partial_bbox_clip() {
        let mask = solid_mask(8, 8, 0, 0, 8, 8);
        let built = build_detection_mask(mask.view(), [-2.0, -2.0, 4.0, 4.0], 8, 8, 0.75, 0.0, 0.5)
            .unwrap();
        assert_eq!(built.compact.area(0).unwrap(), 16, "clipped 4x4 block");
        assert_eq!(built.compact.offsets, vec![(0, 0)]);
    }

    #[test]
    fn roi_largest_class_simple() {
        let dets = vec![person(100.0, 100.0, 200.0, 300.0)];
        let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, None, None).unwrap();
        assert_eq!(
            r,
            CropRect {
                x1: 100,
                y1: 100,
                x2: 200,
                y2: 300
            }
        );
    }

    #[test]
    fn roi_min_region_union() {
        let dets = vec![person(300.0, 100.0, 400.0, 200.0)];
        let r = compute_largest_class_roi(
            &dets,
            "person",
            0.0,
            640,
            480,
            Some([100, 200, 500, 450]),
            None,
        )
        .unwrap();
        assert_eq!(
            r,
            CropRect {
                x1: 100,
                y1: 100,
                x2: 500,
                y2: 450
            }
        );
    }

    #[test]
    fn roi_min_region_fallback() {
        let dets: Vec<Detection> = vec![];
        let r = compute_largest_class_roi(
            &dets,
            "person",
            0.0,
            640,
            480,
            Some([100, 200, 500, 450]),
            None,
        )
        .unwrap();
        assert_eq!(
            r,
            CropRect {
                x1: 100,
                y1: 200,
                x2: 500,
                y2: 450
            }
        );
    }

    #[test]
    fn roi_max_region_clamps() {
        let dets = vec![person(0.0, 0.0, 640.0, 480.0)];
        let r = compute_largest_class_roi(
            &dets,
            "person",
            0.0,
            640,
            480,
            None,
            Some([50, 50, 400, 300]),
        )
        .unwrap();
        assert_eq!(
            r,
            CropRect {
                x1: 50,
                y1: 50,
                x2: 400,
                y2: 300
            }
        );
    }

    #[test]
    fn roi_min_max_together() {
        let dets = vec![person(200.0, 100.0, 300.0, 200.0)];
        let r = compute_largest_class_roi(
            &dets,
            "person",
            0.0,
            640,
            480,
            Some([50, 50, 500, 400]),
            Some([0, 0, 350, 300]),
        )
        .unwrap();
        assert_eq!(
            r,
            CropRect {
                x1: 50,
                y1: 50,
                x2: 350,
                y2: 300
            }
        );
    }

    #[test]
    fn roi_no_class_no_min() {
        let dets: Vec<Detection> = vec![];
        let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, None, None);
        assert!(r.is_none());
    }

    #[test]
    fn roi_from_track_bbox_matches_dynamic_roi() {
        let r = compute_bbox_roi([100.0, 100.0, 200.0, 300.0], 0.0, 640, 480, None, None).unwrap();
        assert_eq!(
            r,
            CropRect {
                x1: 100,
                y1: 100,
                x2: 200,
                y2: 300
            }
        );
    }

    #[test]
    fn upper_square_roi_focuses_on_person_upper_half() {
        let r =
            compute_upper_square_roi([768.0, 190.0, 986.0, 717.0], 320, 0.5, 1920, 1080).unwrap();
        assert_eq!(r.x2 - r.x1, 320);
        assert_eq!(r.y2 - r.y1, 320);
        assert!(r.y1 < 190);
        assert!(r.y2 < 550);
    }

    #[test]
    fn pose_keypoints_follow_dynamic_crop_offset() {
        let mut keypoints = [[10.0, 20.0, 0.9], [30.0, 40.0, 0.8]];
        translate_keypoints(&mut keypoints, 100.0, 50.0);
        assert_eq!(keypoints, [[110.0, 70.0, 0.9], [130.0, 90.0, 0.8]]);
    }

    #[test]
    fn model_postprocess_filter_accepts_only_valid_detections() {
        let filters = PostprocessConfig {
            allow_classes: vec!["person".into()],
            min_confidence: 0.5,
            min_area_ratio: 0.01,
            max_area_ratio: 0.8,
            min_component_area_ratio: 0.0,
            mask_threshold: 0.5,
            nms_iou: 0.5,
            max_detections: None,
        };
        assert!(filters.accepts("person", 0.8, [0.0, 0.0, 20.0, 20.0], 100, 100));
        assert!(!filters.accepts("chair", 0.8, [0.0, 0.0, 20.0, 20.0], 100, 100));
        assert!(!filters.accepts("person", 0.4, [0.0, 0.0, 20.0, 20.0], 100, 100));
        assert!(!filters.accepts("person", 0.8, [0.0, 0.0, 5.0, 5.0], 100, 100));
        assert!(!filters.accepts("person", 0.8, [0.0, 0.0, 100.0, 100.0], 100, 100));
        assert!(!filters.accepts("person", 0.8, [-1.0, 0.0, 20.0, 20.0], 100, 100));
    }

    #[test]
    fn static_roi_clips_boxes_to_roi_bounds() {
        let detection = Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [550.0, 130.0, 1250.0, 830.0],
            keypoints: None,
            mask: None,
        };

        let clipped = clip_detection_to_roi(
            detection,
            CropRect {
                x1: 560,
                y1: 140,
                x2: 1240,
                y2: 820,
            },
        )
        .unwrap();
        assert_eq!(clipped.bbox, [560.0, 140.0, 1240.0, 820.0]);
    }

    #[test]
    fn static_roi_discards_boxes_outside_roi() {
        let detection = Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [0.0, 0.0, 100.0, 100.0],
            keypoints: None,
            mask: None,
        };

        assert!(
            clip_detection_to_roi(
                detection,
                CropRect {
                    x1: 560,
                    y1: 140,
                    x2: 1240,
                    y2: 820
                }
            )
            .is_none()
        );
    }

    #[test]
    fn explicit_nms_suppresses_overlapping_same_class_only() {
        let detections = vec![
            Detection {
                class: "person".into(),
                confidence: 0.9,
                bbox: [0.0, 0.0, 100.0, 100.0],
                keypoints: None,
                mask: None,
            },
            Detection {
                class: "person".into(),
                confidence: 0.8,
                bbox: [10.0, 10.0, 90.0, 90.0],
                keypoints: None,
                mask: None,
            },
            Detection {
                class: "wheelchair".into(),
                confidence: 0.7,
                bbox: [10.0, 10.0, 90.0, 90.0],
                keypoints: None,
                mask: None,
            },
        ];
        let (kept, suppressed) = apply_nms(detections, 0.5);
        assert_eq!(suppressed, 1);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].class, "person");
        assert_eq!(kept[1].class, "wheelchair");
    }

    #[test]
    fn low_face_nms_iou_keeps_highest_confidence_overlap() {
        let detections = vec![
            Detection {
                class: "face".into(),
                confidence: 0.91,
                bbox: [0.0, 0.0, 100.0, 100.0],
                keypoints: None,
                mask: None,
            },
            Detection {
                class: "face".into(),
                confidence: 0.72,
                bbox: [80.0, 0.0, 180.0, 100.0],
                keypoints: None,
                mask: None,
            },
        ];
        let (kept, suppressed) = apply_nms(detections, 0.05);
        assert_eq!(suppressed, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].confidence, 0.91);
    }

    #[test]
    fn max_detections_keeps_top_confidence_after_nms() {
        let detections = vec![
            Detection {
                class: "face".into(),
                confidence: 0.91,
                bbox: [0.0, 0.0, 50.0, 50.0],
                keypoints: None,
                mask: None,
            },
            Detection {
                class: "face".into(),
                confidence: 0.72,
                bbox: [100.0, 0.0, 150.0, 50.0],
                keypoints: None,
                mask: None,
            },
        ];
        let (mut kept, nms_suppressed) = apply_nms(detections, 0.05);
        let removed = apply_max_detections(&mut kept, Some(1));
        assert_eq!(nms_suppressed + removed, 1);
        assert_eq!(kept[0].confidence, 0.91);
    }
}
