//! La etapa de percepción: decode e inferencia, en su propio hilo.
//!
//! Corre a la tasa del keyframe (aperiódica, ~1 Hz) y con latencia variable
//! —221 ms de media, medido en la Fase 1—. Por eso vive fuera del lazo de
//! control: mientras esta etapa trabaja, el lazo tiene que poder ticar igual.
//!
//! Los dos bordes son de muestra y ninguno bloquea (ADR-034):
//!
//! ```text
//!                     Slot<ProcessImage>   evidencia fechada
//!         ┌──────────────────────────────────────────────┐
//!         │                                              ▼
//! [percepción]                                      [control]
//!         ▲                                              │
//!         └──────────────────────────────────────────────┘
//!               Slot<ControlDirective>   qué correr, dónde mirar
//! ```
//!
//! La realimentación no es un accidente: el FSM decide qué modelos corren y el
//! tracker decide dónde recortar. Percepción no es una fuente, es el actuador
//! de un lazo cerrado. Se toma como muestra porque una directiva vieja es
//! aceptable —hoy ya se lee hasta 200 ms desactualizada— y una espera no.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::cascade::CascadeScheduler;
use crate::config::AppConfig;
use crate::detection::{CropRect, DetectionConsolidator};
use crate::domain::ModelRegistry;
use crate::infer::InferEngine;
use crate::ingest::RawKeyframe;
use crate::logger::Event;
use crate::metrics::MetricsEngine;
use crate::slot::Slot;
use crate::snapshot::{FrameDecoder, SnapshotSaver};
use crate::track::Track;
use mana_control::ProcessImage;

use super::CropFrameQueue;
use super::observer::PerceptionObserver;
use crate::occupancy::{RoomCardinality, SecondPersonState, SignalValidity};

/// Lo que el lazo de control le dice a percepción: qué modelos correr y con qué
/// estado de seguimiento resolver los recortes de la cascada.
///
/// Es el borde de realimentación, y es una **muestra**: si control produjo dos
/// directivas antes de que percepción tomara una, la vieja no sirve para nada.
#[derive(Debug, Clone, Default)]
pub struct ControlDirective {
    /// `fsm.current_models()` ya filtrado por habilitación y tareas apagadas.
    pub models: Vec<String>,
    /// Estado del tracker con el que se resuelven la compuerta de presencia y
    /// el ROI de los modelos hijos de la cascada.
    pub tracks: Vec<Track>,
    /// Estado publicado por control **para que el visor lo dibuje**.
    ///
    /// El `VizBridge` tiene máquina de conexión y backoff propios, así que
    /// necesita dueño único, y ese dueño es esta etapa. Los tres dibujos que
    /// produce control viajan acá. Dibujar un estado de hasta un periodo de
    /// antigüedad en una vista de depuración es aceptable; darle al lazo de
    /// control un candado sobre el visor no lo es.
    ///
    /// Se revierte en la Fase 2: cuando el visor tenga su propio hilo
    /// (ADR-035), las dos etapas le mandan directo y la directiva vuelve a ser
    /// sólo `models` + `tracks`.
    pub occupancy: Option<(RoomCardinality, SecondPersonState, SignalValidity)>,
    pub fsm_state: Option<String>,
}

/// Salida de la etapa hacia el lazo de control.
pub struct PerceptionOutput {
    /// La imagen de proceso completa, con sus evidencias fechadas. Control la
    /// instala tal cual: no la mezcla con la anterior.
    pub image: ProcessImage,
    /// Instante del keyframe que la produjo, para que control pueda declarar
    /// señal fresca sin leer el reloj de pared por su cuenta.
    pub captured_at: Instant,
}

/// Estado propio de la etapa. **No es genérico sobre el reader**: percepción
/// nunca tocó la ingesta — el genérico venía arrastrado del `App` monolítico.
pub struct PerceptionStage {
    pub(crate) infer: InferEngine,
    pub(crate) primary_model: String,
    pub(crate) cascade: CascadeScheduler,
    pub(crate) detection_consolidator: DetectionConsolidator,
    pub(crate) models: ModelRegistry,
    pub(crate) depth_context_roi: Option<CropRect>,
    pub(crate) depth_rules: mana_control::DepthRules,
    pub(crate) snapshots: SnapshotSaver,
    pub(crate) crop_frames_pending: Vec<CropFrameQueue>,
    /// Construido **dentro** del hilo: el escalador de ffmpeg guarda un
    /// `*mut SwsContext` sin `impl Send`, así que un `FrameDecoder` no se puede
    /// mover a un hilo spawneado. Ver [`spawn`].
    pub(crate) decoder: FrameDecoder,
    /// El bridge de Rerun vive de este lado a propósito. Su contrapresión
    /// bloqueó el pipeline 41 s en la Fase 0; ahora lo que bloquea es una etapa
    /// de muestreo, y el lazo de control sigue ticando.
    pub(crate) observer: PerceptionObserver,
    pub(crate) metrics: Arc<Mutex<MetricsEngine>>,
    /// Contador de frames de percepción: la coordenada de la evidencia.
    pub(crate) frame_count: u64,
    pub(crate) last_keyframe_at: Instant,
    /// La imagen que se está armando para el keyframe en curso. Se publica
    /// entera al terminar; control nunca ve una a medio construir.
    pub(crate) image: ProcessImage,
    pub(crate) directive: ControlDirective,
    pub(crate) boot_wall: chrono::DateTime<chrono::Utc>,
    pub(crate) boot_instant: Instant,
}

