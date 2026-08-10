//! Asynchronous evidence extraction from decoded keyframes.
//!
//! This crate intentionally has no dependency on `mana-control`: its output is
//! raw measurements and detections, while the application owns adaptation to
//! the control port.

pub mod cascade;
pub mod depth_map;
pub mod detection;
pub mod domain;

pub use depth_map::DepthFrame;
pub use domain::{ClassName, DomStr, ModelId};

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
///
/// `roi` and `region` are in global coordinates; the depth map is local to the ROI.
#[must_use]
pub fn region_stats(depth: &DepthFrame, roi: [u32; 4], region: [u32; 4]) -> Option<RegionStats> {
    let x1 = region[0].max(roi[0]);
    let y1 = region[1].max(roi[1]);
    let x2 = region[2].min(roi[2]);
    let y2 = region[3].min(roi[3]);
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    let (map_width, map_height) = depth.dims();
    let local_x1 = x1 - roi[0];
    let local_y1 = y1 - roi[1];
    let local_x2 = (x2 - roi[0]).min(map_width);
    let local_y2 = (y2 - roi[1]).min(map_height);
    if local_x2 <= local_x1 || local_y2 <= local_y1 {
        return None;
    }
    let mut values = Vec::new();
    for y in local_y1..local_y2 {
        for x in local_x1..local_x2 {
            let value = depth.value_at(y as usize, x as usize);
            if value.is_finite() && value > 0.0 {
                values.push(value);
            }
        }
    }
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f32::total_cmp);
    let n = values.len();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let percentile = |p: f32| {
        let rank = ((p * n as f32).ceil() as usize)
            .saturating_sub(1)
            .min(n - 1);
        values[rank]
    };
    let area = u64::from(local_x2 - local_x1) * u64::from(local_y2 - local_y1);
    Some(RegionStats {
        roi,
        region,
        valid_pixels: n as u64,
        #[allow(clippy::cast_precision_loss)]
        valid_ratio: (area > 0).then(|| n as f32 / area as f32),
        min_depth_m: Some(values[0]),
        median_depth_m: Some(percentile(0.5)),
        p10_depth_m: Some(percentile(0.1)),
        p90_depth_m: Some(percentile(0.9)),
        max_depth_m: Some(values[n - 1]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;
    use ultralytics_inference::DepthMap;

    #[allow(clippy::cast_possible_truncation)]
    fn map_from_rows(rows: &[&[f32]]) -> DepthFrame {
        let data = Array2::from_shape_fn((rows.len(), rows[0].len()), |(y, x)| rows[y][x]);
        DepthFrame::from_ultralytics(DepthMap::new(
            data,
            (rows.len() as u32, rows[0].len() as u32),
        ))
    }

    #[test]
    fn stats_median_and_percentiles() {
        let roi = [0, 0, 4, 4];
        let map = map_from_rows(&[
            &[1.0, 2.0, 3.0, 0.0],
            &[4.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0],
        ]);
        let stats = region_stats(&map, roi, [0, 0, 4, 4]).expect("full ROI query");
        assert_eq!(stats.valid_pixels, 4);
        assert_eq!(stats.valid_ratio, Some(0.25));
        assert_eq!(stats.min_depth_m, Some(1.0));
        assert_eq!(stats.max_depth_m, Some(4.0));
        assert_eq!(stats.median_depth_m, Some(2.0));
        assert_eq!(stats.p10_depth_m, Some(1.0));
        assert_eq!(stats.p90_depth_m, Some(4.0));
    }

    #[test]
    fn region_partially_outside_map_is_clamped() {
        let roi = [10, 10, 14, 14];
        let map = map_from_rows(&[
            &[0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 5.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0],
        ]);
        let stats = region_stats(&map, roi, [12, 12, 100, 100]).expect("clamped query");
        assert_eq!(stats.valid_pixels, 1);
        assert_eq!(stats.valid_ratio, Some(0.25));
    }

    #[test]
    fn region_without_valid_pixels_has_no_stats() {
        let roi = [0, 0, 4, 4];
        let map = map_from_rows(&[&[0.0; 4], &[0.0; 4], &[0.0; 4], &[0.0; 4]]);
        assert_eq!(region_stats(&map, roi, [0, 0, 4, 4]), None);
    }

    #[test]
    fn fully_outside_region_has_no_stats() {
        let roi = [560, 140, 1240, 820];
        let map = map_from_rows(&[&[1.0, 1.0], &[1.0, 1.0]]);
        assert_eq!(region_stats(&map, roi, [0, 0, 100, 100]), None);
    }
}
