//! El lazo de control y su hospedaje: bootstrap, superloop, bordes.
//!
//! Después de la Fase 3, `App` **no es el pipeline**: es el lazo de control y
//! nada más. Decode e inferencia viven en [`perception`], en su propio hilo.
//!
//! El trabajo que hace este lazo está acotado por construcción —leer un slot,
//! avanzar el reloj, evaluar, emitir— y ninguno de sus bordes puede bloquearlo.
//! Ése es el invariante de `HANDOFF.md`: *un PLC cuyo dispositivo de campo es
//! una cámara; el programa corre a cadencia fija aunque el campo esté muerto.*

mod body_parts;
mod bootstrap;
mod cross_model_validation;
mod cycle;
mod deadline;
mod face_pose;
mod inference;
mod ingestion;
mod observer;
pub mod perception;
mod record;
#[cfg(feature = "rerun")]
pub mod viz_relay;

pub use cycle::{CycleContext, FrameSize};
pub use observer::PerceptionObserver;

use self::deadline::ScanDeadline;
use self::perception::{ControlDirective, PerceptionOutput};
use crate::config::AppConfig;
use crate::error::Result;
use crate::face_dwell::FaceDwellLogStrategy;
use crate::ingest::RawKeyframe;
use crate::logger::{scene_events_to_log, Event, LogSink};
use crate::metrics::MetricsEngine;
use crate::pipeline::PipelineState;
use crate::scan::{ControlStamp, ControlState, ScanTimeline, SceneEvent};
use crate::slot::Slot;
#[cfg(feature = "rerun")]
use crate::snapshot::FrameBuffer;
#[cfg(feature = "rerun")]
use mana_media::RawFrameV1;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::signal::unix::{signal, SignalKind};

pub(crate) static VERSION: &str = env!("CARGO_PKG_VERSION");

/// **Ya no es genérico sobre el reader.** El genérico existía porque `App`
/// era dueño de la ingesta; desde la Fase 4 la ingesta es una task con su
/// propio dueño y el lazo de control sólo conoce el slot por el que llegan los
/// keyframes. El parámetro de tipo sobrevivía a su motivo.
pub struct App {
    pub(crate) control: ControlState,
    pub(crate) scan_timeline: ScanTimeline,
    /// Compartido con percepción. Secciones críticas de microsegundos sobre
    /// contadores; ver el invariante en [`perception::PerceptionStage::lock_metrics`].
    pub(crate) metrics: Arc<Mutex<MetricsEngine>>,
    /// Dueño único del JSONL. Percepción manda sus eventos por cola para que
    /// no haya dos hilos intercalando líneas en el mismo archivo.
    pub(crate) log: Box<dyn LogSink>,
    pub(crate) state: PipelineState,
    /// El ancla de pared se fue con percepción: el lazo de control no fecha
    /// nada contra el reloj de pared — su tiempo sale de `ScanTimeline`.
    pub(crate) boot_instant: Instant,
    pub(crate) face_dwell_logger: FaceDwellLogStrategy,
    pub(crate) control_image: mana_control::ProcessImage,
    pub(crate) ports: ControlPorts,
    /// Sólo para el apagado ordenado: se cierra el slot de entrada y se espera.
    pub(crate) perception: Option<std::thread::JoinHandle<()>>,
    pub(crate) ingestion: tokio::task::JoinHandle<()>,
    /// Para no repetir el aviso en cada tick una vez que la ingesta murió.
    pub(crate) ingestion_death_reported: bool,
    /// El lazo no dibuja: sólo necesita poder cerrar el slot al apagar y leer
    /// cuántos lotes se pisaron.
    #[cfg(feature = "rerun")]
    pub(crate) viz_batches: Arc<Slot<viz_relay::VizBatch>>,
    #[cfg(feature = "rerun")]
    pub(crate) viz_thread: Option<std::thread::JoinHandle<()>>,
}

