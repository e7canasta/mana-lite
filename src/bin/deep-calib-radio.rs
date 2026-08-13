//! Muestreo de profundidad por keypoints con radio proporcional al bbox de la
//! persona, con la máscara de segmentación (raster) como base.
//!
//! Sobre una sesión calibrada corre el depth de escena (modelo de la sesión),
//! `pose-standard`, `face-yolo` y `seg-standard`. El fondo es el mapa de
//! profundidad coloreado con las paletas de ultralytics-inference
//! (`--colormap inferno|jet|spectral|gray`) y la normalización
//! (`--depth-viz metric|disparity`); cada keypoint de pose se muestra como un
//! círculo de radio proporcional al bbox de la persona (misma fórmula que el
//! runtime: `ratio * min(ancho, alto)`) pintado con el mismo colormap según su
//! profundidad. La máscara de seg se dibuja como contorno del raster binario
//! (no el polígono simplificado) y se reporta con sus componentes.

use std::error::Error;
use std::path::PathBuf;

use image::Rgba;
use imageproc::drawing::{
    draw_filled_circle_mut, draw_filled_rect_mut, draw_hollow_circle_mut, draw_hollow_rect_mut,
};
use serde::Serialize;
use ultralytics_inference::visualizer::color::{Colormap, DepthViz};

#[path = "common/mod.rs"]
mod common;

use common::*;
use mana_lite::config::{load_app_config, load_model_catalog};
use mana_lite::infer::{collect_detections, translate_detections_to_frame};

/// Índices COCO 17 en orden.
const COCO_17: [&str; 17] = [
    "nose",
    "left_eye",
    "right_eye",
    "left_ear",
    "right_ear",
    "left_shoulder",
    "right_shoulder",
    "left_elbow",
    "right_elbow",
    "left_wrist",
    "right_wrist",
    "left_hip",
    "right_hip",
    "left_knee",
    "right_knee",
    "left_ankle",
    "right_ankle",
];

const KEYPOINT_MIN_CONF: f32 = 0.25;
const CIRCLE_SIDES: usize = 12;

#[derive(Serialize)]
struct RunReport {
    model_key: String,
    image: String,
    depth_roi: [u32; 4],
    depth_roi_margin: f32,
    colormap: String,
    depth_viz: String,
    radius_ratio: f32,
    radius_base: String,
    person_bbox: [f32; 4],
    segment: Option<SegmentReport>,
    zones: Vec<ZoneReport>,
    keypoints: Vec<KeypointReport>,
    face: Option<FaceReport>,
}

#[derive(Serialize)]
struct SegmentReport {
    bbox: [f32; 4],
    area_px: f64,
    area_ratio: f64,
    perimeter_px: f64,
    vertices: Vec<[f32; 2]>,
    raster: RasterReport,
}

#[derive(Serialize)]
struct RasterReport {
    dims: [u32; 2],
    active_pixels: u64,
    components: usize,
    largest_component_ratio: f64,
}

#[derive(Serialize)]
struct ZoneReport {
    zone: String,
    calibrated_m: f32,
    observed_m: Option<f32>,
    delta_m: Option<f32>,
    valid_ratio: Option<f32>,
}

#[derive(Serialize)]
struct KeypointReport {
    person: usize,
    part: String,
    point: [f32; 2],
    conf: f32,
    radius_px: f32,
    depth_m: Option<f32>,
    p10_m: Option<f32>,
    p90_m: Option<f32>,
    valid_ratio: Option<f32>,
    sampled_pixels: u64,
    zone: Option<String>,
    delta_m: Option<f32>,
}

#[derive(Serialize)]
struct FaceReport {
    conf: f32,
    bbox: [f32; 4],
    depth_m: Option<f32>,
    p10_m: Option<f32>,
    p90_m: Option<f32>,
    valid_ratio: Option<f32>,
    sampled_pixels: u64,
    zone: Option<String>,
}

/// Raster binario de la máscara de seg (resolución del modelo).
struct RasterMask {
    width: u32,
    height: u32,
    bits: Vec<u8>,
}

#[derive(Debug)]
struct Options {
    config: PathBuf,
    session: PathBuf,
    image: PathBuf,
    output: PathBuf,
    json: Option<PathBuf>,
    alpha: f32,
    imgsz: Option<u32>,
    radius: f32,
    radius_base: String,
    seg: bool,
    zones: bool,
    colormap: Colormap,
    depth_viz: DepthViz,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_args(std::env::args().skip(1).collect())?;

    let calibration = load_calibration(&options.session)?;
    let frame = open_frame(&options.image, &calibration)?;

