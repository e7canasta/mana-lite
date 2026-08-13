//! Post-predict helpers for InferEngine::run (translate, filter, NMS, masks).

use std::sync::Arc;

use mana_geometry::compact_mask::CompactMask;
use mana_geometry::iou::{OverlapMetric, box_overlap};
use mana_geometry::polygonize::{filter_small_components, mask_to_polygons};
use ndarray::s;
use ultralytics_inference::{DepthMap, Results};

use crate::config::PostprocessConfig;
use crate::detection::{CropRect, Detection, DetectionMask};

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn translate_detections_to_frame(
    detections: &mut [Detection],
    offset_x: f32,
    offset_y: f32,
    w: u32,
    h: u32,
) {
    for d in detections {
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
                    vertex[0] = (vertex[0] * mask.mask_dims[0] as f32 + mask.origin[0] as f32) / fw;
                    vertex[1] = (vertex[1] * mask.mask_dims[1] as f32 + mask.origin[1] as f32) / fh;
                }
            }
        }
    }
}

pub(super) fn apply_static_roi_filter(
    detections: &mut Vec<Detection>,
    static_roi: Option<CropRect>,
    model_key: &str,
) -> usize {
    let Some(roi) = static_roi else {
        return 0;
    };
    let before_roi = detections.len();
    let filtered: Vec<Detection> = std::mem::take(detections)
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
    let rejected = before_roi - filtered.len();
    *detections = filtered;
    rejected
}

pub(super) fn apply_postprocess_filter(
    detections: &mut Vec<Detection>,
    postprocess: &PostprocessConfig,
    crop_rect: Option<CropRect>,
    w: u32,
    h: u32,
    model_key: &str,
) -> usize {
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
        let reason = postprocess.rejection_reason(
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
    before_postprocess - detections.len()
}

pub fn collect_detections(
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

pub(super) fn take_depth(results: &mut [Results]) -> Option<DepthMap> {
    results.iter_mut().find_map(|result| result.depth.take())
}

pub(super) fn translate_keypoints(keypoints: &mut [[f32; 3]], offset_x: f32, offset_y: f32) {
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
pub(super) fn build_detection_mask(
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
    let (x1, y1, x2, y2) = bbox_mask_crop(bbox, mask_w, mask_h)?;
    let crop_w = x2 - x1;
    let crop_h = y2 - y1;
    let crop_bits = threshold_mask_crop(mask_slice, x1, y1, crop_w, crop_h, mask_threshold)?;
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
    let polygons = mask_crop_polygons(
        &crop_bits,
        crop_w,
        crop_h,
        mask_w,
        mask_h,
        x1,
        y1,
        mask_threshold,
        polygon_simplify,
    );

    Some(DetectionMask {
        compact: Arc::new(compact),
        polygons: Arc::new(polygons),
        origin: [0, 0],
        mask_dims: [mask_w, mask_h],
    })
}

fn bbox_mask_crop(bbox: [f32; 4], mask_w: u32, mask_h: u32) -> Option<(u32, u32, u32, u32)> {
    let x1 = bbox[0].floor().max(0.0) as u32;
    let y1 = bbox[1].floor().max(0.0) as u32;
    let x2 = bbox[2].ceil().min(mask_w as f32) as u32;
    let y2 = bbox[3].ceil().min(mask_h as f32) as u32;
    (x2 > x1 && y2 > y1).then_some((x1, y1, x2, y2))
}

fn threshold_mask_crop(
    mask_slice: ndarray::ArrayView2<f32>,
    x1: u32,
    y1: u32,
    crop_w: u32,
    crop_h: u32,
    mask_threshold: f32,
) -> Option<Vec<u8>> {
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
    above_threshold.then_some(crop_bits)
}

/// Contours over the bbox crop only: `mask_to_polygons` returns vertices
/// already normalized 0..1 over the crop, and the crop is offset by
/// (x1, y1) inside the mask output. Rescale to mask-space normalization
/// by (crop/mask) and add the normalized offset — otherwise the polygon
/// is anchored to the mask-space origin and lands far from its bbox.
fn mask_crop_polygons(
    crop_bits: &[u8],
    crop_w: u32,
    crop_h: u32,
    mask_w: u32,
    mask_h: u32,
    x1: u32,
    y1: u32,
    mask_threshold: f32,
    polygon_simplify: f64,
) -> Vec<Vec<[f32; 2]>> {
    let crop_flat: Vec<f32> = crop_bits.iter().map(|&b| b as f32).collect();
    let scale_x = crop_w as f32 / mask_w as f32;
    let scale_y = crop_h as f32 / mask_h as f32;
    let off_x = x1 as f32 / mask_w as f32;
    let off_y = y1 as f32 / mask_h as f32;
    mask_to_polygons(
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
    .collect()
}

pub(super) fn clip_detection_to_roi(mut detection: Detection, roi: CropRect) -> Option<Detection> {
    detection.bbox[0] = detection.bbox[0].max(roi.x1 as f32);
    detection.bbox[1] = detection.bbox[1].max(roi.y1 as f32);
    detection.bbox[2] = detection.bbox[2].min(roi.x2 as f32);
    detection.bbox[3] = detection.bbox[3].min(roi.y2 as f32);
    (detection.bbox[2] > detection.bbox[0] && detection.bbox[3] > detection.bbox[1])
        .then_some(detection)
}

pub(super) fn apply_nms(
    mut detections: Vec<Detection>,
    iou_threshold: f32,
) -> (Vec<Detection>, usize) {
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

pub(super) fn apply_max_detections(
    detections: &mut Vec<Detection>,
    max_detections: Option<usize>,
) -> usize {
    let Some(max_detections) = max_detections else {
        return 0;
    };
    let removed = detections.len().saturating_sub(max_detections);
    detections.truncate(max_detections);
    removed
}
