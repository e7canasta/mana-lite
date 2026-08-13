//! Preview visual de la calibración de superficies.
//!
//! Lee una sesión `deep-calib.toml`, corre el mismo modelo depth de escena
//! sobre una imagen y pinta cada zona `bed`/`floor` con un relleno
//! semitransparente: `bed` en escala de azules, `floor` en escala de
//! amarillos (cerca = tono y opacidad máximos). La consola imprime la
//! comparación entre la mediana calibrada y la observada para auditar la
//! calibración.

use std::error::Error;
use std::path::PathBuf;

use image::imageops;

#[path = "common/mod.rs"]
mod common;

use common::*;

#[derive(Debug)]
struct Options {
    config: PathBuf,
    session: PathBuf,
    image: PathBuf,
    output: PathBuf,
    alpha: f32,
    imgsz: Option<u32>,
    model_key: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_args(std::env::args().skip(1).collect())?;

    let calibration = load_calibration(&options.session)?;
    let frame = open_frame(&options.image, &calibration)?;

    let model_key = options
        .model_key
        .as_deref()
        .unwrap_or(&calibration.model_key);
    let (model_path, catalog_roi, roi_margin) = catalog_depth_context(&options.config, model_key)?;
    let model_path = resolve_model_path(model_path, &options.config)?;
    let depth_roi = require_derived_depth_roi(&calibration, catalog_roi, roi_margin)?;
    let mut model = load_model(
        &model_path,
        Some(depth_roi),
        options.imgsz.or(model_key
            .rsplit('-')
            .next()
            .and_then(|size| size.parse::<u32>().ok())),
    )?;
    let run = run_depth(&mut model, &frame.image, &options.image, depth_roi)?;

    let stats = zone_stats(
        &run.depth,
        run.roi,
        &calibration,
        frame.frame_width,
        frame.frame_height,
    );
    print_zone_table(&stats);

    let overlay = draw_zone_fills(
        &calibration,
        &run.depth,
        run.roi,
        frame.frame_width,
        frame.frame_height,
        options.alpha,
    );
    let mut rgba = frame.image.to_rgba8();
    imageops::overlay(&mut rgba, &overlay, 0, 0);
    let mut rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    draw_zone_borders(&mut rgb, &calibration);
    rgb.save(&options.output)?;

    print_legend(&calibration);
    println!("output={}", options.output.display());
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
        image: PathBuf::new(),
        output: PathBuf::new(),
        alpha: 0.45,
        imgsz: None,
        model_key: None,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--config" => options.config = PathBuf::from(next(&args, &mut index, "--config")?),
            "--session" => options.session = PathBuf::from(next(&args, &mut index, "--session")?),
            "--image" => options.image = PathBuf::from(next(&args, &mut index, "--image")?),
            "--output" => options.output = PathBuf::from(next(&args, &mut index, "--output")?),
            "--model-key" => options.model_key = Some(next(&args, &mut index, "--model-key")?),
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
            .with_file_name(format!("{stem}.calib-preview.png"));
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
        "deep-calib-preview --session config/deep-calib.toml --image IMAGE \\
         [--config config/mana.toml] [--output OUT.png] [--alpha 0.45] [--imgsz N] [--model-key depth-...]"
    );
}