    let (depth_path, catalog_roi, roi_margin) =
        catalog_depth_context(&options.config, &calibration.model_key)?;
    let depth_path = resolve_model_path(depth_path, &options.config)?;
    let depth_roi = require_derived_depth_roi(&calibration, catalog_roi, roi_margin)?;
    println!(
        "depth roi: configured={catalog_roi:?} margin={roi_margin:.3} effective={depth_roi:?}"
    );
    let mut depth_model = load_model(
        &depth_path,
        Some(depth_roi),
        options.imgsz,
    )?;
    let (run, raw_depth) = run_depth_raw(
        &mut depth_model,
        &frame.image,
        &options.image,
        depth_roi,
    )?;

    let app_config = load_app_config(&options.config)?;
    let catalog = load_model_catalog(&app_config.inference.model_catalog)?;
    let segment_entry = catalog.models.get("seg-standard").ok_or(
        "model 'seg-standard' is absent from the catalog (needed for masks)",
    )?;
    let polygon_simplify = segment_entry.polygon_simplify;
    let (min_component_area_ratio, mask_threshold) = (
        segment_entry.postprocess.min_component_area_ratio,
        segment_entry.postprocess.mask_threshold,
    );

    let stats = zone_stats(
        &run.depth,
        run.roi,
        &calibration,
        frame.frame_width,
        frame.frame_height,
    );
    print_zone_table(&stats);
    let zones = stats
        .iter()
        .map(|row| ZoneReport {
            zone: row.label.clone(),
            calibrated_m: row.calibrated_m,
            observed_m: row.observed_m,
            delta_m: row.delta_m,
            valid_ratio: row.valid_ratio,
        })
        .collect();
    let observed = observed_zone_medians(
        &run.depth,
        run.roi,
        &calibration,
        frame.frame_width,
        frame.frame_height,
    );
    let reference = reference_median(&observed);

    let (lo, inv, disparity) = viz_bounds(&raw_depth, options.depth_viz);
    println!(
        "depth viz: {} ({}), bounds lo={:.3} inv={:.4} disparity={disparity}",
        colormap_name(options.colormap),
        depth_viz_name(options.depth_viz),
        lo,
        inv
    );

    // Fondo: mapa de profundidad coloreado (o relleno por zona con --zones).
    let mut overlay = if options.zones {
        draw_zone_fills(
            &calibration,
            &run.depth,
            run.roi,
            frame.frame_width,
            frame.frame_height,
            options.alpha,
        )
    } else {
        depth_colormap_overlay(
            &raw_depth,
            run.roi,
            frame.frame_width,
            frame.frame_height,
            options.colormap,
            options.alpha,
            (lo, inv, disparity),
        )?
    };

    let crop = zones_crop(&calibration, frame.frame_width, frame.frame_height);
    let (input, offset) = pose_input(&frame.image, crop);

    let pose_dets = run_model_detections(
        &options.config,
        "pose-standard",
        &input,
        offset,
        &options.image,
        frame.frame_width,
        frame.frame_height,
        polygon_simplify,
        min_component_area_ratio,
        mask_threshold,
    )?;
    let face_dets = run_model_detections(
        &options.config,
        "face-yolo",
        &input,
        offset,
        &options.image,
        frame.frame_width,
        frame.frame_height,
        polygon_simplify,
        min_component_area_ratio,
        mask_threshold,
    )?;
    let (seg_dets, seg_raster) = if options.seg {
        run_seg(
            &options.config,
            &input,
            offset,
            &options.image,
            frame.frame_width,
            frame.frame_height,
            polygon_simplify,
            min_component_area_ratio,
            mask_threshold,
        )?
    } else {
        (Vec::new(), None)
    };

    let best_pose = pose_dets
        .iter()
        .filter(|detection| detection.keypoints.is_some())
        .max_by(|left, right| left.confidence.total_cmp(&right.confidence));
    let Some(person) = best_pose else {
        println!("pose: no person with keypoints");
        return Ok(());
    };
    let person_bbox = person.bbox;
    println!(
        "person bbox: ({:.0}, {:.0}) -> ({:.0}, {:.0}) conf={:.3}",
        person_bbox[0], person_bbox[1], person_bbox[2], person_bbox[3], person.confidence
    );

    let mut clip_polygons: Option<Vec<Vec<[f32; 2]>>> = None;
    let segment_report = if let Some(raster) = &seg_raster {
        draw_raster_contour(&mut overlay, raster, crop.unwrap_or([0, 0, frame.frame_width, frame.frame_height]), frame.frame_width);
        clip_polygons = Some(
            seg_dets
                .iter()
                .filter_map(|detection| detection.mask.as_ref())
                .flat_map(|mask| mask.polygons.iter().cloned())
                .collect(),
        );
        segment_report(&seg_dets, raster, frame.frame_width, frame.frame_height)
    } else {
        None
    };

    let radius_px = part_radius(person_bbox, options.radius, &options.radius_base);
    println!(
        "keypoint radius: {:.1} px (ratio {:.3}, base {})",
        radius_px, options.radius, options.radius_base
    );

