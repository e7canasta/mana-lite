//! Validate and inspect an offline posture-profile matrix.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use mana_lite::posture_analysis::{LoadedPostureProfiles, analyze_reports, load_profile_set};
use serde::Serialize;

const DEFAULT_MASTER: &str = "config/posture-analysis/l-640/master.toml";

#[derive(Debug, Default)]
struct Options {
    master: PathBuf,
    radio: Option<PathBuf>,
    parts: Option<PathBuf>,
    json: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct MatrixReport {
    schema_version: u32,
    engine: String,
    model_key: String,
    depth_semantics: String,
    semantic_min_total_score: f32,
    surface_calibration: CalibrationReport,
    profiles: Vec<ProfileReport>,
}

#[derive(Debug, Serialize)]
struct CalibrationReport {
    model_key: String,
    model_fingerprint: Option<String>,
    frame_width: u32,
    frame_height: u32,
    roi: [u32; 4],
    surface_spatial_padding_px: f32,
    surface_depth_padding_m: f32,
    bed_zones: usize,
    floor_zones: usize,
}

#[derive(Debug, Serialize)]
struct ProfileReport {
    posture_id: String,
    label: String,
    training_sample: String,
    base_posture: String,
    plane: String,
    feature_count: usize,
    min_observed_features: usize,
    min_observed_components: usize,
    allow_partial: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_args(std::env::args().skip(1).collect())?;
    let encoded = match (options.radio.as_deref(), options.parts.as_deref()) {
        (Some(radio), Some(parts)) => {
            let report = analyze_reports(&options.master, radio, parts)?;
            serde_json::to_string_pretty(&report)? + "\n"
        }
        (None, None) => {
            let loaded = load_profile_set(&options.master)?;
            let report = build_report(&loaded);
            serde_json::to_string_pretty(&report)? + "\n"
        }
        _ => return Err("--radio and --parts must be provided together".into()),
    };

    if let Some(path) = options.json {
        fs::write(path, &encoded)?;
    }
    print!("{encoded}");
    Ok(())
}

fn build_report(loaded: &LoadedPostureProfiles) -> MatrixReport {
    let calibration = &loaded.surface_calibration;
    MatrixReport {
        schema_version: loaded.master.schema_version,
        engine: loaded.master.engine.clone(),
        model_key: loaded.master.model_key.clone(),
        depth_semantics: loaded.master.depth_semantics.clone(),
        semantic_min_total_score: loaded.master.semantic_min_total_score,
        surface_calibration: CalibrationReport {
            model_key: calibration.model_key.clone(),
            model_fingerprint: calibration.model_fingerprint.clone(),
            frame_width: calibration.frame_width,
            frame_height: calibration.frame_height,
            roi: calibration.roi,
            surface_spatial_padding_px: loaded.master.surface_spatial_padding_px,
            surface_depth_padding_m: loaded.master.surface_depth_padding_m,
            bed_zones: calibration.bed.len(),
            floor_zones: calibration.floor.len(),
        },
        profiles: loaded
            .profiles
            .iter()
            .map(|profile| ProfileReport {
                posture_id: profile.posture_id.clone(),
                label: profile.label.clone(),
                training_sample: profile.training_sample.clone(),
                base_posture: profile.base_posture.clone(),
                plane: profile.plane.clone(),
                feature_count: profile.features.len(),
                min_observed_features: profile.policy.min_observed_features,
                min_observed_components: profile.policy.min_observed_components,
                allow_partial: profile.policy.allow_partial,
            })
            .collect(),
    }
}

fn parse_args(args: Vec<String>) -> Result<Options, Box<dyn Error>> {
    let mut options = Options {
        master: PathBuf::from(DEFAULT_MASTER),
        ..Options::default()
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--master" => {
                options.master = PathBuf::from(args.next().ok_or("--master requires a path")?);
            }
            "--radio" => {
                options.radio = Some(PathBuf::from(args.next().ok_or("--radio requires a path")?));
            }
            "--parts" => {
                options.parts = Some(PathBuf::from(args.next().ok_or("--parts requires a path")?));
            }
            "--json" => {
                options.json = Some(PathBuf::from(args.next().ok_or("--json requires a path")?));
            }
            other => return Err(format!("unknown argument '{other}'").into()),
        }
    }
    Ok(options)
}

fn print_usage() {
    println!(
        "posture-analysis --master PATH [--radio PATH --parts PATH] [--json PATH]\n\
         \nValidates a matrix or analyzes one offline report pair.\n\
         \nDefault master: {DEFAULT_MASTER}"
    );
}
