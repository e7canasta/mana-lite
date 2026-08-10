//! Application bootstrap: load catalogs, validate, wire engines.

mod catalogs;
mod control;
mod observers;
mod perception;
mod validate;

use crate::config::AppConfig;
use crate::detection::DetectionConsolidator;
use crate::error::Result;
use crate::face_dwell::FaceDwellLogStrategy;
use crate::ingest::{FrameReader, RetinaReader};

use self::catalogs::load_catalogs;
use self::control::build_control_state;
use self::observers::{open_log_sink, wire_observers_and_sinks};
use self::perception::build_perception_engines;
use self::validate::{validate_app_config, validate_bootstrap};

use super::{App, FanoutObserver};

impl App<RetinaReader> {
    pub async fn bootstrap(config: &AppConfig, config_path: &std::path::Path) -> Result<Self> {
        let reader = RetinaReader::connect(
            &config.source.url,
            config.source.username.as_deref(),
            config.source.password.as_deref(),
            &config.source.transport,
            &config.ingest,
        )
        .await?;
        Self::bootstrap_with_reader(config, config_path, reader).await
    }
}

impl<R: FrameReader> App<R> {
    pub async fn bootstrap_with_reader(
        config: &AppConfig,
        config_path: &std::path::Path,
        reader: R,
    ) -> Result<Self> {
        // Stage order preserves side effects: config checks → catalogs →
        // validate → log startup → perception → remaining sinks → control → App.
        validate_app_config(config)?;
        let catalogs = load_catalogs(config)?;
        let mut validated = validate_bootstrap(catalogs)?;
        let log = open_log_sink(config, config_path, &validated)?;
        let mut perception = build_perception_engines(config, &validated)?;
        let wired = wire_observers_and_sinks(config, reader, &validated, &mut perception, log)?;
        let control = build_control_state(config, validated.fsm_program.take(), &mut perception);
        Ok(assemble_app(config, validated, perception, control, wired))
    }
}

/// Stage 6: assemble the [`App`] from stage outputs.
fn assemble_app<R: FrameReader>(
    config: &AppConfig,
    validated: validate::ValidatedBootstrap,
    perception: perception::PerceptionEngines,
    control: control::ControlBundle,
    wired: observers::WiredObservers<R>,
) -> App<R> {
    App {
        infer: perception.infer,
        primary_model: validated.primary_model,
        models: perception.models,
        control: control.control,
        scan_timeline: control.scan_timeline,
        cascade: perception.cascade,
        detection_consolidator: DetectionConsolidator::new(
            config.detection.face_component_coverage,
            config.detection.face_max_center_y_ratio,
            config.detection.same_class_iou,
        ),
        ingest: wired.ingest,
        metrics: wired.metrics,
        depth_context_roi: perception.depth_context_roi,
        depth_rules: validated.depth_rules,
        decoder: wired.decoder,
        snapshots: wired.snapshots,
        #[cfg(feature = "rerun")]
        observer: FanoutObserver::new(wired.viz, wired.log),
        #[cfg(not(feature = "rerun"))]
        observer: FanoutObserver::new(wired.log),
        state: wired.state,
        boot_wall: control.boot_wall,
        boot_instant: control.boot_instant,
        crop_frames_pending: Vec::new(),
        face_dwell_logger: FaceDwellLogStrategy,
        control_image: mana_control::ProcessImage::empty(),
    }
}