    let mut keypoint_reports = Vec::new();
    println!("person\tpart\tconf\tradius_px\tdepth_m\tzone\tdelta");
    if let Some(keypoints) = &person.keypoints {
        for (index, [x, y, conf]) in keypoints.iter().enumerate() {
            if *conf < KEYPOINT_MIN_CONF || !x.is_finite() || !y.is_finite() {
                continue;
            }
            let name = COCO_17.get(index).copied().unwrap_or("unknown");
            let center = [*x, *y];
            let footprint = circle_polygon(center, radius_px);
            let footprint_refs = vec![footprint.as_slice()];
            let stats = polygon_stats_clipped(
                &run.depth,
                run.roi,
                &footprint_refs,
                frame.frame_width,
                frame.frame_height,
                clip_polygons.as_deref(),
            );
            let median = stats.as_ref().and_then(|stats| stats.median_depth_m);
            let p10 = stats.as_ref().and_then(|stats| stats.p10_depth_m);
            let p90 = stats.as_ref().and_then(|stats| stats.p90_depth_m);
            let valid_ratio = stats.as_ref().and_then(|stats| stats.valid_ratio);
            let sampled = stats.as_ref().map_or(0, |stats| stats.sampled_pixels);
            let zone = median.and_then(|median| nearest_zone_by(&calibration, median, &reference));
            let fill = median.map_or([128, 128, 128], |median| {
                options
                    .colormap
                    .sample(depth_t(median, lo, inv, disparity))
            });
            match zone {
                Some((layer, zone)) => {
                    let delta = median.map_or(0.0, |median| (median - reference(layer, zone)).abs());
                    println!(
                        "{}\t{name}\t{conf:.2}\t{radius_px:.1}\t{}\t{}/{}\t{delta:.3}",
                        "0",
                        format_option(median),
                        layer.as_str(),
                        zone.name
                    );
                    keypoint_reports.push(KeypointReport {
                        person: 0,
                        part: name.to_string(),
                        point: center,
                        conf: *conf,
                        radius_px,
                        depth_m: median,
                        p10_m: p10,
                        p90_m: p90,
                        valid_ratio,
                        sampled_pixels: sampled,
                        zone: Some(format!("{}/{}", layer.as_str(), zone.name)),
                        delta_m: median.map(|median| (median - reference(layer, zone)).abs()),
                    });
                }
                None => {
                    println!(
                        "{}\t{name}\t{conf:.2}\t{radius_px:.1}\t{}\tunclassified\t-",
                        "0",
                        format_option(median)
                    );
                    keypoint_reports.push(KeypointReport {
                        person: 0,
                        part: name.to_string(),
                        point: center,
                        conf: *conf,
                        radius_px,
                        depth_m: median,
                        p10_m: p10,
                        p90_m: p90,
                        valid_ratio,
                        sampled_pixels: sampled,
                        zone: None,
                        delta_m: None,
                    });
                }
            };
            let center = (x.round() as i32, y.round() as i32);
            let radius = radius_px.round().max(1.0) as i32;
            draw_filled_circle_mut(
                &mut overlay,
                center,
                radius,
                Rgba([fill[0], fill[1], fill[2], 230]),
            );
            draw_hollow_circle_mut(&mut overlay, center, radius, Rgba([255, 255, 255, 255]));
        }
    }

    let face_report = run_face_and_draw(
        &run.depth,
        run.roi,
        &calibration,
        &reference,
        &face_dets,
        clip_polygons.as_deref(),
        frame.frame_width,
        frame.frame_height,
        options.colormap,
        (lo, inv, disparity),
        &mut overlay,
    )?;

    let mut rgba = frame.image.to_rgba8();
    image::imageops::overlay(&mut rgba, &overlay, 0, 0);
    let mut rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    draw_zone_borders(&mut rgb, &calibration);
    draw_color_bar(&mut rgb, 16, 16, 180, options.colormap, lo, inv, disparity);
    rgb.save(&options.output)?;

    if let Some(json_path) = &options.json {
        let report = RunReport {
            model_key: calibration.model_key.clone(),
            image: options
                .image
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?")
                .to_string(),
            depth_roi,
            depth_roi_margin: roi_margin,
            colormap: colormap_name(options.colormap).to_string(),
            depth_viz: depth_viz_name(options.depth_viz).to_string(),
            radius_ratio: options.radius,
            radius_base: options.radius_base.clone(),
            person_bbox,
            segment: segment_report,
            zones,
            keypoints: keypoint_reports,
            face: face_report,
        };
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(json_path, json)?;
        println!("json={}", json_path.display());
    }

    print_legend(&calibration);
    println!("output={}", options.output.display());
    Ok(())
}

