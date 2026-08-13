//! Relative depth references for fixed scene surfaces.
//!
//! This module deliberately contains no runtime policy. It describes a
//! camera/model-specific envelope that an application may use as evidence.

use serde::{Deserialize, Serialize};

use crate::PolygonStats;

pub const SURFACE_CALIBRATION_SCHEMA_VERSION: u32 = 1;

/// A fixed-scene reference profile for bed and floor surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceCalibration {
    pub schema_version: u32,
    pub model_key: String,
    #[serde(default)]
    pub model_fingerprint: Option<String>,
    pub frame_width: u32,
    pub frame_height: u32,
    pub roi: [u32; 4],
    #[serde(default)]
    pub bed: Vec<SurfaceZone>,
    #[serde(default)]
    pub floor: Vec<SurfaceZone>,
}

/// A polygonal patch of a known surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceZone {
    pub name: String,
    pub polygon: Vec<[f32; 2]>,
    pub median_depth: f32,
    pub p10_depth: f32,
    pub p90_depth: f32,
    pub mad_depth: f32,
    pub valid_ratio: f32,
    pub frame_samples: u32,
    pub valid_frames: u32,
    #[serde(default)]
    pub tolerance: f32,
}

/// The result of comparing an observed depth to a surface envelope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceMatch {
    pub residual: f32,
    pub in_envelope: bool,
    pub distance_to_envelope: f32,
}

/// A frame-level accumulator used by the isolated calibration tool.
#[derive(Debug, Default)]
pub struct SurfaceAccumulator {
    medians: Vec<f32>,
    valid_ratios: Vec<f32>,
    frame_samples: u32,
}

impl SurfaceCalibration {
    #[must_use]
    pub fn new(
        model_key: String,
        model_fingerprint: Option<String>,
        frame_width: u32,
        frame_height: u32,
        roi: [u32; 4],
    ) -> Self {
        Self {
            schema_version: SURFACE_CALIBRATION_SCHEMA_VERSION,
            model_key,
            model_fingerprint,
            frame_width,
            frame_height,
            roi,
            bed: Vec::new(),
            floor: Vec::new(),
        }
    }

    /// Validates the profile before it is used or promoted.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SURFACE_CALIBRATION_SCHEMA_VERSION {
            return Err(format!(
                "unsupported surface calibration schema {}",
                self.schema_version
            ));
        }
        if self.model_key.trim().is_empty() {
            return Err("surface calibration model_key is empty".into());
        }
        if self.frame_width == 0 || self.frame_height == 0 {
            return Err("surface calibration frame dimensions must be positive".into());
        }
        validate_roi(self.roi, self.frame_width, self.frame_height)?;
        validate_zones("bed", &self.bed, self.frame_width, self.frame_height)?;
        validate_zones("floor", &self.floor, self.frame_width, self.frame_height)?;
        Ok(())
    }

    pub fn upsert_zone(&mut self, layer: SurfaceLayer, zone: SurfaceZone) -> Result<(), String> {
        zone.validate(self.frame_width, self.frame_height)?;
        let zones = self.zones_mut(layer);
        if let Some(existing) = zones.iter_mut().find(|existing| existing.name == zone.name) {
            *existing = zone;
        } else {
            zones.push(zone);
        }
        Ok(())
    }

    #[must_use]
    pub fn zones(&self, layer: SurfaceLayer) -> &[SurfaceZone] {
        match layer {
            SurfaceLayer::Bed => &self.bed,
            SurfaceLayer::Floor => &self.floor,
        }
    }

    fn zones_mut(&mut self, layer: SurfaceLayer) -> &mut Vec<SurfaceZone> {
        match layer {
            SurfaceLayer::Bed => &mut self.bed,
            SurfaceLayer::Floor => &mut self.floor,
        }
    }
}

