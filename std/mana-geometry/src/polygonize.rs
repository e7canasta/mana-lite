//! Mask polygonization and scanline fill (mechanism).
//!
//! [`mask_to_polygons`] turns a probability mask into simplified normalized
//! polygons (contour following + Ramer-Douglas-Peucker). The crate-internal
//! scanline fill rasterizes polygons back onto an overlay buffer for the
//! segmentation policy.

use image::GrayImage;
use image::Luma;
use image::{Rgb, RgbImage};
use imageproc::contours::find_contours;
use imageproc::geometry::approximate_polygon_dp;
use imageproc::point::Point;

/// Extract simplified polygons from a binary mask using contour detection.
///
/// Uses Suzuki-Abe border following (`imageproc::contours::find_contours`),
/// Ramer-Douglas-Peucker simplification with iterative epsilon search,
/// and optional area filtering. `min_area` / `max_area` are fractions of the
/// total mask area; vertices are returned normalized to 0..1.
///
/// # Example
///
/// ```
/// use mana_geometry::polygonize::mask_to_polygons;
///
/// // A solid 6x6 block inside an 8x8 probability mask.
/// let mut mask = vec![0.0f32; 8 * 8];
/// for y in 1..7 {
///     for x in 1..7 {
///         mask[y * 8 + x] = 1.0;
///     }
/// }
/// let polys = mask_to_polygons(&mask, 8, 8, 0.5, 0.0, None, None);
/// assert!(!polys.is_empty());
/// ```
pub fn mask_to_polygons(
    mask: &[f32],
    w: usize,
    h: usize,
    mask_threshold: f32,
    approximation_percentage: f64,
    min_area: Option<f32>,
    max_area: Option<f32>,
) -> Vec<Vec<[f32; 2]>> {
    if mask.is_empty() || w == 0 || h == 0 {
        return Vec::new();
    }

    let mut gray = GrayImage::new(w as u32, h as u32);
    let mut all_above = true;
    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let set = idx < mask.len() && mask[idx] > mask_threshold;
            if set {
                gray.put_pixel(x as u32, y as u32, Luma([255u8]));
            } else {
                all_above = false;
            }
        }
    }

    let mut contour_points: Vec<Vec<Point<i32>>> = find_contours::<i32>(&gray)
        .into_iter()
        .map(|c| c.points)
        .collect();

    // imageproc's Suzuki-Abe assumes a background border: a fully-white mask
    // (detection covers the whole region) yields no contours. A full mask is
    // a valid shape — its contour is the image border rectangle.
    if contour_points.is_empty() && all_above && w > 1 && h > 1 {
        contour_points.push(vec![
            Point::new(0, 0),
            Point::new(w as i32 - 1, 0),
            Point::new(w as i32 - 1, h as i32 - 1),
            Point::new(0, h as i32 - 1),
        ]);
    }

    let total_area = (w * h) as f32;
    let min_px = min_area.map(|pct| (pct * total_area) as f64);
    let max_px = max_area.map(|pct| (pct * total_area) as f64);
    let pct = approximation_percentage.clamp(0.0, 0.99);

    let mut polygons: Vec<Vec<[f32; 2]>> = Vec::new();

    for points in &contour_points {
        if points.len() < 3 {
            continue;
        }

        let simplified = simplify_contour(points, pct);
        if simplified.len() < 3 {
            continue;
        }

        if min_px.is_some() || max_px.is_some() {
            let area = polygon_area(&simplified);
            if let Some(min_a) = min_px {
                if area < min_a {
                    continue;
                }
            }
            if let Some(max_a) = max_px {
                if area > max_a {
                    continue;
                }
            }
        }

        let wf = w as f32;
        let hf = h as f32;
        let norm: Vec<[f32; 2]> = simplified
            .iter()
            .map(|p| [p.x as f32 / wf, p.y as f32 / hf])
            .collect();
        polygons.push(norm);
    }

    polygons
}

