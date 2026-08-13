//! Utilidades compartidas por las herramientas de calibración de profundidad
//! (`deep-calib-preview`, `deep-calib-pose`, `deep-calib-matrix`).
//!
//! Carga de sesión/imagen, resolución de modelos desde el catálogo, corrida del
//! modelo depth de escena, estadísticas por zona, escalas de color por capa y
//! dibujo de overlays.
#![allow(dead_code)]

use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use image::{DynamicImage, GenericImageView, Rgb, Rgba, RgbaImage};
use imageproc::drawing::{draw_hollow_polygon_mut, draw_polygon_mut};
use imageproc::point::Point;
use mana_lite::config::{CropType, load_app_config, load_model_catalog};
use mana_lite::depth_map::DepthFrame;
use mana_lite::{SurfaceCalibration, SurfaceLayer, SurfaceZone, polygon_stats};
use ultralytics_inference::{InferenceConfig, YOLOModel};

/// Resultado de correr el modelo depth de escena sobre un frame.
pub struct DepthRun {
    pub depth: DepthFrame,
    pub roi: [u32; 4],
}

/// Imagen de entrada ya validada contra la sesión.
pub struct Frame {
    pub image: DynamicImage,
    pub frame_width: u32,
    pub frame_height: u32,
}

/// Estadísticas de una zona para la tabla de auditoría.
pub struct ZoneStat {
    pub label: String,
    pub calibrated_m: f32,
    pub observed_m: Option<f32>,
    pub delta_m: Option<f32>,
    pub valid_ratio: Option<f32>,
}

/// Lee y valida una sesión `deep-calib.toml`.
pub fn load_calibration(session: &Path) -> Result<SurfaceCalibration, Box<dyn Error>> {
    let content = std::fs::read_to_string(session)?;
    let calibration: SurfaceCalibration = toml::from_str(&content)?;
    calibration.validate()?;
    Ok(calibration)
}

/// Abre la imagen y verifica que sus dimensiones coincidan con la sesión.
pub fn open_frame(
    image_path: &Path,
    calibration: &SurfaceCalibration,
) -> Result<Frame, Box<dyn Error>> {
    let image = image::open(image_path)?;
    let (frame_width, frame_height) = image.dimensions();
    if frame_width != calibration.frame_width || frame_height != calibration.frame_height {
        return Err(format!(
            "image {} is {frame_width}x{frame_height}, session expects {}x{}",
            image_path.display(),
            calibration.frame_width,
            calibration.frame_height
        )
        .into());
    }
    Ok(Frame {
        image,
        frame_width,
        frame_height,
    })
}

/// Resuelve la ruta del modelo y su ROI estático (si existe) desde el catálogo.
pub fn catalog_model_context(
    config_path: &Path,
    model_key: &str,
) -> Result<(PathBuf, Option<[u32; 4]>), Box<dyn Error>> {
    let app_config = load_app_config(config_path)?;
    let catalog = load_model_catalog(&app_config.inference.model_catalog)?;
    let entry = catalog
        .models
        .get(model_key)
        .ok_or_else(|| format!("model '{model_key}' is absent from the catalog"))?;
    let configured_roi = entry
        .crop
        .as_ref()
        .filter(|crop| crop.crop_type == CropType::Static)
        .and_then(|crop| crop.region);
    Ok((entry.path.clone(), configured_roi))
}

/// Returns the scene-depth model context and the configured margin used to
/// derive the effective ROI. Scene depth variants without their own static
/// crop inherit the canonical `depth-standard` crop configuration.
pub fn catalog_depth_context(
    config_path: &Path,
    model_key: &str,
) -> Result<(PathBuf, Option<[u32; 4]>, f32), Box<dyn Error>> {
    let app_config = load_app_config(config_path)?;
    let catalog = load_model_catalog(&app_config.inference.model_catalog)?;
    let entry = catalog
        .models
        .get(model_key)
        .ok_or_else(|| format!("model '{model_key}' is absent from the catalog"))?;
    let own_crop = entry
        .crop
        .as_ref()
        .filter(|crop| crop.crop_type == CropType::Static);
    let canonical_crop = catalog
        .models
        .get("depth-standard")
        .and_then(|entry| entry.crop.as_ref())
        .filter(|crop| crop.crop_type == CropType::Static);
    let crop = own_crop.or(canonical_crop);
    Ok((
        entry.path.clone(),
        crop.and_then(|crop| crop.region),
        crop.map_or(0.0, |crop| crop.margin),
    ))
}

