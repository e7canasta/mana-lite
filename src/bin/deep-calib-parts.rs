//! Body parts del runtime (BodyPartsEstimator) sobre la sesión de calibración.
//!
//! Reutiliza la implementación existente de `mana_lite::app::body_parts`:
//! corre depth de escena (modelo de la sesión), pose (`pose-standard`), face
//! (`face-yolo`) y segmentación (`seg-standard`) sobre el recorte de zonas,
//! los traduce a `Detection` como hace el runtime, estima las 6 partes por
//! actor y les adjunta profundidad y evidencia de superficie calibrada
//! (`attach_depth` + `attach_surface_evidence`). El JSON y el preview permiten
//! comparar modelos sin re-correr.

use std::error::Error;
use std::path::PathBuf;

use image::Rgba;
use imageproc::drawing::{
    draw_filled_circle_mut, draw_filled_rect_mut, draw_hollow_circle_mut,
    draw_hollow_polygon_mut, draw_hollow_rect_mut, draw_polygon_mut,
};
use serde::Serialize;

#[path = "common/mod.rs"]
mod common;

use common::*;
use mana_lite::app::body_parts::{
    ActorRef, BodyGeometry, BodyPartsEstimator, PendingBodyPartsEvidence, attach_depth,
    attach_surface_evidence,
};
use mana_lite::app::cross_model_validation::EvidenceKind;
use mana_lite::config::{load_app_config, load_model_catalog};
use mana_lite::infer::{collect_detections, translate_detections_to_frame};