/// Remove disconnected foreground components smaller than a fraction of the
/// mask crop area.
///
/// Components use 8-connectivity so diagonally touching foreground pixels
/// remain part of the same body. The returned raster has the same dimensions
/// and contains only components whose pixel area is at least
/// `min_area_ratio * (w * h)`. A non-positive ratio keeps the input unchanged.
pub fn filter_small_components(mask: &[u8], w: usize, h: usize, min_area_ratio: f32) -> Vec<u8> {
    let Some(len) = w.checked_mul(h) else {
        return Vec::new();
    };
    if len == 0 || mask.len() < len {
        return Vec::new();
    }
    if min_area_ratio <= 0.0 {
        return mask[..len].to_vec();
    }

    let min_pixels = ((len as f32) * min_area_ratio).ceil() as usize;
    let mut visited = vec![false; len];
    let mut filtered = vec![0u8; len];
    let neighbors = [
        (-1isize, -1isize),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];

    for y in 0..h {
        for x in 0..w {
            let start = y * w + x;
            if mask[start] == 0 || visited[start] {
                continue;
            }

            let mut queue = vec![start];
            let mut component = Vec::new();
            visited[start] = true;

            while let Some(index) = queue.pop() {
                component.push(index);
                let current_x = index % w;
                let current_y = index / w;

                for (dx, dy) in neighbors {
                    let next_x = current_x as isize + dx;
                    let next_y = current_y as isize + dy;
                    if next_x < 0 || next_y < 0 || next_x >= w as isize || next_y >= h as isize {
                        continue;
                    }
                    let next = next_y as usize * w + next_x as usize;
                    if mask[next] != 0 && !visited[next] {
                        visited[next] = true;
                        queue.push(next);
                    }
                }
            }

            if component.len() >= min_pixels {
                for index in component {
                    filtered[index] = mask[index];
                }
            }
        }
    }

    filtered
}

/// Fill polygons onto an RGB image buffer using per-class color.
///
/// Each polygon is rasterized via a simple scanline fill.
/// Returns true if any pixels were drawn.
#[allow(dead_code)]
pub(crate) fn fill_polygons_into_image(
    img: &mut RgbImage,
    polygons: &[Vec<[f32; 2]>],
    mask_dims: (usize, usize),
    color: &Rgb<u8>,
) -> bool {
    let (mw, mh) = mask_dims;
    let raw = img.as_mut();
    let mut drawn = false;

    for poly in polygons {
        if poly.len() < 3 {
            continue;
        }
        let pts: Vec<(i32, i32)> = poly
            .iter()
            .map(|p| {
                let px = (p[0] * mw as f32).round() as i32;
                let py = (p[1] * mh as f32).round() as i32;
                (px, py)
            })
            .collect();

        let ys: Vec<i32> = pts.iter().map(|p| p.1).collect();
        let min_y = *ys.iter().min().unwrap_or(&0).max(&0) as usize;
        let max_y = (*ys.iter().max().unwrap_or(&0).min(&(mh as i32 - 1))) as usize;

        for y in min_y..=max_y {
            let mut xs: Vec<i32> = Vec::new();
            for i in 0..pts.len() {
                let j = (i + 1) % pts.len();
                let yi = pts[i].1;
                let yj = pts[j].1;
                if (yi <= y as i32 && yj > y as i32) || (yj <= y as i32 && yi > y as i32) {
                    let xi = pts[i].0;
                    let xj = pts[j].0;
                    let t = (y as i32 - yi) as f32 / (yj - yi) as f32;
                    xs.push((xi as f32 + t * (xj - xi) as f32).round() as i32);
                }
            }
            xs.sort_unstable();
            for chunk in xs.chunks(2) {
                if chunk.len() == 2 {
                    let x0 = chunk[0].max(0).min(mw as i32 - 1) as usize;
                    let x1 = chunk[1].max(0).min(mw as i32 - 1) as usize;
                    for x in x0..=x1 {
                        let off = (y * mw + x) * 3;
                        raw[off] = color[0];
                        raw[off + 1] = color[1];
                        raw[off + 2] = color[2];
                        drawn = true;
                    }
                }
            }
        }
    }
    drawn
}

