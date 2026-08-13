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

/// Raw depth statistics for one or more polygonal sampling footprints.
///
/// The polygons and optional clipping contours are expressed in source-frame
/// pixel coordinates, while mask contours remain normalized to the source
/// frame, matching [`DetectionMask`](crate::detection::DetectionMask). The
/// depth map itself stays local to `roi` and may have a different resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonStats {
    pub roi: [u32; 4],
    pub sampled_pixels: u64,
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
    let (local_x1, local_x2) = map_axis_range(x1, x2, roi[0], roi[2], map_width)?;
    let (local_y1, local_y2) = map_axis_range(y1, y2, roi[1], roi[3], map_height)?;
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
    let area = u64::from(local_x2 - local_x1) * u64::from(local_y2 - local_y1);
    let stats = sorted_stats(&values, area);
    Some(RegionStats {
        roi,
        region,
        valid_pixels: stats.valid_pixels,
        valid_ratio: stats.valid_ratio,
        min_depth_m: stats.min_depth_m,
        median_depth_m: stats.median_depth_m,
        p10_depth_m: stats.p10_depth_m,
        p90_depth_m: stats.p90_depth_m,
        max_depth_m: stats.max_depth_m,
    })
}

/// Computes robust statistics over the union of polygonal footprints.
///
/// The map is sampled at pixel centers after transforming each local map
/// pixel back into global ROI coordinates. This keeps the query correct when
/// the model output is 320x320 but the source crop is, for example, 680x680.
/// When `clip_polygons` is `Some`, a sample must also fall inside at least one
/// normalized segmentation contour.
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn polygon_stats(
    depth: &DepthFrame,
    roi: [u32; 4],
    polygons: &[&[[f32; 2]]],
    frame_width: u32,
    frame_height: u32,
    clip_polygons: Option<&[Vec<[f32; 2]>]>,
) -> Option<PolygonStats> {
    if frame_width == 0
        || frame_height == 0
        || roi[2] <= roi[0]
        || roi[3] <= roi[1]
        || polygons.is_empty()
        || polygons.iter().all(|polygon| polygon.len() < 3)
    {
        return None;
    }
    if clip_polygons.is_some_and(|polygons| polygons.is_empty()) {
        return None;
    }

    let (map_width, map_height) = depth.dims();
    if map_width == 0 || map_height == 0 {
        return None;
    }
    let (local_x1, local_x2, local_y1, local_y2) =
        polygon_map_bounds(polygons, roi, map_width, map_height)?;
    if local_x2 <= local_x1 || local_y2 <= local_y1 {
        return None;
    }

    let mut values = Vec::new();
    let mut sampled_pixels = 0_u64;
    for y in local_y1..local_y2 {
        for x in local_x1..local_x2 {
            let global = map_pixel_to_global(x, y, roi, map_width, map_height);
            if !polygons
                .iter()
                .any(|polygon| point_in_polygon(global, polygon))
            {
                continue;
            }
            if let Some(clips) = clip_polygons
                && !clips.iter().any(|polygon| {
                    point_in_polygon(
                        [
                            global[0] / frame_width as f32,
                            global[1] / frame_height as f32,
                        ],
                        polygon,
                    )
                })
            {
                continue;
            }
            sampled_pixels += 1;
            let value = depth.value_at(y as usize, x as usize);
            if value.is_finite() && value > 0.0 {
                values.push(value);
            }
        }
    }

    if sampled_pixels == 0 {
        return None;
    }
    values.sort_unstable_by(f32::total_cmp);
    let stats = sorted_stats(&values, sampled_pixels);
    Some(PolygonStats {
        roi,
        sampled_pixels,
        valid_pixels: stats.valid_pixels,
        valid_ratio: stats.valid_ratio,
        min_depth_m: stats.min_depth_m,
        median_depth_m: stats.median_depth_m,
        p10_depth_m: stats.p10_depth_m,
        p90_depth_m: stats.p90_depth_m,
        max_depth_m: stats.max_depth_m,
    })
}

#[derive(Debug, Clone, Copy)]
struct SortedStats {
    valid_pixels: u64,
    valid_ratio: Option<f32>,
    min_depth_m: Option<f32>,
    median_depth_m: Option<f32>,
    p10_depth_m: Option<f32>,
    p90_depth_m: Option<f32>,
    max_depth_m: Option<f32>,
}