fn colormap_name(colormap: Colormap) -> &'static str {
    match colormap {
        Colormap::Inferno => "inferno",
        Colormap::Jet => "jet",
        Colormap::Spectral => "spectral",
        Colormap::Gray => "gray",
    }
}

fn depth_viz_name(viz: DepthViz) -> &'static str {
    match viz {
        DepthViz::Metric => "metric",
        DepthViz::Disparity => "disparity",
    }
}

/// Bounds de normalización, espejo de `DepthMap::colorize` de
/// ultralytics-inference: `Metric` = min/max de píxeles válidos;
/// `Disparity` = percentiles 2-98 de `1/d`.
fn viz_bounds(depth: &ultralytics_inference::DepthMap, viz: DepthViz) -> (f32, f32, bool) {
    match viz {
        DepthViz::Metric => {
            let vmin = depth.min_depth().unwrap_or(0.0);
            let vmax = depth.max_depth().unwrap_or(1.0);
            if vmax <= vmin {
                (vmin, 1.0, false)
            } else {
                (vmin, 1.0 / (vmax - vmin), false)
            }
        }
        DepthViz::Disparity => {
            let mut disp: Vec<f32> = depth
                .data
                .iter()
                .filter(|&&d| d > 0.0)
                .map(|&d| 1.0 / d)
                .collect();
            if disp.is_empty() {
                return (0.0, 1.0, true);
            }
            let (lo, hi) = percentile_2_98(&mut disp);
            (lo, 1.0 / (hi - lo).max(1e-6), true)
        }
    }
}

fn depth_t(depth_m: f32, lo: f32, inv: f32, disparity: bool) -> f32 {
    if depth_m <= 0.0 {
        return 0.0;
    }
    let value = if disparity { 1.0 / depth_m } else { depth_m };
    ((value - lo) * inv).clamp(0.0, 1.0)
}

fn percentile_2_98(vals: &mut [f32]) -> (f32, f32) {
    let n = vals.len();
    let idx = |p: f32| ((p * (n - 1) as f32).round() as usize).min(n - 1);
    let (lo_i, hi_i) = (idx(0.02), idx(0.98));
    vals.select_nth_unstable_by(lo_i, f32::total_cmp);
    let lo = vals[lo_i];
    vals.select_nth_unstable_by(hi_i, f32::total_cmp);
    (lo, vals[hi_i])
}

/// Fondo coloreado por profundidad: mismo colormap y mismos bounds que los
/// puntos (coherente barra/fondo/marcadores), mapa del modelo escalado al ROI
/// y mezclado con alpha.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn depth_colormap_overlay(
    depth: &ultralytics_inference::DepthMap,
    roi: [u32; 4],
    frame_width: u32,
    frame_height: u32,
    colormap: Colormap,
    alpha: f32,
    bounds: (f32, f32, bool),
) -> Result<image::RgbaImage, Box<dyn Error>> {
    let (height, width) = depth.data.dim();
    let mut pixels = Vec::with_capacity(width * height * 3);
    for &value in depth.data.iter() {
        if value > 0.0 {
            pixels.extend_from_slice(&colormap.sample(depth_t(value, bounds.0, bounds.1, bounds.2)));
        } else {
            pixels.extend_from_slice(&[0, 0, 0]);
        }
    }
    let map = image::RgbImage::from_raw(width as u32, height as u32, pixels)
        .ok_or("colorized depth has invalid dims")?;
    let roi_w = (roi[2] - roi[0]).max(1);
    let roi_h = (roi[3] - roi[1]).max(1);
    let scaled = image::imageops::resize(&map, roi_w, roi_h, image::imageops::FilterType::Triangle);
    let mut canvas = image::RgbaImage::new(frame_width, frame_height);
    for (x, y, pixel) in scaled.enumerate_pixels() {
        let blend = (alpha * 255.0).round() as u8;
        canvas.put_pixel(roi[0] + x, roi[1] + y, Rgba([pixel[0], pixel[1], pixel[2], blend]));
    }
    Ok(canvas)
}

/// Barra vertical del colormap con el mapeo de metros.
#[allow(clippy::cast_possible_truncation)]
fn draw_color_bar(
    rgb: &mut image::RgbImage,
    x: u32,
    y: u32,
    height: u32,
    colormap: Colormap,
    lo: f32,
    inv: f32,
    disparity: bool,
) {
    const WIDTH: u32 = 14;
    for row in 0..height {
        let t = row as f32 / (height - 1).max(1) as f32;
        let color = colormap.sample(t);
        for col in 0..WIDTH {
            if let Some(pixel) = rgb.get_pixel_mut_checked(x + col, y + row) {
                *pixel = image::Rgb(color);
            }
        }
    }
    let _ = (lo, inv, disparity);
}

