//! mana-geometry/src/bbox.rs — Bounding Box Operations
//! ======================================================
//! All coordinates are normalized 0..1 (fraction of frame).
//! All functions are const/plain — no alloc, no std.

/// Convert center-format (cx, cy, w, h) to corner-format (x1, y1, x2, y2).
#[inline]
pub fn center_to_corners(cx: f32, cy: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    (cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0)
}

/// Convert corner-format (x1, y1, x2, y2) to center-format (cx, cy, w, h).
#[inline]
pub fn corners_to_center(x1: f32, y1: f32, x2: f32, y2: f32) -> (f32, f32, f32, f32) {
    ((x1 + x2) / 2.0, (y1 + y2) / 2.0, x2 - x1, y2 - y1)
}

/// Scale normalized coordinates to pixel coordinates.
#[inline]
pub fn to_pixels(norm_x: f32, norm_y: f32, frame_w: u32, frame_h: u32) -> (f32, f32) {
    (norm_x * frame_w as f32, norm_y * frame_h as f32)
}

/// Scale normalized centre+size to pixel (centre, half_w, half_h) for
/// Rerun `Boxes2D::from_centers_and_half_sizes`.
#[inline]
pub fn box_halfsize_to_pixels(
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    frame_w: u32,
    frame_h: u32,
) -> (f32, f32, f32, f32) {
    let fw = frame_w as f32;
    let fh = frame_h as f32;
    (cx * fw, cy * fh, w * fw / 2.0, h * fh / 2.0)
}

/// Clamp a normalized value to [0.0, 1.0].
#[inline]
pub fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

// ── Pixel-space box operations ────────────────────────────────────

/// Clip corner-format boxes `(x1, y1, x2, y2)` to `(0, 0, w_px, h_px)`.
///
/// Input/output in pixel coordinates.
#[inline]
pub fn clip_boxes_px(xyxy: &mut [(f32, f32, f32, f32)], frame_w: u32, frame_h: u32) {
    let fw = frame_w as f32;
    let fh = frame_h as f32;
    for box_ in xyxy.iter_mut() {
        box_.0 = box_.0.clamp(0.0, fw);
        box_.1 = box_.1.clamp(0.0, fh);
        box_.2 = box_.2.clamp(0.0, fw);
        box_.3 = box_.3.clamp(0.0, fh);
    }
}

/// Pad corner-format boxes by pixel margin.
///
/// Expands each box outward by `px` horizontally and `py` vertically.
/// `py` defaults to `px` when `None`.
pub fn pad_boxes_px(xyxy: &mut [(f32, f32, f32, f32)], px: f32, py: Option<f32>) {
    let py = py.unwrap_or(px);
    for box_ in xyxy.iter_mut() {
        box_.0 -= px;
        box_.1 -= py;
        box_.2 += px;
        box_.3 += py;
    }
}

/// Scale corner-format boxes about their centre.
///
/// `factor > 1.0` enlarges, `factor < 1.0` shrinks.
pub fn scale_boxes_px(xyxy: &mut [(f32, f32, f32, f32)], factor: f32) {
    for box_ in xyxy.iter_mut() {
        let cx = (box_.0 + box_.2) * 0.5;
        let cy = (box_.1 + box_.3) * 0.5;
        let hw = (box_.2 - box_.0) * 0.5 * factor;
        let hh = (box_.3 - box_.1) * 0.5 * factor;
        box_.0 = cx - hw;
        box_.1 = cy - hh;
        box_.2 = cx + hw;
        box_.3 = cy + hh;
    }
}

/// Convert normalised `[0,1]` corner-format boxes to pixel coordinates.
pub fn denormalize_boxes(
    xyxy: &[(f32, f32, f32, f32)],
    frame_w: u32,
    frame_h: u32,
) -> Vec<(f32, f32, f32, f32)> {
    let fw = frame_w as f32;
    let fh = frame_h as f32;
    xyxy.iter()
        .map(|(x1, y1, x2, y2)| (x1 * fw, y1 * fh, x2 * fw, y2 * fh))
        .collect()
}