/// Rebasa la ruta `tools/model-tools/...` hacia el repo hermano `model-tools`
/// cuando el artefacto no existe en el árbol del workspace.
pub fn resolve_model_path(path: PathBuf, config_path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if path.is_absolute() || path.exists() {
        return Ok(path);
    }
    let current_dir = std::env::current_dir()?;
    let config_path = if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        current_dir.join(config_path)
    };
    let Some(repo_root) = config_path.parent().and_then(Path::parent) else {
        return Ok(path);
    };
    let Ok(relative_artifact) = path.strip_prefix("tools/model-tools") else {
        return Ok(path);
    };
    let Some(workspace_root) = repo_root.parent() else {
        return Ok(path);
    };
    let sibling_artifact = workspace_root.join("model-tools").join(relative_artifact);
    if sibling_artifact.exists() {
        Ok(sibling_artifact)
    } else {
        Ok(path)
    }
}

/// Carga un modelo YOLO con ROI estático opcional e `imgsz` opcional.
pub fn load_model(
    model_path: &Path,
    roi: Option<[u32; 4]>,
    imgsz: Option<u32>,
) -> Result<YOLOModel, Box<dyn Error>> {
    let mut model_config = InferenceConfig::default();
    if let Some([x1, y1, x2, y2]) = roi {
        model_config = model_config.with_roi(x1, y1, x2, y2);
    }
    if let Some(size) = imgsz {
        model_config = model_config.with_imgsz(size as usize, size as usize);
    }
    model_config = model_config.with_save(false);
    Ok(YOLOModel::load_with_config(model_path, model_config)?)
}

/// Corre el modelo depth y valida que el ROI del resultado coincida con el de
/// la sesión.
pub fn run_depth(
    model: &mut YOLOModel,
    image: &DynamicImage,
    image_path: &Path,
    expected_roi: [u32; 4],
) -> Result<DepthRun, Box<dyn Error>> {
    let results = model.predict_image(image, image_path.to_string_lossy().into_owned())?;
    let result = results.first().ok_or("depth model returned no result")?;
    let depth = result
        .depth
        .as_ref()
        .ok_or("depth model returned no depth map")?;
    let depth = DepthFrame::from_ultralytics(depth.clone());
    let actual_roi = result
        .roi
        .map(|(x1, y1, x2, y2)| [x1, y1, x2, y2])
        .unwrap_or(expected_roi);
    if actual_roi != expected_roi {
        return Err(format!(
            "depth result ROI {actual_roi:?} differs from session ROI {expected_roi:?}"
        )
        .into());
    }
    Ok(DepthRun {
        depth,
        roi: actual_roi,
    })
}

/// Igual que `run_depth`, pero además devuelve el `DepthMap` crudo del modelo
/// (necesario para colorear el mapa con las paletas de ultralytics-inference).
pub fn run_depth_raw(
    model: &mut YOLOModel,
    image: &DynamicImage,
    image_path: &Path,
    expected_roi: [u32; 4],
) -> Result<(DepthRun, ultralytics_inference::DepthMap), Box<dyn Error>> {
    let results = model.predict_image(image, image_path.to_string_lossy().into_owned())?;
    let result = results.first().ok_or("depth model returned no result")?;
    let raw = result
        .depth
        .as_ref()
        .ok_or("depth model returned no depth map")?;
    let depth = DepthFrame::from_ultralytics(raw.clone());
    let actual_roi = result
        .roi
        .map(|(x1, y1, x2, y2)| [x1, y1, x2, y2])
        .unwrap_or(expected_roi);
    if actual_roi != expected_roi {
        return Err(format!(
            "depth result ROI {actual_roi:?} differs from session ROI {expected_roi:?}"
        )
        .into());
    }
    Ok((DepthRun { depth, roi: actual_roi }, raw.clone()))
}

/// Mediana y percentiles calibrados por capa.
pub fn layer_bounds(calibration: &SurfaceCalibration, layer: SurfaceLayer) -> (f32, f32) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for zone in calibration.zones(layer) {
        min = min.min(zone.median_depth);
        max = max.max(zone.median_depth);
    }
    if !min.is_finite() || !max.is_finite() {
        return (0.0, 1.0);
    }
    if max <= min {
        max = min + 1.0;
    }
    (min, max)
}