/// Iterative Ramer-Douglas-Peucker: binary search for epsilon to hit target point count.
fn simplify_contour(points: &[Point<i32>], percentage: f64) -> Vec<Point<i32>> {
    let target = (points.len() as f64 * (1.0 - percentage)).max(3.0) as usize;
    if points.len() <= target {
        return points.to_vec();
    }

    let min_x = points.iter().map(|p| p.x).min().unwrap_or(0) as f64;
    let max_x = points.iter().map(|p| p.x).max().unwrap_or(0) as f64;
    let min_y = points.iter().map(|p| p.y).min().unwrap_or(0) as f64;
    let max_y = points.iter().map(|p| p.y).max().unwrap_or(0) as f64;
    let diagonal = ((max_x - min_x).powi(2) + (max_y - min_y).powi(2)).sqrt();
    let mut lo = 0.0;
    let mut hi = diagonal;
    let mut best = points.to_vec();

    for _ in 0..20 {
        let mid = (lo + hi) * 0.5;
        let approx = approximate_polygon_dp(points, mid, true);
        if approx.len() < 3 {
            hi = mid;
        } else if approx.len() <= target {
            best = approx;
            hi = mid;
        } else {
            lo = mid;
        }
        if hi - lo < 0.125 {
            break;
        }
    }
    if best.len() >= 3 {
        best
    } else {
        points.to_vec()
    }
}

fn polygon_area(pts: &[Point<i32>]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        area += pts[i].x as f64 * pts[j].y as f64;
        area -= pts[j].x as f64 * pts[i].y as f64;
    }
    (area * 0.5).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_small_components_removes_isolated_noise() {
        let mut mask = vec![0u8; 10 * 10];
        for y in 2..6 {
            for x in 2..6 {
                mask[y * 10 + x] = 1;
            }
        }
        mask[8 * 10 + 8] = 1;

        let filtered = filter_small_components(&mask, 10, 10, 0.05);

        assert_eq!(filtered.iter().filter(|&&pixel| pixel != 0).count(), 16);
        assert_eq!(filtered[8 * 10 + 8], 0);
    }

    #[test]
    fn filter_small_components_keeps_multiple_large_parts() {
        let mut mask = vec![0u8; 10 * 10];
        for y in 1..4 {
            for x in 1..4 {
                mask[y * 10 + x] = 1;
            }
        }
        for y in 6..9 {
            for x in 6..9 {
                mask[y * 10 + x] = 1;
            }
        }

        let filtered = filter_small_components(&mask, 10, 10, 0.05);

        assert_eq!(filtered.iter().filter(|&&pixel| pixel != 0).count(), 18);
    }

    #[test]
    fn filter_small_components_uses_eight_connectivity() {
        let mut mask = vec![0u8; 3 * 3];
        mask[0] = 1;
        mask[4] = 1;

        let filtered = filter_small_components(&mask, 3, 3, 0.2);

        assert_eq!(filtered.iter().filter(|&&pixel| pixel != 0).count(), 2);
    }

    #[test]
    fn simplify_contour_binary_search_respects_target() {
        let points: Vec<Point<i32>> = (0..100)
            .map(|i| {
                let x = if (i / 5) % 2 == 0 { i * 2 } else { 200 - i * 2 };
                Point::new(x, i * 3)
            })
            .collect();
        assert_eq!(points.len(), 100);

        let result = simplify_contour(&points, 0.5);
        assert!(
            result.len() >= 3,
            "must have at least 3 vertices, got {}",
            result.len()
        );
        assert!(
            result.len() <= 50,
            "must respect 50% target, got {}",
            result.len()
        );

        let result = simplify_contour(&points, 0.75);
        assert!(result.len() >= 3);
        assert!(
            result.len() <= 25,
            "must respect 75% target, got {}",
            result.len()
        );

        let result = simplify_contour(&points, 0.95);
        assert!(result.len() >= 3);
        assert!(
            result.len() <= 5,
            "extreme simplification, got {}",
            result.len()
        );
    }

    #[test]
    fn solid_crop_mask_produces_contours() {
        let mask = vec![1.0f32; 4 * 4];
        let polys = mask_to_polygons(&mask, 4, 4, 0.5, 0.0, None, None);
        assert!(!polys.is_empty(), "all-white crop must polygonize");
    }

    #[test]
    fn simplify_contour_already_small() {
        let pts = vec![Point::new(0, 0), Point::new(10, 0), Point::new(10, 10)];
        let result = simplify_contour(&pts, 0.5);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], Point::new(0, 0));
    }

    #[test]
    fn simplify_contour_rectangle() {
        let pts = vec![
            Point::new(0, 0),
            Point::new(100, 0),
            Point::new(100, 100),
            Point::new(0, 100),
        ];
        let result = simplify_contour(&pts, 0.0);
        assert!(
            result.len() <= 5,
            "rect corners should simplify, got {}",
            result.len()
        );
        assert!(
            result.len() >= 4,
            "must keep at least 4 corners, got {}",
            result.len()
        );
    }
}

