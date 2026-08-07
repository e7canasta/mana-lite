use std::error::Error;
use std::path::{Path, PathBuf};

use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use imageproc::drawing::draw_hollow_rect_mut;
use imageproc::rect::Rect;
use ultralytics_inference::visualizer::color::{Colormap, DepthViz};
use ultralytics_inference::{InferenceConfig, YOLOModel};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: depth-image-probe <model.onnx> <image> <annotated.png> [--rerun addr|--rrd file] [--roi x1 y1 x2 y2]".into(),
        );
    }

    let model_path = &args[1];
    let image_path = &args[2];
    let output_path = PathBuf::from(&args[3]);
    let (rerun_addr, rrd_path, roi) = parse_options(&args[4..])?;

    let image = image::open(image_path)?;
    let (width, height) = image.dimensions();
    let mut config = InferenceConfig::default();
    if let Some([x1, y1, x2, y2]) = roi {
        config = config.with_roi(x1, y1, x2, y2);
    }
    let mut model = YOLOModel::load_with_config(model_path, config)?;
    let results = model.predict_image(&image, image_path.clone())?;
    let result = results.first().ok_or("inference returned no results")?;
    let depth = result.depth.as_ref().ok_or("inference returned no depth map")?;

    let shape = depth.data.shape();
    if shape != [height as usize, width as usize] {
        return Err(format!(
            "depth shape {:?} does not match image dimensions {}x{}",
            shape, width, height
        )
        .into());
    }

    let colors = depth.colorize(Colormap::default(), DepthViz::default());
    let colorized = rgb_image_from_pixels(&colors, width, height)?;
    let mut annotated = blend_depth(&image, &colors, 0.6)?;
    draw_roi_overlay(&mut annotated, result.roi);
    annotated.save(&output_path)?;

    let depth_output = depth_output_path(&output_path);
    colorized.save(&depth_output)?;

    let valid_pixels = depth
        .data
        .iter()
        .filter(|&&value| value.is_finite() && value > 0.0)
        .count();
    println!(
        "model={} image={} output={} depth_output={} image={}x{} map={:?} valid_pixels={} min_depth_m={:?} max_depth_m={:?} roi={:?}",
        model_path,
        image_path,
        output_path.display(),
        depth_output.display(),
        width,
        height,
        shape,
        valid_pixels,
        finite_min(depth),
        finite_max(depth),
        result.roi,
    );

    if let Some(addr) = rerun_addr {
        log_rerun(addr, &image, &colorized, &annotated, depth, result.roi)?;
        println!("rerun=connected addr={addr}");
    }
    if let Some(path) = rrd_path {
        log_rrd(path, &image, &colorized, &annotated, depth, result.roi)?;
        println!("rrd=saved path={path}");
    }

    Ok(())
}

fn parse_options(
    options: &[String],
) -> Result<(Option<&str>, Option<&str>, Option<[u32; 4]>), Box<dyn Error>> {
    let mut rerun_addr = None;
    let mut rrd_path = None;
    let mut roi = None;
    let mut index = 0;
    while index < options.len() {
        match options[index].as_str() {
            "--rerun" => {
                index += 1;
                rerun_addr = Some(
                    options
                        .get(index)
                        .ok_or("--rerun requires an address")?
                        .as_str(),
                );
            }
            "--rrd" => {
                index += 1;
                rrd_path = Some(
                    options
                        .get(index)
                        .ok_or("--rrd requires an output path")?
                        .as_str(),
                );
            }
            "--roi" => {
                if index + 4 >= options.len() {
                    return Err("--roi requires x1 y1 x2 y2".into());
                }
                let mut values = [0u32; 4];
                for (offset, value) in values.iter_mut().enumerate() {
                    *value = options[index + offset + 1].parse()?;
                }
                if values[2] <= values[0] || values[3] <= values[1] {
                    return Err("--roi must have positive width and height".into());
                }
                roi = Some(values);
                index += 4;
            }
            unknown => return Err(format!("unknown option: {unknown}").into()),
        }
        index += 1;
    }
    Ok((rerun_addr, rrd_path, roi))
}

fn rgb_image_from_pixels(
    pixels: &[[u8; 3]],
    width: u32,
    height: u32,
) -> Result<RgbImage, Box<dyn Error>> {
    let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
    RgbImage::from_raw(width, height, bytes).ok_or_else(|| "invalid RGB image dimensions".into())
}

fn blend_depth(
    image: &DynamicImage,
    colors: &[[u8; 3]],
    alpha: f32,
) -> Result<RgbImage, Box<dyn Error>> {
    let mut annotated = image.to_rgb8();
    let alpha = alpha.clamp(0.0, 1.0);
    if annotated.pixels().len() != colors.len() {
        return Err("depth and source image pixel counts differ".into());
    }
    for (pixel, heat) in annotated.pixels_mut().zip(colors) {
        for (channel, &value) in pixel.0.iter_mut().zip(heat) {
            *channel = f32::from(*channel)
                .mul_add(1.0 - alpha, f32::from(value) * alpha)
                .round() as u8;
        }
    }
    Ok(annotated)
}