/// Los bordes del lazo. Ninguno puede hacerlo esperar.
pub(crate) struct ControlPorts {
    /// Salida hacia percepción. Muestra: si percepción sigue ocupada con el
    /// keyframe anterior, el nuevo lo pisa y se cuenta.
    pub(crate) keyframes: Arc<Slot<RawKeyframe>>,
    /// Entrada de evidencia. `take()` nunca bloquea: si no hay imagen nueva, el
    /// lazo sigue con la anterior y su edad crece — que es exactamente cómo
    /// control tiene que degradar.
    pub(crate) images: Arc<Slot<PerceptionOutput>>,
    /// Realimentación hacia percepción: qué modelos correr y con qué tracks.
    pub(crate) directives: Arc<Slot<ControlDirective>>,
    /// Eventos de percepción. Cola: no se pierde ninguno.
    pub(crate) events: Receiver<Vec<Event>>,
}

pub(crate) struct CropFrameQueue {
    pub(crate) model: String,
    pub(crate) crop_frame: Option<crate::infer::CropFrameInfo>,
}

impl App {
    pub async fn run(&mut self, config: &AppConfig) -> Result<()> {
        let mut term = signal(SignalKind::terminate())?;
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        let mut shutdown_reason = "signal";
        let scan_config = config.scan.to_scan_config();
        // Anclado en `boot_instant`, el mismo origen que `scan_timeline`: los
        // dos relojes tienen que recorrer la misma grilla o el tiempo de
        // control se corre respecto del de pared. Ver `deadline::ScanDeadline`.
        let mut scan_deadline = ScanDeadline::anchored_at(self.boot_instant, scan_config.period());

        // El `select!` ya no arbitra entre trabajo y reloj: sólo entre el
        // reloj y las señales de apagado. El lazo de control es un
        // temporizador puro, que es exactamente lo que un PLC es.
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(scan_deadline.next().into()) => {
                    let now = Instant::now();
                    // Cuánto después de su vencimiento arranca este scan.
                    self.lock_metrics().tick_scan_deadline(scan_deadline.arrive(now));
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        self.scan_tick(config, now);
                    }));
                    match result {
                        Ok(()) => {
                            let _ = self.state.on_ok();
                        }
                        Err(e) => {
                            log::error!("scan processing panicked: {}", panic_message(&e));
                            // No reanudar desde estado roto: blind forzado, y
                            // que el ciclo siguiente reconstruya desde idle.
                            if let Some(fsm) = self.control.fsm_engine.as_mut() {
                                fsm.force_safe_state(now);
                            }
                            if self.state.on_panic() {
                                shutdown_reason = "panic";
                                log::error!(
                                    "panic density in the last {} cycles exceeded {} — exiting",
                                    config.health.panic_window_cycles,
                                    config.health.max_panics_in_window
                                );
                                break;
                            }
                        }
                    }
                }
                _ = term.recv() => break,
                _ = &mut ctrl_c => break,
            }
        }

        self.shutdown(shutdown_reason);
        Ok(())
    }

    /// Un tick del programa. Todo lo que hace está acotado: ningún paso puede
    /// esperar a la red, al disco de otra etapa ni a un modelo.
    fn scan_tick(&mut self, _config: &AppConfig, now: Instant) {
        self.lock_metrics().tick_cycle_at(now);
        self.check_ingestion_alive();
        self.drain_perception_events();
        self.install_fresh_evidence(now);
        self.drain_slot_drops();

        // The first tick runs at the timeline origin; every later tick advances
        // one period first, so `timeline.now()` is the instant for this scan.
        if self.control.scan_seq != 0 {
            self.scan_timeline.advance();
        }
        let scene_events =
            crate::scan::scan(&mut self.control, &self.control_image, &self.scan_timeline);
        // La edad de lo que este scan acaba de usar para decidir, contra el
        // reloj de control. Es el número clínico del sistema: todo lo demás que
        // se mide es salud del motor.
        let control_now = self.scan_timeline.now().as_instant();
        if self.control_image.observations.is_some() {
            let age_ms = self.control_image.observations_age_ms(control_now);
            self.lock_metrics().tick_evidence_age(age_ms);
        }
        self.control_image.measurement_pending = false;

        let mut last_stamp: Option<ControlStamp> = None;
        let mut occupancy = None;
        let mut fsm_state = None;
        for event in &scene_events {
            match event {
                SceneEvent::Occupancy {
                    state,
                    second_person,
                    signal,
                } => occupancy = Some((*state, *second_person, *signal)),
                SceneEvent::FsmState(state) => fsm_state = Some(state.clone()),
                SceneEvent::Presence { stamp, .. }
                | SceneEvent::Track { stamp, .. }
                | SceneEvent::Zone { stamp, .. }
                | SceneEvent::SceneSignals { stamp, .. } => {
                    last_stamp = Some(*stamp);
                }
                SceneEvent::EntityBoxes(_)
                | SceneEvent::FsmTransition(_)
                | SceneEvent::Health(_) => {}
            }
        }

        for event in scene_events_to_log(&scene_events) {
            self.log.emit(event);
        }

        // Face-dwell needs the full FsmSnapshot, which SceneEvent::FsmState does
        // not carry — emit from App-owned state after the scan batch.
        if let (Some(fsm), Some(stamp)) = (self.control.fsm_engine.as_ref(), last_stamp) {
            let snapshot = fsm.snapshot_at(now);
            self.log.emit(self.face_dwell_logger.keyframe_event(
                stamp,
                &self.control.signal_snapshot,
                &snapshot,
            ));
        }

        self.publish_directive(occupancy, fsm_state);

        if self.control.health.is_blind() {
            self.lock_metrics().tick_blind();
        }
        {
            // El `Arc` se clona para soltar el préstamo de `self` y poder
            // pasar el sink en la misma llamada.
            let metrics = Arc::clone(&self.metrics);
            let mut metrics = metrics
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.state.emit_metrics(self.log.as_mut(), &mut metrics);
        }
        self.log.flush();
    }

    /// Instala la evidencia más fresca que haya publicado percepción.
    ///
    /// Si no hay ninguna, **no pasa nada malo**: se sigue con la anterior y su
    /// edad crece. Control degrada por edad de la evidencia, no por ausencia de
    /// frame, así que una percepción muerta se convierte sola en `blind` sin
    /// que este lazo tenga que enterarse de nada.
    fn install_fresh_evidence(&mut self, now: Instant) {
        let Some(output) = self.ports.images.take() else {
            return;
        };
        self.control_image = output.image;
        // Sólo un keyframe que produjo imagen llega hasta acá, así que esto es
        // señal fresca de verdad — un decode roto no publica nada.
        self.state
            .mark_health_fresh(&mut self.control.health, self.log.as_mut(), now);
    }

    /// Vuelca al JSONL lo que percepción produjo desde el tick anterior.
    ///
    /// `try_iter` no bloquea: si percepción está a mitad de una inferencia, no
    /// hay nada en la cola y el lazo sigue.
    fn drain_perception_events(&mut self) {
        while let Ok(batch) = self.ports.events.try_recv() {
            for event in batch {
                self.log.emit(event);
            }
        }
    }

    /// Publica el estado con el que percepción resuelve su próximo keyframe.
    ///
    /// Es el brazo de vuelta del lazo cerrado: el FSM decide qué modelos correr
    /// y el tracker de dónde recortar. Va por slot porque es una **muestra** —
    /// si percepción no llegó a tomar la anterior, la vieja no le sirve.
    fn publish_directive(
        &mut self,
        occupancy: Option<(
            crate::occupancy::RoomCardinality,
            crate::occupancy::SecondPersonState,
            crate::occupancy::SignalValidity,
        )>,
        fsm_state: Option<String>,
    ) {
        let models = self
            .control
            .fsm_engine
            .as_ref()
            .map(crate::fsm::FsmEngine::current_models)
            .unwrap_or_default();
        let tracks = self
            .control
            .tracker
            .as_ref()
            .map_or_else(Vec::new, |tracker| {
                tracker.current_tracks().into_iter().cloned().collect()
            });
        self.ports.directives.put(ControlDirective {
            models,
            tracks,
            occupancy,
            fsm_state,
            urgent_requests: Vec::new(),
        });
    }

    /// Una ingesta muerta hace que el lazo se vaya a `blind` por edad de
    /// evidencia, que es la degradación correcta — pero `blind` sin causa es
    /// indistinguible de "se murió la cámara".
    ///
    /// La task no puede avisar por sí misma: un pánico dentro de `tokio::spawn`
    /// se traga en el `JoinHandle` y nadie lo mira. Así que lo mira el lazo,
    /// sin bloquearse, y lo dice una sola vez.
    fn check_ingestion_alive(&mut self) {
        if self.ingestion_death_reported || !self.ingestion.is_finished() {
            return;
        }
        self.ingestion_death_reported = true;
        log::error!("la task de ingesta terminó sola: no van a llegar más keyframes");
        self.log.emit(Event::health_stage_died("ingest"));
    }

    /// Lo pisado en los bordes que este lazo toca.
    ///
    /// Los contadores de transporte ya no se leen desde acá: los vuelca la
    /// task de ingesta, que es quien los produce. Lo que sigue siendo del lazo
    /// es cuánto se descartó en sus propios slots — un descarte silencioso es
    /// la misma clase de mentira que un knob que se ignora (ADR-034).
    fn drain_slot_drops(&mut self) {
        let dropped_keyframes = self.ports.keyframes.drain_overwritten();
        let dropped_images = self.ports.images.drain_overwritten();
        #[cfg(feature = "rerun")]
        let dropped_viz = self.viz_batches.drain_overwritten();
        #[cfg(not(feature = "rerun"))]
        let dropped_viz = 0;
        self.lock_metrics()
            .tick_slot_drops(dropped_keyframes, dropped_images, dropped_viz);
    }

    pub(crate) fn lock_metrics(&self) -> std::sync::MutexGuard<'_, MetricsEngine> {
        self.metrics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Apagado ordenado: se cierra la entrada de percepción para que su hilo
    /// salga del `take_blocking`, se lo espera, y recién ahí se cierra el log —
    /// así los últimos eventos que produjo llegan al archivo.
    fn shutdown(&mut self, reason: &str) {
        // Primero la ingesta: abortar es seguro porque el drenaje guarda el
        // keyframe en el engine y no en la pila del future. Después el slot,
        // para que percepción salga de su espera y termine lo que tenga.
        self.ingestion.abort();
        self.ports.keyframes.close();
        if let Some(handle) = self.perception.take() {
            if handle.join().is_err() {
                log::error!("el hilo de percepción terminó en pánico");
            }
        }
        // El visor último: puede estar peleando con un enlace saturado y nadie
        // más lo espera. Su slot se cierra cuando percepción ya no publica.
        #[cfg(feature = "rerun")]
        {
            self.viz_batches.close();
            if let Some(handle) = self.viz_thread.take() {
                if handle.join().is_err() {
                    log::error!("el hilo del visor terminó en pánico");
                }
            }
        }
        self.drain_perception_events();
        self.log.shutdown(reason);
    }
}