// Oriented bounding boxes via PCA (mechanism).
//
// Both [`compute_mask_obb`] (binary mask) and [`compute_polygon_obb`]
// (polygon vertices) treat their input as a 2-D point cloud and share the
// same covariance accumulation and principal-axis estimation, then apply
// their own extent logic (axis projection vs. eigenvalues).

/// Oriented Bounding Box computed from points or a binary mask.
///
/// All fields normalized 0..1 relative to input dimensions.
#[derive(Copy, Clone, Debug)]
pub struct MaskObb {
    /// Center x, normalized 0..1.
    pub cx: f32,
    /// Center y, normalized 0..1.
    pub cy: f32,
    /// Width, normalized 0..1.
    pub w: f32,
    /// Height, normalized 0..1.
    pub h: f32,
    /// Rotation angle in radians, in [-PI/2, PI/2].
    pub rotation: f32,
}

fn covariance_2d(points: &[(f32, f32)]) -> Option<(f32, f32, f32, f32, f32)> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f32;
    let cx = points.iter().map(|p| p.0).sum::<f32>() / n;
    let cy = points.iter().map(|p| p.1).sum::<f32>() / n;

    let mut cov_xx = 0.0f32;
    let mut cov_xy = 0.0f32;
    let mut cov_yy = 0.0f32;
    for &(x, y) in points {
        let dx = x - cx;
        let dy = y - cy;
        cov_xx += dx * dx;
        cov_xy += dx * dy;
        cov_yy += dy * dy;
    }
    cov_xx /= n;
    cov_xy /= n;
    cov_yy /= n;
    Some((cx, cy, cov_xx, cov_xy, cov_yy))
}

#[inline]
fn principal_angle(cov_xx: f32, cov_xy: f32, cov_yy: f32) -> f32 {
    0.5 * f32::atan2(2.0 * cov_xy, cov_xx - cov_yy)
}

/// Compute the minimum-area oriented bounding box from a binary mask.
///
/// Uses PCA on non-zero pixels to find the principal axis, then computes
/// the extent along that axis for a tight-fitting rotated rectangle.
/// Returns `None` for masks with fewer than 3 non-zero pixels.
///
/// # Example
///
/// ```
/// use mana_geometry::polygonize::compute_mask_obb;
///
/// // A horizontal bar in a 20x20 mask is roughly axis-aligned.
/// let mut mask = vec![0u8; 20 * 20];
/// for y in 8..11 {
///     for x in 4..14 {
///         mask[y * 20 + x] = 1;
///     }
/// }
/// let obb = compute_mask_obb(&mask, 20, 20).unwrap();
/// assert!(obb.rotation.abs() < 0.01);
/// ```
pub fn compute_mask_obb(mask: &[u8], mask_w: usize, mask_h: usize) -> Option<MaskObb> {
    let mut points = Vec::new();
    for y in 0..mask_h {
        let row_off = y * mask_w;
        for x in 0..mask_w {
            if mask[row_off + x] != 0 {
                points.push((x as f32, y as f32));
            }
        }
    }

    let (cx, cy, cov_xx, cov_xy, cov_yy) = covariance_2d(&points)?;
    let angle = principal_angle(cov_xx, cov_xy, cov_yy);
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    let mut min_u = f32::MAX;
    let mut max_u = f32::MIN;
    let mut min_v = f32::MAX;
    let mut max_v = f32::MIN;

    for &(x, y) in &points {
        let dx = x - cx;
        let dy = y - cy;
        let u = dx * cos_a + dy * sin_a;
        let v = -dx * sin_a + dy * cos_a;
        min_u = min_u.min(u);
        max_u = max_u.max(u);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }

    let w_local = max_u - min_u + 1.0;
    let h_local = max_v - min_v + 1.0;
    if w_local < 1.0 || h_local < 1.0 {
        return None;
    }

    let cx_local = (min_u + max_u) * 0.5;
    let cy_local = (min_v + max_v) * 0.5;
    let cx_world = cx + cx_local * cos_a - cy_local * sin_a;
    let cy_world = cy + cx_local * sin_a + cy_local * cos_a;

    Some(MaskObb {
        cx: cx_world / mask_w as f32,
        cy: cy_world / mask_h as f32,
        w: w_local / mask_w as f32,
        h: h_local / mask_h as f32,
        rotation: angle,
    })
}