/// Radio del muestreo, proporcional al bbox de la persona. `min` replica la
/// fórmula del runtime (`segment_radius_ratio * min(ancho, alto)`); las otras
/// bases permiten explorar la sensibilidad.
fn part_radius(bbox: [f32; 4], ratio: f32, base: &str) -> f32 {
    let width = (bbox[2] - bbox[0]).abs();
    let height = (bbox[3] - bbox[1]).abs();
    let extent = match base {
        "width" => width,
        "height" => height,
        "max" => width.max(height),
        "diag" => (width * width + height * height).sqrt(),
        _ => width.min(height),
    };
    (extent * ratio).max(1.0)
}

fn circle_polygon(center: [f32; 2], radius: f32) -> Vec<[f32; 2]> {
    (0..CIRCLE_SIDES)
        .map(|index| {
            let angle = std::f32::consts::TAU * index as f32 / CIRCLE_SIDES as f32;
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]
        })
        .collect()
}

/// polygon_stats con clip opcional (máscara de seg normalizada a frame).
#[allow(clippy::too_many_arguments)]
fn polygon_stats_clipped(
    depth: &mana_lite::depth_map::DepthFrame,
    roi: [u32; 4],
    footprints: &[&[[f32; 2]]],
    frame_width: u32,
    frame_height: u32,
    clip_polygons: Option<&[Vec<[f32; 2]>]>,
) -> Option<mana_lite::PolygonStats> {
    mana_lite::polygon_stats(
        depth,
        roi,
        footprints,
        frame_width,
        frame_height,
        clip_polygons,
    )
}

/// Reporte del polígono de la máscara (área, perímetro, bbox, vértices — en
/// coordenadas de frame) y del raster binario (dims, píxeles activos,
/// componentes 4-conectados).
#[allow(clippy::cast_precision_loss)]
fn segment_report(
    seg_dets: &[mana_lite::detection::Detection],
    raster: &RasterMask,
    frame_width: u32,
    frame_height: u32,
) -> Option<SegmentReport> {
    let polygons = seg_dets
        .iter()
        .filter_map(|detection| detection.mask.as_ref())
        .flat_map(|mask| mask.polygons.iter().cloned())
        .map(|poly| {
            poly.iter()
                .map(|[x, y]| [x * frame_width as f32, y * frame_height as f32])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let total_area = polygons
        .iter()
        .map(|poly| shoelace_area(poly))
        .sum::<f64>();
    let perimeter = polygons.iter().map(|poly| poly_perimeter(poly)).sum::<f64>();
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for poly in &polygons {
        for [x, y] in poly {
            min_x = min_x.min(*x);
            min_y = min_y.min(*y);
            max_x = max_x.max(*x);
            max_y = max_y.max(*y);
        }
    }
    if !min_x.is_finite() {
        return None;
    }
    let active = raster
        .bits
        .iter()
        .filter(|&&bit| bit != 0)
        .count() as u64;
    let components = count_components(raster);
    let largest_ratio = largest_component_ratio(raster);
    println!(
        "seg raster: {}x{} active={} components={} (mayor {:.1}%) | polygon bbox ({:.0}, {:.0}) -> ({:.0}, {:.0}) area={:.0} px ({:.4} del frame) perimeter={:.0} px vertices={}",
        raster.width,
        raster.height,
        active,
        components,
        largest_ratio * 100.0,
        min_x,
        min_y,
        max_x,
        max_y,
        total_area,
        total_area / (frame_width * frame_height) as f64,
        perimeter,
        polygons.iter().map(Vec::len).sum::<usize>()
    );
    Some(SegmentReport {
        bbox: [min_x, min_y, max_x, max_y],
        area_px: total_area,
        area_ratio: total_area / (frame_width * frame_height) as f64,
        perimeter_px: perimeter,
        vertices: polygons.into_iter().flat_map(|poly| poly.into_iter()).collect(),
        raster: RasterReport {
            dims: [raster.width, raster.height],
            active_pixels: active,
            components,
            largest_component_ratio: largest_ratio,
        },
    })
}

/// Contorno grueso del raster de la máscara (píxeles de borde), escalado del
/// recorte de entrada al frame. El polígono simplificado no se dibuja: el
/// raster es la forma fiel de la máscara.
fn draw_raster_contour(
    overlay: &mut image::RgbaImage,
    raster: &RasterMask,
    crop: [u32; 4],
    frame_width: u32,
) {
    let (x1, y1) = (crop[0] as f32, crop[1] as f32);
    let crop_w = (crop[2] - crop[0]).max(1) as f32;
    let crop_h = (crop[3] - crop[1]).max(1) as f32;
    let cell_w = crop_w / raster.width as f32;
    let cell_h = crop_h / raster.height as f32;
    let cell = cell_w.max(cell_h).min(4.0);
    for row in 0..raster.height {
        for col in 0..raster.width {
            let index = (row * raster.width + col) as usize;
            if raster.bits[index] == 0 {
                continue;
            }
            let up = row == 0 || raster.bits[index - raster.width as usize] == 0;
            let down =
                row + 1 >= raster.height || raster.bits[index + raster.width as usize] == 0;
            let left = col == 0 || raster.bits[index - 1] == 0;
            let right = col + 1 >= raster.width || raster.bits[index + 1] == 0;
            if !(up || down || left || right) {
                continue;
            }
            let px = x1 + col as f32 * cell_w;
            let py = y1 + row as f32 * cell_h;
            let rect = imageproc::rect::Rect::at(px.round() as i32, py.round() as i32)
                .of_size(cell.ceil().max(2.0) as u32, cell.ceil().max(2.0) as u32);
            draw_filled_rect_mut(overlay, rect, Rgba([255, 255, 255, 255]));
            let _ = frame_width;
        }
    }
}

/// Componentes 4-conectados del raster.
fn count_components(raster: &RasterMask) -> usize {
    let mut visited = vec![false; raster.bits.len()];
    let mut count = 0;
    for start in 0..raster.bits.len() {
        if raster.bits[start] == 0 || visited[start] {
            continue;
        }
        count += 1;
        let mut stack = vec![start];
        visited[start] = true;
        while let Some(index) = stack.pop() {
            let row = index / raster.width as usize;
            let col = index % raster.width as usize;
            for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (nr, nc) = (row as i32 + dr, col as i32 + dc);
                if nr < 0 || nc < 0 || nr >= raster.height as i32 || nc >= raster.width as i32 {
                    continue;
                }
                let next = nr as usize * raster.width as usize + nc as usize;
                if !visited[next] && raster.bits[next] != 0 {
                    visited[next] = true;
                    stack.push(next);
                }
            }
        }
    }
    count
}

fn largest_component_ratio(raster: &RasterMask) -> f64 {
    let mut visited = vec![false; raster.bits.len()];
    let mut largest = 0usize;
    for start in 0..raster.bits.len() {
        if raster.bits[start] == 0 || visited[start] {
            continue;
        }
        let mut size = 0;
        let mut stack = vec![start];
        visited[start] = true;
        while let Some(index) = stack.pop() {
            size += 1;
            let row = index / raster.width as usize;
            let col = index % raster.width as usize;
            for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (nr, nc) = (row as i32 + dr, col as i32 + dc);
                if nr < 0 || nc < 0 || nr >= raster.height as i32 || nc >= raster.width as i32 {
                    continue;
                }
                let next = nr as usize * raster.width as usize + nc as usize;
                if !visited[next] && raster.bits[next] != 0 {
                    visited[next] = true;
                    stack.push(next);
                }
            }
        }
        largest = largest.max(size);
    }
    let active = raster
        .bits
        .iter()
        .filter(|&&bit| bit != 0)
        .count() as f64;
    if active <= 0.0 {
        return 0.0;
    }
    largest as f64 / active
}

