//! Stage 5: wire observers and sinks (log, ingest, metrics, viz, snapshots).

use crate::config::AppConfig;
use crate::error::Result;
use crate::ingest::{FrameReader, IngestEngine};
use crate::logger::{Event, JsonlLevel, LogManager, LogSink};
use crate::metrics::MetricsEngine;
use crate::pipeline::PipelineState;
use crate::snapshot::{FrameDecoder, SnapshotSaver};
#[cfg(feature = "rerun")]
use crate::viz::VizBridge;

use super::super::VERSION;
use super::perception::PerceptionEngines;
use super::validate::ValidatedBootstrap;

pub(super) struct WiredObservers<R: FrameReader> {
    pub(super) log: Box<dyn LogSink>,
    pub(super) ingest: IngestEngine<R>,
    pub(super) metrics: MetricsEngine,
    pub(super) decoder: FrameDecoder,
    pub(super) snapshots: SnapshotSaver,
    pub(super) state: PipelineState,
    #[cfg(feature = "rerun")]
    pub(super) viz: VizBridge,
}

/// Open the JSONL sink and emit startup meta events (before model load).
pub(super) fn open_log_sink(
    config: &AppConfig,
    config_path: &std::path::Path,
    validated: &ValidatedBootstrap,
) -> Result<Box<dyn LogSink>> {
    let jsonl_level = JsonlLevel::from_str(&config.output.jsonl_level);
    let mut log = if let Some(ref dir) = config.output.save_dir {
        LogManager::rotating(dir.clone(), &config.output.rotate, jsonl_level)?
    } else {
        LogManager::new(jsonl_level)
    };
    log.set_jsonl_config(validated.metrics_log.metrics.jsonl.clone());
    let mut log: Box<dyn LogSink> = Box::new(log);

    log::info!(
        "mana-lite v{VERSION} starting (output {})",
        config.output.format
    );
    log::info!("source: {}", config.source.url);
    log.emit(Event::meta_startup(
        VERSION,
        &config_path.display().to_string(),
    ));
    log.emit(Event::meta_model_loaded(
        &validated.primary_model,
        &validated.default_model_path.display().to_string(),
        validated.default_model_task.as_str(),
        0,
    ));
    Ok(log)
}

/// Wire ingest, metrics, decoder, snapshots, viz, and pipeline state.
pub(super) fn wire_observers_and_sinks<R: FrameReader>(
    config: &AppConfig,
    reader: R,
    validated: &ValidatedBootstrap,
    perception: &mut PerceptionEngines,
    log: Box<dyn LogSink>,
) -> Result<WiredObservers<R>> {
    let ingest = IngestEngine::new(reader);

    let metrics = MetricsEngine::new(
        validated.metrics_log.metrics.report_interval_s,
        config.health.cycle_budget_ms,
    );
    let decoder = FrameDecoder::new()?;
    let snapshots = SnapshotSaver::new(
        config.output.snapshot_dir.clone(),
        config.output.snapshot_verbose,
    )?;

    #[cfg(feature = "rerun")]
    let viz = {
        let fixed_rois = std::mem::take(&mut perception.fixed_rois);
        if config.viz.enabled {
            log::info!(
                "viz: will connect to rerun at {} when viewer opens",
                config.viz.rerun_addr
            );
            VizBridge::new(
                &config.viz.rerun_addr,
                &validated.viz_data.viz.send,
                &validated.rerun_blueprint.rerun,
                fixed_rois,
                &perception.models,
            )
        } else {
            VizBridge::disabled()
        }
    };
    #[cfg(not(feature = "rerun"))]
    {
        let _ = (&validated.viz_data, &validated.rerun_blueprint);
        let _ = perception;
    }

    let state = PipelineState::new(
        validated.metrics_log.metrics.text.clone(),
        config.health.panic_window_cycles,
        config.health.max_panics_in_window,
    );

    Ok(WiredObservers {
        log,
        ingest,
        metrics,
        decoder,
        snapshots,
        state,
        #[cfg(feature = "rerun")]
        viz,
    })
}