fn panic_message(e: &Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_else(|| "unknown panic".into())
}

#[cfg(feature = "rerun")]
pub(crate) fn raw_frame_header(fb: &FrameBuffer, frame_id: u64, timestamp_ns: i64) -> RawFrameV1 {
    RawFrameV1 {
        width: fb.w,
        height: fb.h,
        frame_id,
        timestamp_ns,
        ..Default::default()
    }
}

/// Mapea un instante monotónico del proceso a nanosegundos de pared anclados
/// en el bootstrap. El resultado nunca decrece con `now`, porque el monotónico
/// solo avanza: un salto de NTP no puede desordenar ni duplicar el JSONL.
/// Degradado: si el ancla de pared falla (epoch fuera de rango), se parte de 0
/// pero la propiedad de monotonía se conserva igual.
pub(crate) fn frame_timestamp_ns(
    boot_wall: &chrono::DateTime<chrono::Utc>,
    boot_instant: Instant,
    now: Instant,
) -> i64 {
    let boot_ns = boot_wall.timestamp_nanos_opt().unwrap_or(0);
    let since_boot = now.saturating_duration_since(boot_instant).as_nanos();
    let delta_ns = i64::try_from(since_boot).unwrap_or(i64::MAX);
    boot_ns.saturating_add(delta_ns)
}

#[cfg(test)]
mod tests;