impl SurfaceZone {
    pub fn validate(&self, frame_width: u32, frame_height: u32) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("surface zone name is empty".into());
        }
        validate_polygon(&self.polygon, frame_width, frame_height)?;
        if !self.median_depth.is_finite()
            || !self.p10_depth.is_finite()
            || !self.p90_depth.is_finite()
            || !self.mad_depth.is_finite()
            || !self.tolerance.is_finite()
            || self.median_depth <= 0.0
            || self.p10_depth <= 0.0
            || self.p90_depth <= 0.0
            || self.mad_depth < 0.0
            || self.tolerance < 0.0
        {
            return Err(format!(
                "surface zone '{}' has invalid depth statistics",
                self.name
            ));
        }
        if self.p10_depth > self.median_depth || self.median_depth > self.p90_depth {
            return Err(format!(
                "surface zone '{}' depth percentiles are not ordered",
                self.name
            ));
        }
        if !self.valid_ratio.is_finite() || !(0.0..=1.0).contains(&self.valid_ratio) {
            return Err(format!(
                "surface zone '{}' has invalid valid_ratio",
                self.name
            ));
        }
        if self.frame_samples == 0 || self.valid_frames > self.frame_samples {
            return Err(format!(
                "surface zone '{}' has invalid frame counts",
                self.name
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn matches(&self, observed_depth: f32, extra_tolerance: f32) -> Option<SurfaceMatch> {
        if !observed_depth.is_finite() || observed_depth <= 0.0 {
            return None;
        }
        let tolerance = self.tolerance.max(extra_tolerance.max(0.0));
        let lower = self.p10_depth - tolerance;
        let upper = self.p90_depth + tolerance;
        let distance_to_envelope = if observed_depth < lower {
            lower - observed_depth
        } else if observed_depth > upper {
            observed_depth - upper
        } else {
            0.0
        };
        Some(SurfaceMatch {
            residual: observed_depth - self.median_depth,
            in_envelope: distance_to_envelope == 0.0,
            distance_to_envelope,
        })
    }

    #[must_use]
    pub fn contains(&self, point: [f32; 2]) -> bool {
        point_in_polygon(point, &self.polygon)
    }
}

impl SurfaceAccumulator {
    pub fn push(&mut self, stats: &PolygonStats, min_valid_ratio: f32) -> bool {
        self.frame_samples = self.frame_samples.saturating_add(1);
        let Some(median) = stats.median_depth_m else {
            return false;
        };
        let valid_ratio = stats.valid_ratio.unwrap_or(0.0);
        if !median.is_finite()
            || median <= 0.0
            || !valid_ratio.is_finite()
            || valid_ratio < min_valid_ratio
        {
            return false;
        }
        self.medians.push(median);
        self.valid_ratios.push(valid_ratio);
        true
    }

    #[must_use]
    pub fn finish(
        self,
        name: String,
        polygon: Vec<[f32; 2]>,
        tolerance: f32,
    ) -> Option<SurfaceZone> {
        if self.medians.is_empty() {
            return None;
        }
        let mut medians = self.medians;
        medians.sort_unstable_by(f32::total_cmp);
        let median_depth = percentile(&medians, 0.5);
        let p10_depth = percentile(&medians, 0.1);
        let p90_depth = percentile(&medians, 0.9);
        let mut deviations: Vec<f32> = medians
            .iter()
            .map(|value| (value - median_depth).abs())
            .collect();
        deviations.sort_unstable_by(f32::total_cmp);
        let valid_ratio = self.valid_ratios.iter().sum::<f32>() / self.valid_ratios.len() as f32;
        Some(SurfaceZone {
            name,
            polygon,
            median_depth,
            p10_depth,
            p90_depth,
            mad_depth: percentile(&deviations, 0.5),
            valid_ratio,
            frame_samples: self.frame_samples,
            valid_frames: self.valid_ratios.len() as u32,
            tolerance: tolerance.max(0.0),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceLayer {
    Bed,
    Floor,
}

impl SurfaceLayer {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bed" => Some(Self::Bed),
            "floor" => Some(Self::Floor),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bed => "bed",
            Self::Floor => "floor",
        }
    }
}

fn validate_zones(
    layer: &str,
    zones: &[SurfaceZone],
    frame_width: u32,
    frame_height: u32,
) -> Result<(), String> {
    for (index, zone) in zones.iter().enumerate() {
        zone.validate(frame_width, frame_height)
            .map_err(|error| format!("{layer}[{index}]: {error}"))?;
    }
    for (index, zone) in zones.iter().enumerate() {
        if zones[..index].iter().any(|other| other.name == zone.name) {
            return Err(format!("duplicate {layer} surface zone '{}'", zone.name));
        }
    }
    Ok(())
}

fn validate_roi(roi: [u32; 4], frame_width: u32, frame_height: u32) -> Result<(), String> {
    if roi[2] <= roi[0] || roi[3] <= roi[1] || roi[2] > frame_width || roi[3] > frame_height {
        return Err(format!("invalid surface calibration ROI {roi:?}"));
    }
    Ok(())
}

fn validate_polygon(
    polygon: &[[f32; 2]],
    frame_width: u32,
    frame_height: u32,
) -> Result<(), String> {
    if polygon.len() < 3 {
        return Err("surface polygon needs at least three vertices".into());
    }
    for [x, y] in polygon {
        if !x.is_finite()
            || !y.is_finite()
            || *x < 0.0
            || *y < 0.0
            || *x > frame_width as f32
            || *y > frame_height as f32
        {
            return Err("surface polygon contains a vertex outside the frame".into());
        }
    }
    if polygon_area(polygon) <= f32::EPSILON {
        return Err("surface polygon is degenerate".into());
    }
    Ok(())
}

fn polygon_area(polygon: &[[f32; 2]]) -> f32 {
    polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .map(|([x1, y1], [x2, y2])| x1 * y2 - x2 * y1)
        .sum::<f32>()
        .abs()
        * 0.5
}

fn point_in_polygon(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let [px, py] = point;
    for (a, b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        let [ax, ay] = *a;
        let [bx, by] = *b;
        let crosses = (ay > py) != (by > py);
        if crosses {
            let x_at_y = (bx - ax) * (py - ay) / (by - ay) + ax;
            if px < x_at_y {
                inside = !inside;
            }
        }
    }
    inside
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn percentile(values: &[f32], percentile: f32) -> f32 {
    let rank = (percentile * values.len() as f32).ceil() as usize;
    values[rank.saturating_sub(1).min(values.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(median: f32, valid_ratio: f32) -> PolygonStats {
        PolygonStats {
            roi: [0, 0, 10, 10],
            sampled_pixels: 100,
            valid_pixels: (valid_ratio * 100.0) as u64,
            valid_ratio: Some(valid_ratio),
            min_depth_m: Some(median),
            median_depth_m: Some(median),
            p10_depth_m: Some(median),
            p90_depth_m: Some(median),
            max_depth_m: Some(median),
        }
    }

    #[test]
    fn accumulator_builds_envelope_and_mad() {
        let mut accumulator = SurfaceAccumulator::default();
        assert!(accumulator.push(&stats(1.0, 1.0), 0.5));
        assert!(accumulator.push(&stats(1.2, 1.0), 0.5));
        assert!(accumulator.push(&stats(1.4, 1.0), 0.5));
        let zone = accumulator
            .finish("head".into(), vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]], 0.1)
            .expect("zone");
        assert_eq!(zone.frame_samples, 3);
        assert_eq!(zone.valid_frames, 3);
        assert!((zone.median_depth - 1.2).abs() < f32::EPSILON);
        assert!((zone.p10_depth - 1.0).abs() < f32::EPSILON);
        assert!((zone.p90_depth - 1.4).abs() < f32::EPSILON);
        assert!((zone.mad_depth - 0.2).abs() < f32::EPSILON);
        assert!(zone.matches(1.45, 0.1).expect("match").in_envelope);
        assert!(!zone.matches(1.8, 0.1).expect("match").in_envelope);
    }

    #[test]
    fn invalid_polygon_is_rejected() {
        let zone = SurfaceZone {
            name: "bed".into(),
            polygon: vec![[0.0, 0.0], [1.0, 1.0]],
            median_depth: 1.0,
            p10_depth: 1.0,
            p90_depth: 1.0,
            mad_depth: 0.0,
            valid_ratio: 1.0,
            frame_samples: 1,
            valid_frames: 1,
            tolerance: 0.0,
        };
        assert!(zone.validate(10, 10).is_err());
    }

    #[test]
    fn calibration_rejects_duplicate_zone_names() {
        let mut calibration =
            SurfaceCalibration::new("depth-standard".into(), None, 10, 10, [0, 0, 10, 10]);
        let zone = SurfaceZone {
            name: "head".into(),
            polygon: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]],
            median_depth: 1.0,
            p10_depth: 1.0,
            p90_depth: 1.0,
            mad_depth: 0.0,
            valid_ratio: 1.0,
            frame_samples: 1,
            valid_frames: 1,
            tolerance: 0.0,
        };
        calibration.bed.push(zone.clone());
        calibration.bed.push(zone);
        assert!(calibration.validate().is_err());
    }
}