fn sorted_stats(values: &[f32], sampled_pixels: u64) -> SortedStats {
    let valid_pixels = values.len() as u64;
    let Some(&min_depth_m) = values.first() else {
        return SortedStats {
            valid_pixels: 0,
            valid_ratio: (sampled_pixels > 0).then_some(0.0),
            min_depth_m: None,
            median_depth_m: None,
            p10_depth_m: None,
            p90_depth_m: None,
            max_depth_m: None,
        };
    };
    SortedStats {
        valid_pixels,
        #[allow(clippy::cast_precision_loss)]
        valid_ratio: (sampled_pixels > 0).then(|| valid_pixels as f32 / sampled_pixels as f32),
        min_depth_m: Some(min_depth_m),
        median_depth_m: Some(percentile(values, 0.5)),
        p10_depth_m: Some(percentile(values, 0.1)),
        p90_depth_m: Some(percentile(values, 0.9)),
        max_depth_m: values.last().copied(),
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn percentile(values: &[f32], p: f32) -> f32 {
    let rank = ((p * values.len() as f32).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    values[rank]
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn map_axis_range(
    global_start: u32,
    global_end: u32,
    roi_start: u32,
    roi_end: u32,
    map_len: u32,
) -> Option<(u32, u32)> {
    if global_end <= global_start || roi_end <= roi_start || map_len == 0 {
        return None;
    }
    let start = global_start.max(roi_start).min(roi_end);
    let end = global_end.max(roi_start).min(roi_end);
    if end <= start {
        return None;
    }
    let roi_len = (roi_end - roi_start) as f64;
    let local_start = (((start - roi_start) as f64 * map_len as f64) / roi_len).floor() as u32;
    let local_end = (((end - roi_start) as f64 * map_len as f64) / roi_len).ceil() as u32;
    Some((local_start.min(map_len), local_end.min(map_len)))
}

#[allow(clippy::cast_precision_loss)]
fn map_pixel_to_global(x: u32, y: u32, roi: [u32; 4], map_width: u32, map_height: u32) -> [f32; 2] {
    [
        roi[0] as f32 + (x as f32 + 0.5) * (roi[2] - roi[0]) as f32 / map_width as f32,
        roi[1] as f32 + (y as f32 + 0.5) * (roi[3] - roi[1]) as f32 / map_height as f32,
    ]
}

#[allow(clippy::cast_precision_loss)]
fn polygon_map_bounds(
    polygons: &[&[[f32; 2]]],
    roi: [u32; 4],
    map_width: u32,
    map_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for polygon in polygons {
        for &[x, y] in *polygon {
            if x.is_finite() && y.is_finite() {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let start_x = min_x.floor().max(roi[0] as f32) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let end_x = max_x.ceil().min(roi[2] as f32) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let start_y = min_y.floor().max(roi[1] as f32) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let end_y = max_y.ceil().min(roi[3] as f32) as u32;
    let (x1, x2) = map_axis_range(start_x, end_x, roi[0], roi[2], map_width)?;
    let (y1, y2) = map_axis_range(start_y, end_y, roi[1], roi[3], map_height)?;
    Some((x1, x2, y1, y2))
}

fn point_in_polygon(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    if polygon.len() < 3 || point.iter().any(|value| !value.is_finite()) {
        return false;
    }
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        if !current[0].is_finite() || !current[1].is_finite() {
            previous = current;
            continue;
        }
        let crosses = (current[1] > point[1]) != (previous[1] > point[1]);
        if crosses {
            let denominator = previous[1] - current[1];
            if denominator.abs() > f32::EPSILON {
                let x_intersection =
                    (previous[0] - current[0]) * (point[1] - current[1]) / denominator + current[0];
                if point[0] < x_intersection {
                    inside = !inside;
                }
            }
        }
        previous = current;
    }
    inside
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

    #[test]
    fn polygon_stats_maps_global_polygon_to_a_lower_resolution_roi() {
        let roi = [10, 10, 18, 18];
        let map = map_from_rows(&[
            &[1.0, 2.0, 3.0, 4.0],
            &[5.0, 6.0, 7.0, 8.0],
            &[9.0, 10.0, 11.0, 12.0],
            &[13.0, 14.0, 15.0, 16.0],
        ]);
        let polygon = vec![[12.0, 12.0], [16.0, 12.0], [16.0, 16.0], [12.0, 16.0]];
        let stats = polygon_stats(&map, roi, &[polygon.as_slice()], 20, 20, None)
            .expect("polygon inside ROI");
        assert_eq!(stats.sampled_pixels, 4);
        assert_eq!(stats.valid_pixels, 4);
        assert_eq!(stats.median_depth_m, Some(7.0));
    }

    #[test]
    fn polygon_stats_clips_samples_to_normalized_segmentation_contour() {
        let roi = [0, 0, 4, 4];
        let map = map_from_rows(&[
            &[1.0, 2.0, 3.0, 4.0],
            &[5.0, 6.0, 7.0, 8.0],
            &[9.0, 10.0, 11.0, 12.0],
            &[13.0, 14.0, 15.0, 16.0],
        ]);
        let footprint = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let clip = vec![vec![[0.0, 0.0], [0.5, 0.0], [0.5, 1.0], [0.0, 1.0]]];
        let stats = polygon_stats(&map, roi, &[footprint.as_slice()], 4, 4, Some(&clip))
            .expect("clipped polygon");
        assert_eq!(stats.sampled_pixels, 8);
        assert_eq!(stats.valid_pixels, 8);
        assert_eq!(stats.min_depth_m, Some(1.0));
        assert_eq!(stats.max_depth_m, Some(14.0));
    }
}