// ── Format converters ────────────────────────────────────────────

/// Convert corner-format `(x1, y1, x2, y2)` to `(cx, cy, aspect_ratio, height)`.
///
/// This is the measurement-space format used by SORT/DeepSORT Kalman filters.
pub fn xyxy_to_xcycarh(x1: f32, y1: f32, x2: f32, y2: f32) -> (f32, f32, f32, f32) {
    let w = x2 - x1;
    let h = y2 - y1;
    let cx = x1 + w * 0.5;
    let cy = y1 + h * 0.5;
    let ar = if h > 1e-10 { w / h } else { 0.0 };
    (cx, cy, ar, h)
}

/// Convert `(cx, cy, aspect_ratio, height)` back to corner format.
pub fn xcycarh_to_xyxy(cx: f32, cy: f32, ar: f32, h: f32) -> (f32, f32, f32, f32) {
    let w = ar * h;
    let hw = w * 0.5;
    let hh = h * 0.5;
    (cx - hw, cy - hh, cx + hw, cy + hh)
}

/// Clamp a bounding box to [0, 1] range.
///
/// Shrinks width/height if the box centre is too close to the frame edge
/// (so the box stays entirely within [0, 1]).
#[inline]
pub fn clamp_box(cx: f32, cy: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    let cx = clamp01(cx);
    let cy = clamp01(cy);
    let max_half_w = cx.min(1.0 - cx);
    let max_half_h = cy.min(1.0 - cy);
    (cx, cy, w.min(2.0 * max_half_w), h.min(2.0 * max_half_h))
}

