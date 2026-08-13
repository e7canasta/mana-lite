//! Clasificación de body parts por zona calibrada.
//!
//! Corre el modelo depth de escena (de la sesión de calibración) y el modelo
//! de pose (`pose-standard`) sobre un frame. La pose se ejecuta sobre el
//! recorte del ROI que encierra las zonas calibradas (donde vive la persona),
//! no sobre el frame completo. Cada keypoint se muestrea en su píxel de
//! profundidad y se pinta con el color de la zona cuya mediana calibrada más
//! se aproxima (`bed` = familia azul, `floor` = familia amarilla; la
//! intensidad refleja qué tan cerca de la cama o el piso queda la parte). La
//! consola imprime por parte la confianza, la profundidad, la zona asignada y
//! el residuo.

use std::error::Error;
use std::path::PathBuf;

use image::Rgba;
use imageproc::drawing::{
    draw_filled_circle_mut, draw_filled_rect_mut, draw_hollow_circle_mut, draw_hollow_rect_mut,
};
use serde::Serialize;

#[path = "common/mod.rs"]
mod common;

use common::*;
use mana_lite::region_stats;

/// Resultado completo de una corrida, serializable a JSON para comparar entre
/// modelos sin re-correr.
#[derive(Serialize)]
struct RunReport {
    model_key: String,
    image: String,
    zones: Vec<ZoneReport>,
    keypoints: Vec<KeypointReport>,
    face: Option<FaceReport>,
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
    conf: f32,
    depth_m: Option<f32>,
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
    zone: Option<String>,
}

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

const KEYPOINT_MIN_CONF: f32 = 0.30;

#[derive(Debug)]
struct Options {
    config: PathBuf,
    session: PathBuf,
    image: PathBuf,
    output: PathBuf,
    json: Option<PathBuf>,
    alpha: f32,
    imgsz: Option<u32>,
    face: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_args(std::env::args().skip(1).collect())?;

    let calibration = load_calibration(&options.session)?;
    let frame = open_frame(&options.image, &calibration)?;

    let (depth_path, catalog_roi, roi_margin) =
        catalog_depth_context(&options.config, &calibration.model_key)?;
    let depth_path = resolve_model_path(depth_path, &options.config)?;
    let depth_roi = require_derived_depth_roi(&calibration, catalog_roi, roi_margin)?;
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

    let (pose_path, _) = catalog_model_context(&options.config, "pose-standard")?;
    let pose_path = resolve_model_path(pose_path, &options.config)?;
    let mut pose_model = load_model(&pose_path, None, None)?;

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
    let (pose_input, offset) = pose_input(&frame.image, crop);
    let keypoints = run_pose(&mut pose_model, &pose_input, offset, &options.image)?;

    let mut keypoint_reports = Vec::new();
    if !keypoints.is_empty() {
        println!("person\tpart\tconf\tdepth_m\tzone\tdelta_m");
    }
    for (keypoint_index, (person, [x, y, conf])) in keypoints.iter().enumerate() {
        let name = COCO_17[keypoint_index % COCO_17.len()];
        if *conf < KEYPOINT_MIN_CONF || !x.is_finite() || !y.is_finite() {
            continue;
        }
        let depth_m = depth_at_point(&run.depth, run.roi, *x, *y);
        let zone = depth_m.and_then(|depth_m| nearest_zone_by(&calibration, depth_m, &reference));
        let (fill, fill_alpha) = match zone {
            Some((layer, zone)) => {
                let (layer_min, layer_max) = layer_bounds(&calibration, layer);
                let delta = depth_m.map_or(0.0, |depth| (depth - reference(layer, zone)).abs());
                println!(
                    "{person}\t{name}\t{conf:.2}\t{}\t{}/{}\t{delta:.3}",
                    format_option(depth_m),
                    layer.as_str(),
                    zone.name
                );
                keypoint_reports.push(KeypointReport {
                    person: *person,
                    part: name.to_string(),
                    conf: *conf,
                    depth_m,
                    zone: Some(format!("{}/{}", layer.as_str(), zone.name)),
                    delta_m: depth_m.map(|depth| (depth - reference(layer, zone)).abs()),
                });
                layer_scale(
                    layer,
                    depth_m.unwrap_or(reference(layer, zone)),
                    layer_min,
                    layer_max,
                    0.9,
                )
            }
            None => {
                println!(
                    "{person}\t{name}\t{conf:.2}\t{}\tunclassified\t-",
                    format_option(depth_m)
                );
                keypoint_reports.push(KeypointReport {
                    person: *person,
                    part: name.to_string(),
                    conf: *conf,
                    depth_m,
                    zone: None,
                    delta_m: None,
                });
                ([128, 128, 128], 230)
            }
        };
        let center = (x.round() as i32, y.round() as i32);
        draw_filled_circle_mut(
            &mut overlay,
            center,
            6,
            Rgba([fill[0], fill[1], fill[2], fill_alpha]),
        );
        draw_hollow_circle_mut(&mut overlay, center, 6, Rgba([255, 255, 255, 255]));
    }

