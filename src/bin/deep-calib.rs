//! Isolated scene-surface calibration helper.
//!
//! This binary intentionally does not bootstrap mana-lite. It loads one depth
//! model, samples explicit frame-global polygons, and persists a resumable TOML
//! profile that the runtime may consume later.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use image::GenericImageView;
use mana_lite::config::{CropType, load_app_config, load_model_catalog};
use mana_lite::depth_map::DepthFrame;
use mana_lite::{SurfaceAccumulator, SurfaceCalibration, SurfaceLayer, polygon_stats};
use ultralytics_inference::{InferenceConfig, YOLOModel};

#[derive(Debug, Default)]
struct Options {
    config: PathBuf,
    session: Option<PathBuf>,
    promote: Option<PathBuf>,
    model: Option<PathBuf>,
    model_key: String,
    images: Vec<PathBuf>,
    layer: Option<SurfaceLayer>,
    zone: Option<String>,
    polygon: Option<Vec<[f32; 2]>>,
    roi: Option<[u32; 4]>,
    imgsz: Option<u32>,
    half: bool,
    min_valid_ratio: f32,
    tolerance: f32,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_args(std::env::args().skip(1).collect())?;
    let session_path = options.session.as_deref().ok_or("--session is required")?;

    if let Some(destination) = options.promote.as_deref() {
        promote(session_path, destination)?;
        return Ok(());
    }

    let layer = options.layer.ok_or("--layer is required")?;
    let zone = options.zone.ok_or("--zone is required")?;
    let polygon = options.polygon.ok_or("--polygon is required")?;
    if options.images.is_empty() {
        return Err("at least one --image is required".into());
    }

    let (model_path, configured_roi) = if let Some(model_path) = options.model {
        (model_path, None)
    } else {
        let app_config = load_app_config(&options.config)?;
        let catalog = load_model_catalog(&app_config.inference.model_catalog)?;
        let entry = catalog
            .models
            .get(&options.model_key)
            .ok_or_else(|| format!("model '{}' is absent from the catalog", options.model_key))?;
        let configured_roi = entry
            .crop
            .as_ref()
            .filter(|crop| crop.crop_type == CropType::Static)
            .and_then(|crop| crop.region);
        (entry.path.clone(), configured_roi)
    };

    let first_image = image::open(&options.images[0])?;
    let (frame_width, frame_height) = first_image.dimensions();
    let roi = options
        .roi
        .or(configured_roi)
        .unwrap_or([0, 0, frame_width, frame_height]);
    let fingerprint = model_fingerprint(&model_path)?;
    let mut calibration = load_or_create_session(
        session_path,
        &options.model_key,
        Some(&fingerprint),
        frame_width,
        frame_height,
        roi,
    )?;

    let mut model_config = InferenceConfig::default();
    if let Some([x1, y1, x2, y2]) = options.roi.or(configured_roi) {
        model_config = model_config.with_roi(x1, y1, x2, y2);
    }
    if let Some(size) = options.imgsz {
        model_config = model_config.with_imgsz(size as usize, size as usize);
    }
    if options.half {
        model_config = model_config.with_half(true);
    }
    model_config = model_config.with_save(false);
    let mut model = YOLOModel::load_with_config(&model_path, model_config)?;
    let mut accumulator = SurfaceAccumulator::default();

    for image_path in &options.images {
        let image = image::open(image_path)?;
        if image.dimensions() != (frame_width, frame_height) {
            return Err(format!(
                "image {} has dimensions {:?}, expected {}x{}",
                image_path.display(),
                image.dimensions(),
                frame_width,
                frame_height
            )
            .into());
        }
        let results = model.predict_image(&image, image_path.to_string_lossy().into_owned())?;
        let result = results.first().ok_or("depth model returned no result")?;
        let depth = result
            .depth
            .as_ref()
            .ok_or("depth model returned no depth map")?;
        let depth = DepthFrame::from_ultralytics(depth.clone());
        let actual_roi = result
            .roi
            .map(|(x1, y1, x2, y2)| [x1, y1, x2, y2])
            .unwrap_or(roi);
        if actual_roi != roi {
            return Err(format!(
                "depth result ROI {actual_roi:?} differs from session ROI {roi:?}"
            )
            .into());
        }
        let polygon_refs = vec![polygon.as_slice()];
        let stats = polygon_stats(
            &depth,
            actual_roi,
            &polygon_refs,
            frame_width,
            frame_height,
            None,
        )
        .ok_or_else(|| {
            format!(
                "zone {zone} has no sampled pixels in image {}",
                image_path.display()
            )
        })?;
        accumulator.push(&stats, options.min_valid_ratio);
    }