/// Escala de color por capa: `bed` en familia azul, `floor` en familia amarilla.
/// Cerca = tono más fuerte y más opaco; lejos = más tenue.
pub fn layer_scale(
    layer: SurfaceLayer,
    value: f32,
    layer_min: f32,
    layer_max: f32,
    global_alpha: f32,
) -> ([u8; 3], u8) {
    let closeness = ((layer_max - value) / (layer_max - layer_min)).clamp(0.0, 1.0);
    let strength = 0.25 + 0.75 * closeness;
    let base = match layer {
        SurfaceLayer::Bed => [0.0, 0.0, 255.0],
        SurfaceLayer::Floor => [255.0, 255.0, 0.0],
    };
    let color = [
        (base[0] * strength + 255.0 * (1.0 - strength)).round() as u8,
        (base[1] * strength + 255.0 * (1.0 - strength)).round() as u8,
        (base[2] * strength + 255.0 * (1.0 - strength)).round() as u8,
    ];
    let alpha = (0.3 + 0.5 * closeness) * global_alpha.clamp(0.0, 1.0) * 255.0;
    (color, alpha.round() as u8)
}

/// Estadísticas por zona para la tabla de auditoría.
pub fn zone_stats(
    depth: &DepthFrame,
    roi: [u32; 4],
    calibration: &SurfaceCalibration,
    frame_width: u32,
    frame_height: u32,
) -> Vec<ZoneStat> {
    let mut rows = Vec::new();
    for layer in [SurfaceLayer::Bed, SurfaceLayer::Floor] {
        for zone in calibration.zones(layer) {
            let stats = polygon_stats(
                depth,
                roi,
                &[zone.polygon.as_slice()],
                frame_width,
                frame_height,
                None,
            );
            let observed = stats.as_ref().and_then(|stats| stats.median_depth_m);
            let delta = observed.map(|observed| observed - zone.median_depth);
            let valid_ratio = stats.and_then(|stats| stats.valid_ratio);
            rows.push(ZoneStat {
                label: format!("{}/{}", layer.as_str(), zone.name),
                calibrated_m: zone.median_depth,
                observed_m: observed,
                delta_m: delta,
                valid_ratio,
            });
        }
    }
    rows
}

pub fn print_zone_table(rows: &[ZoneStat]) {
    println!("zone\tcalibrated_m\tobserved_m\tdelta_m\tvalid_ratio");
    for row in rows {
        println!(
            "{}\t{:.3}\t{}\t{}\t{}",
            row.label,
            row.calibrated_m,
            format_option(row.observed_m),
            format_option(row.delta_m),
            row.valid_ratio
                .map_or_else(|| "none".to_string(), |ratio| format!("{ratio:.3}"))
        );
    }
}

pub fn print_legend(calibration: &SurfaceCalibration) {
    let (bed_min, bed_max) = layer_bounds(calibration, SurfaceLayer::Bed);
    let (floor_min, floor_max) = layer_bounds(calibration, SurfaceLayer::Floor);
    println!(
        "legend: bed=blue, floor=yellow; closer=stronger tone/opacity\nbed range=[{bed_min:.3}, {bed_max:.3}] floor range=[{floor_min:.3}, {floor_max:.3}]"
    );
}

/// Rellena las zonas `bed`/`floor` con la escala de color por capa según la
/// mediana observada en este frame.
pub fn draw_zone_fills(
    calibration: &SurfaceCalibration,
    depth: &DepthFrame,
    roi: [u32; 4],
    frame_width: u32,
    frame_height: u32,
    alpha: f32,
) -> RgbaImage {
    let mut overlay = RgbaImage::new(frame_width, frame_height);
    for layer in [SurfaceLayer::Bed, SurfaceLayer::Floor] {
        let (layer_min, layer_max) = layer_bounds(calibration, layer);
        for zone in calibration.zones(layer) {
            let points = polygon_points(&zone.polygon);
            let stats = polygon_stats(
                depth,
                roi,
                &[zone.polygon.as_slice()],
                frame_width,
                frame_height,
                None,
            );
            let observed = stats.as_ref().and_then(|stats| stats.median_depth_m);
            let (color, zone_alpha) = layer_scale(
                layer,
                observed.unwrap_or(zone.median_depth),
                layer_min,
                layer_max,
                alpha,
            );
            draw_polygon_mut(
                &mut overlay,
                &points,
                Rgba([color[0], color[1], color[2], zone_alpha]),
            );
        }
    }
    overlay
}

