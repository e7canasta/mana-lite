//! Segment postprocess helpers (formerly in mana-geometry).
//!
//! Kept here so mana-geometry stays free of inference-family payload types.

#![allow(dead_code)]

use mana_geometry::compact_mask::CompactMask;

// ── Shared segment utilities (used by YOLO, YOLO26, RF-DETR) ────────

/// Per-segment decode result: the raw output of a segment postprocessor
/// before formatting into the wire ABI. Shared across all three inference
/// families (YOLO, YOLO26, RF-DETR).
#[derive(Debug, Clone)]
pub struct SegmentResult {
    pub class_id: u16,
    pub confidence: f32,
    pub box_x: f32,
    pub box_y: f32,
    pub box_w: f32,
    pub box_h: f32,
    pub polygons: Vec<Vec<[f32; 2]>>,
    pub compact_mask_payload: Option<Vec<u8>>,
    pub crop_h: u16,
    pub crop_w: u16,
}

/// Build a CompactMask wire-format payload from a full-frame f32 mask
/// and bbox pixel coords.
///
/// Extracts the bbox-scale crop from `mask` (row-major, length `frame_w * frame_h`),
/// thresholds at `threshold`, encodes losslessly via [`CompactMask::from_dense`],
/// and serialises the iceoryx2 wire-format payload.
///
/// Returns `None` if the bbox is degenerate (zero-area) or encoding fails.
/// On success returns `(payload, crop_width, crop_height)`.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn build_compact_payload(
    mask: &[f32],
    ox: usize,
    oy: usize,
    x2: usize,
    y2: usize,
    frame_w: usize,
    frame_h: usize,
    threshold: f32,
) -> Option<(Vec<u8>, u16, u16)> {
    let crop_w = x2.saturating_sub(ox);
    let crop_h = y2.saturating_sub(oy);
    if crop_w == 0 || crop_h == 0 {
        return None;
    }

    let mut crop_u8 = vec![0u8; crop_w * crop_h];
    for dy in 0..crop_h {
        let src_off = (oy + dy) * frame_w + ox;
        let dst_off = dy * crop_w;
        for dx in 0..crop_w {
            crop_u8[dst_off + dx] = if mask[src_off + dx] >= threshold {
                1
            } else {
                0
            };
        }
    }

    CompactMask::from_dense(
        &crop_u8,
        crop_h as u32,
        crop_w as u32,
        (ox as u32, oy as u32),
        (frame_h as u32, frame_w as u32),
    )
    .ok()
    .and_then(|cm| cm.encode_iceoryx2_payload(0).ok())
    .map(|payload| (payload, crop_w as u16, crop_h as u16))
}

/// Bilinear resize for a single-channel float mask.
///
/// Resizes `data` (layout: row-major, `src_w × src_h`) to `dst_w × dst_h`.
/// Used by both classic and end-to-end segment decoders across all families.
pub fn bilinear_resize_mask(
    data: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<f32> {
    let mut dst = vec![0.0_f32; dst_w * dst_h];
    let scale_x = src_w as f32 / dst_w as f32;
    let scale_y = src_h as f32 / dst_h as f32;

    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let sx = (dx as f32 + 0.5) * scale_x - 0.5;
            let sy = (dy as f32 + 0.5) * scale_y - 0.5;

            let x0 = (sx.floor() as isize).max(0).min(src_w as isize - 1) as usize;
            let y0 = (sy.floor() as isize).max(0).min(src_h as isize - 1) as usize;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);

            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;

            let v = (1.0 - fx) * (1.0 - fy) * data[y0 * src_w + x0]
                + fx * (1.0 - fy) * data[y0 * src_w + x1]
                + (1.0 - fx) * fy * data[y1 * src_w + x0]
                + fx * fy * data[y1 * src_w + x1];

            dst[dy * dst_w + dx] = v;
        }
    }

    dst
}

