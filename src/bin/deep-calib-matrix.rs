//! Workshop: matriz de modelos depth sobre imágenes de prueba.
//!
//! Para cada modelo depth del catálogo (s/m/l/x × 320/640) y cada imagen
//! (`frame.jpeg` cama vacía, `acostado-1.jpeg` persona), corre el modelo sobre
//! la sesión y reporta las medianas observadas por zona, la deriva del piso
//! contra la calibración y el head probe (mediana del bbox de la cara sobre la
//! zona calibrada). Sirve para elegir el modelo más preciso/estable sin tocar
//! el runtime. Ver `docs/subprojects/deep-fusion/Memoria spec - comportamiento
//! del modelo depth.md`.

use std::error::Error;
use std::path::PathBuf;

use mana_lite::region_stats;

#[path = "common/mod.rs"]
mod common;

use common::*;

const FACE_MIN_CONF: f32 = 0.30;

#[derive(Debug)]
struct Options {
    config: PathBuf,
    session: PathBuf,
    images: Vec<PathBuf>,
    models: Vec<String>,
    face: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_args(std::env::args().skip(1).collect())?;
    let calibration = load_calibration(&options.session)?;

    let (face_path, _) = catalog_model_context(&options.config, "face-yolo")?;
    let face_path = resolve_model_path(face_path, &options.config)?;
    let mut face_model = load_model(&face_path, None, None)?;

    println!(
        "model\timage\tbed/head\tbed/body\tbed/feet\tfloor/head\tfloor/body\tfloor/feet\tface_m\tface_zone"
    );
    for image_path in &options.images {
        let frame = open_frame(image_path, &calibration)?;
        let crop = zones_crop(&calibration, frame.frame_width, frame.frame_height);
        let face_bbox = if options.face {
            detect_face(&mut face_model, &frame.image, crop, image_path)?
        } else {
            None
        };
        for model_key in &options.models {
            let (model_path, catalog_roi, roi_margin) =
                catalog_depth_context(&options.config, model_key)?;
            let model_path = resolve_model_path(model_path, &options.config)?;
            let depth_roi = require_derived_depth_roi(&calibration, catalog_roi, roi_margin)?;
            let imgsz = model_key
                .rsplit('-')
                .next()
                .and_then(|size| size.parse::<u32>().ok());
            let mut model = load_model(&model_path, Some(depth_roi), imgsz)?;
            let run = run_depth(&mut model, &frame.image, image_path, depth_roi)?;
            let observed = observed_zone_medians(
                &run.depth,
                run.roi,
                &calibration,
                frame.frame_width,
                frame.frame_height,
            );
            let reference = reference_median(&observed);
            let median_of = |layer: &str, zone: &str| {
                observed
                    .get(&(layer.to_string(), zone.to_string()))
                    .copied()
                    .unwrap_or(f32::NAN)
            };
            let (face_m, face_zone) = match face_bbox {
                Some([x1, y1, x2, y2]) => {
                    let region = [
                        x1.round() as u32,
                        y1.round() as u32,
                        x2.round() as u32,
                        y2.round() as u32,
                    ];
                    let stats = region_stats(&run.depth, run.roi, region);
                    let median = stats.as_ref().and_then(|stats| stats.median_depth_m);
                    let zone = median.and_then(|median| {
                        nearest_zone_by(&calibration, median, &reference)
                            .map(|(layer, zone)| format!("{}/{}", layer.as_str(), zone.name))
                    });
                    (
                        median.map_or_else(|| "none".to_string(), |value| format!("{value:.3}")),
                        zone.unwrap_or_else(|| "-".to_string()),
                    )
                }
                None => ("none".to_string(), "-".to_string()),
            };
            println!(
                "{model_key}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{face_m}\t{face_zone}",
                image_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?"),
                median_of("bed", "head"),
                median_of("bed", "body"),
                median_of("bed", "feet"),
                median_of("floor", "head"),
                median_of("floor", "body"),
                median_of("floor", "feet")
            );
        }
    }
    Ok(())
}

/// Detecta la mejor cara (mayor confianza) sobre el recorte de zonas y la
/// devuelve en coordenadas de frame.
fn detect_face(
    face_model: &mut ultralytics_inference::YOLOModel,
    image: &image::DynamicImage,
    crop: Option<[u32; 4]>,
    image_path: &PathBuf,
) -> Result<Option<[f32; 4]>, Box<dyn Error>> {
    let (input, offset): (image::DynamicImage, (f32, f32)) = match crop {
        Some([x1, y1, x2, y2]) => {
            let cropped = image::imageops::crop_imm(image, x1, y1, x2 - x1, y2 - y1).to_image();
            (
                image::DynamicImage::ImageRgba8(cropped),
                (x1 as f32, y1 as f32),
            )
        }
        None => (image.clone(), (0.0, 0.0)),
    };
    let results = face_model.predict_image(&input, image_path.to_string_lossy().into_owned())?;
    let Some(boxes) = results.first().and_then(|result| result.boxes.as_ref()) else {
        return Ok(None);
    };
    let mut best: Option<([f32; 4], f32)> = None;
    for index in 0..boxes.len() {
        let conf = boxes.conf()[[index]];
        if conf < FACE_MIN_CONF {
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
    Ok(best.map(|(bbox, _)| bbox))
}

fn parse_args(args: Vec<String>) -> Result<Options, Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        std::process::exit(0);
    }
    let mut options = Options {
        config: PathBuf::from("config/mana.toml"),
        session: PathBuf::from("config/deep-calib.toml"),
        images: Vec::new(),
        models: vec![
            "depth-s-320".to_string(),
            "depth-m-320".to_string(),
            "depth-l-320".to_string(),
            "depth-x-320".to_string(),
            "depth-s-640".to_string(),
            "depth-m-640".to_string(),
            "depth-l-640".to_string(),
            "depth-x-640".to_string(),
        ],
        face: true,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--config" => options.config = PathBuf::from(next(&args, &mut index, "--config")?),
            "--session" => options.session = PathBuf::from(next(&args, &mut index, "--session")?),
            "--image" => options
                .images
                .push(PathBuf::from(next(&args, &mut index, "--image")?)),
            "--models" => {
                options.models = next(&args, &mut index, "--models")?
                    .split(',')
                    .map(ToOwned::to_owned)
                    .collect();
            }
            "--no-face" => options.face = false,
            unknown => return Err(format!("unknown option: {unknown}").into()),
        }
        index += 1;
    }
    if options.images.is_empty() {
        return Err("at least one --image is required".into());
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
        "deep-calib-matrix --session config/deep-calib.toml --image frame.jpeg --image acostado-1.jpeg \\
         [--config config/mana.toml] [--models depth-s-320,...] [--no-face]"
    );
}