/// Bordes que conectan la etapa con el resto del sistema.
pub struct PerceptionPorts {
    /// Entrada: keyframes de la ingesta. Bloqueante — sin keyframe, esta etapa
    /// no tiene nada que hacer, y dormir acá no acopla a nadie.
    pub keyframes: Arc<Slot<RawKeyframe>>,
    /// Salida de evidencia. Muestra: se pisa si control se atrasó.
    pub images: Arc<Slot<PerceptionOutput>>,
    /// Realimentación desde control. Muestra.
    pub directives: Arc<Slot<ControlDirective>>,
    /// Salida de eventos. **Cola, no slot**: perder una detección del JSONL es
    /// un bug de auditoría, no una degradación aceptable.
    pub events: Sender<Vec<Event>>,
}

/// Lo único que la etapa necesita de la configuración.
///
/// Se extrae en el bootstrap en vez de mandar el `AppConfig` entero. No es por
/// el clone: es que percepción resulta depender de **cuatro knobs**, y tenerlos
/// enumerados hace verificable que ninguna política clínica se cuele en esta
/// etapa. El `AppConfig` completo dejaría esa puerta abierta sin que se note.
#[derive(Debug, Clone)]
pub struct PerceptionConfig {
    pub snapshot: bool,
    pub infer: bool,
    /// Clase que abre la compuerta de la cascada y que se cuenta como persona.
    pub presence_class: String,
    pub disabled_tasks: Vec<String>,
}

impl PerceptionConfig {
    pub fn from_app(config: &AppConfig) -> Self {
        Self {
            snapshot: config.pipeline.snapshot,
            infer: config.pipeline.infer,
            presence_class: config.presence.class.clone(),
            disabled_tasks: config.inference.disabled_tasks.clone(),
        }
    }
}

/// Todo lo necesario para armar la etapa, **menos el decoder**.
///
/// Existe por una restricción real y verificada: `FrameDecoder` contiene un
/// `ffmpeg_next::software::scaling::Context`, que guarda un `*mut SwsContext`
/// sin `unsafe impl Send`. La etapa entera es por lo tanto `!Send` y no se
/// puede mover a un hilo spawneado; la semilla sí, y el decoder se construye
/// del otro lado.
///
/// Eso mueve un fallo de arranque a después de que `bootstrap` devolvió `Ok`,
/// así que el hilo **reporta hacia atrás** si no pudo construirlo: un ffmpeg
/// roto tiene que seguir siendo falla de arranque y no una sorpresa en runtime.
pub struct PerceptionSeed {
    pub infer: InferEngine,
    pub primary_model: String,
    pub cascade: CascadeScheduler,
    pub detection_consolidator: DetectionConsolidator,
    pub models: ModelRegistry,
    pub depth_context_roi: Option<CropRect>,
    pub depth_rules: mana_control::DepthRules,
    pub snapshots: SnapshotSaver,
    pub observer: PerceptionObserver,
    pub metrics: Arc<Mutex<MetricsEngine>>,
    pub boot_wall: chrono::DateTime<chrono::Utc>,
    pub boot_instant: Instant,
}

impl PerceptionSeed {
    pub(crate) fn grow(self, decoder: FrameDecoder) -> PerceptionStage {
        PerceptionStage {
            infer: self.infer,
            primary_model: self.primary_model,
            cascade: self.cascade,
            detection_consolidator: self.detection_consolidator,
            models: self.models,
            depth_context_roi: self.depth_context_roi,
            depth_rules: self.depth_rules,
            snapshots: self.snapshots,
            crop_frames_pending: Vec::new(),
            decoder,
            observer: self.observer,
            metrics: self.metrics,
            frame_count: 0,
            last_keyframe_at: self.boot_instant,
            image: ProcessImage::empty(),
            directive: ControlDirective::default(),
            boot_wall: self.boot_wall,
            boot_instant: self.boot_instant,
        }
    }
}