fn shoelace_area(polygon: &[[f32; 2]]) -> f64 {
    let mut area = 0.0;
    for index in 0..polygon.len() {
        let [x1, y1] = polygon[index];
        let [x2, y2] = polygon[(index + 1) % polygon.len()];
        area += (x1 as f64 * y2 as f64) - (x2 as f64 * y1 as f64);
    }
    area.abs() / 2.0
}

fn poly_perimeter(polygon: &[[f32; 2]]) -> f64 {
    let mut perimeter = 0.0;
    for index in 0..polygon.len() {
        let [x1, y1] = polygon[index];
        let [x2, y2] = polygon[(index + 1) % polygon.len()];
        perimeter += ((x2 as f64 - x1 as f64).powi(2) + (y2 as f64 - y1 as f64).powi(2)).sqrt();
    }
    perimeter
}

/// Muestrea el bbox de la cara y lo pinta con el colormap según su
/// profundidad.
#[allow(clippy::too_many_arguments)]
fn run_face_and_draw(
    depth: &mana_lite::depth_map::DepthFrame,
    roi: [u32; 4],
    calibration: &mana_lite::SurfaceCalibration,
    reference: &impl Fn(mana_lite::SurfaceLayer, &mana_lite::SurfaceZone) -> f32,
    face_dets: &[mana_lite::detection::Detection],
    clip_polygons: Option<&[Vec<[f32; 2]>]>,
    frame_width: u32,
    frame_height: u32,
    colormap: Colormap,
    bounds: (f32, f32, bool),
    overlay: &mut image::RgbaImage,
) -> Result<Option<FaceReport>, Box<dyn Error>> {
    let Some(best) = face_dets
        .iter()
        .max_by(|left, right| left.confidence.total_cmp(&right.confidence))
    else {
        println!("face: no detections");
        return Ok(None);
    };
    let [x1, y1, x2, y2] = best.bbox;
    let rect = imageproc::rect::Rect::at(x1.round() as i32, y1.round() as i32)
        .of_size((x2 - x1).round().max(1.0) as u32, (y2 - y1).round().max(1.0) as u32);
    let footprint = vec![vec![[x1, y1], [x2, y1], [x2, y2], [x1, y2]]];
    let footprint_refs = vec![footprint[0].as_slice()];
    let stats = polygon_stats_clipped(
        depth,
        roi,
        &footprint_refs,
        frame_width,
        frame_height,
        clip_polygons,
    );
    let median = stats.as_ref().and_then(|stats| stats.median_depth_m);
    let p10 = stats.as_ref().and_then(|stats| stats.p10_depth_m);
    let p90 = stats.as_ref().and_then(|stats| stats.p90_depth_m);
    let valid_ratio = stats.as_ref().and_then(|stats| stats.valid_ratio);
    let sampled = stats.as_ref().map_or(0, |stats| stats.sampled_pixels);
    let zone = median.and_then(|median| nearest_zone_by(calibration, median, reference));
    let report = Some(FaceReport {
        conf: best.confidence,
        bbox: best.bbox,
        depth_m: median,
        p10_m: p10,
        p90_m: p90,
        valid_ratio,
        sampled_pixels: sampled,
        zone: zone
            .as_ref()
            .map(|(layer, zone)| format!("{}/{}", layer.as_str(), zone.name)),
    });
    let fill = median.map_or([128, 128, 128], |median| {
        colormap.sample(depth_t(median, bounds.0, bounds.1, bounds.2))
    });
    match zone {
        Some((layer, zone)) => {
            let delta = median.map_or(0.0, |median| (median - reference(layer, zone)).abs());
            println!(
                "face\tbbox\t{:.2}\t{}\t{}/{}\t{delta:.3}",
                best.confidence,
                format_option(median),
                layer.as_str(),
                zone.name
            );
        }
        None => {
            println!(
                "face\tbbox\t{:.2}\t{}\tunclassified\t-",
                best.confidence,
                format_option(median)
            );
        }
    }
    draw_filled_rect_mut(
        overlay,
        rect,
        Rgba([fill[0], fill[1], fill[2], 200]),
    );
    draw_hollow_rect_mut(overlay, rect, Rgba([255, 255, 255, 255]));
    Ok(report)
}

