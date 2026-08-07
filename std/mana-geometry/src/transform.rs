//! mana-geometry/src/transform.rs — Letterbox coordinate transforms.
//! ==============================================================
//! Translates between inference-space (model input) and original image-space.
//! Used by every YOLO postprocess pass (detect, pose, segment).
//!
//! All coordinates in normalized 0..1 unless stated otherwise.

/// Undo letterbox padding on a single coordinate.
///
/// Maps from model-input pixel space back to ROI-pixel space:
/// `(raw_coord - pad) / scale`. Use `pad = 0.0` for width/height (sizes
/// don't have a letterbox offset).
///
/// Called per-detection, per-keypoint in the inference hot path —
/// `#[inline(always)]` ensures it's never outlined across crate boundaries.
#[inline(always)]
pub fn unletterbox(raw_coord: f32, pad: f32, scale: f32) -> f32 {
    (raw_coord - pad) / scale
}

/// Carries the parameters needed to reverse a letterbox preprocess.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    /// Original image dimensions `(height, width)` in pixels.
    pub orig_shape: (u32, u32),
    /// Scale factor applied to fit image into inference size.
    pub scale: f32,
    /// Padding `(top, left)` in inference-space pixels.
    pub padding: (f32, f32),
}

impl Transform {
    #[must_use]
    pub const fn identity(orig_shape: (u32, u32)) -> Self {
        Self {
            orig_shape,
            scale: 1.0,
            padding: (0.0, 0.0),
        }
    }

    /// Map centre-form box `(cx, cy, w, h)` from inference-space to image-space.
    #[inline]
    pub fn scale_coords(&self, coords: &[f32; 4]) -> [f32; 4] {
        let (pt, pl) = self.padding;
        let s = self.scale;
        [
            (coords[0] - pl) / s,
            (coords[1] - pt) / s,
            (coords[2] - pl) / s,
            (coords[3] - pt) / s,
        ]
    }

    /// Clamp corner-form box to image bounds.
    #[inline]
    pub fn clip_coords(&self, coords: &[f32; 4]) -> [f32; 4] {
        let h = self.orig_shape.0 as f32;
        let w = self.orig_shape.1 as f32;
        [
            coords[0].clamp(0.0, w),
            coords[1].clamp(0.0, h),
            coords[2].clamp(0.0, w),
            coords[3].clamp(0.0, h),
        ]
    }

    /// Scale then clip in one call.
    #[inline]
    pub fn scale_and_clip(&self, coords: &[f32; 4]) -> [f32; 4] {
        let s = self.scale_coords(coords);
        self.clip_coords(&s)
    }
}

/// Convert centre-form `(cx, cy, w, h)` to corner form `[x1, y1, x2, y2]`.
#[inline]
#[must_use]
pub fn xywh_to_xyxy(cx: f32, cy: f32, w: f32, h: f32) -> [f32; 4] {
    [cx - w * 0.5, cy - h * 0.5, cx + w * 0.5, cy + h * 0.5]
}

/// Calculate intersection-over-union of two axis-aligned boxes in corner form.
#[must_use]
pub fn calculate_iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    crate::iou::box_overlap(
        (a[0], a[1], a[2], a[3]),
        (b[0], b[1], b[2], b[3]),
        crate::iou::OverlapMetric::Iou,
    )
}

/// Compute proto mask crop region from letterbox geometry.
///
/// Given the model input size and the ROI (region-of-interest) dimensions,
/// returns `(crop_x, crop_y, crop_w, crop_h)` in proto pixel space — the
/// sub-region of the proto mask that corresponds to actual image content
/// (excluding letterbox padding).
///
/// Used by segment decoders across all families (YOLO, YOLO26, RF-DETR) to
/// crop proto masks before bilinear resize.
#[must_use]
pub fn proto_crop_region(
    input_w: u32,
    input_h: u32,
    roi_w: u32,
    roi_h: u32,
    proto_w: u32,
    proto_h: u32,
) -> (usize, usize, usize, usize) {
    debug_assert!(
        roi_w > 0 && roi_h > 0,
        "proto_crop_region: roi_w and roi_h must be > 0"
    );
    debug_assert!(
        proto_w > 0 && proto_h > 0,
        "proto_crop_region: proto_w and proto_h must be > 0"
    );
    let iw = input_w as f32;
    let ih = input_h as f32;
    let scale_in = (iw / roi_w as f32).min(ih / roi_h as f32);
    let pad_left = (iw - roi_w as f32 * scale_in) / 2.0;
    let pad_top = (ih - roi_h as f32 * scale_in) / 2.0;

    let proto_scale_w = proto_w as f32 / iw;
    let proto_scale_h = proto_h as f32 / ih;
    let crop_x = (pad_left * proto_scale_w).max(0.0) as usize;
    let crop_y = (pad_top * proto_scale_h).max(0.0) as usize;
    let crop_w = ((proto_w as f32 - 2.0 * crop_x as f32).max(1.0)) as usize;
    let crop_h = ((proto_h as f32 - 2.0 * crop_y as f32).max(1.0)) as usize;

    (crop_x, crop_y, crop_w, crop_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xywh_center_square() {
        let b = xywh_to_xyxy(5.0, 5.0, 4.0, 4.0);
        assert!((b[0] - 3.0).abs() < 1e-6);
        assert!((b[3] - 7.0).abs() < 1e-6);
    }

    #[test]
    fn transform_scale_and_clip() {
        let t = Transform {
            orig_shape: (480, 640),
            scale: 1.0,
            padding: (10.0, 10.0),
        };
        let s = t.scale_coords(&[120.0, 100.0, 220.0, 200.0]);
        assert!((s[0] - 110.0).abs() < 1e-6);
    }

    #[test]
    fn transform_clip_oob() {
        let t = Transform::identity((480, 640));
        let c = t.clip_coords(&[-10.0, -20.0, 700.0, 520.0]);
        assert_eq!(c, [0.0, 0.0, 640.0, 480.0]);
    }

    #[test]
    fn iou_half() {
        let a = [0.0, 0.0, 10.0, 10.0];
        let b = [5.0, 5.0, 15.0, 15.0];
        let v = calculate_iou(&a, &b);
        assert!((v - 0.142857).abs() < 0.001);
    }
}