/// Compute oriented bounding box from polygon vertices (normalized coords).
pub fn compute_polygon_obb(polygon: &[[f32; 2]]) -> Option<MaskObb> {
    if polygon.len() < 3 {
        return None;
    }
    let points: Vec<(f32, f32)> = polygon.iter().map(|p| (p[0], p[1])).collect();
    let (cx, cy, cov_xx, cov_xy, cov_yy) = covariance_2d(&points)?;

    let trace = cov_xx + cov_yy;
    let det = cov_xx * cov_yy - cov_xy * cov_xy;
    let half_trace = trace * 0.5;
    let disc = (half_trace * half_trace - det).max(0.0).sqrt();
    let eig1 = half_trace + disc;
    let eig2 = (half_trace - disc).max(0.0);

    let angle = if cov_xx > cov_yy {
        cov_xy.atan2(eig1 - cov_xx + 1e-10)
    } else {
        (eig1 - cov_yy).atan2(cov_xy + 1e-10)
    };

    let w = (eig1.sqrt() * 2.0).clamp(0.0, 1.0);
    let h = (eig2.sqrt() * 2.0).clamp(0.0, 1.0);
    if w < 1e-6 || h < 1e-6 {
        return None;
    }

    Some(MaskObb {
        cx,
        cy,
        w,
        h,
        rotation: angle,
    })
}

#[cfg(test)]
mod obb_tests {
    use super::*;

    #[test]
    fn obb_empty_mask_returns_none() {
        let mask = vec![0u8; 10 * 10];
        let obb = compute_mask_obb(&mask, 10, 10);
        assert!(obb.is_none());
    }

    #[test]
    fn obb_two_pixels_returns_none() {
        let mut mask = vec![0u8; 10 * 10];
        mask[0] = 1;
        mask[1] = 1;
        let obb = compute_mask_obb(&mask, 10, 10);
        assert!(obb.is_none());
    }

    #[test]
    fn obb_axis_aligned_rectangle() {
        let mut mask = vec![0u8; 20 * 20];
        for y in 8..11 {
            for x in 4..14 {
                mask[y * 20 + x] = 1;
            }
        }
        let obb = compute_mask_obb(&mask, 20, 20).unwrap();
        assert!(obb.rotation.abs() < 0.01, "angle={}", obb.rotation);
        assert!((obb.w - 0.5).abs() < 0.02, "w={}", obb.w);
        assert!((obb.h - 0.15).abs() < 0.02, "h={}", obb.h);
        assert!((obb.cx - 0.425).abs() < 0.02, "cx={}", obb.cx);
        assert!((obb.cy - 0.45).abs() < 0.02, "cy={}", obb.cy);
    }

    #[test]
    fn obb_diagonal_line() {
        let mut mask = vec![0u8; 20 * 20];
        for i in 0..10 {
            mask[i * 20 + i] = 1;
        }
        let obb = compute_mask_obb(&mask, 20, 20).unwrap();
        let expected = std::f32::consts::FRAC_PI_4;
        assert!(
            (obb.rotation - expected).abs() < 0.1,
            "expected ~{:.3}, got {:.3}",
            expected,
            obb.rotation
        );
    }
}
