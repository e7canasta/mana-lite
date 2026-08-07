//! mana-geometry/src/primitives.rs — Geometric Primitive Algebra
//! ================================================================
//! Closed set: {Point, BBox, OBB, Polygon}
//! Operations:  centroid(), contains(zone), overlap(zone, mode),
//!              distance_to_edge(zone)
//!
//! Coordinates: normalised 0..1 unless otherwise stated.
//! All algebra uses convex zone polygons for correctness.
//!
//! This is the uapi geométrico described in sprint-01-body-estimator §4:
//! every perceptor writes against this algebra, never against the
//! source of the primitive (keypoint, mask, inference).

use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    #[inline]
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
}

impl BBox {
    #[inline]
    pub fn new(cx: f32, cy: f32, w: f32, h: f32) -> Self {
        debug_assert!(w >= 0.0, "BBox width must be non-negative");
        debug_assert!(h >= 0.0, "BBox height must be non-negative");
        Self { cx, cy, w, h }
    }

    #[inline]
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    #[inline]
    pub fn corners(&self) -> [Point; 4] {
        let hw = self.w * 0.5;
        let hh = self.h * 0.5;
        [
            Point::new(self.cx - hw, self.cy - hh),
            Point::new(self.cx + hw, self.cy - hh),
            Point::new(self.cx + hw, self.cy + hh),
            Point::new(self.cx - hw, self.cy + hh),
        ]
    }

    #[inline]
    pub fn edges(&self) -> [(Point, Point); 4] {
        let c = self.corners();
        [(c[0], c[1]), (c[1], c[2]), (c[2], c[3]), (c[3], c[0])]
    }

    #[inline]
    pub fn contains_point(&self, p: Point) -> bool {
        let hw = self.w * 0.5;
        let hh = self.h * 0.5;
        p.x >= self.cx - hw && p.x <= self.cx + hw && p.y >= self.cy - hh && p.y <= self.cy + hh
    }