/// Resultado completo de una corrida, serializable a JSON.
#[derive(Serialize)]
struct RunReport {
    model_key: String,
    image: String,
    zones: Vec<ZoneReport>,
    actors: Vec<ActorReport>,
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
struct ActorReport {
    actor_ref: String,
    frame_number: u64,
    overall_quality: f32,
    parts: Vec<PartReport>,
}

#[derive(Serialize)]
struct PartReport {
    part: String,
    geometry: String,
    bbox: Option<[f32; 4]>,
    points: Option<Vec<[f32; 2]>>,
    radius: Option<f32>,
    support: Vec<String>,
    source_models: Vec<String>,
    quality: f32,
    mask_coverage: Option<f32>,
    depth: Option<DepthReport>,
}

#[derive(Serialize)]
struct DepthReport {
    source_model: String,
    roi: [u32; 4],
    map_width: u32,
    map_height: u32,
    sampled_pixels: u64,
    valid_pixels: u64,
    valid_ratio: Option<f32>,
    min_depth_m: Option<f32>,
    median_depth_m: Option<f32>,
    p10_depth_m: Option<f32>,
    p90_depth_m: Option<f32>,
    max_depth_m: Option<f32>,
    relative_to_torso_m: Option<f32>,
    surface_evidence: Vec<SurfaceReport>,
}

#[derive(Serialize)]
struct SurfaceReport {
    surface: String,
    zone: String,
    sampled_pixels: u64,
    valid_ratio: Option<f32>,
    observed_median: Option<f32>,
    reference_median: f32,
    residual: Option<f32>,
    in_envelope: bool,
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
    let run = run_depth(
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

    let mut overlay = draw_zone_fills(
        &calibration,
        &run.depth,
        run.roi,
        frame.frame_width,
        frame.frame_height,
        options.alpha,
    );

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
    let seg_dets = run_model_detections(
        &options.config,
        "seg-standard",
        &input,
        offset,
        &options.image,
        frame.frame_width,
        frame.frame_height,
        polygon_simplify,
        min_component_area_ratio,
        mask_threshold,
    )?;
    println!(
        "detections: pose={} face={} seg={}",
        pose_dets.len(),
        face_dets.len(),
        seg_dets.len()
    );

    let inputs = [
        PendingBodyPartsEvidence {
            model_key: "pose-standard",
            kind: EvidenceKind::Pose,
            target: None,
            detections: &pose_dets,
        },
        PendingBodyPartsEvidence {
            model_key: "face-yolo",
            kind: EvidenceKind::Face,
            target: None,
            detections: &face_dets,
        },
        PendingBodyPartsEvidence {
            model_key: "seg-standard",
            kind: EvidenceKind::Segment,
            target: None,
            detections: &seg_dets,
        },
    ];

    let estimator = BodyPartsEstimator::new(&app_config.perception.body_parts);
    let mut estimates = estimator.estimate(
        &inputs,
        &[],
        0,
        frame.frame_width,
        frame.frame_height,
    );
    for estimate in &mut estimates {
        attach_depth(
            estimate,
            &calibration.model_key,
            &run.depth,
            run.roi,
            frame.frame_width,
            frame.frame_height,
        );
    }
    attach_surface_evidence(
        &mut estimates,
        &calibration.model_key,
        &run.depth,
        run.roi,
        frame.frame_width,
        frame.frame_height,
        &calibration,
    );

    let mut actor_reports = Vec::new();
    if !estimates.is_empty() {
        println!("actor\tpart\tsupport\tquality\tcoverage\tmedian\tzone\tdelta");
    }
    for estimate in &mut estimates {
        println!(
            "actor {} quality={:.3} parts={}",
            actor_ref_str(&estimate.actor_ref),
            estimate.overall_quality,
            estimate.parts.len()
        );
        let mut part_reports = Vec::new();
        for part in &estimate.parts {
            let median = part
                .depth
                .as_ref()
                .and_then(|depth| depth.median_depth_m);
            let zone = median.and_then(|median| nearest_zone_by(&calibration, median, &reference));
            let (fill, fill_alpha) = match zone {
                Some((layer, zone)) => {
                    let (layer_min, layer_max) = layer_bounds(&calibration, layer);
                    let delta = median.map_or(0.0, |depth| (depth - reference(layer, zone)).abs());
                    println!(
                        "{}\t{}\t{}\t{:.3}\t{}\t{}\t{}/{}\t{delta:.3}",
                        actor_ref_str(&estimate.actor_ref),
                        part.part.as_str(),
                        part.support
                            .iter()
                            .map(|support| support.as_str())
                            .collect::<Vec<_>>()
                            .join("+"),
                        part.quality,
                        format_option(part.mask_coverage),
                        format_option(median),
                        layer.as_str(),
                        zone.name
                    );
                    layer_scale(
                        layer,
                        median.unwrap_or(reference(layer, zone)),
                        layer_min,
                        layer_max,
                        0.9,
                    )
                }
                None => {
                    println!(
                        "{}\t{}\t{}\t{:.3}\t{}\t{}\tunclassified\t-",
                        actor_ref_str(&estimate.actor_ref),
                        part.part.as_str(),
                        part.support
                            .iter()
                            .map(|support| support.as_str())
                            .collect::<Vec<_>>()
                            .join("+"),
                        part.quality,
                        format_option(part.mask_coverage),
                        format_option(median),
                    );
                    ([128, 128, 128], 230)
                }
            };
            draw_part(&mut overlay, part, [fill[0], fill[1], fill[2]], fill_alpha);
            part_reports.push(part_report(part));
        }
        actor_reports.push(ActorReport {
            actor_ref: actor_ref_str(&estimate.actor_ref),
            frame_number: estimate.frame_number,
            overall_quality: estimate.overall_quality,
            parts: part_reports,
        });
    }

    let mut rgba = frame.image.to_rgba8();
    image::imageops::overlay(&mut rgba, &overlay, 0, 0);
    let mut rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    draw_zone_borders(&mut rgb, &calibration);
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
            zones,
            actors: actor_reports,
        };
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(json_path, json)?;
        println!("json={}", json_path.display());
    }