    let surface_zone = accumulator
        .finish(zone.clone(), polygon, options.tolerance)
        .ok_or_else(|| format!("zone {zone} has no valid calibration frames"))?;
    calibration.upsert_zone(layer, surface_zone)?;
    calibration.validate()?;
    write_atomic(session_path, &calibration)?;
    println!(
        "calibration_updated session={} layer={} zone={} samples={}",
        session_path.display(),
        layer.as_str(),
        zone,
        options.images.len()
    );
    Ok(())
}

fn load_or_create_session(
    path: &Path,
    model_key: &str,
    model_fingerprint: Option<&str>,
    frame_width: u32,
    frame_height: u32,
    roi: [u32; 4],
) -> Result<SurfaceCalibration, Box<dyn Error>> {
    if path.exists() {
        let content = fs::read_to_string(path)?;
        let calibration: SurfaceCalibration = toml::from_str(&content)?;
        calibration.validate()?;
        if calibration.model_key != model_key
            || calibration.frame_width != frame_width
            || calibration.frame_height != frame_height
            || calibration.roi != roi
        {
            return Err(format!(
                "session context differs: model={} frame={}x{} roi={:?}, requested model={} frame={}x{} roi={:?}",
                calibration.model_key,
                calibration.frame_width,
                calibration.frame_height,
                calibration.roi,
                model_key,
                frame_width,
                frame_height,
                roi
            )
            .into());
        }
        if calibration.model_fingerprint.as_deref() != model_fingerprint {
            return Err("session model fingerprint differs; start a new session".into());
        }
        return Ok(calibration);
    }
    Ok(SurfaceCalibration::new(
        model_key.to_string(),
        model_fingerprint.map(str::to_string),
        frame_width,
        frame_height,
        roi,
    ))
}

fn promote(session: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    let content = fs::read_to_string(session)?;
    let calibration: SurfaceCalibration = toml::from_str(&content)?;
    calibration.validate()?;
    write_atomic(destination, &calibration)?;
    println!(
        "calibration_promoted session={} destination={}",
        session.display(),
        destination.display()
    );
    Ok(())
}

fn write_atomic(path: &Path, calibration: &SurfaceCalibration) -> Result<(), Box<dyn Error>> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or("calibration path must contain a file name")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let encoded = toml::to_string_pretty(calibration)?;
    fs::write(&temporary, encoded)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn model_fingerprint(path: &Path) -> Result<String, Box<dyn Error>> {
    let metadata = fs::metadata(path)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs());
    Ok(format!("bytes:{}:mtime:{}", metadata.len(), modified))
}

fn parse_args(args: Vec<String>) -> Result<Options, Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        std::process::exit(0);
    }
    let mut options = Options {
        config: PathBuf::from("config/mana.toml"),
        model_key: "depth-standard".into(),
        min_valid_ratio: 0.5,
        ..Options::default()
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--config" => {
                options.config = PathBuf::from(next(&args, &mut index, "--config")?);
            }
            "--session" => {
                options.session = Some(PathBuf::from(next(&args, &mut index, "--session")?))
            }
            "--promote" => {
                options.promote = Some(PathBuf::from(next(&args, &mut index, "--promote")?))
            }
            "--model" => options.model = Some(PathBuf::from(next(&args, &mut index, "--model")?)),
            "--model-key" => options.model_key = next(&args, &mut index, "--model-key")?,
            "--image" => options
                .images
                .push(PathBuf::from(next(&args, &mut index, "--image")?)),
            "--layer" => {
                let value = next(&args, &mut index, "--layer")?;
                options.layer = SurfaceLayer::parse(&value);
                if options.layer.is_none() {
                    return Err("--layer must be 'bed' or 'floor'".into());
                }
            }
            "--zone" => options.zone = Some(next(&args, &mut index, "--zone")?),
            "--polygon" => {
                options.polygon = Some(parse_polygon(&next(&args, &mut index, "--polygon")?)?);
            }
            "--roi" => options.roi = Some(parse_rect(&args, &mut index, "--roi")?),
            "--imgsz" => {
                options.imgsz = Some(parse_positive::<u32>(
                    &next(&args, &mut index, "--imgsz")?,
                    "--imgsz",
                )?);
            }
            "--half" => options.half = true,
            "--min-valid-ratio" => {
                options.min_valid_ratio = parse_fraction(
                    &next(&args, &mut index, "--min-valid-ratio")?,
                    "--min-valid-ratio",
                )?;
            }
            "--tolerance" => {
                options.tolerance =
                    parse_non_negative(&next(&args, &mut index, "--tolerance")?, "--tolerance")?;
            }
            unknown => return Err(format!("unknown option: {unknown}").into()),
        }
        index += 1;
    }
    Ok(options)
}