    let face_report = if options.face {
        run_face_and_draw(
            &options.config,
            &mut overlay,
            &run.depth,
            run.roi,
            &calibration,
            &reference,
            &pose_input,
            offset,
            &options.image,
        )?
    } else {
        None
    };

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

/// Imagen de entrada para pose/face: el recorte de zonas con margen, o el
/// frame completo. Devuelve la imagen y el offset (en frame coords) de su
/// origen.
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

/// Corre pose sobre la imagen ya recortada y devuelve los keypoints en
/// coordenadas de frame: `(persona, [x, y, conf])`.
fn run_pose(
    pose_model: &mut ultralytics_inference::YOLOModel,
    input: &image::DynamicImage,
    offset: (f32, f32),
    image_path: &PathBuf,
) -> Result<Vec<(usize, [f32; 3])>, Box<dyn Error>> {
    let results = pose_model.predict_image(input, image_path.to_string_lossy().into_owned())?;
    let Some(keypoints) = results.first().and_then(|result| result.keypoints.as_ref()) else {
        println!("pose: no keypoints");
        return Ok(Vec::new());
    };
    let mut translated = Vec::new();
    for person in 0..keypoints.len() {
        for index in 0..COCO_17.len() {
            let x = keypoints.data[[person, index, 0]] + offset.0;
            let y = keypoints.data[[person, index, 1]] + offset.1;
            let conf = keypoints
                .conf()
                .as_ref()
                .map_or(1.0, |conf| conf[[person, index]]);
            translated.push((person, [x, y, conf]));
        }
    }
    Ok(translated)
}

/// Corre `face-yolo` sobre la imagen recortada, rellena el bbox de la cara
/// con el color de la zona calibrada (como los keypoints), clasifica la
/// región por profundidad y devuelve el reporte (o `None` sin detección).
#[allow(clippy::cast_possible_truncation)]
fn run_face_and_draw(
    config: &PathBuf,
    overlay: &mut image::RgbaImage,
    depth: &mana_lite::depth_map::DepthFrame,
    roi: [u32; 4],
    calibration: &mana_lite::SurfaceCalibration,
    reference: &impl Fn(mana_lite::SurfaceLayer, &mana_lite::SurfaceZone) -> f32,
    input: &image::DynamicImage,
    offset: (f32, f32),
    image_path: &PathBuf,
) -> Result<Option<FaceReport>, Box<dyn Error>> {
    let (face_path, _) = catalog_model_context(config, "face-yolo")?;
    let face_path = resolve_model_path(face_path, config)?;
    let mut face_model = load_model(&face_path, None, None)?;
    let results = face_model.predict_image(input, image_path.to_string_lossy().into_owned())?;

    let Some(boxes) = results.first().and_then(|result| result.boxes.as_ref()) else {
        println!("face: no detections");
        return Ok(None);
    };
    let mut best: Option<([f32; 4], f32)> = None;
    for index in 0..boxes.len() {
        let conf = boxes.conf()[[index]];
        if conf < 0.30 {
            continue;
        }
        let bbox = [
            boxes.data[[index, 0]] + offset.0,
            boxes.data[[index, 1]] + offset.1,
            boxes.data[[index, 2]] + offset.0,
            boxes.data[[index, 3]] + offset.1,
        ];
        if best.is_none_or(|(_, best_conf)| conf > best_conf) {
            best = Some((bbox, conf));
        }
    }
    let Some((bbox, conf)) = best else {
        println!("face: no detections");
        return Ok(None);
    };
    let [x1, y1, x2, y2] = bbox;
    let rect = imageproc::rect::Rect::at(x1.round() as i32, y1.round() as i32)
        .of_size((x2 - x1).round() as u32, (y2 - y1).round() as u32);

    let region = [
        x1.round() as u32,
        y1.round() as u32,
        x2.round() as u32,
        y2.round() as u32,
    ];
    let stats = region_stats(depth, roi, region);
    let median = stats.as_ref().and_then(|stats| stats.median_depth_m);
    let p10 = stats.as_ref().and_then(|stats| stats.p10_depth_m);
    let p90 = stats.as_ref().and_then(|stats| stats.p90_depth_m);
    let valid_ratio = stats.as_ref().and_then(|stats| stats.valid_ratio);
    let zone = median.and_then(|median| nearest_zone_by(calibration, median, reference));
    let report = Some(FaceReport {
        conf,
        bbox,
        depth_m: median,
        p10_m: p10,
        p90_m: p90,
        valid_ratio,
        zone: zone
            .as_ref()
            .map(|(layer, zone)| format!("{}/{}", layer.as_str(), zone.name)),
    });
    let (fill, fill_alpha) = match zone {
        Some((layer, zone)) => {
            let (layer_min, layer_max) = layer_bounds(calibration, layer);
            let delta = median.map_or(0.0, |median| (median - reference(layer, zone)).abs());
            println!(
                "face\tbbox\t{conf:.2}\t{}\t{}/{}\t{delta:.3}\tp10={} p90={} valid={}",
                format_option(median),
                layer.as_str(),
                zone.name,
                p10.map_or_else(|| "none".to_string(), |value| format!("{value:.3}")),
                p90.map_or_else(|| "none".to_string(), |value| format!("{value:.3}")),
                valid_ratio.map_or_else(|| "none".to_string(), |value| format!("{value:.3}"))
            );
            layer_scale(
                layer,
                median.unwrap_or(reference(layer, zone)),
                layer_min,
                layer_max,
                0.6,
            )
        }
        None => {
            println!(
                "face\tbbox\t{conf:.2}\t{}\tunclassified\t-",
                format_option(median)
            );
            ([128, 128, 128], 200)
        }
    };
    draw_filled_rect_mut(overlay, rect, Rgba([fill[0], fill[1], fill[2], fill_alpha]));
    draw_hollow_rect_mut(overlay, rect, Rgba([255, 255, 255, 255]));
    let center = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
    draw_filled_circle_mut(
        overlay,
        (center.0.round() as i32, center.1.round() as i32),
        4,
        Rgba([fill[0], fill[1], fill[2], 230]),
    );
    Ok(report)
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
        face: false,
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
            "--face" => options.face = true,
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
            .with_file_name(format!("{stem}.calib-pose.png"));
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
        "deep-calib-pose --session config/deep-calib.toml --image IMAGE \\
         [--config config/mana.toml] [--output OUT.png] [--json OUT.json] [--alpha 0.45] \\
         [--imgsz N] [--face]"
    );
}