/// Corre un modelo sobre la imagen recortada y traduce sus `Detection` a
/// coordenadas de frame con la misma semántica que el runtime.
#[allow(clippy::too_many_arguments)]
fn run_model_detections(
    config: &PathBuf,
    model_key: &str,
    input: &image::DynamicImage,
    offset: (f32, f32),
    image_path: &PathBuf,
    frame_width: u32,
    frame_height: u32,
    polygon_simplify: f64,
    min_component_area_ratio: f32,
    mask_threshold: f32,
) -> Result<Vec<mana_lite::detection::Detection>, Box<dyn Error>> {
    let (model_path, _) = catalog_model_context(config, model_key)?;
    let model_path = resolve_model_path(model_path, config)?;
    let mut model = load_model(&model_path, None, None)?;
    let results = model.predict_image(input, image_path.to_string_lossy().into_owned())?;
    let mut detections = collect_detections(
        &results,
        polygon_simplify,
        min_component_area_ratio,
        mask_threshold,
    );
    translate_detections_to_frame(
        &mut detections,
        offset.0,
        offset.1,
        frame_width,
        frame_height,
    );
    Ok(detections)
}

/// Corre `seg-standard`, traduce sus detecciones y devuelve además el raster
/// binario de la máscara (resolución del modelo), unión de todas las
/// detecciones.
#[allow(clippy::too_many_arguments)]
fn run_seg(
    config: &PathBuf,
    input: &image::DynamicImage,
    offset: (f32, f32),
    image_path: &PathBuf,
    frame_width: u32,
    frame_height: u32,
    polygon_simplify: f64,
    min_component_area_ratio: f32,
    mask_threshold: f32,
) -> Result<(Vec<mana_lite::detection::Detection>, Option<RasterMask>), Box<dyn Error>> {
    let (model_path, _) = catalog_model_context(config, "seg-standard")?;
    let model_path = resolve_model_path(model_path, config)?;
    let mut model = load_model(&model_path, None, None)?;
    let results = model.predict_image(input, image_path.to_string_lossy().into_owned())?;
    let raster = results
        .iter()
        .find_map(|result| result.masks.as_ref())
        .map(|masks| {
            let (h, w) = (masks.data.shape()[1], masks.data.shape()[2]);
            let mut bits = vec![0u8; w * h];
            for index in 0..w * h {
                let row = index / w;
                let col = index % w;
                let mut above = mask_threshold;
                for detection in 0..masks.data.shape()[0] {
                    above = above.max(masks.data[[detection, row, col]]);
                }
                if above > mask_threshold {
                    bits[index] = 1;
                }
            }
            RasterMask {
                width: w as u32,
                height: h as u32,
                bits,
            }
        });
    let mut detections = collect_detections(
        &results,
        polygon_simplify,
        min_component_area_ratio,
        mask_threshold,
    );
    translate_detections_to_frame(
        &mut detections,
        offset.0,
        offset.1,
        frame_width,
        frame_height,
    );
    Ok((detections, raster))
}

