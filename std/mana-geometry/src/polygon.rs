//! mana-geometry/src/polygon.rs — Polygon Geometry
//! ======================================================
//! Polygon centre, area (shoelace), bounding box.
//! All coordinates in normalised 0..1 unless stated otherwise.

/// Signed-area weighted centroid of a polygon.
///
/// Returns arithmetic mean of vertices if total signed area ≈ 0
/// (degenerate/self-intersecting polygon).
pub fn polygon_center(vertices: &[[f32; 2]]) -> (f32, f32) {
    let n = vertices.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    if n == 1 {
        return (vertices[0][0], vertices[0][1]);
    }

    let mut signed_area_total = 0.0f32;
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;

    for i in 0..n {
        let j = (i + 1) % n;
        let x_i = vertices[i][0];
        let y_i = vertices[i][1];
        let x_j = vertices[j][0];
        let y_j = vertices[j][1];

        let cross = x_i * y_j - x_j * y_i;
        signed_area_total += cross;

        cx += (x_i + x_j) * cross;
        cy += (y_i + y_j) * cross;
    }

    if signed_area_total.abs() < 1e-10 {
        let sum_x: f32 = vertices.iter().map(|v| v[0]).sum();
        let sum_y: f32 = vertices.iter().map(|v| v[1]).sum();
        return (sum_x / n as f32, sum_y / n as f32);
    }

    let factor = 1.0 / (3.0 * signed_area_total);
    (cx * factor, cy * factor)
}

/// Shoelace area of a polygon (absolute value).
pub fn polygon_area(vertices: &[[f32; 2]]) -> f32 {
    let n = vertices.len();
    if n < 3 {
        return 0.0;
    }

    let mut area = 0.0f32;
    for i in 0..n {
        let j = (i + 1) % n;
        area += vertices[i][0] * vertices[j][1];
        area -= vertices[j][0] * vertices[i][1];
    }
    (area * 0.5).abs()
}

/// Axis-aligned bounding box of a polygon.
///
/// Returns `(x_min, y_min, x_max, y_max)`.
pub fn polygon_to_xyxy(vertices: &[[f32; 2]]) -> (f32, f32, f32, f32) {
    if vertices.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }

    let mut x_min = f32::MAX;
    let mut y_min = f32::MAX;
    let mut x_max = f32::MIN;
    let mut y_max = f32::MIN;

    for v in vertices {
        x_min = x_min.min(v[0]);
        y_min = y_min.min(v[1]);
        x_max = x_max.max(v[0]);
        y_max = y_max.max(v[1]);
    }

    (x_min, y_min, x_max, y_max)
}

/// 2D cross product of points relative to a reference line start→end.
///
/// Positive = point lies to the left of the directed line,
/// negative = right, zero = collinear.
#[inline]
pub fn cross_product(
    px: f32,
    py: f32,
    line_start_x: f32,
    line_start_y: f32,
    line_end_x: f32,
    line_end_y: f32,
) -> f32 {
    let dx_line = line_end_x - line_start_x;
    let dy_line = line_end_y - line_start_y;
    let dx_pt = px - line_start_x;
    let dy_pt = py - line_start_y;
    dx_line * dy_pt - dy_line * dx_pt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_square() {
        let sq = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let (cx, cy) = polygon_center(&sq);
        assert!((cx - 0.5).abs() < 1e-6);
        assert!((cy - 0.5).abs() < 1e-6);
    }

    #[test]
    fn center_triangle() {
        let tri = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let (cx, cy) = polygon_center(&tri);
        assert!((cx - 0.3333333).abs() < 0.01);
        assert!((cy - 0.3333333).abs() < 0.01);
    }

    #[test]
    fn center_empty() {
        let (cx, cy) = polygon_center(&[]);
        assert_eq!(cx, 0.0);
        assert_eq!(cy, 0.0);
    }

    #[test]
    fn center_single_vertex() {
        let (cx, cy) = polygon_center(&[[0.3, 0.7]]);
        assert!((cx - 0.3).abs() < 1e-6);
        assert!((cy - 0.7).abs() < 1e-6);
    }

    #[test]
    fn area_rectangle() {
        let rect = [[0.0, 0.0], [2.0, 0.0], [2.0, 3.0], [0.0, 3.0]];
        assert!((polygon_area(&rect) - 6.0).abs() < 1e-6);
    }

    #[test]
    fn area_degenerate() {
        let line = [[0.0, 0.0], [1.0, 1.0]];
        assert!((polygon_area(&line) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn to_xyxy_simple() {
        let tri = [[0.1, 0.3], [0.5, 0.1], [0.3, 0.7]];
        let (x1, y1, x2, y2) = polygon_to_xyxy(&tri);
        assert!((x1 - 0.1).abs() < 1e-6);
        assert!((y1 - 0.1).abs() < 1e-6);
        assert!((x2 - 0.5).abs() < 1e-6);
        assert!((y2 - 0.7).abs() < 1e-6);
    }

    #[test]
    fn cross_product_left_right() {
        // Line from (0,0) to (1,0). Point (0.5, 1) is above → left (positive).
        let cp = cross_product(0.5, 1.0, 0.0, 0.0, 1.0, 0.0);
        assert!(cp > 0.0);
        // Point (0.5, -1) is below → right (negative).
        let cp2 = cross_product(0.5, -1.0, 0.0, 0.0, 1.0, 0.0);
        assert!(cp2 < 0.0);
    }

    #[test]
    fn cross_product_collinear() {
        let cp = cross_product(0.5, 0.0, 0.0, 0.0, 1.0, 0.0);
        assert!((cp - 0.0).abs() < 1e-6);
    }
}
