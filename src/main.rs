mod config;
mod error;
mod logger;

use config::*;
use error::*;
use logger::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

static VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config_path = parse_args()?;
    let app_config = load_app_config(&config_path)?;

    let model_catalog = load_model_catalog(&app_config.inference.model_catalog)?;
    let zones = app_config.inference.zones_file.as_ref()
        .map(|p| load_zone_catalog(p))
        .transpose()?;
    let fsm = app_config.inference.fsm_file.as_ref()
        .map(|p| load_fsm_catalog(p))
        .transpose()?;

    let default_model = model_catalog.models.get(&app_config.inference.default_model)
        .ok_or_else(|| ManaError::ModelNotFound(app_config.inference.default_model.clone()))?;
    log::info!("default model: {} ({})", app_config.inference.default_model, default_model.path.display());

    if let Some(ref z) = zones {
        log::info!("zones loaded: {} zones", z.zones.len());
    }
    if let Some(ref f) = fsm {
        log::info!("fsm loaded: {} states, {} transitions", f.fsm.states.len(), f.fsm.transitions.len());
        let errors = validate_fsm(f, &model_catalog, &zones);
        for e in &errors {
            log::error!("fsm validation: {e}");
        }
        if !errors.is_empty() {
            return Err(ManaError::FsmGuardError(format!("{} FSM validation errors", errors.len())));
        }
    }

    let mut log = if let Some(ref dir) = app_config.output.save_dir {
        std::fs::create_dir_all(dir)?;
        let filename = dir.join(format!("mana-{}.jsonl", chrono::Utc::now().format("%Y%m%dT%H%M%S")));
        Logger::with_file(&filename.display().to_string())?
    } else {
        Logger::new()
    };

    log::info!("mana-lite v{VERSION} starting");
    log::info!("source: {}", app_config.source.url);
    log.emit(Event::meta_startup(VERSION, &config_path.display().to_string()));
    log.emit(Event::meta_model_loaded(
        &app_config.inference.default_model,
        &default_model.path.display().to_string(),
        &default_model.task,
        0,
    ));

    let mut frame_count: u64 = 0;
    let mut cycle_start = Instant::now();
    #[allow(unused_assignments)]
    let mut last_frame_at = Instant::now();
    #[allow(unused_variables)]
    let panic_count: u32 = 0;
    let mut current_state: Option<String> = fsm.as_ref().map(|f| f.fsm.initial.clone());

    loop {
        let cycle_us = cycle_start.elapsed().as_micros() as u64;
        cycle_start = Instant::now();
        log.set_frame(frame_count);

        // PHASE 1: TIMERS (no-op until fsm.rs)

        // PHASE 2: EVALUATE (no-op until inference pipeline)

        // PHASE 3: INGEST (stub)
        {
            frame_count += 1;
            last_frame_at = Instant::now();
            log.emit(Event::frame_ingest(frame_count, true, 0));
        }

        // PHASE 4: INFER (stub)
        if frame_count == 1 && current_state.is_some() {
            log.emit(Event::fsm_transition("idle", "watching", "bed_occupied", 0));
            current_state = Some("watching".into());
        }

        // PHASE 5: ZONES (no-op until inference)

        // PHASE 6: FSM (no-op until fsm.rs)

        // PHASE 7: PUBLISH
        log.flush();

        // Health
        let stale_ms = last_frame_at.elapsed().as_millis() as u64;
        if stale_ms > app_config.health.data_stale_ms {
            log.emit(Event::health_blind(stale_ms));
            log.flush();
        } else if stale_ms > app_config.health.data_stale_ms / 2 {
            log.emit(Event::health_stale("ingest", stale_ms));
        }

        if frame_count.rem_euclid(app_config.health.heartbeat_every_n_cycles) == 0 {
            log.emit(Event::health_heartbeat(frame_count, "publish", cycle_us));
        }

        if frame_count >= 5 && app_config.source.url.contains("demo") {
            log::info!("demo: exiting after 5 frames");
            break;
        }

        // Stub back-pressure — when real RTSP is connected this is unnecessary
        if app_config.source.url.contains("demo") {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[allow(unreachable_code)]
    log.shutdown("loop_exit")?;
    Ok(())
}

fn parse_args() -> Result<PathBuf> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        println!("mana-lite v{VERSION}");
        std::process::exit(0);
    }

    if args.len() >= 3 && args[1] == "--config" {
        return Ok(PathBuf::from(&args[2]));
    }

    if args.len() >= 2 && !args[1].starts_with('-') {
        return Ok(PathBuf::from(&args[1]));
    }

    Err(ManaError::Config(ConfigError::InvalidValue {
        field: "args".into(),
        msg: "Usage: mana-lite --config <mana.toml>".into(),
    }))
}