/// Area of a bounding box (corner format).
#[inline]
pub fn area(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    (x2 - x1) * (y2 - y1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_to_corners_and_back() {
        let (x1, y1, x2, y2) = center_to_corners(0.5, 0.5, 0.2, 0.4);
        let (cx, cy, w, h) = corners_to_center(x1, y1, x2, y2);
        assert!((cx - 0.5).abs() < 1e-6);
        assert!((cy - 0.5).abs() < 1e-6);
        assert!((w - 0.2).abs() < 1e-6);
        assert!((h - 0.4).abs() < 1e-6);
    }

    #[test]
    fn to_pixels_4k() {
        let (px, py) = to_pixels(0.5, 0.25, 3840, 2160);
        assert!((px - 1920.0).abs() < 1.0);
        assert!((py - 540.0).abs() < 1.0);
    }

    #[test]
    fn clamp_edges() {
        let (_cx, _cy, w, _h) = clamp_box(0.05, 0.5, 0.3, 0.4);
        // center near left edge, w should be clamped to 2*0.05 = 0.1
        assert!(
            (w - 0.1).abs() < 1e-6,
            "w should be clamped to 2 * min(x, 1-x) = 0.1, got {}",
            w
        );
    }

    #[test]
    fn clamp_01_bounds() {
        assert_eq!(clamp01(-0.5), 0.0);
        assert_eq!(clamp01(1.5), 1.0);
        assert_eq!(clamp01(0.5), 0.5);
    }

    #[test]
    fn clip_boxes_px_clamps_oob() {
        let mut boxes = vec![(-10.0, -5.0, 700.0, 500.0)];
        clip_boxes_px(&mut boxes, 640, 480);
        assert_eq!(boxes[0], (0.0, 0.0, 640.0, 480.0));
    }

    #[test]
    fn pad_boxes_expands() {
        let mut boxes = vec![(10.0, 20.0, 30.0, 40.0)];
        pad_boxes_px(&mut boxes, 5.0, Some(10.0));
        assert_eq!(boxes[0], (5.0, 10.0, 35.0, 50.0));
    }

    #[test]
    fn pad_boxes_default_py() {
        let mut boxes = vec![(10.0, 10.0, 20.0, 20.0)];
        pad_boxes_px(&mut boxes, 3.0, None);
        assert_eq!(boxes[0], (7.0, 7.0, 23.0, 23.0));
    }

    #[test]
    fn scale_boxes_enlarge() {
        let mut boxes = vec![(0.0, 0.0, 10.0, 10.0)];
        scale_boxes_px(&mut boxes, 2.0);
        assert_eq!(boxes[0], (-5.0, -5.0, 15.0, 15.0));
    }

    #[test]
    fn scale_boxes_shrink() {
        let mut boxes = vec![(0.0, 0.0, 10.0, 10.0)];
        scale_boxes_px(&mut boxes, 0.5);
        assert_eq!(boxes[0], (2.5, 2.5, 7.5, 7.5));
    }

    #[test]
    fn denormalize_to_pixels() {
        let boxes = vec![(0.1, 0.2, 0.5, 0.6)];
        let px = denormalize_boxes(&boxes, 640, 480);
        assert!((px[0].0 - 64.0).abs() < 1.0);
        assert!((px[0].1 - 96.0).abs() < 1.0);
        assert!((px[0].2 - 320.0).abs() < 1.0);
        assert!((px[0].3 - 288.0).abs() < 1.0);
    }

    #[test]
    fn xyxy_to_xcycarh_roundtrip() {
        let (cx, cy, ar, h) = xyxy_to_xcycarh(10.0, 20.0, 40.0, 60.0);
        assert!((cx - 25.0).abs() < 1e-6);
        assert!((cy - 40.0).abs() < 1e-6);
        assert!((ar - 0.75).abs() < 1e-6);
        assert!((h - 40.0).abs() < 1e-6);
        let (x1, y1, x2, y2) = xcycarh_to_xyxy(cx, cy, ar, h);
        assert!((x1 - 10.0).abs() < 1e-6);
        assert!((y1 - 20.0).abs() < 1e-6);
        assert!((x2 - 40.0).abs() < 1e-6);
        assert!((y2 - 60.0).abs() < 1e-6);
    }

    #[test]
    fn xcycarh_zero_height() {
        let (_, _, ar, _) = xyxy_to_xcycarh(0.0, 0.0, 10.0, 0.0);
        assert!((ar - 0.0).abs() < 1e-6);
    }

    #[test]
    fn halfsize_to_pixels_1080p() {
        let (cx, cy, hw, hh) = box_halfsize_to_pixels(0.5, 0.5, 0.2, 0.4, 1920, 1080);
        assert!((cx - 960.0).abs() < 1.0);
        assert!((cy - 540.0).abs() < 1.0);
        assert!((hw - 192.0).abs() < 1.0);
        assert!((hh - 216.0).abs() < 1.0);
    }

    #[test]
    fn halfsize_to_pixels_zero_dims_handled() {
        let (cx, cy, hw, hh) = box_halfsize_to_pixels(0.0, 0.0, 1.0, 1.0, 0, 0);
        assert!((cx - 0.0).abs() < f32::EPSILON);
        assert!((cy - 0.0).abs() < f32::EPSILON);
        assert!((hw - 0.0).abs() < f32::EPSILON);
        assert!((hh - 0.0).abs() < f32::EPSILON);
    }
}

/// Distance from a point to the nearest edge of an axis-aligned rectangle.
/// All coordinates normalized [0, 1]. Returns the minimum distance to any edge.
#[inline]
pub fn point_to_rect_edge_dist(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dl = (px - x1).abs();
    let dr = (x2 - px).abs();
    let dt = (py - y1).abs();
    let db = (y2 - py).abs();
    dl.min(dr).min(dt).min(db)
}

/// Check if a point is inside an axis-aligned rectangle (inclusive edges).
#[inline]
pub fn point_in_rect(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> bool {
    px >= x1 && px <= x2 && py >= y1 && py <= y2
}
