//! mana-geometry/src/iou.rs — Intersection over Union
//! ======================================================
//! Overlap metrics (IoU/IOS) between corner-format boxes.
//! All coordinates in normalized 0..1 space.

/// Overlap metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapMetric {
    Iou,
    Ios,
}

/// Compute overlap (IoU or IOS) between two corner-format boxes.
///
/// Boxes are `(x1, y1, x2, y2)` in any consistent coordinate space.
#[inline]
#[must_use]
pub fn box_overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32), metric: OverlapMetric) -> f32 {
    let inter_x1 = a.0.max(b.0);
    let inter_y1 = a.1.max(b.1);
    let inter_x2 = a.2.min(b.2);
    let inter_y2 = a.3.min(b.3);

    if inter_x1 >= inter_x2 || inter_y1 >= inter_y2 {
        return 0.0;
    }

    let inter = (inter_x2 - inter_x1) * (inter_y2 - inter_y1);
    let area_a = (a.2 - a.0) * (a.3 - a.1);
    let area_b = (b.2 - b.0) * (b.3 - b.1);

    let norm = match metric {
        OverlapMetric::Iou => area_a + area_b - inter,
        OverlapMetric::Ios => area_a.min(area_b),
    };

    if norm <= 0.0 { 0.0 } else { inter / norm }
}

/// Pairwise overlap matrix between two sets of corner-format boxes.
///
/// Returns a flat `Vec<f32>` of length `boxes_a.len() * boxes_b.len()`,
/// row-major (`out[i * boxes_b.len() + j]` is the overlap between
/// `boxes_a[i]` and `boxes_b[j]`).
#[must_use]
pub fn box_overlap_batch(
    boxes_a: &[(f32, f32, f32, f32)],
    boxes_b: &[(f32, f32, f32, f32)],
    metric: OverlapMetric,
) -> Vec<f32> {
    let n = boxes_a.len();
    let m = boxes_b.len();
    let mut out = vec![0.0f32; n * m];
    for (i, &box_a) in boxes_a.iter().enumerate() {
        let row_off = i * m;
        for (j, &box_b) in boxes_b.iter().enumerate() {
            out[row_off + j] = box_overlap(box_a, box_b, metric);
        }
    }
    out
}

/// Compute IoU between a centre-format bounding box and a rectangular zone.
/// Zone is in corner format (x1, y1, x2, y2) normalized 0..1.
#[allow(clippy::too_many_arguments)]
pub fn with_zone(
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    zone_x1: f32,
    zone_y1: f32,
    zone_x2: f32,
    zone_y2: f32,
) -> f32 {
    let (dx1, dy1, dx2, dy2) = crate::bbox::center_to_corners(cx, cy, w, h);

    let inter_x1 = dx1.max(zone_x1);
    let inter_y1 = dy1.max(zone_y1);
    let inter_x2 = dx2.min(zone_x2);
    let inter_y2 = dy2.min(zone_y2);

    if inter_x1 >= inter_x2 || inter_y1 >= inter_y2 {
        return 0.0;
    }

    let inter_area = (inter_x2 - inter_x1) * (inter_y2 - inter_y1);
    let det_area = w * h;
    let zone_area = (zone_x2 - zone_x1) * (zone_y2 - zone_y1);
    let union = det_area + zone_area - inter_area;

    if union <= 0.0 {
        0.0
    } else {
        inter_area / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_overlap_identical() {
        let a = (0.0, 0.0, 10.0, 10.0);
        let v = box_overlap(a, a, OverlapMetric::Iou);
        assert!((v - 1.0).abs() < 1e-6);
    }

    #[test]
    fn box_overlap_ios_larger_than_iou() {
        let a = (0.0, 0.0, 4.0, 4.0);
        let b = (2.0, 2.0, 6.0, 6.0);
        let iou = box_overlap(a, b, OverlapMetric::Iou);
        let ios = box_overlap(a, b, OverlapMetric::Ios);
        assert!(
            ios > iou,
            "IOS ({ios}) should be larger than IOU ({iou}) for different-sized boxes"
        );
    }

    #[test]
    fn box_overlap_no_overlap() {
        let a = (0.0, 0.0, 2.0, 2.0);
        let b = (10.0, 10.0, 12.0, 12.0);
        assert!((box_overlap(a, b, OverlapMetric::Iou) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn box_overlap_batch_shape() {
        let a = vec![(0.0, 0.0, 10.0, 10.0)];
        let b = vec![(5.0, 5.0, 15.0, 15.0), (20.0, 20.0, 30.0, 30.0)];
        let mat = box_overlap_batch(&a, &b, OverlapMetric::Iou);
        assert_eq!(mat.len(), 2);
        // first pair: box_a[0] vs box_b[0] should overlap
        assert!(mat[0] > 0.0);
        // second pair: box_a[0] vs box_b[1] should not overlap
        assert!((mat[1] - 0.0).abs() < 1e-6);
    }
}
