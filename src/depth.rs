use ultralytics_inference::DepthMap;

/// Estadisticas robustas de una consulta de region sobre el mapa depth
/// local al ROI (spec §7/§8). La region se da en coordenadas globales y se
/// interseca con el ROI; nunca se consulta el frame completo.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthRoiStats {
    pub roi: [u32; 4],
    pub region: [u32; 4],
    pub local: [u32; 4],
    pub map_width: u32,
    pub map_height: u32,
    pub valid_pixels: u64,
    pub valid_ratio: Option<f32>,
    pub min_depth_m: Option<f32>,
    pub median_depth_m: Option<f32>,
    pub p10_depth_m: Option<f32>,
    pub p90_depth_m: Option<f32>,
    pub max_depth_m: Option<f32>,
}

/// Interseccion global->local de una region contra el ROI (§7).
///
/// `roi` y `region` estan en coordenadas globales; el resultado es una caja
/// local al ROI. `None` si no hay interseccion.
#[must_use]
pub fn region_intersection(roi: [u32; 4], region: [u32; 4]) -> Option<[u32; 4]> {
    let ix1 = region[0].max(roi[0]);
    let iy1 = region[1].max(roi[1]);
    let ix2 = region[2].min(roi[2]);
    let iy2 = region[3].min(roi[3]);
    if ix2 <= ix1 || iy2 <= iy1 {
        None
    } else {
        Some([ix1 - roi[0], iy1 - roi[1], ix2 - roi[0], iy2 - roi[1]])
    }
}

/// Estadisticas de profundidad para una region global consultada contra el
/// mapa local al ROI. `None` si la region no interseca el ROI o el mapa no
/// tiene ningun valor valido.
#[must_use]
pub fn region_stats(depth: &DepthMap, roi: [u32; 4], region: [u32; 4]) -> Option<DepthRoiStats> {
    let mut local = region_intersection(roi, region)?;
    let (map_height, map_width) = map_dims(depth);
    local[2] = local[2].min(map_width);
    local[3] = local[3].min(map_height);
    if local[2] <= local[0] || local[3] <= local[1] {
        return None;
    }

    let mut values: Vec<f32> = Vec::new();
    for y in local[1]..local[3] {
        for x in local[0]..local[2] {
            let value = depth.data[[y as usize, x as usize]];
            if value.is_finite() && value > 0.0 {
                values.push(value);
            }
        }
    }
    if values.is_empty() {
        return None;
    }

    values.sort_unstable_by(f32::total_cmp);
    let area = u64::from(local[2] - local[0]) * u64::from(local[3] - local[1]);
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

    Some(DepthRoiStats {
        roi,
        region,
        local,
        map_width,
        map_height,
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

/// Dimensiones del mapa (ancho, alto) en coordenadas locales al ROI.
#[must_use]
pub fn map_dims(depth: &DepthMap) -> (u32, u32) {
    let shape = depth.data.shape();
    #[allow(clippy::cast_possible_truncation)]
    (shape[1] as u32, shape[0] as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    fn map_from_rows(rows: &[&[f32]]) -> DepthMap {
        let data = Array2::from_shape_fn((rows.len(), rows[0].len()), |(y, x)| rows[y][x]);
        DepthMap::new(data, (rows.len() as u32, rows[0].len() as u32))
    }

    #[test]
    fn section7_intersection_example() {
        let roi = [560, 140, 1240, 820];
        let region = [768, 320, 900, 500];
        assert_eq!(region_intersection(roi, region), Some([208, 180, 340, 360]));
    }

    #[test]
    fn fully_outside_region_has_no_depth() {
        let roi = [560, 140, 1240, 820];
        assert_eq!(region_intersection(roi, [0, 0, 100, 100]), None);
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
        assert_eq!(stats.local, [2, 2, 4, 4]);
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
    fn map_dims_from_data_shape() {
        let map = map_from_rows(&[&[1.0, 2.0], &[3.0, 4.0], &[5.0, 6.0]]);
        assert_eq!(map_dims(&map), (2, 3));
    }
}