/// Dibuja los bordes blancos de las zonas sobre la imagen final.
pub fn draw_zone_borders(rgb: &mut image::RgbImage, calibration: &SurfaceCalibration) {
    for layer in [SurfaceLayer::Bed, SurfaceLayer::Floor] {
        for zone in calibration.zones(layer) {
            draw_hollow_polygon_mut(
                rgb,
                &polygon_points_f32(&zone.polygon),
                Rgb([255, 255, 255]),
            );
        }
    }
}

/// Profundidad (m) en un punto del frame, o `None` si el punto cae fuera del
/// mapa depth o el valor es inválido. Usado solo por `deep-calib-pose`.
#[allow(dead_code)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn depth_at_point(depth: &DepthFrame, roi: [u32; 4], x: f32, y: f32) -> Option<f32> {
    let (map_width, map_height) = depth.dims();
    let roi_width = roi[2] - roi[0];
    let roi_height = roi[3] - roi[1];
    if map_width == 0 || map_height == 0 || roi_width == 0 || roi_height == 0 {
        return None;
    }
    let local_x = (x - roi[0] as f32) * map_width as f32 / roi_width as f32;
    let local_y = (y - roi[1] as f32) * map_height as f32 / roi_height as f32;
    if local_x < 0.0 || local_y < 0.0 {
        return None;
    }
    let lx = local_x.floor() as usize;
    let ly = local_y.floor() as usize;
    if lx >= map_width as usize || ly >= map_height as usize {
        return None;
    }
    let value = depth.value_at(ly, lx);
    (value.is_finite() && value > 0.0).then_some(value)
}

/// Zona cuya mediana de referencia está más cerca de `depth_m`. `median_of`
/// permite clasificar contra medianas observadas en el mismo run en vez de
/// contra las calibradas (que arrastran la deriva de escala entre corridas).
#[allow(dead_code)]
pub fn nearest_zone_by<F>(
    calibration: &SurfaceCalibration,
    depth_m: f32,
    median_of: F,
) -> Option<(SurfaceLayer, &SurfaceZone)>
where
    F: Fn(SurfaceLayer, &SurfaceZone) -> f32,
{
    let mut best: Option<(SurfaceLayer, &SurfaceZone, f32)> = None;
    for layer in [SurfaceLayer::Bed, SurfaceLayer::Floor] {
        for zone in calibration.zones(layer) {
            let distance = (median_of(layer, zone) - depth_m).abs();
            if best.is_none_or(|(_, _, best_distance)| distance < best_distance) {
                best = Some((layer, zone, distance));
            }
        }
    }
    best.map(|(layer, zone, _)| (layer, zone))
}

/// Medianas observadas (misma corrida depth) por zona, indexadas por
/// `(layer.as_str(), zone.name)`.
#[allow(dead_code)]
pub fn observed_zone_medians(
    depth: &DepthFrame,
    roi: [u32; 4],
    calibration: &SurfaceCalibration,
    frame_width: u32,
    frame_height: u32,
) -> HashMap<(String, String), f32> {
    let mut medians = HashMap::new();
    for layer in [SurfaceLayer::Bed, SurfaceLayer::Floor] {
        for zone in calibration.zones(layer) {
            let stats = polygon_stats(
                depth,
                roi,
                &[zone.polygon.as_slice()],
                frame_width,
                frame_height,
                None,
            );
            if let Some(median) = stats.as_ref().and_then(|stats| stats.median_depth_m) {
                medians.insert((layer.as_str().to_string(), zone.name.clone()), median);
            }
        }
    }
    medians
}

/// Referencia de clasificación: mediana observada de la zona, o la calibrada
/// si la zona no tiene mediana observada en este run.
#[allow(dead_code)]
pub fn reference_median(
    observed: &HashMap<(String, String), f32>,
) -> impl Fn(SurfaceLayer, &SurfaceZone) -> f32 + '_ {
    move |layer, zone| {
        observed
            .get(&(layer.as_str().to_string(), zone.name.clone()))
            .copied()
            .unwrap_or(zone.median_depth)
    }
}