fn draw_roi_overlay(image: &mut RgbImage, roi: Option<(u32, u32, u32, u32)>) {
    let Some((x1, y1, x2, y2)) = roi else {
        return;
    };
    let width = x2.saturating_sub(x1);
    let height = y2.saturating_sub(y1);
    if width == 0 || height == 0 {
        return;
    }
    let yellow = Rgb([255, 255, 0]);
    draw_hollow_rect_mut(image, Rect::at(x1 as i32, y1 as i32).of_size(width, height), yellow);
    draw_hollow_rect_mut(
        image,
        Rect::at(x1 as i32 + 1, y1 as i32 + 1)
            .of_size(width.saturating_sub(2), height.saturating_sub(2)),
        yellow,
    );
}

fn depth_output_path(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("depth-annotated");
    output.with_file_name(format!("{stem}.depth.png"))
}

fn log_rerun(
    addr: &str,
    image: &DynamicImage,
    colorized: &RgbImage,
    annotated: &RgbImage,
    depth: &ultralytics_inference::DepthMap,
    roi: Option<(u32, u32, u32, u32)>,
) -> Result<(), Box<dyn Error>> {
    let url = format!("rerun+http://{addr}/proxy");
    let rec = rerun::RecordingStreamBuilder::new("mana-depth-probe")
        .connect_grpc_opts(url)?;
    write_recording(&rec, image, colorized, annotated, depth, roi)
}

fn log_rrd(
    path: &str,
    image: &DynamicImage,
    colorized: &RgbImage,
    annotated: &RgbImage,
    depth: &ultralytics_inference::DepthMap,
    roi: Option<(u32, u32, u32, u32)>,
) -> Result<(), Box<dyn Error>> {
    let rec = rerun::RecordingStreamBuilder::new("mana-depth-probe").save(path)?;
    write_recording(&rec, image, colorized, annotated, depth, roi)
}

fn write_recording(
    rec: &rerun::RecordingStream,
    image: &DynamicImage,
    colorized: &RgbImage,
    annotated: &RgbImage,
    depth: &ultralytics_inference::DepthMap,
    roi: Option<(u32, u32, u32, u32)>,
) -> Result<(), Box<dyn Error>> {
    rec.set_time_sequence("frame_ns", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let (width, height) = image.dimensions();
    rec.log(
        "/world/camera/bgr",
        &rerun::Image::from_rgb24(image.to_rgb8().into_raw(), [width, height]),
    )?;
    rec.log(
        "/world/camera/depth/original/disparity",
        &rerun::Image::from_rgb24(colorized.as_raw().clone(), [width, height]),
    )?;
    rec.log(
        "/world/camera/depth/original/annotated",
        &rerun::Image::from_rgb24(annotated.as_raw().clone(), [width, height]),
    )?;
    if let Some((x1, y1, x2, y2)) = roi {
        let bbox = rerun::Boxes2D::from_centers_and_half_sizes(
            [rerun::datatypes::Vec2D([
                (x1 + x2) as f32 / 2.0,
                (y1 + y2) as f32 / 2.0,
            ])],
            [rerun::datatypes::Vec2D([
                (x2 - x1) as f32 / 2.0,
                (y2 - y1) as f32 / 2.0,
            ])],
        )
        .with_labels(["ROI"])
        .with_colors([rerun::Color::from_rgb(255, 255, 0)])
        .with_radii([2.0]);
        rec.log("/world/camera/depth/original/roi", &bbox)?;
    }
    rec.log(
        "/world/camera/depth/original/stats/valid_pixels",
        &rerun::Scalars::single(
            depth
                .data
                .iter()
                .filter(|&&value| value.is_finite() && value > 0.0)
                .count() as f64,
        ),
    )?;
    if let Some(value) = finite_min(depth) {
        rec.log(
            "/world/camera/depth/original/stats/min_depth_m",
            &rerun::Scalars::single(f64::from(value)),
        )?;
    }
    if let Some(value) = finite_max(depth) {
        rec.log(
            "/world/camera/depth/original/stats/max_depth_m",
            &rerun::Scalars::single(f64::from(value)),
        )?;
    }
    rec.flush_with_timeout(std::time::Duration::from_secs(2))?;
    Ok(())
}

fn finite_min(depth: &ultralytics_inference::DepthMap) -> Option<f32> {
    depth
        .data
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .reduce(f32::min)
}

fn finite_max(depth: &ultralytics_inference::DepthMap) -> Option<f32> {
    depth
        .data
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .reduce(f32::max)
}