    #[inline]
    pub fn to_xyxy(&self) -> (f32, f32, f32, f32) {
        let hw = self.w * 0.5;
        let hh = self.h * 0.5;
        (self.cx - hw, self.cy - hh, self.cx + hw, self.cy + hh)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OBB {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    /// Counter-clockwise rotation in radians.
    pub rotation: f32,
}

impl OBB {
    #[inline]
    pub fn new(cx: f32, cy: f32, w: f32, h: f32, rotation: f32) -> Self {
        debug_assert!(w >= 0.0, "OBB width must be non-negative");
        debug_assert!(h >= 0.0, "OBB height must be non-negative");
        Self {
            cx,
            cy,
            w,
            h,
            rotation,
        }
    }

    #[inline]
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    pub fn corners(&self) -> [Point; 4] {
        let hw = self.w * 0.5;
        let hh = self.h * 0.5;
        let cos = self.rotation.cos();
        let sin = self.rotation.sin();
        let local = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
        let mut corners = [Point::new(0.0, 0.0); 4];
        for i in 0..4 {
            corners[i].x = self.cx + local[i].0 * cos - local[i].1 * sin;
            corners[i].y = self.cy + local[i].0 * sin + local[i].1 * cos;
        }
        corners
    }

    pub fn edges(&self) -> [(Point, Point); 4] {
        let c = self.corners();
        [(c[0], c[1]), (c[1], c[2]), (c[2], c[3]), (c[3], c[0])]
    }

    pub fn axes(&self) -> [(f32, f32); 2] {
        let cos = self.rotation.cos();
        let sin = self.rotation.sin();
        [(cos, sin), (-sin, cos)]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    vertices: Vec<Point>,
}

impl Polygon {
    pub fn new(vertices: Vec<Point>) -> Self {
        let mut verts = vertices;
        normalize_winding(&mut verts);
        Self { vertices: verts }
    }

    pub fn from_slice(points: &[[f32; 2]]) -> Self {
        Self::new(points.iter().map(|p| Point::new(p[0], p[1])).collect())
    }

    pub fn from_f32_slice(points: &[(f32, f32)]) -> Self {
        Self::new(points.iter().map(|p| Point::new(p.0, p.1)).collect())
    }

    pub fn vertices(&self) -> &[Point] {
        &self.vertices
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn area(&self) -> f32 {
        let n = self.vertices.len();
        if n < 3 {
            return 0.0;
        }
        let mut a = 0.0f32;
        for i in 0..n {
            let j = (i + 1) % n;
            a += self.vertices[i].x * self.vertices[j].y;
            a -= self.vertices[j].x * self.vertices[i].y;
        }
        (a * 0.5).abs()
    }

    #[must_use]
    pub fn edges(&self) -> impl ExactSizeIterator<Item = (Point, Point)> + '_ {
        PolygonEdgeIter {
            verts: &self.vertices,
            idx: 0,
        }
    }

    pub fn aabb(&self) -> BBox {
        if self.vertices.is_empty() {
            return BBox::new(0.0, 0.0, 0.0, 0.0);
        }
        let mut x_min = f32::MAX;
        let mut y_min = f32::MAX;
        let mut x_max = f32::MIN;
        let mut y_max = f32::MIN;
        for v in &self.vertices {
            x_min = x_min.min(v.x);
            y_min = y_min.min(v.y);
            x_max = x_max.max(v.x);
            y_max = y_max.max(v.y);
        }
        BBox::new(
            (x_min + x_max) * 0.5,
            (y_min + y_max) * 0.5,
            x_max - x_min,
            y_max - y_min,
        )
    }

    pub fn contains_point(&self, p: Point) -> bool {
        point_in_polygon(p, &self.vertices)
    }
}

struct PolygonEdgeIter<'a> {
    verts: &'a [Point],
    idx: usize,
}

impl<'a> Iterator for PolygonEdgeIter<'a> {
    type Item = (Point, Point);

    fn next(&mut self) -> Option<Self::Item> {
        let n = self.verts.len();
        if n < 2 || self.idx >= n {
            return None;
        }
        let i = self.idx;
        let j = (i + 1) % n;
        self.idx += 1;
        Some((self.verts[i], self.verts[j]))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.verts.len().saturating_sub(self.idx);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for PolygonEdgeIter<'_> {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverlapMode {
    Any,
    IoU(f32),
    IoS(f32),
}

// ═══════════════════════════════════════════════════════════════
// Centroid
// ═══════════════════════════════════════════════════════════════

pub trait Centroid {
    fn centroid(&self) -> Point;
}

impl Centroid for Point {
    #[inline]
    fn centroid(&self) -> Point {
        *self
    }
}

impl Centroid for BBox {
    #[inline]
    fn centroid(&self) -> Point {
        Point::new(self.cx, self.cy)
    }
}

impl Centroid for OBB {
    #[inline]
    fn centroid(&self) -> Point {
        Point::new(self.cx, self.cy)
    }
}

impl Centroid for Polygon {
    fn centroid(&self) -> Point {
        let n = self.vertices.len();
        if n == 0 {
            return Point::new(0.0, 0.0);
        }
        if n == 1 {
            return self.vertices[0];
        }
        let mut signed_area_total = 0.0f32;
        let mut cx = 0.0f32;
        let mut cy = 0.0f32;
        for i in 0..n {
            let j = (i + 1) % n;
            let xi = self.vertices[i].x;
            let yi = self.vertices[i].y;
            let xj = self.vertices[j].x;
            let yj = self.vertices[j].y;
            let cross = xi * yj - xj * yi;
            signed_area_total += cross;
            cx += (xi + xj) * cross;
            cy += (yi + yj) * cross;
        }
        if signed_area_total.abs() < 1e-10 {
            let sum_x: f32 = self.vertices.iter().map(|v| v.x).sum();
            let sum_y: f32 = self.vertices.iter().map(|v| v.y).sum();
            return Point::new(sum_x / n as f32, sum_y / n as f32);
        }
        let factor = 1.0 / (3.0 * signed_area_total);
        Point::new(cx * factor, cy * factor)
    }
}

// ═══════════════════════════════════════════════════════════════
// IsContainedIn (self fully inside zone polygon)
// ═══════════════════════════════════════════════════════════════

/// Semantics: `self.is_contained_in(zone)` → true if `self` is fully inside
/// `zone`, including boundary-touching. For convex zone polygons (the common
/// case: bed trapezoids), checking all vertices/corners is sufficient.
pub trait IsContainedIn {
    fn is_contained_in(&self, zone: &Polygon) -> bool;
}

impl IsContainedIn for Point {
    fn is_contained_in(&self, zone: &Polygon) -> bool {
        point_in_polygon(*self, zone.vertices())
    }
}

impl IsContainedIn for BBox {
    fn is_contained_in(&self, zone: &Polygon) -> bool {
        let corners = self.corners();
        for c in &corners {
            if !point_in_polygon(*c, zone.vertices()) {
                return false;
            }
        }
        if polygon_edges_cross_bbox(zone, self) {
            return false;
        }
        true
    }
}

impl IsContainedIn for OBB {
    fn is_contained_in(&self, zone: &Polygon) -> bool {
        let corners = self.corners();
        for c in &corners {
            if !point_in_polygon(*c, zone.vertices()) {
                return false;
            }
        }
        if segment_set_crosses_polygon(&self.edges(), zone) {
            return false;
        }
        true
    }
}

impl IsContainedIn for Polygon {
    fn is_contained_in(&self, zone: &Polygon) -> bool {
        let verts = self.vertices();
        let zone_verts = zone.vertices();
        let nz = zone_verts.len();
        for v in verts {
            if !point_in_polygon(*v, zone_verts) {
                return false;
            }
        }
        for (a, b) in self.edges() {
            for i in 0..nz {
                let j = (i + 1) % nz;
                if segments_properly_intersect(a, b, zone_verts[i], zone_verts[j]) {
                    return false;
                }
            }
        }
        true
    }
}

// ═══════════════════════════════════════════════════════════════
// Overlap (any intersection, or threshold-based)
// ═══════════════════════════════════════════════════════════════

pub trait Overlap {
    fn overlap(&self, zone: &Polygon, mode: OverlapMode) -> bool;
}

impl Overlap for Point {
    fn overlap(&self, zone: &Polygon, _mode: OverlapMode) -> bool {
        point_in_polygon(*self, zone.vertices())
    }
}

impl Overlap for BBox {
    fn overlap(&self, zone: &Polygon, mode: OverlapMode) -> bool {
        match mode {
            OverlapMode::Any => bbox_overlaps_polygon_any(self, zone),
            OverlapMode::IoU(threshold) => bbox_polygon_iou(self, zone) >= threshold,
            OverlapMode::IoS(threshold) => {
                let intersection = bbox_polygon_intersection_area(self, zone);
                let self_area = self.area();
                if self_area < 1e-10 {
                    return false;
                }
                intersection / self_area >= threshold
            }
        }
    }
}

impl Overlap for OBB {
    fn overlap(&self, zone: &Polygon, mode: OverlapMode) -> bool {
        match mode {
            OverlapMode::Any => convex_overlap_obb_polygon(*self, zone),
            OverlapMode::IoU(threshold) => {
                let intersection = obb_polygon_intersection_area(*self, zone);
                let union = self.area() + zone.area() - intersection;
                if union < 1e-10 {
                    return false;
                }
                intersection / union >= threshold
            }
            OverlapMode::IoS(threshold) => {
                let intersection = obb_polygon_intersection_area(*self, zone);
                let self_area = self.area();
                if self_area < 1e-10 {
                    return false;
                }
                intersection / self_area >= threshold
            }
        }
    }
}

impl Overlap for Polygon {
    fn overlap(&self, zone: &Polygon, mode: OverlapMode) -> bool {
        match mode {
            OverlapMode::Any => convex_polygons_overlap(self, zone),
            OverlapMode::IoU(threshold) => {
                let intersection = convex_polygons_intersection_area(self, zone);
                let union = self.area() + zone.area() - intersection;
                if union < 1e-10 {
                    return false;
                }
                intersection / union >= threshold
            }
            OverlapMode::IoS(threshold) => {
                let intersection = convex_polygons_intersection_area(self, zone);
                let self_area = self.area();
                if self_area < 1e-10 {
                    return false;
                }
                intersection / self_area >= threshold
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Distance to edge
// ═══════════════════════════════════════════════════════════════

pub trait DistanceToEdge {
    fn distance_to_edge(&self, zone: &Polygon) -> f32;
}

impl DistanceToEdge for Point {
    fn distance_to_edge(&self, zone: &Polygon) -> f32 {
        point_to_polygon_edge_dist(*self, zone)
    }
}

impl DistanceToEdge for BBox {
    fn distance_to_edge(&self, zone: &Polygon) -> f32 {
        let mut min_d = f32::MAX;
        for corner in &self.corners() {
            min_d = min_d.min(point_to_polygon_edge_dist(*corner, zone));
        }
        for (a, b) in self.edges() {
            let mx = (a.x + b.x) * 0.5;
            let my = (a.y + b.y) * 0.5;
            min_d = min_d.min(point_to_polygon_edge_dist(Point::new(mx, my), zone));
        }
        min_d
    }
}

impl DistanceToEdge for OBB {
    fn distance_to_edge(&self, zone: &Polygon) -> f32 {
        let mut min_d = f32::MAX;
        for corner in &self.corners() {
            min_d = min_d.min(point_to_polygon_edge_dist(*corner, zone));
        }
        for (a, b) in self.edges() {
            let mx = (a.x + b.x) * 0.5;
            let my = (a.y + b.y) * 0.5;
            min_d = min_d.min(point_to_polygon_edge_dist(Point::new(mx, my), zone));
        }
        min_d
    }
}

impl DistanceToEdge for Polygon {
    fn distance_to_edge(&self, zone: &Polygon) -> f32 {
        let mut min_d = f32::MAX;
        for v in self.vertices() {
            min_d = min_d.min(point_to_polygon_edge_dist(*v, zone));
        }
        min_d
    }
}

// ═══════════════════════════════════════════════════════════════
// Internal algorithms
// ═══════════════════════════════════════════════════════════════

fn normalize_winding(vertices: &mut [Point]) {
    let n = vertices.len();
    if n < 3 {
        return;
    }
    let mut area = 0.0f32;
    for i in 0..n {
        let j = (i + 1) % n;
        area += vertices[i].x * vertices[j].y;
        area -= vertices[j].x * vertices[i].y;
    }
    if area < 0.0 {
        vertices.reverse();
    }
}

pub fn point_in_polygon(p: Point, vertices: &[Point]) -> bool {
    let n = vertices.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        let j = (i + 1) % n;
        if point_on_segment(p, vertices[i], vertices[j]) {
            return true;
        }
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let vi = vertices[i];
        let vj = vertices[j];
        let dy = vj.y - vi.y;
        let intersects = (vi.y > p.y) != (vj.y > p.y)
            && dy.abs() >= f32::EPSILON
            && p.x < (vj.x - vi.x) * (p.y - vi.y) / dy + vi.x;
        if intersects {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn point_on_segment(p: Point, a: Point, b: Point) -> bool {
    point_to_segment_dist(p, a, b) < 1e-6
}

fn segments_intersect(a1: Point, a2: Point, b1: Point, b2: Point) -> bool {
    let d1 = cross(b1, b2, a1);
    let d2 = cross(b1, b2, a2);
    let d3 = cross(a1, a2, b1);
    let d4 = cross(a1, a2, b2);

    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }

    if d1.abs() < 1e-10 && on_segment(b1, b2, a1) {
        return true;
    }
    if d2.abs() < 1e-10 && on_segment(b1, b2, a2) {
        return true;
    }
    if d3.abs() < 1e-10 && on_segment(a1, a2, b1) {
        return true;
    }
    if d4.abs() < 1e-10 && on_segment(a1, a2, b2) {
        return true;
    }

    false
}

fn cross(a: Point, b: Point, c: Point) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn on_segment(a: Point, b: Point, c: Point) -> bool {
    c.x >= a.x.min(b.x) && c.x <= a.x.max(b.x) && c.y >= a.y.min(b.y) && c.y <= a.y.max(b.y)
}

fn polygon_edges_cross_bbox(zone: &Polygon, bb: &BBox) -> bool {
    let zone_verts = zone.vertices();
    let n = zone_verts.len();
    if n < 2 {
        return false;
    }
    for i in 0..n {
        let j = (i + 1) % n;
        if segment_crosses_bbox(zone_verts[i], zone_verts[j], bb) {
            return true;
        }
    }
    false
}

fn segment_crosses_bbox(a: Point, b: Point, bb: &BBox) -> bool {
    if bb.contains_point(a) || bb.contains_point(b) {
        return true;
    }
    for (e1, e2) in bb.edges() {
        if segments_intersect(a, b, e1, e2) {
            return true;
        }
    }
    false
}

fn segment_set_crosses_polygon(edges: &[(Point, Point)], zone: &Polygon) -> bool {
    let zone_verts = zone.vertices();
    let n = zone_verts.len();
    for &(a, b) in edges {
        for i in 0..n {
            let j = (i + 1) % n;
            if segments_properly_intersect(a, b, zone_verts[i], zone_verts[j]) {
                return true;
            }
        }
    }
    false
}

fn segments_properly_intersect(a1: Point, a2: Point, b1: Point, b2: Point) -> bool {
    let d1 = cross(b1, b2, a1);
    let d2 = cross(b1, b2, a2);
    let d3 = cross(a1, a2, b1);
    let d4 = cross(a1, a2, b2);

    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

fn bbox_overlaps_polygon_any(bb: &BBox, zone: &Polygon) -> bool {
    for corner in &bb.corners() {
        if point_in_polygon(*corner, zone.vertices()) {
            return true;
        }
    }
    for v in zone.vertices() {
        if bb.contains_point(*v) {
            return true;
        }
    }
    polygon_edges_cross_bbox(zone, bb)
}

fn bbox_polygon_intersection_area(bb: &BBox, zone: &Polygon) -> f32 {
    let clipped = clip_bbox_to_polygon(bb, zone);
    clipped.area()
}

fn bbox_polygon_iou(bb: &BBox, zone: &Polygon) -> f32 {
    let intersection = bbox_polygon_intersection_area(bb, zone);
    let union = bb.area() + zone.area() - intersection;
    if union < 1e-10 {
        return 0.0;
    }
    intersection / union
}

fn clip_bbox_to_polygon(bb: &BBox, zone: &Polygon) -> Polygon {
    let mut subject: Vec<Point> = bb.corners().to_vec();
    let zone_verts = zone.vertices();
    let n = zone_verts.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let edge_a = zone_verts[i];
        let edge_b = zone_verts[j];
        subject = clip_polygon_by_edge(&subject, edge_a, edge_b);
        if subject.is_empty() {
            break;
        }
    }
    Polygon::new(subject)
}

fn clip_polygon_by_edge(subject: &[Point], edge_a: Point, edge_b: Point) -> Vec<Point> {
    if subject.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(subject.len() + 1);
    let n = subject.len();
    for i in 0..n {
        let cur = subject[i];
        let prev = if i == 0 {
            subject[n - 1]
        } else {
            subject[i - 1]
        };
        let cur_inside = is_inside_edge(cur, edge_a, edge_b);
        let prev_inside = is_inside_edge(prev, edge_a, edge_b);

        if cur_inside {
            if !prev_inside {
                if let Some(intersect) = line_intersection(prev, cur, edge_a, edge_b) {
                    output.push(intersect);
                }
            }
            output.push(cur);
        } else if prev_inside {
            if let Some(intersect) = line_intersection(prev, cur, edge_a, edge_b) {
                output.push(intersect);
            }
        }
    }
    output
}

fn is_inside_edge(p: Point, edge_a: Point, edge_b: Point) -> bool {
    cross(edge_a, edge_b, p) >= -1e-10
}

fn line_intersection(p1: Point, p2: Point, p3: Point, p4: Point) -> Option<Point> {
    let d = (p1.x - p2.x) * (p3.y - p4.y) - (p1.y - p2.y) * (p3.x - p4.x);
    if d.abs() < 1e-10 {
        return None;
    }
    let t = ((p1.x - p3.x) * (p3.y - p4.y) - (p1.y - p3.y) * (p3.x - p4.x)) / d;
    Some(Point::new(
        p1.x + t * (p2.x - p1.x),
        p1.y + t * (p2.y - p1.y),
    ))
}

fn point_to_polygon_edge_dist(p: Point, zone: &Polygon) -> f32 {
    let verts = zone.vertices();
    let n = verts.len();
    if n < 2 {
        return 0.0;
    }
    let mut min_d = f32::MAX;
    for i in 0..n {
        let j = (i + 1) % n;
        min_d = min_d.min(point_to_segment_dist(p, verts[i], verts[j]));
    }
    min_d
}

fn point_to_segment_dist(p: Point, a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let seg_len_sq = dx * dx + dy * dy;
    if seg_len_sq < 1e-10 {
        let ddx = p.x - a.x;
        let ddy = p.y - a.y;
        return (ddx * ddx + ddy * ddy).sqrt();
    }
    let mut t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / seg_len_sq;
    t = t.clamp(0.0, 1.0);
    let proj_x = a.x + t * dx;
    let proj_y = a.y + t * dy;
    let ddx = p.x - proj_x;
    let ddy = p.y - proj_y;
    (ddx * ddx + ddy * ddy).sqrt()
}

// ── OBB‑Polygon overlap via Separating Axis Theorem ──────────────

fn convex_overlap_obb_polygon(obb: OBB, zone: &Polygon) -> bool {
    let zone_verts = zone.vertices();
    let n = zone_verts.len();
    if n < 3 {
        return false;
    }
    let obb_corners = obb.corners();
    let obb_axes = obb.axes();

    // OBB axes
    for axis in &obb_axes {
        if sat_gap(&obb_corners, zone_verts, *axis) {
            return false;
        }
    }

    // Polygon edge normals
    for i in 0..n {
        let j = (i + 1) % n;
        let dx = zone_verts[j].y - zone_verts[i].y;
        let dy = -(zone_verts[j].x - zone_verts[i].x);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-10 {
            continue;
        }
        let axis = (dx / len, dy / len);
        if sat_gap(&obb_corners, zone_verts, axis) {
            return false;
        }
    }

    true
}

fn sat_gap(a: &[Point], b: &[Point], axis: (f32, f32)) -> bool {
    let (min_a, max_a) = project_points(a, axis);
    let (min_b, max_b) = project_points(b, axis);
    if min_a.is_nan() || min_b.is_nan() {
        return true;
    }
    max_a < min_b || max_b < min_a
}

fn project_points(points: &[Point], axis: (f32, f32)) -> (f32, f32) {
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for p in points {
        let proj = p.x * axis.0 + p.y * axis.1;
        min = min.min(proj);
        max = max.max(proj);
    }
    (min, max)
}

// ── Polygon‑Polygon overlap via SAT ──────────────────────────────

fn convex_polygons_overlap(a: &Polygon, b: &Polygon) -> bool {
    let av = a.vertices();
    let bv = b.vertices();
    if av.len() < 3 || bv.len() < 3 {
        return false;
    }
    let axes_a = polygon_edge_normals(av);
    let axes_b = polygon_edge_normals(bv);

    for axis in axes_a.iter().chain(axes_b.iter()) {
        if sat_gap(av, bv, *axis) {
            return false;
        }
    }

    true
}

fn polygon_edge_normals(verts: &[Point]) -> Vec<(f32, f32)> {
    let n = verts.len();
    if n < 3 {
        return Vec::new();
    }
    let mut axes = Vec::with_capacity(n);
    for i in 0..n {
        let j = (i + 1) % n;
        let dx = verts[j].y - verts[i].y;
        let dy = -(verts[j].x - verts[i].x);
        let len = (dx * dx + dy * dy).sqrt();
        if len > 1e-10 {
            axes.push((dx / len, dy / len));
        }
    }
    axes
}

fn convex_polygons_intersection_area(a: &Polygon, b: &Polygon) -> f32 {
    let mut subject: Vec<Point> = a.vertices().to_vec();
    let bv = b.vertices();
    let n = bv.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let edge_a = bv[i];
        let edge_b = bv[j];
        if subject.len() < 3 {
            return 0.0;
        }
        subject = clip_polygon_by_edge(&subject, edge_a, edge_b);
        if subject.len() < 3 {
            return 0.0;
        }
    }
    if subject.len() < 3 {
        return 0.0;
    }
    let clipped = Polygon::new(subject);
    clipped.area()
}

fn obb_polygon_intersection_area(obb: OBB, zone: &Polygon) -> f32 {
    let corners = obb.corners();
    let obb_poly = Polygon::new(corners.to_vec());
    convex_polygons_intersection_area(&obb_poly, zone)
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn sq05() -> Polygon {
        Polygon::from_slice(&[[0.0, 0.0], [0.5, 0.0], [0.5, 0.5], [0.0, 0.5]])
    }

    fn sq1() -> Polygon {
        Polygon::from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
    }

    fn sq_half() -> Polygon {
        Polygon::from_slice(&[[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]])
    }

    fn trap_bed() -> Polygon {
        Polygon::from_slice(&[[0.1, 0.1], [0.9, 0.1], [0.85, 0.9], [0.15, 0.9]])
    }

    fn triangle() -> Polygon {
        Polygon::from_slice(&[[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]])
    }

    // ── Point tests ──────────────────────────────────────────────

    #[test]
    fn point_in_square() {
        let p = Point::new(0.2, 0.2);
        assert!(p.is_contained_in(&sq05()));
        let p_out = Point::new(0.6, 0.6);
        assert!(!p_out.is_contained_in(&sq05()));
    }

    #[test]
    fn point_centroid() {
        let p = Point::new(0.3, 0.7);
        let c = p.centroid();
        assert!((c.x - 0.3).abs() < 1e-6);
        assert!((c.y - 0.7).abs() < 1e-6);
    }

    #[test]
    fn point_to_edge_dist_inside() {
        let p = Point::new(0.25, 0.25);
        let d = p.distance_to_edge(&sq_half());
        assert!(d >= 0.0);
        // Point is at corner, min dist ~0 (on edge)
        let p_edge = Point::new(0.25, 0.25);
        assert!((p_edge.distance_to_edge(&sq_half())).abs() < 0.01);
    }

    #[test]
    fn point_to_edge_dist_outside() {
        let p = Point::new(0.0, 0.5);
        let d = p.distance_to_edge(&sq_half());
        assert!(d > 0.0);
        assert!((d - 0.25).abs() < 0.01);
    }

    #[test]
    fn point_overlap_is_contains() {
        let p = Point::new(0.3, 0.3);
        assert_eq!(
            p.overlap(&sq1(), OverlapMode::Any),
            p.is_contained_in(&sq1())
        );
    }

    // ── BBox tests ───────────────────────────────────────────────

    #[test]
    fn bbox_centroid() {
        let bb = BBox::new(0.5, 0.4, 0.2, 0.6);
        let c = bb.centroid();
        assert!((c.x - 0.5).abs() < 1e-6);
        assert!((c.y - 0.4).abs() < 1e-6);
    }

    #[test]
    fn bbox_area() {
        let bb = BBox::new(0.5, 0.5, 0.4, 0.3);
        assert!((bb.area() - 0.12).abs() < 1e-6);
    }

    #[test]
    fn bbox_contains_in_square() {
        let bb = BBox::new(0.4, 0.4, 0.2, 0.2);
        assert!(bb.is_contained_in(&sq_half()));
    }

    #[test]
    fn bbox_contains_partial_fails() {
        let bb = BBox::new(0.1, 0.1, 0.3, 0.3);
        assert!(!bb.is_contained_in(&sq_half()));
    }

    #[test]
    fn bbox_contains_outside_fails() {
        let bb = BBox::new(0.9, 0.9, 0.05, 0.05);
        assert!(!bb.is_contained_in(&sq_half()));
    }

    #[test]
    fn bbox_overlap_any() {
        let bb = BBox::new(0.4, 0.4, 0.2, 0.2);
        assert!(bb.overlap(&sq_half(), OverlapMode::Any));
    }

    #[test]
    fn bbox_overlap_any_partial() {
        let bb = BBox::new(0.1, 0.1, 0.3, 0.3);
        assert!(bb.overlap(&sq_half(), OverlapMode::Any));
    }

    #[test]
    fn bbox_overlap_any_no() {
        let bb = BBox::new(0.0, 0.0, 0.1, 0.1);
        assert!(!bb.overlap(&sq_half(), OverlapMode::Any));
    }

    #[test]
    fn bbox_overlap_iou_full() {
        let bb = BBox::new(0.5, 0.5, 0.5, 0.5);
        assert!(bb.overlap(&sq1(), OverlapMode::IoU(0.2)));
    }

    #[test]
    fn bbox_overlap_iou_none() {
        let bb = BBox::new(0.0, 0.0, 0.05, 0.05);
        assert!(!bb.overlap(&sq_half(), OverlapMode::IoU(0.01)));
    }

    #[test]
    fn bbox_overlap_ios() {
        let bb = BBox::new(0.5, 0.5, 0.5, 0.5);
        assert!(bb.overlap(&sq1(), OverlapMode::IoS(0.5)));
    }

    #[test]
    fn bbox_distance_to_edge_inside() {
        let bb = BBox::new(0.5, 0.5, 0.2, 0.2);
        let d = bb.distance_to_edge(&sq1());
        assert!(d >= 0.0);
        assert!(d <= 0.5);
    }

    #[test]
    fn bbox_distance_to_edge_outside() {
        let bb = BBox::new(1.2, 0.5, 0.1, 0.1);
        let d = bb.distance_to_edge(&sq1());
        assert!(d > 0.0);
        assert!(d <= 0.3);
    }

    #[test]
    fn bbox_corners() {
        let bb = BBox::new(0.5, 0.5, 0.4, 0.2);
        let c = bb.corners();
        assert!((c[0].x - 0.3).abs() < 1e-6);
        assert!((c[0].y - 0.4).abs() < 1e-6);
        assert!((c[1].x - 0.7).abs() < 1e-6);
        assert!((c[1].y - 0.4).abs() < 1e-6);
        assert!((c[2].x - 0.7).abs() < 1e-6);
        assert!((c[2].y - 0.6).abs() < 1e-6);
        assert!((c[3].x - 0.3).abs() < 1e-6);
        assert!((c[3].y - 0.6).abs() < 1e-6);
    }

    #[test]
    fn bbox_contains_convex_polygon() {
        let big = Polygon::from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let small_bb = BBox::new(0.4, 0.4, 0.2, 0.2);
        assert!(small_bb.is_contained_in(&big));
    }

    // ── OBB tests ────────────────────────────────────────────────

    #[test]
    fn obb_centroid() {
        let obb = OBB::new(0.3, 0.7, 0.2, 0.4, 0.5);
        let c = obb.centroid();
        assert!((c.x - 0.3).abs() < 1e-6);
        assert!((c.y - 0.7).abs() < 1e-6);
    }

    #[test]
    fn obb_corners_identity() {
        let obb = OBB::new(0.5, 0.5, 0.4, 0.2, 0.0);
        let c = obb.corners();
        assert!((c[0].x - 0.3).abs() < 1e-6);
        assert!((c[1].x - 0.7).abs() < 1e-6);
        assert!((c[2].x - 0.7).abs() < 1e-6);
        assert!((c[3].x - 0.3).abs() < 1e-6);
    }

    #[test]
    fn obb_corners_rotation_preserves_center() {
        let obb = OBB::new(0.5, 0.5, 0.4, 0.2, std::f32::consts::FRAC_PI_4);
        let c = obb.corners();
        let sum_x: f32 = c.iter().map(|p| p.x).sum();
        let sum_y: f32 = c.iter().map(|p| p.y).sum();
        let avg_x = sum_x / 4.0;
        let avg_y = sum_y / 4.0;
        assert!((avg_x - 0.5).abs() < 1e-5);
        assert!((avg_y - 0.5).abs() < 1e-5);
    }

    #[test]
    fn obb_overlap_any_small_in_big() {
        let obb = OBB::new(0.5, 0.5, 0.1, 0.1, 0.0);
        assert!(obb.overlap(&sq_half(), OverlapMode::Any));
    }

    #[test]
    fn obb_overlap_any_rotated() {
        let obb = OBB::new(0.5, 0.5, 0.4, 0.1, std::f32::consts::FRAC_PI_4);
        assert!(obb.overlap(&sq1(), OverlapMode::Any));
    }

    #[test]
    fn obb_overlap_any_outside() {
        let obb = OBB::new(2.0, 2.0, 0.1, 0.1, 0.0);
        assert!(!obb.overlap(&sq_half(), OverlapMode::Any));
    }

    #[test]
    fn obb_overlap_iou() {
        let obb = OBB::new(0.25, 0.25, 0.5, 0.5, 0.0);
        assert!(obb.overlap(&sq_half(), OverlapMode::IoU(0.1)));
    }

    #[test]
    fn obb_contains() {
        let big = Polygon::from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let obb = OBB::new(0.4, 0.4, 0.2, 0.2, 0.0);
        assert!(obb.is_contained_in(&big));
    }

    #[test]
    fn obb_contains_rotated_in_big() {
        let big = Polygon::from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let obb = OBB::new(0.5, 0.5, 0.1, 0.1, std::f32::consts::FRAC_PI_4);
        assert!(obb.is_contained_in(&big));
    }

    #[test]
    fn obb_distance_to_edge() {
        let obb = OBB::new(0.5, 0.5, 0.2, 0.2, 0.0);
        let d = obb.distance_to_edge(&sq1());
        assert!(d >= 0.0);
    }

    #[test]
    fn obb_area() {
        let obb = OBB::new(0.5, 0.5, 0.4, 0.5, 0.3);
        assert!((obb.area() - 0.2).abs() < 1e-6);
    }

    // ── Polygon tests ────────────────────────────────────────────

    #[test]
    fn polygon_centroid_square() {
        let c = sq_half().centroid();
        assert!((c.x - 0.5).abs() < 1e-6);
        assert!((c.y - 0.5).abs() < 1e-6);
    }

    #[test]
    fn polygon_centroid_triangle() {
        let c = triangle().centroid();
        assert!((c.x - 0.5).abs() < 0.02);
        assert!((c.y - 0.333).abs() < 0.02);
    }

    #[test]
    fn polygon_centroid_empty() {
        let poly = Polygon::new(Vec::new());
        let c = poly.centroid();
        assert_eq!(c, Point::new(0.0, 0.0));
    }

    #[test]
    fn polygon_area() {
        assert!((sq1().area() - 1.0).abs() < 1e-6);
        assert!((sq_half().area() - 0.25).abs() < 1e-6);
        assert!((triangle().area() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn polygon_edges_len_matches_vertex_count() {
        assert_eq!(sq1().edges().len(), 4);
        assert_eq!(triangle().edges().len(), 3);
        assert_eq!(trap_bed().edges().len(), 4);
    }

    #[test]
    fn polygon_contains_self() {
        assert!(sq_half().is_contained_in(&sq_half()));
    }

    #[test]
    fn polygon_overlap_any() {
        let small = Polygon::from_slice(&[[0.4, 0.4], [0.6, 0.4], [0.6, 0.6], [0.4, 0.6]]);
        assert!(small.overlap(&sq_half(), OverlapMode::Any));
    }

    #[test]
    fn polygon_overlap_iou() {
        let half = sq_half();
        let quarter = Polygon::from_slice(&[[0.25, 0.25], [0.5, 0.25], [0.5, 0.5], [0.25, 0.5]]);
        let iou = quarter.overlap(&half, OverlapMode::IoU(0.2));
        assert!(iou);
    }

    #[test]
    fn polygon_overlap_ios() {
        let quarter = Polygon::from_slice(&[[0.25, 0.25], [0.5, 0.25], [0.5, 0.5], [0.25, 0.5]]);
        let half = sq_half();
        assert!(quarter.overlap(&half, OverlapMode::IoS(0.8)));
    }

    #[test]
    fn polygon_distance_to_edge() {
        let d = sq_half().distance_to_edge(&sq1());
        assert!(d >= 0.0);
        assert!((d - 0.25).abs() < 0.01);
    }

    #[test]
    fn polygon_aabb() {
        let aabb = trap_bed().aabb();
        assert!((aabb.cx - 0.5).abs() < 0.01);
        assert!((aabb.cy - 0.5).abs() < 0.01);
    }

    #[test]
    fn polygon_contains_point() {
        assert!(sq1().contains_point(Point::new(0.5, 0.5)));
        assert!(!sq1().contains_point(Point::new(1.5, 0.5)));
    }

    #[test]
    fn polygon_contains_in_trapezoid() {
        let small = Polygon::from_slice(&[[0.3, 0.3], [0.7, 0.3], [0.7, 0.7], [0.3, 0.7]]);
        assert!(small.is_contained_in(&trap_bed()));
    }

    #[test]
    fn polygon_overlap_no() {
        let far = Polygon::from_slice(&[[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 3.0]]);
        assert!(!far.overlap(&sq_half(), OverlapMode::Any));
    }

    // ── Trap bed zone — clinical region test ─────────────────────

    #[test]
    fn head_bbox_in_bed_zone() {
        let head = BBox::new(0.5, 0.15, 0.1, 0.1);
        assert!(head.is_contained_in(&trap_bed()));
    }

    #[test]
    fn head_bbox_outside_bed_zone() {
        let head = BBox::new(0.5, 0.05, 0.1, 0.1);
        assert!(!head.is_contained_in(&trap_bed()));
    }

    #[test]
    fn head_bbox_overlaps_bed_edge() {
        let head = BBox::new(0.5, 0.08, 0.1, 0.1);
        assert!(head.overlap(&trap_bed(), OverlapMode::Any));
    }

    #[test]
    fn head_distance_to_bed_edge() {
        let head = BBox::new(0.5, 0.3, 0.1, 0.1);
        let d = head.distance_to_edge(&trap_bed());
        assert!(d >= 0.0);
    }

    // ── bbox_polygon_intersection_area ───────────────────────────

    #[test]
    fn bbox_intersection_sq1() {
        let bb = BBox::new(0.5, 0.5, 0.5, 0.5);
        let area = bbox_polygon_intersection_area(&bb, &sq1());
        assert!((area - 0.25).abs() < 1e-5);
    }

    #[test]
    fn bbox_intersection_no_overlap() {
        let bb = BBox::new(0.0, 0.0, 0.05, 0.05);
        let area = bbox_polygon_intersection_area(&bb, &sq_half());
        assert!((area - 0.0).abs() < 1e-5);
    }

    // ── Invariant: contains ⇒ overlap(Any) ───────────────────────

    #[test]
    fn contains_implies_overlap_any_bbox() {
        let bb = BBox::new(0.6, 0.6, 0.1, 0.1);
        if bb.is_contained_in(&sq_half()) {
            assert!(bb.overlap(&sq_half(), OverlapMode::Any));
        }
    }

    #[test]
    fn contains_implies_overlap_any_obb() {
        let obb = OBB::new(0.6, 0.6, 0.05, 0.05, 0.0);
        if obb.is_contained_in(&sq_half()) {
            assert!(obb.overlap(&sq_half(), OverlapMode::Any));
        }
    }

    // ── Invariant: centroid inside hull ──────────────────────────

    #[test]
    fn centroid_inside_hull() {
        let zone = sq_half();
        let c = zone.centroid();
        assert!(zone.contains_point(c));
    }

    #[test]
    fn centroid_inside_triangle() {
        let t = triangle();
        let c = t.centroid();
        assert!(t.contains_point(c));
    }

    // ── Invariant: distance ≥ 0 ──────────────────────────────────

    #[test]
    fn point_distance_nonnegative() {
        let d = Point::new(0.5, 0.5).distance_to_edge(&sq_half());
        assert!(d >= 0.0);
    }

    #[test]
    fn bbox_distance_nonnegative() {
        let d = BBox::new(0.5, 0.5, 0.2, 0.2).distance_to_edge(&sq1());
        assert!(d >= 0.0);
    }

    #[test]
    fn obb_distance_nonnegative() {
        let d = OBB::new(0.5, 0.5, 0.1, 0.1, 0.3).distance_to_edge(&sq1());
        assert!(d >= 0.0);
    }

    #[test]
    fn polygon_distance_nonnegative() {
        let d = sq_half().distance_to_edge(&sq1());
        assert!(d >= 0.0);
    }

    // ── Constructor validation ────────────────────────────────────

    #[test]
    #[should_panic(expected = "non-negative")]
    fn bbox_negative_width_panics_in_debug() {
        BBox::new(0.5, 0.5, -0.1, 0.3);
    }

    #[test]
    #[should_panic(expected = "non-negative")]
    fn obb_negative_height_panics_in_debug() {
        OBB::new(0.5, 0.5, 0.4, -0.1, 0.0);
    }

    // ── NaN guard ────────────────────────────────────────────────

    #[test]
    fn sat_nan_rejects_overlap() {
        let zone = sq_half();
        let nan_bb = BBox::new(f32::NAN, 0.5, 0.1, 0.1);
        assert!(!nan_bb.overlap(&zone, OverlapMode::Any));
        let nan_obb = OBB::new(f32::NAN, 0.5, 0.1, 0.1, 0.0);
        assert!(!nan_obb.overlap(&zone, OverlapMode::Any));
    }

    #[test]
    fn sat_nan_iou_rejects() {
        let zone = sq_half();
        let nan_bb = BBox::new(f32::NAN, 0.5, 0.1, 0.1);
        assert!(!nan_bb.overlap(&zone, OverlapMode::IoU(0.01)));
    }

    // ── CW winding normalization ─────────────────────────────────

    #[test]
    fn cw_polygon_is_normalized() {
        let cw = Polygon::from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let ccw = Polygon::from_slice(&[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
        assert_eq!(cw.area(), ccw.area());
        assert!((cw.area() - 1.0).abs() < 1e-6, "both must have same area");
    }

    #[test]
    fn cw_polygon_contains_works() {
        let cw = Polygon::from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let bb = BBox::new(0.4, 0.4, 0.2, 0.2);
        assert!(bb.is_contained_in(&cw));
    }

    #[test]
    fn cw_polygon_overlaps() {
        let cw = Polygon::from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let other = Polygon::from_slice(&[[0.4, 0.4], [0.6, 0.4], [0.6, 0.6], [0.4, 0.6]]);
        assert!(other.overlap(&cw, OverlapMode::Any));
    }

    #[test]
    fn cw_polygon_iou_matches_normalized() {
        let cw = Polygon::from_slice(&[[0.0, 0.0], [0.5, 0.0], [0.5, 0.5], [0.0, 0.5]]);
        let bb = BBox::new(0.25, 0.25, 0.5, 0.5);
        assert!(bb.overlap(&cw, OverlapMode::IoU(0.4)));
    }

    // ── OBB degenerate axes ──────────────────────────────────────

    #[test]
    fn obb_zero_width_still_overlaps_with_rotation() {
        let obb = OBB::new(0.5, 0.5, 0.0, 0.3, std::f32::consts::FRAC_PI_4);
        assert!(obb.overlap(&sq1(), OverlapMode::Any));
    }

    // ── Edge midpoint distance ───────────────────────────────────

    #[test]
    fn bbox_distance_uses_edges_too() {
        let bb = BBox::new(0.5, 0.5, 0.4, 0.04);
        let zone = Polygon::from_slice(&[[0.3, 0.5], [0.7, 0.5], [0.7, 0.7], [0.3, 0.7]]);
        let d = bb.distance_to_edge(&zone);
        assert!(d >= 0.0);
    }

    // ── Property tests (proptest) ──────────────────────────────────

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn coord() -> impl Strategy<Value = f32> {
            0.0f32..1.0f32
        }

        fn small_coord() -> impl Strategy<Value = f32> {
            0.1f32..0.9f32
        }

        fn small_size() -> impl Strategy<Value = f32> {
            0.01f32..0.4f32
        }

        fn rotation() -> impl Strategy<Value = f32> {
            -std::f32::consts::FRAC_PI_2..std::f32::consts::FRAC_PI_2
        }

        fn convex_quad() -> impl Strategy<Value = Polygon> {
            (small_coord(), small_coord(), small_size(), small_size()).prop_map(|(cx, cy, w, h)| {
                Polygon::from_slice(&[
                    [cx - w, cy - h],
                    [cx + w, cy - h],
                    [cx + w, cy + h],
                    [cx - w, cy + h],
                ])
            })
        }

        fn point() -> impl Strategy<Value = Point> {
            (coord(), coord()).prop_map(|(x, y)| Point::new(x, y))
        }

        fn bbox() -> impl Strategy<Value = BBox> {
            (small_coord(), small_coord(), small_size(), small_size())
                .prop_map(|(cx, cy, w, h)| BBox::new(cx, cy, w, h))
        }

        fn obb() -> impl Strategy<Value = OBB> {
            (
                small_coord(),
                small_coord(),
                small_size(),
                small_size(),
                rotation(),
            )
                .prop_map(|(cx, cy, w, h, r)| OBB::new(cx, cy, w, h, r))
        }

        proptest! {
            #[test]
            fn prop_distance_nonnegative_point(
                p in point(),
                zone in convex_quad(),
            ) {
                let d = p.distance_to_edge(&zone);
                prop_assert!(d >= 0.0);
            }

            #[test]
            fn prop_distance_nonnegative_bbox(
                bb in bbox(),
                zone in convex_quad(),
            ) {
                let d = bb.distance_to_edge(&zone);
                prop_assert!(d >= 0.0);
            }

            #[test]
            fn prop_distance_nonnegative_obb(
                obb in obb(),
                zone in convex_quad(),
            ) {
                let d = obb.distance_to_edge(&zone);
                prop_assert!(d >= 0.0);
            }

            #[test]
            fn prop_distance_nonnegative_polygon(
                a in convex_quad(),
                b in convex_quad(),
            ) {
                let d = a.distance_to_edge(&b);
                prop_assert!(d >= 0.0);
            }

            #[test]
            fn prop_centroid_inside_hull(zone in convex_quad()) {
                let c = zone.centroid();
                prop_assert!(zone.contains_point(c));
            }

            #[test]
            fn prop_polygon_area_nonnegative(zone in convex_quad()) {
                prop_assert!(zone.area() >= 0.0);
            }

            #[test]
            fn prop_contains_implies_overlap_any_bbox(
                bb in bbox(),
                zone in convex_quad(),
            ) {
                if bb.is_contained_in(&zone) {
                    prop_assert!(bb.overlap(&zone, OverlapMode::Any));
                }
            }

            #[test]
            fn prop_contains_implies_overlap_any_obb(
                obb in obb(),
                zone in convex_quad(),
            ) {
                if obb.is_contained_in(&zone) {
                    prop_assert!(obb.overlap(&zone, OverlapMode::Any));
                }
            }

            #[test]
            fn prop_obb_contains_consistent_with_rotation(
                cx in small_coord(),
                cy in small_coord(),
                w in small_size(),
                h in small_size(),
                r1 in rotation(),
                r2 in rotation(),
                zone in convex_quad(),
            ) {
                let obb1 = OBB::new(cx, cy, w, h, r1);
                let obb2 = OBB::new(cx, cy, w, h, r2);
                if obb1.is_contained_in(&zone) {
                    prop_assert!(obb2.overlap(&zone, OverlapMode::Any));
                }
            }

            #[test]
            fn prop_ios_implies_any_overlap(
                bb in bbox(),
                zone in convex_quad(),
            ) {
                if bb.area() < 1e-10 {
                    return Ok(());
                }
                if bb.overlap(&zone, OverlapMode::IoS(0.5)) {
                    prop_assert!(bb.overlap(&zone, OverlapMode::Any));
                }
            }

            #[test]
            fn prop_polygon_overlap_any_symmetric(
                a in convex_quad(),
                b in convex_quad(),
            ) {
                let ab = a.overlap(&b, OverlapMode::Any);
                let ba = b.overlap(&a, OverlapMode::Any);
                prop_assert_eq!(ab, ba);
            }

            #[test]
            fn prop_obb_centroid_equals_center(
                cx in coord(),
                cy in coord(),
                w in small_size(),
                h in small_size(),
                r in rotation(),
            ) {
                let obb = OBB::new(cx, cy, w, h, r);
                let c = obb.centroid();
                prop_assert!((c.x - cx).abs() < 1e-5);
                prop_assert!((c.y - cy).abs() < 1e-5);
            }
        }
    }
}