/// Bounding box (frame coords) que encierra todas las zonas calibradas.
#[allow(dead_code)]
pub fn zones_bbox(calibration: &SurfaceCalibration) -> Option<[f32; 4]> {
    let mut bbox: Option<[f32; 4]> = None;
    for layer in [SurfaceLayer::Bed, SurfaceLayer::Floor] {
        for zone in calibration.zones(layer) {
            for [x, y] in &zone.polygon {
                let current = bbox.get_or_insert([*x, *y, *x, *y]);
                current[0] = current[0].min(*x);
                current[1] = current[1].min(*y);
                current[2] = current[2].max(*x);
                current[3] = current[3].max(*y);
            }
        }
    }
    bbox
}

/// Margen de expansión del recorte de zonas.
pub const ZONES_CROP_MARGIN: f32 = 0.10;

/// Recorte del ROI de zonas calibradas con margen, o `None` si no hay zonas.
#[allow(dead_code)]
#[allow(clippy::cast_possible_truncation)]
pub fn zones_crop(
    calibration: &SurfaceCalibration,
    frame_width: u32,
    frame_height: u32,
) -> Option<[u32; 4]> {
    let [x1, y1, x2, y2] = zones_bbox(calibration)?;
    let expand_w = (x2 - x1) * ZONES_CROP_MARGIN;
    let expand_h = (y2 - y1) * ZONES_CROP_MARGIN;
    let x1 = (x1 - expand_w).max(0.0) as u32;
    let y1 = (y1 - expand_h).max(0.0) as u32;
    let x2 = ((x2 + expand_w) as u32).min(frame_width);
    let y2 = ((y2 + expand_h) as u32).min(frame_height);
    (x2 > x1 && y2 > y1).then_some([x1, y1, x2, y2])
}

/// Calculates the effective scene-depth ROI from the configured base ROI and
/// every calibrated region. The margin is a fraction of the union bbox size,
/// added on each side; the base ROI is preserved even when it is larger.
#[allow(clippy::cast_possible_truncation)]
pub fn derive_depth_roi(
    calibration: &SurfaceCalibration,
    configured_roi: Option<[u32; 4]>,
    margin: f32,
) -> Result<[u32; 4], Box<dyn Error>> {
    let base = configured_roi.unwrap_or(calibration.roi);
    let Some(regions_bbox) = zones_bbox(calibration) else {
        validate_roi(base, calibration.frame_width, calibration.frame_height)?;
        return Ok(base);
    };
    derive_depth_roi_from_bbox(
        base,
        regions_bbox,
        margin,
        calibration.frame_width,
        calibration.frame_height,
    )
}

/// Variant used while a calibration session is being assembled, before all
/// zones have been persisted into a `SurfaceCalibration`.
#[allow(clippy::cast_possible_truncation)]
pub fn derive_depth_roi_from_bbox(
    base: [u32; 4],
    regions_bbox: [f32; 4],
    margin: f32,
    frame_width: u32,
    frame_height: u32,
) -> Result<[u32; 4], Box<dyn Error>> {
    if !margin.is_finite() || margin < 0.0 {
        return Err(format!("depth ROI margin must be finite and non-negative: {margin}").into());
    }
    validate_roi(base, frame_width, frame_height)?;
    let expanded = expand_bbox(
        regions_bbox,
        margin,
        frame_width,
        frame_height,
    )
    .ok_or("calibrated regions have an invalid bbox")?;
    Ok(union_roi(base, expanded))
}

/// Requires the persisted calibration ROI to equal the ROI derived from the
/// depth catalog and all calibrated regions. A mismatch means the session must
/// be recalibrated instead of silently sampling clipped polygons.
pub fn require_derived_depth_roi(
    calibration: &SurfaceCalibration,
    configured_roi: Option<[u32; 4]>,
    margin: f32,
) -> Result<[u32; 4], Box<dyn Error>> {
    let derived = derive_depth_roi(calibration, configured_roi, margin)?;
    if derived != calibration.roi {
        return Err(format!(
            "calibration ROI {:?} does not cover configured depth ROI/regions; derived ROI is {:?}; recalibrate the session",
            calibration.roi, derived
        )
        .into());
    }
    Ok(derived)
}

