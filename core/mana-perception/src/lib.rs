//! Asynchronous evidence extraction from decoded keyframes.
//!
//! This crate intentionally has no dependency on `mana-control`: its output is
//! raw measurements and detections, while the application owns adaptation to
//! the control port.

// FIXME: Cascade scheduling still accepts application configuration and
// control-owned tracks; it remains here while its input port is narrowed.
// The scheduler implementation is retained in `cascade.rs` for that follow-up.
pub mod depth_map;
pub mod detection;

/// Raw depth-map storage in local ROI coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthFrame {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

impl DepthFrame {
    #[must_use]
    pub fn value_at(&self, x: u32, y: u32) -> Option<f32> {
        (x < self.width && y < self.height)
            .then(|| self.values.get((y * self.width + x) as usize).copied())
            .flatten()
    }
}

/// Raw, policy-free depth statistics for a region.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionStats {
    pub roi: [u32; 4],
    pub region: [u32; 4],
    pub valid_pixels: u64,
    pub valid_ratio: Option<f32>,
    pub min_depth_m: Option<f32>,
    pub median_depth_m: Option<f32>,
    pub p10_depth_m: Option<f32>,
    pub p90_depth_m: Option<f32>,
    pub max_depth_m: Option<f32>,
}

/// Computes region statistics without thresholds, rule names, or policy.
#[must_use]
pub fn region_stats(depth: &DepthFrame, roi: [u32; 4], region: [u32; 4]) -> Option<RegionStats> {
    let x1 = region[0].max(roi[0]);
    let y1 = region[1].max(roi[1]);
    let x2 = region[2].min(roi[2]);
    let y2 = region[3].min(roi[3]);
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    let local_x1 = x1 - roi[0];
    let local_y1 = y1 - roi[1];
    let local_x2 = (x2 - roi[0]).min(depth.width);
    let local_y2 = (y2 - roi[1]).min(depth.height);
    if local_x2 <= local_x1 || local_y2 <= local_y1 {
        return None;
    }
    let mut values = Vec::new();
    for y in local_y1..local_y2 {
        for x in local_x1..local_x2 {
            if let Some(value) = depth
                .value_at(x, y)
                .filter(|value| value.is_finite() && *value > 0.0)
            {
                values.push(value);
            }
        }
    }
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f32::total_cmp);
    let percentile = |p: f32| {
        values[((p * values.len() as f32).ceil() as usize)
            .saturating_sub(1)
            .min(values.len() - 1)]
    };
    let area = u64::from(local_x2 - local_x1) * u64::from(local_y2 - local_y1);
    Some(RegionStats {
        roi,
        region,
        valid_pixels: values.len() as u64,
        valid_ratio: (area > 0).then(|| values.len() as f32 / area as f32),
        min_depth_m: Some(values[0]),
        median_depth_m: Some(percentile(0.5)),
        p10_depth_m: Some(percentile(0.1)),
        p90_depth_m: Some(percentile(0.9)),
        max_depth_m: Some(values[values.len() - 1]),
    })
}
