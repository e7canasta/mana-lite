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

use super::ingestion;
use super::perception as perception_stage;
use super::perception::{PerceptionConfig, PerceptionPorts, PerceptionSeed};
use super::{App, ControlPorts, PerceptionObserver};
use crate::slot::Slot;
use std::sync::{Arc, Mutex};

impl App {
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

impl App {
    /// El genérico queda acá y no en `App`: el reader es un detalle del
    /// arranque, no del lazo. Lo consume la task de ingesta y nadie más vuelve
    /// a nombrarlo.
    pub async fn bootstrap_with_reader<R: FrameReader>(
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
        let mut engines = build_perception_engines(config, &validated)?;
        let wired = wire_observers_and_sinks(config, reader, &validated, &mut engines, log)?;
        let control = build_control_state(config, validated.fsm_program.take(), &mut engines);
        assemble_app(config, validated, engines, control, wired)
    }
}

/// Stage 6: cablear los dos lados del lazo y levantar el hilo de percepción.
///
/// Acá se materializa la topología de ADR-033: los bordes se crean primero,
/// cada etapa se queda con su punta, y ninguna de las dos tiene una referencia
/// a la otra. Que percepción no pueda nombrar a control —ni al revés— es lo que
/// hace que el aislamiento sea estructural y no una regla que alguien recuerda.
fn assemble_app<R: FrameReader>(
    config: &AppConfig,
    validated: validate::ValidatedBootstrap,
    engines: perception::PerceptionEngines,
    control: control::ControlBundle,
    wired: observers::WiredObservers<R>,
) -> Result<App> {
    // El hilo del visor arranca primero y es dueño único del `VizBridge`.
    // Percepción sólo recibe un handle que encola dibujos: ni siquiera ella
    // puede quedar bloqueada por el enlace (ADR-035).
    #[cfg(feature = "rerun")]
    let viz_batches = Arc::new(Slot::new());
    #[cfg(feature = "rerun")]
    let viz_thread = super::viz_relay::spawn(wired.viz, Arc::clone(&viz_batches)).map_err(|e| {
        crate::error::ManaError::Config(crate::error::ConfigError::InvalidValue {
            field: "viz.thread".into(),
            msg: format!("no se pudo levantar el hilo del visor: {e}"),
        })
    })?;

    let keyframes = Arc::new(Slot::new());
    let images = Arc::new(Slot::new());
    let directives = Arc::new(Slot::new());
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let metrics = Arc::new(Mutex::new(wired.metrics));

    let seed = PerceptionSeed {
        infer: engines.infer,
        primary_model: validated.primary_model,
        cascade: engines.cascade,
        detection_consolidator: DetectionConsolidator::new(
            config.detection.face_component_coverage,
            config.detection.face_max_center_y_ratio,
            config.detection.same_class_iou,
        ),
        models: engines.models,
        depth_context_roi: engines.depth_context_roi,
        depth_rules: validated.depth_rules,
        snapshots: wired.snapshots,
        #[cfg(feature = "rerun")]
        observer: PerceptionObserver::new(super::viz_relay::VizHandle::new(Arc::clone(
            &viz_batches,
        ))),
        #[cfg(not(feature = "rerun"))]
        observer: PerceptionObserver::new(),
        metrics: Arc::clone(&metrics),
        boot_wall: control.boot_wall,
        boot_instant: control.boot_instant,
    };

    let handle = perception_stage::spawn(
        seed,
        PerceptionPorts {
            keyframes: Arc::clone(&keyframes),
            images: Arc::clone(&images),
            directives: Arc::clone(&directives),
            events: events_tx,
        },
        Arc::new(PerceptionConfig::from_app(config)),
    )?;

    // La ingesta arranca antes que el lazo: su primer keyframe puede llegar
    // mientras control todavía está armándose, y el slot lo espera.
    let ingestion = ingestion::spawn(wired.ingest, Arc::clone(&keyframes), Arc::clone(&metrics));

    Ok(App {
        control: control.control,
        scan_timeline: control.scan_timeline,
        metrics,
        log: wired.log,
        state: wired.state,
        boot_instant: control.boot_instant,
        face_dwell_logger: FaceDwellLogStrategy,
        control_image: mana_control::ProcessImage::empty(),
        ports: ControlPorts {
            keyframes,
            images,
            directives,
            events: events_rx,
        },
        perception: Some(handle),
        ingestion,
        ingestion_death_reported: false,
        #[cfg(feature = "rerun")]
        viz_batches,
        #[cfg(feature = "rerun")]
        viz_thread: Some(viz_thread),
    })
}
