//! Crop / ROI helpers for cascaded inference.

use image::RgbImage;

use super::CropFrameInfo;
use crate::detection::{CropRect, Detection};

pub(super) fn extract_crop_frame(
    rgb: &[u8],
    frame_w: u32,
    frame_h: u32,
    rect: CropRect,
) -> Option<CropFrameInfo> {
    let crop_w = rect.x2.checked_sub(rect.x1)?;
    let crop_h = rect.y2.checked_sub(rect.y1)?;
    if crop_w == 0
        || crop_h == 0
        || rect.x2 > frame_w
        || rect.y2 > frame_h
        || rgb.len()
            < (frame_w as usize)
                .checked_mul(frame_h as usize)?
                .checked_mul(3)?
    {
        return None;
    }

    let mut cropped = vec![
        0u8;
        (crop_w as usize)
            .checked_mul(crop_h as usize)?
            .checked_mul(3)?
    ];
    for row in rect.y1..rect.y2 {
        let src_off = ((row as usize) * frame_w as usize + rect.x1 as usize) * 3;
        let dst_off = ((row - rect.y1) as usize * crop_w as usize) * 3;
        let row_len = crop_w as usize * 3;
        cropped[dst_off..dst_off + row_len].copy_from_slice(&rgb[src_off..src_off + row_len]);
    }

    Some(CropFrameInfo {
        rgb: cropped,
        w: crop_w,
        h: crop_h,
    })
}

pub fn compute_largest_class_roi(
    detections: &[Detection],
    target_class: &str,
    margin: f32,
    frame_w: u32,
    frame_h: u32,
    min_region: Option<[u32; 4]>,
    max_region: Option<[u32; 4]>,
) -> Option<CropRect> {
    let best_bbox = detections
        .iter()
        .filter(|d| d.class == target_class)
        .max_by(|a, b| {
            let area_a = (a.bbox[2] - a.bbox[0]) * (a.bbox[3] - a.bbox[1]);
            let area_b = (b.bbox[2] - b.bbox[0]) * (b.bbox[3] - b.bbox[1]);
            area_a
                .partial_cmp(&area_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|d| d.bbox);

    match best_bbox {
        Some(bbox) => compute_bbox_roi(bbox, margin, frame_w, frame_h, min_region, max_region),
        None => min_region.and_then(|region| {
            compute_bbox_roi(
                [
                    region[0] as f32,
                    region[1] as f32,
                    region[2] as f32,
                    region[3] as f32,
                ],
                0.0,
                frame_w,
                frame_h,
                None,
                max_region,
            )
        }),
    }
}

pub fn compute_bbox_roi(
    bbox: [f32; 4],
    margin: f32,
    frame_w: u32,
    frame_h: u32,
    min_region: Option<[u32; 4]>,
    max_region: Option<[u32; 4]>,
) -> Option<CropRect> {
    let bw = (bbox[2] - bbox[0]).max(0.0);
    let bh = (bbox[3] - bbox[1]).max(0.0);
    if bw <= 0.0 || bh <= 0.0 {
        return None;
    }

    let expand_w = bw * margin;
    let expand_h = bh * margin;
    let class_rect = CropRect {
        x1: (bbox[0] - expand_w).max(0.0) as u32,
        y1: (bbox[1] - expand_h).max(0.0) as u32,
        x2: ((bbox[2] + expand_w) as u32).min(frame_w),
        y2: ((bbox[3] + expand_h) as u32).min(frame_h),
    };

    let mut result = match min_region {
        Some([mx1, my1, mx2, my2]) => CropRect {
            x1: class_rect.x1.min(mx1),
            y1: class_rect.y1.min(my1),
            x2: class_rect.x2.max(mx2),
            y2: class_rect.y2.max(my2),
        },
        None => class_rect,
    };

    if let Some([mx1, my1, mx2, my2]) = max_region {
        result.x1 = result.x1.max(mx1);
        result.y1 = result.y1.max(my1);
        result.x2 = result.x2.min(mx2);
        result.y2 = result.y2.min(my2);
    }

    if result.x2 <= result.x1 || result.y2 <= result.y1 {
        None
    } else {
        Some(result)
    }
}

/// Builds a square crop centered on the upper part of a parent bbox.
/// This keeps small faces from being diluted by the full-height person crop.
pub fn compute_upper_square_roi(
    bbox: [f32; 4],
    square_size: u32,
    upper_fraction: f32,
    frame_w: u32,
    frame_h: u32,
) -> Option<CropRect> {
    let bw = (bbox[2] - bbox[0]).max(0.0);
    let bh = (bbox[3] - bbox[1]).max(0.0);
    if bw <= 0.0 || bh <= 0.0 || square_size == 0 || frame_w == 0 || frame_h == 0 {
        return None;
    }

    let upper_fraction = upper_fraction.clamp(0.0, 1.0);
    if upper_fraction <= 0.0 {
        return None;
    }

    let side = (square_size as f32)
        .max(bw)
        .min(frame_w as f32)
        .min(frame_h as f32)
        .round() as u32;
    let center_x = (bbox[0] + bbox[2]) / 2.0;
    let upper_center_y = bbox[1] + bh * upper_fraction / 2.0;
    let max_x = (frame_w - side) as f32;
    let max_y = (frame_h - side) as f32;
    let x1 = (center_x - side as f32 / 2.0).round().clamp(0.0, max_x) as u32;
    let y1 = (upper_center_y - side as f32 / 2.0)
        .round()
        .clamp(0.0, max_y) as u32;

    Some(CropRect {
        x1,
        y1,
        x2: x1 + side,
        y2: y1 + side,
    })
}