fn next(args: &[String], index: &mut usize, option: &str) -> Result<String, Box<dyn Error>> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("{option} requires a value").into())
}

fn parse_rect(
    args: &[String],
    index: &mut usize,
    option: &str,
) -> Result<[u32; 4], Box<dyn Error>> {
    let values = [
        parse_positive::<u32>(&next(args, index, option)?, option)?,
        parse_positive::<u32>(&next(args, index, option)?, option)?,
        parse_positive::<u32>(&next(args, index, option)?, option)?,
        parse_positive::<u32>(&next(args, index, option)?, option)?,
    ];
    if values[2] <= values[0] || values[3] <= values[1] {
        return Err(format!("{option} must have positive width and height").into());
    }
    Ok(values)
}

fn parse_polygon(value: &str) -> Result<Vec<[f32; 2]>, Box<dyn Error>> {
    let polygon: Result<Vec<[f32; 2]>, Box<dyn Error>> = value
        .split(';')
        .map(|point| {
            let mut coordinates = point.split(',');
            let x = coordinates
                .next()
                .ok_or("polygon point needs x")?
                .parse::<f32>()?;
            let y = coordinates
                .next()
                .ok_or("polygon point needs y")?
                .parse::<f32>()?;
            if coordinates.next().is_some() {
                return Err("polygon point has too many coordinates".into());
            }
            Ok([x, y])
        })
        .collect();
    let polygon = polygon?;
    if polygon.len() < 3 {
        return Err("--polygon needs at least three x,y points separated by ';'".into());
    }
    Ok(polygon)
}

fn parse_positive<T: std::str::FromStr>(value: &str, option: &str) -> Result<T, Box<dyn Error>>
where
    T::Err: std::fmt::Display,
{
    let parsed = value
        .parse::<T>()
        .map_err(|error| format!("{option} has invalid value {value}: {error}"))?;
    Ok(parsed)
}

fn parse_fraction(value: &str, option: &str) -> Result<f32, Box<dyn Error>> {
    let parsed = parse_non_negative(value, option)?;
    if parsed > 1.0 {
        return Err(format!("{option} must be between 0 and 1").into());
    }
    Ok(parsed)
}

fn parse_non_negative(value: &str, option: &str) -> Result<f32, Box<dyn Error>> {
    let parsed = value
        .parse::<f32>()
        .map_err(|error| format!("{option} has invalid value {value}: {error}"))?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(format!("{option} must be finite and non-negative").into());
    }
    Ok(parsed)
}

fn print_usage() {
    println!(
        "deep-calib --session PATH --image IMAGE [--image IMAGE ...] \\
         --layer bed|floor --zone NAME --polygon x,y;x,y;x,y \\
         [--config config/mana.toml] [--model MODEL] [--roi x1 y1 x2 y2] \\
         [--imgsz N] [--half] \\
         [--min-valid-ratio F] [--tolerance F]\n\n         deep-calib --session PATH --promote PATH"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use mana_lite::SurfaceZone;

    #[test]
    fn polygon_parser_accepts_frame_global_vertices() {
        let polygon = parse_polygon("1,2;10,2;10,20;1,20").expect("polygon");
        assert_eq!(
            polygon,
            vec![[1.0, 2.0], [10.0, 2.0], [10.0, 20.0], [1.0, 20.0]]
        );
    }

    #[test]
    fn session_round_trip_is_valid() {
        let path =
            std::env::temp_dir().join(format!("mana-deep-calib-{}-{}.toml", std::process::id(), 1));
        let mut calibration = SurfaceCalibration::new(
            "depth-standard".into(),
            Some("test-fingerprint".into()),
            100,
            100,
            [0, 0, 100, 100],
        );
        calibration
            .upsert_zone(
                SurfaceLayer::Bed,
                SurfaceZone {
                    name: "head".into(),
                    polygon: vec![[1.0, 1.0], [40.0, 1.0], [40.0, 40.0]],
                    median_depth: 2.0,
                    p10_depth: 1.9,
                    p90_depth: 2.1,
                    mad_depth: 0.01,
                    valid_ratio: 1.0,
                    frame_samples: 3,
                    valid_frames: 3,
                    tolerance: 0.1,
                },
            )
            .expect("zone");
        write_atomic(&path, &calibration).expect("write session");
        let encoded = fs::read_to_string(&path).expect("read session");
        let decoded: SurfaceCalibration = toml::from_str(&encoded).expect("decode session");
        decoded.validate().expect("valid session");
        assert_eq!(decoded, calibration);
        fs::remove_file(path).expect("remove session");
    }
}