    print_legend(&calibration);
    println!("output={}", options.output.display());
    Ok(())
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

fn actor_ref_str(actor_ref: &ActorRef) -> String {
    match actor_ref {
        ActorRef::Track(id) => format!("track:{id}"),
        ActorRef::FrameLocal {
            frame_number,
            index,
        } => format!("frame_local:{frame_number}:{index}"),
    }
}

fn part_report(part: &mana_lite::app::body_parts::BodyPartEstimate) -> PartReport {
    let mut report = PartReport {
        part: part.part.as_str().to_string(),
        geometry: match part.geometry {
            BodyGeometry::Bbox(_) => "bbox",
            BodyGeometry::Polygon(_) => "polygon",
            BodyGeometry::Polyline { .. } => "polyline",
        }
        .to_string(),
        bbox: None,
        points: None,
        radius: None,
        support: part.support.iter().map(|support| support.as_str().to_string()).collect(),
        source_models: part.source_models.clone(),
        quality: part.quality,
        mask_coverage: part.mask_coverage,
        depth: None,
    };
    match &part.geometry {
        BodyGeometry::Bbox(bbox) => report.bbox = Some(*bbox),
        BodyGeometry::Polygon(points) => report.points = Some(points.clone()),
        BodyGeometry::Polyline { points, radius } => {
            report.points = Some(points.clone());
            report.radius = Some(*radius);
        }
    }
    if let Some(depth) = &part.depth {
        report.depth = Some(DepthReport {
            source_model: depth.source_model.clone(),
            roi: depth.roi,
            map_width: depth.map_width,
            map_height: depth.map_height,
            sampled_pixels: depth.sampled_pixels,
            valid_pixels: depth.valid_pixels,
            valid_ratio: depth.valid_ratio,
            min_depth_m: depth.min_depth_m,
            median_depth_m: depth.median_depth_m,
            p10_depth_m: depth.p10_depth_m,
            p90_depth_m: depth.p90_depth_m,
            max_depth_m: depth.max_depth_m,
            relative_to_torso_m: depth.relative_to_torso_m,
            surface_evidence: depth
                .surface_evidence
                .iter()
                .map(|evidence| SurfaceReport {
                    surface: evidence.surface.clone(),
                    zone: evidence.zone.clone(),
                    sampled_pixels: evidence.sampled_pixels,
                    valid_ratio: evidence.valid_ratio,
                    observed_median: evidence.observed_median,
                    reference_median: evidence.reference_median,
                    residual: evidence.residual,
                    in_envelope: evidence.in_envelope,
                })
                .collect(),
        });
    }
    report
}

/// Dibuja la geometría de una parte sobre el overlay con el color de su zona.
fn draw_part(
    overlay: &mut image::RgbaImage,
    part: &mana_lite::app::body_parts::BodyPartEstimate,
    fill: [u8; 3],
    fill_alpha: u8,
) {
    match &part.geometry {
        BodyGeometry::Bbox([x1, y1, x2, y2]) => {
            let rect = imageproc::rect::Rect::at(x1.round() as i32, y1.round() as i32)
                .of_size((x2 - x1).round().max(1.0) as u32, (y2 - y1).round().max(1.0) as u32);
            draw_filled_rect_mut(overlay, rect, Rgba([fill[0], fill[1], fill[2], fill_alpha]));
            draw_hollow_rect_mut(overlay, rect, Rgba([255, 255, 255, 255]));
        }
        BodyGeometry::Polygon(points) => {
            let polygon = polygon_points(points);
            draw_polygon_mut(
                overlay,
                &polygon,
                Rgba([fill[0], fill[1], fill[2], fill_alpha]),
            );
            draw_hollow_polygon_mut(
                overlay,
                &polygon_points_f32(points),
                Rgba([255, 255, 255, 255]),
            );
        }
        BodyGeometry::Polyline { points, radius } => {
            let radius = radius.round().max(1.0) as i32;
            for pair in points.windows(2) {
                draw_capsule(
                    overlay,
                    pair[0],
                    pair[1],
                    radius,
                    Rgba([fill[0], fill[1], fill[2], fill_alpha]),
                );
            }
            for point in points {
                let center = (point[0].round() as i32, point[1].round() as i32);
                draw_filled_circle_mut(
                    overlay,
                    center,
                    radius,
                    Rgba([fill[0], fill[1], fill[2], fill_alpha]),
                );
                draw_hollow_circle_mut(overlay, center, radius, Rgba([255, 255, 255, 255]));
            }
        }
    }
}

/// Cápsula aproximada por círculos a lo largo del segmento.
fn draw_capsule(
    overlay: &mut image::RgbaImage,
    start: [f32; 2],
    end: [f32; 2],
    radius: i32,
    fill: Rgba<u8>,
) {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length = (dx * dx + dy * dy).sqrt();
    let steps = (length / radius.max(1) as f32).ceil().max(1.0) as u32;
    for index in 0..=steps {
        let t = index as f32 / steps as f32;
        let point = (
            (start[0] + dx * t).round() as i32,
            (start[1] + dy * t).round() as i32,
        );
        draw_filled_circle_mut(overlay, point, radius, fill);
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
            .with_file_name(format!("{stem}.calib-parts.png"));
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
        "deep-calib-parts --session config/deep-calib.toml --image IMAGE \\
         [--config config/mana.toml] [--output OUT.png] [--json OUT.json] [--alpha 0.45] \\
         [--imgsz N]"
    );
}