/// Levanta el hilo de percepción y espera a que confirme que arrancó.
///
/// La espera es sólo por la construcción del decoder —milisegundos, una vez—
/// para que un ffmpeg roto siga siendo un error de arranque. Después de eso el
/// hilo no vuelve a sincronizar con nadie.
pub fn spawn(
    seed: PerceptionSeed,
    ports: PerceptionPorts,
    config: Arc<PerceptionConfig>,
) -> crate::error::Result<std::thread::JoinHandle<()>> {
    let (started_tx, started_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let handle = std::thread::Builder::new()
        .name("perception".into())
        .spawn(move || {
            let decoder = match FrameDecoder::new() {
                Ok(decoder) => {
                    let _ = started_tx.send(Ok(()));
                    decoder
                }
                Err(e) => {
                    let _ = started_tx.send(Err(e.to_string()));
                    return;
                }
            };
            run(seed.grow(decoder), ports, &config);
        })
        .map_err(|e| {
            crate::error::ManaError::Config(crate::error::ConfigError::InvalidValue {
                field: "perception.thread".into(),
                msg: format!("no se pudo levantar el hilo de percepción: {e}"),
            })
        })?;

    match started_rx.recv() {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(msg)) => Err(crate::error::ManaError::Config(
            crate::error::ConfigError::InvalidValue {
                field: "perception.decoder".into(),
                msg: format!("el hilo de percepción no pudo construir el decoder: {msg}"),
            },
        )),
        Err(_) => Err(crate::error::ManaError::Config(
            crate::error::ConfigError::InvalidValue {
                field: "perception.thread".into(),
                msg: "el hilo de percepción murió antes de arrancar".into(),
            },
        )),
    }
}

/// Cuerpo del hilo: un keyframe por vuelta, sin reloj propio.
///
/// La etapa no tiene cadencia: corre cuando hay keyframe y duerme cuando no.
/// Quien tiene cadencia es el lazo de control, y ya no depende de esto.
fn run(mut stage: PerceptionStage, ports: PerceptionPorts, config: &PerceptionConfig) {
    while let Some(kf) = ports.keyframes.take_blocking() {
        let now = Instant::now();
        stage.refresh_directive(&ports.directives);

        // Un pánico acá no puede llevarse el lazo de control: se reporta, se
        // descarta el keyframe y la etapa sigue. Si percepción dejara de
        // producir del todo, control lo ve por la **edad** de la evidencia y se
        // va a `blind` solo — que es el invariante que esta fase existe para
        // hacer cierto.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stage.process_keyframe(kf, config, now)
        }));

        match outcome {
            Ok(output) => {
                if let Some(output) = output {
                    ports.images.put(output);
                }
            }
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "unknown panic".into());
                log::error!("perception panicked: {msg}");
                stage.observer.emit(Event::health_perception_panic(&msg));
                // No reanudar desde estado roto, la misma política que aplica
                // el lazo de control cuando entra en pánico.
                //
                // Un pánico entre `reset_depth` y `publish_clinical_sample`
                // deja la imagen con profundidad de este keyframe y
                // observaciones del anterior: **evidencia de dos épocas en una
                // sola imagen de proceso**. No llega a control en este frame
                // —no se publica— pero persistiría al siguiente.
                //
                // Descartarla entera hace que la evidencia envejezca y que el
                // lazo se vaya a `blind` solo, que es lo que corresponde: es
                // preferible declararse ciego a decidir sobre algo mezclado.
                stage.image = ProcessImage::empty();
            }
        }

        // Los eventos van sí o sí, incluso los del keyframe que entró en
        // pánico: es la traza de lo que llegó a pasar antes de romperse.
        let events = stage.observer.drain_pending();
        if !events.is_empty() && ports.events.send(events).is_err() {
            log::warn!("perception: el lazo de control cerró la cola de eventos");
            break;
        }
        stage.observer.flush();
    }
    ports.images.close();
}

