//! Workshop: head probe con los modelos de profundidad de crop de persona.
//!
//! Réplica del pipeline del runtime (`detect-fast` → crop de la persona con
//! margen 0.20 → `depth-person-*`) para medir qué lee la cabeza en el depth de
//! crop de persona, comparándolo en la misma corrida con `depth-standard`
//! (escena). Responde la pregunta abierta de la memoria spec: ¿el crop de
//! persona acerca la cabeza a bed/head (~3.4 m) en vez de los ~4.05 m del
//! modelo de escena?

use std::error::Error;
use std::path::PathBuf;

use mana_lite::region_stats;

#[path = "common/mod.rs"]
mod common;

use common::*;

const PERSON_CLASS_ID: u32 = 0; // COCO: person
const PERSON_MIN_CONF: f32 = 0.30;
const PERSON_MARGIN: f32 = 0.20;
const FACE_MIN_CONF: f32 = 0.30;

#[derive(Debug)]
struct Options {
    config: PathBuf,
    session: PathBuf,
    images: Vec<PathBuf>,
    models: Vec<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_args(std::env::args().skip(1).collect())?;
    let calibration = load_calibration(&options.session)?;

    let (detect_path, _) = catalog_model_context(&options.config, "detect-fast")?;
    let detect_path = resolve_model_path(detect_path, &options.config)?;
    let mut detect_model = load_model(&detect_path, None, Some(320))?;

    let (face_path, _) = catalog_model_context(&options.config, "face-yolo")?;
    let face_path = resolve_model_path(face_path, &options.config)?;
    let mut face_model = load_model(&face_path, None, None)?;

    println!(
        "model\timage\troi_min\troi_med\troi_max\tface_m\tface_p10\tface_p90\tface_valid\tbed/head_obs"
    );
    for image_path in &options.images {
        let frame = open_frame(image_path, &calibration)?;
        let Some(person_bbox) = detect_best_bbox(
            &mut detect_model,
            &frame.image,
            image_path,
            Some(PERSON_CLASS_ID),
            PERSON_MIN_CONF,
        )?
        else {
            println!("no person detected in {}", image_path.display());
            continue;
        };
        let Some(person_crop) = expand_bbox(
            person_bbox,
            PERSON_MARGIN,
            frame.frame_width,
            frame.frame_height,
        ) else {
            println!("person bbox too small in {}", image_path.display());
            continue;
        };
        let crop_width = person_crop[2] - person_crop[0];
        let crop_height = person_crop[3] - person_crop[1];
        let crop_image = image::DynamicImage::ImageRgba8(
            image::imageops::crop_imm(
                &frame.image,
                person_crop[0],
                person_crop[1],
                crop_width,
                crop_height,
            )
            .to_image(),
        );
        let face_bbox = detect_best_bbox(
            &mut face_model,
            &crop_image,
            image_path,
            None,
            FACE_MIN_CONF,
        )?;

        println!(
            "person bbox: {person_bbox:?}  person crop: {person_crop:?}  face (crop): {face_bbox:?}"
        );
        for model_key in &options.models {
            let (model_path, catalog_roi) = catalog_model_context(&options.config, model_key)?;
            let model_path = resolve_model_path(model_path, &options.config)?;
            let imgsz = model_key
                .rsplit('-')
                .next()
                .and_then(|size| size.parse::<u32>().ok());
            let mut model = load_model(&model_path, catalog_roi, imgsz)?;

            let (image, roi) = if catalog_roi.is_some() {
                (&frame.image, calibration.roi)
            } else {
                (&crop_image, [0, 0, crop_width, crop_height])
            };
            let run = run_depth(&mut model, image, image_path, roi)?;

            let probe = face_bbox.map(|[x1, y1, x2, y2]| {
                let region = [
                    x1.round() as u32,
                    y1.round() as u32,
                    x2.round() as u32,
                    y2.round() as u32,
                ];
                if catalog_roi.is_some() {
                    [
                        region[0] + person_crop[0],
                        region[1] + person_crop[1],
                        region[2] + person_crop[0],
                        region[3] + person_crop[1],
                    ]
                } else {
                    region
                }
            });

            let stats = probe.and_then(|region| region_stats(&run.depth, run.roi, region));
            let face_m = stats.as_ref().and_then(|stats| stats.median_depth_m);
            let face_p10 = stats.as_ref().and_then(|stats| stats.p10_depth_m);
            let face_p90 = stats.as_ref().and_then(|stats| stats.p90_depth_m);
            let face_valid = stats
                .as_ref()
                .and_then(|stats| stats.valid_ratio)
                .unwrap_or(0.0);

            let roi_stats = if catalog_roi.is_none() {
                region_stats(&run.depth, run.roi, run.roi)
            } else {
                None
            };
            let roi_min = roi_stats.as_ref().and_then(|stats| stats.min_depth_m);
            let roi_med = roi_stats.as_ref().and_then(|stats| stats.median_depth_m);
            let roi_max = roi_stats.as_ref().and_then(|stats| stats.max_depth_m);

            let observed = observed_zone_medians(
                &run.depth,
                run.roi,
                &calibration,
                frame.frame_width,
                frame.frame_height,
            );
            let bed_head_obs = observed
                .get(&("bed".to_string(), "head".to_string()))
                .copied()
                .unwrap_or(f32::NAN);

            println!(
                "{model_key}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}",
                image_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?"),
                format_option(roi_min),
                format_option(roi_med),
                format_option(roi_max),
                format_option(face_m),
                format_option(face_p10),
                format_option(face_p90),
                face_valid,
                bed_head_obs
            );
        }
    }
    Ok(())
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
            "depth-person-s-320".to_string(),
            "depth-standard".to_string(),
        ],
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
        "deep-calib-person --session config/deep-calib.toml --image acostado-1.jpeg \\
         [--config config/mana.toml] [--models depth-person-s-320,depth-person-s-192,depth-standard]"
    );
}