/// Imagen de entrada para pose/face/seg: el recorte de zonas con margen, o el
/// frame completo.
fn pose_input(
    image: &image::DynamicImage,
    crop: Option<[u32; 4]>,
) -> (image::DynamicImage, (f32, f32)) {
    match crop {
        Some([x1, y1, x2, y2]) => {
            let cropped = image::imageops::crop_imm(image, x1, y1, x2 - x1, y2 - y1).to_image();
            println!("zones roi: ({x1}, {y1}) -> ({x2}, {y2})");
            (
                image::DynamicImage::ImageRgba8(cropped),
                (x1 as f32, y1 as f32),
            )
        }
        None => {
            println!("zones roi: full frame");
            (image.clone(), (0.0, 0.0))
        }
    }
}

fn parse_args(args: Vec<String>) -> Result<Options, Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        std::process::exit(0);
    }
    let mut options = Options {
        config: PathBuf::from("config/mana.toml"),
        session: PathBuf::from("config/deep-calib.toml"),
        image: PathBuf::new(),
        output: PathBuf::new(),
        json: None,
        alpha: 0.45,
        imgsz: None,
        radius: 0.035,
        radius_base: "min".to_string(),
        seg: false,
        zones: false,
        colormap: Colormap::Jet,
        depth_viz: DepthViz::Metric,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--config" => options.config = PathBuf::from(next(&args, &mut index, "--config")?),
            "--session" => options.session = PathBuf::from(next(&args, &mut index, "--session")?),
            "--image" => options.image = PathBuf::from(next(&args, &mut index, "--image")?),
            "--output" => options.output = PathBuf::from(next(&args, &mut index, "--output")?),
            "--json" => options.json = Some(PathBuf::from(next(&args, &mut index, "--json")?)),
            "--alpha" => {
                options.alpha = next(&args, &mut index, "--alpha")?.parse::<f32>()?;
                if !options.alpha.is_finite() || !(0.0..=1.0).contains(&options.alpha) {
                    return Err("--alpha must be between 0 and 1".into());
                }
            }
            "--imgsz" => {
                options.imgsz = Some(next(&args, &mut index, "--imgsz")?.parse::<u32>()?);
                if options.imgsz == Some(0) {
                    return Err("--imgsz must be positive".into());
                }
            }
            "--radius" => {
                options.radius = next(&args, &mut index, "--radius")?.parse::<f32>()?;
                if !options.radius.is_finite() || options.radius <= 0.0 {
                    return Err("--radius must be positive".into());
                }
            }
            "--radius-base" => {
                options.radius_base = next(&args, &mut index, "--radius-base")?;
                if !["min", "width", "height", "max", "diag"]
                    .contains(&options.radius_base.as_str())
                {
                    return Err("--radius-base must be min|width|height|max|diag".into());
                }
            }
            "--seg" => options.seg = true,
            "--zones" => options.zones = true,
            "--colormap" => {
                options.colormap = next(&args, &mut index, "--colormap")?
                    .parse()
                    .map_err(|error: String| error)?;
            }
            "--depth-viz" => {
                options.depth_viz = next(&args, &mut index, "--depth-viz")?
                    .parse()
                    .map_err(|error: String| error)?;
            }
            unknown => return Err(format!("unknown option: {unknown}").into()),
        }
        index += 1;
    }
    if options.image.as_os_str().is_empty() {
        return Err("--image is required".into());
    }
    if options.output.as_os_str().is_empty() {
        let stem = options
            .image
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("frame");
        options.output = options
            .image
            .with_file_name(format!("{stem}.calib-radio.png"));
    }
    Ok(options)
}

fn next(args: &[String], index: &mut usize, option: &str) -> Result<String, Box<dyn Error>> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("{option} requires a value").into())
}

fn print_usage() {
    println!(
        "deep-calib-radio --session config/deep-calib.toml --image IMAGE \\
         [--config config/mana.toml] [--output OUT.png] [--json OUT.json] [--alpha 0.45] \\
         [--imgsz N] [--radius 0.035] [--radius-base min|width|height|max|diag] [--seg] \\
         [--colormap inferno|jet|spectral|gray] [--depth-viz metric|disparity] [--zones]"
    );
}