fn validate_roi(roi: [u32; 4], frame_width: u32, frame_height: u32) -> Result<(), Box<dyn Error>> {
    if roi[0] >= roi[2]
        || roi[1] >= roi[3]
        || roi[2] > frame_width
        || roi[3] > frame_height
    {
        return Err(format!("invalid depth ROI {roi:?} for frame {frame_width}x{frame_height}").into());
    }
    Ok(())
}

fn union_roi(left: [u32; 4], right: [u32; 4]) -> [u32; 4] {
    [
        left[0].min(right[0]),
        left[1].min(right[1]),
        left[2].max(right[2]),
        left[3].max(right[3]),
    ]
}

/// Mejor bbox del modelo (mayor área) filtrando clase y confianza mínima, en
/// coordenadas de la imagen dada. `class_id` filtra por índice de clase (p.ej.
/// 0 = person en COCO para detect-fast).
#[allow(dead_code)]
pub fn detect_best_bbox(
    model: &mut YOLOModel,
    image: &DynamicImage,
    image_path: &Path,
    class_id: Option<u32>,
    min_conf: f32,
) -> Result<Option<[f32; 4]>, Box<dyn Error>> {
    let results = model.predict_image(image, image_path.to_string_lossy().into_owned())?;
    let Some(boxes) = results.first().and_then(|result| result.boxes.as_ref()) else {
        return Ok(None);
    };
    let mut best: Option<([f32; 4], f32)> = None;
    for index in 0..boxes.len() {
        if boxes.conf()[[index]] < min_conf {
            continue;
        }
        if let Some(class_id) = class_id {
            if boxes.cls()[[index]] != class_id as f32 {
                continue;
            }
        }
        let bbox = [
            boxes.data[[index, 0]],
            boxes.data[[index, 1]],
            boxes.data[[index, 2]],
            boxes.data[[index, 3]],
        ];
        let area = (bbox[2] - bbox[0]) * (bbox[3] - bbox[1]);
        if best.is_none_or(|(_, best_area)| area > best_area) {
            best = Some((bbox, area));
        }
    }
    if best.is_none() && boxes.len() > 0 {
        eprintln!(
            "detect_best_bbox: no box passed (class {class_id:?}, min_conf {min_conf}); raw:"
        );
        for index in 0..boxes.len().min(10) {
            eprintln!(
                "  box[{index}] xyxy={:?} conf={:.3} cls={}",
                [
                    boxes.data[[index, 0]],
                    boxes.data[[index, 1]],
                    boxes.data[[index, 2]],
                    boxes.data[[index, 3]],
                ],
                boxes.conf()[[index]],
                boxes.cls()[[index]]
            );
        }
    }
    Ok(best.map(|(bbox, _)| bbox))
}

/// Expande `bbox` por `margin` (fracción de su tamaño) y la recorta al frame.
/// Espejo de `compute_bbox_roi` del runtime (`src/infer/crop.rs`).
#[allow(dead_code)]
#[allow(clippy::cast_possible_truncation)]
pub fn expand_bbox(
    bbox: [f32; 4],
    margin: f32,
    frame_width: u32,
    frame_height: u32,
) -> Option<[u32; 4]> {
    let width = (bbox[2] - bbox[0]).max(0.0);
    let height = (bbox[3] - bbox[1]).max(0.0);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let x1 = (bbox[0] - width * margin).max(0.0) as u32;
    let y1 = (bbox[1] - height * margin).max(0.0) as u32;
    let x2 = ((bbox[2] + width * margin) as u32).min(frame_width);
    let y2 = ((bbox[3] + height * margin) as u32).min(frame_height);
    (x2 > x1 && y2 > y1).then_some([x1, y1, x2, y2])
}

pub fn format_option(value: Option<f32>) -> String {
    value.map_or_else(|| "none".to_string(), |value| format!("{value:.3}"))
}

#[allow(clippy::cast_possible_truncation)]
pub fn polygon_points(polygon: &[[f32; 2]]) -> Vec<Point<i32>> {
    polygon
        .iter()
        .map(|[x, y]| Point::new(x.round() as i32, y.round() as i32))
        .collect()
}

pub fn polygon_points_f32(polygon: &[[f32; 2]]) -> Vec<Point<f32>> {
    polygon.iter().map(|[x, y]| Point::new(*x, *y)).collect()
}