impl PerceptionStage {
    /// Consume un keyframe de punta a punta y publica la imagen de proceso.
    ///
    /// Es el cuerpo del hilo, expuesto aparte para poder ejercitarlo sin
    /// levantar hilos ni sockets.
    pub(crate) fn process_keyframe(
        &mut self,
        kf: RawKeyframe,
        config: &PerceptionConfig,
        now: Instant,
    ) -> Option<PerceptionOutput> {
        let frame_timestamp_ns =
            super::frame_timestamp_ns(&self.boot_wall, self.boot_instant, now);
        let (frame_buf, decode_us) = self.decoder.decode_timed(&kf.h264);
        let dt_ms = self.on_keyframe(decode_us, now);

        #[cfg(feature = "rerun")]
        {
            self.observer.viz.set_frame_time(self.frame_count, frame_timestamp_ns);
            self.observer.viz.log_keyframe_selection(
                kf.keyframes_seen,
                kf.keyframes_dropped,
                kf.source_window_ms,
            );
            self.observer.viz.log_decode_latency(decode_us);
            self.observer.viz.log_keyframe_gap(dt_ms);
        }
        if config.snapshot {
            self.snapshots.save(&kf.h264, frame_buf.as_ref());
        }

        // Un decode fallido no es información: no produce imagen, y control
        // nunca ve señal fresca por un keyframe que no se pudo decodificar.
        let fb = frame_buf?;

        // Las guardas de profundidad exigen evidencia de este frame, nunca un
        // resultado viejo.
        self.image.reset_depth(now);
        if config.infer {
            let cycle = super::CycleContext::new(
                &fb,
                now,
                dt_ms,
                kf.source_window_ms,
                kf.keyframes_seen,
                kf.keyframes_dropped,
                self.frame_count,
                frame_timestamp_ns,
            );
            self.run_inference(cycle, config);
        }
        self.flush_viz_frame(fb, frame_timestamp_ns);

        Some(PerceptionOutput {
            image: self.image.clone(),
            captured_at: now,
        })
    }

    fn on_keyframe(&mut self, decode_us: u64, now: Instant) -> u64 {
        self.frame_count += 1;
        // `try_from` y no `as`, igual que el resto de las conversiones de esta
        // sesión: la truncación necesitaría un gap de 584 millones de años, pero
        // dos formas distintas de convertir lo mismo en el mismo archivo es lo
        // que hace que alguien elija la equivocada la próxima vez.
        let dt_ms = u64::try_from(now.duration_since(self.last_keyframe_at).as_millis())
            .unwrap_or(u64::MAX);
        self.last_keyframe_at = now;
        {
            let mut metrics = self.lock_metrics();
            metrics.tick_keyframe(dt_ms);
            metrics.tick_decode(decode_us);
        }
        if self.frame_count % 5 == 1 {
            log::info!(
                "frame #{} ingested (decode {}us)",
                self.frame_count,
                decode_us
            );
        }
        self.observer
            .emit(Event::frame_ingest(self.frame_count, true, decode_us, dt_ms));
        dt_ms
    }

    /// Toma la directiva más fresca que haya publicado control y dibuja lo que
    /// trae. Si no hay ninguna nueva, se sigue con la anterior: una directiva
    /// vieja es utilizable, esperar por una nueva no.
    pub(crate) fn refresh_directive(&mut self, slot: &Slot<ControlDirective>) {
        let Some(directive) = slot.take() else { return };
        #[cfg(feature = "rerun")]
        {
            if let Some((state, second, signal)) = directive.occupancy {
                self.observer.on_occupancy(state, second, signal);
            }
            if let Some(state) = directive.fsm_state.as_deref() {
                self.observer.viz.log_face_state(state);
            }
            self.observer.viz.log_entity_boxes(&directive.tracks);
        }
        self.directive = directive;
    }

    /// Sección crítica de microsegundos sobre contadores. El invariante que
    /// hace aceptable este candado en un sistema que está sacando los bloqueos
    /// del lazo: **nunca se sostiene a través de trabajo de latencia variable**
    /// —ni inferencia, ni decode, ni I/O—. Un `Mutex` de contadores y un canal
    /// que espera a la red no son la misma clase de espera.
    pub(crate) fn lock_metrics(&self) -> std::sync::MutexGuard<'_, MetricsEngine> {
        self.metrics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn frame_number(&self) -> u64 {
        self.frame_count
    }

    /// Consume el `FrameBuffer`: los 6,2 MB de RGB se **mueven** al lote de
    /// dibujo en vez de copiarse. Es la última etapa que los usa.
    #[cfg(feature = "rerun")]
    fn flush_viz_frame(&mut self, fb: crate::snapshot::FrameBuffer, timestamp_ns: i64) {
        let header = super::raw_frame_header(&fb, self.frame_count, timestamp_ns);
        for entry in self.crop_frames_pending.drain(..) {
            if let Some(crop) = entry.crop_frame {
                self.observer.viz.log_crop_frame(&entry.model, header, crop);
            }
        }
        self.observer.viz.log_frame(header, fb.rgb);
    }

    #[cfg(not(feature = "rerun"))]
    fn flush_viz_frame(&mut self, fb: crate::snapshot::FrameBuffer, timestamp_ns: i64) {
        let _ = (fb, timestamp_ns);
        self.crop_frames_pending.clear();
    }
}
