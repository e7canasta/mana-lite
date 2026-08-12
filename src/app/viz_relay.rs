//! El visor en su propio hilo: dibujar deja de poder frenar a nadie.
//!
//! Cierra lo que quedaba de [ADR-035](../../../docs/adrs/035-observability-port.md).
//! El diagnóstico de esa ADR era correcto —observabilidad sin puerto termina
//! cableada inline y frena al productor— pero su solución, un trait `VizSink`
//! de veinte métodos, resolvía un problema de sustitución que nadie tenía. El
//! problema real era **de quién es el hilo**.
//!
//! rerun aplica contrapresión: `re_chunk` no ofrece política de descarte, así
//! que un `log()` sobre un enlace saturado **bloquea al que llama**. Medido en
//! la Fase 0: 41 segundos. La Fase 3 movió ese bloqueo del lazo de control a
//! percepción —una mejora de categoría— pero percepción bloqueada sigue siendo
//! el sistema sin evidencia fresca.
//!
//! Acá el bloqueo deja de alcanzar a ninguna etapa del pipeline:
//!
//! ```text
//! [percepción] ── Slot<VizBatch> ──► [hilo viz] ── rerun
//!                 se pisa y se cuenta      puede bloquear todo lo que quiera
//! ```
//!
//! Un lote es **un keyframe entero de dibujo**, y se descarta entero. Medio
//! frame de overlays sobre el frame siguiente sería peor que no dibujar nada.

use std::sync::Arc;

use crate::detection::{ConsolidatedObservation, CropRect, Detection};
use crate::depth_map::DepthFrame;
use crate::infer::CropFrameInfo;
use crate::metrics::PerClassFrameStats;
use crate::occupancy::{RoomCardinality, SecondPersonState, SignalValidity};
use crate::slot::Slot;
use crate::track::Track;
use crate::viz::VizBridge;
use mana_media::RawFrameV1;

use super::FrameSize;

/// Un dibujo diferido. El `FnOnce` captura sus argumentos ya en propiedad.
type VizCmd = Box<dyn FnOnce(&mut VizBridge) + Send>;

/// Los dibujos de un keyframe, en orden. El orden importa: `set_frame_time`
/// tiene que aplicarse antes que lo que fecha.
#[derive(Default)]
pub struct VizBatch {
    cmds: Vec<VizCmd>,
}

impl VizBatch {
    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }
}

/// Lo que percepción usa en vez del `VizBridge`.
///
/// Espeja su superficie método por método, y cada uno **convierte sus
/// argumentos prestados en propios** para poder cruzar el hilo. Ese costo
/// existía igual —rerun copia a su propio buffer— y acá queda concentrado en un
/// solo archivo en vez de repartido por el pipeline, que era el punto de diseño
/// que ADR-035 declaraba.
///
/// No es un trait: hay un solo dueño y un solo implementador. Un trait acá no
/// volvería imposible ningún error (ADR-028).
pub struct VizHandle {
    pending: VizBatch,
    batches: Arc<Slot<VizBatch>>,
}

impl VizHandle {
    pub fn new(batches: Arc<Slot<VizBatch>>) -> Self {
        Self {
            pending: VizBatch::default(),
            batches,
        }
    }

    fn push(&mut self, cmd: impl FnOnce(&mut VizBridge) + Send + 'static) {
        self.pending.cmds.push(Box::new(cmd));
    }

    /// Publica el lote del keyframe. **Nunca bloquea**: si el hilo del visor
    /// sigue peleando con el enlace, este lote pisa al anterior y se cuenta.
    pub fn commit(&mut self) {
        let batch = std::mem::take(&mut self.pending);
        if !batch.is_empty() {
            self.batches.put(batch);
        }
    }

    // ── Superficie espejada del bridge ──

    pub fn set_frame_time(&mut self, frame_number: u64, timestamp_ns: i64) {
        self.push(move |v| v.set_frame_time(frame_number, timestamp_ns));
    }

    pub fn log_keyframe_selection(&mut self, seen: u64, dropped: u64, source_window_ms: u64) {
        self.push(move |v| v.log_keyframe_selection(seen, dropped, source_window_ms));
    }

    pub fn log_decode_latency(&mut self, us: u64) {
        self.push(move |v| v.log_decode_latency(us));
    }

    pub fn log_keyframe_gap(&mut self, dt_ms: u64) {
        self.push(move |v| v.log_keyframe_gap(dt_ms));
    }

    pub fn log_infer_latency(&mut self, model: &str, backend_us: u64, pipeline_us: u64) {
        let model = model.to_owned();
        self.push(move |v| v.log_infer_latency(&model, backend_us, pipeline_us));
    }

    pub fn log_roi_boxes(&mut self, model: &str, rect: CropRect) {
        let model = model.to_owned();
        self.push(move |v| v.log_roi_boxes(&model, rect));
    }

    pub fn log_per_frame_class_stats(&mut self, model: &str, per_class: &PerClassFrameStats) {
        let (model, per_class) = (model.to_owned(), per_class.clone());
        self.push(move |v| v.log_per_frame_class_stats(&model, &per_class));
    }

    pub fn log_model_detections(
        &mut self,
        model: &str,
        detections: &[Detection],
        crop: Option<CropRect>,
        frame: FrameSize,
    ) {
        let (model, detections) = (model.to_owned(), detections.to_vec());
        self.push(move |v| v.log_model_detections(&model, &detections, crop, frame));
    }

    pub fn log_model_pose(&mut self, model: &str, detections: &[Detection]) {
        let (model, detections) = (model.to_owned(), detections.to_vec());
        self.push(move |v| v.log_model_pose(&model, &detections));
    }

    pub fn log_model_masks(&mut self, model: &str, detections: &[Detection], frame: FrameSize) {
        let (model, detections) = (model.to_owned(), detections.to_vec());
        self.push(move |v| v.log_model_masks(&model, &detections, frame));
    }

    pub fn log_depth_context_boxes(
        &mut self,
        model: &str,
        detections: &[Detection],
        roi: Option<CropRect>,
    ) {
        let (model, detections) = (model.to_owned(), detections.to_vec());
        self.push(move |v| v.log_depth_context_boxes(&model, &detections, roi));
    }

    pub fn log_depth_context_polygons(
        &mut self,
        model: &str,
        detections: &[Detection],
        roi: Option<CropRect>,
        frame: FrameSize,
    ) {
        let (model, detections) = (model.to_owned(), detections.to_vec());
        self.push(move |v| v.log_depth_context_polygons(&model, &detections, roi, frame));
    }

    pub fn clear_depth_context_boxes(&mut self) {
        self.push(|v| v.clear_depth_context_boxes());
    }

    pub fn log_consolidated_observations(
        &mut self,
        observations: &[ConsolidatedObservation],
        frame: FrameSize,
    ) {
        let observations = observations.to_vec();
        self.push(move |v| v.log_consolidated_observations(&observations, frame));
    }

    pub fn log_entity_boxes(&mut self, tracks: &[Track]) {
        let tracks = tracks.to_vec();
        self.push(move |v| {
            let refs: Vec<&Track> = tracks.iter().collect();
            v.log_entity_boxes(&refs);
        });
    }

    pub fn log_face_state(&mut self, state: &str) {
        let state = state.to_owned();
        self.push(move |v| v.log_face_state(&state));
    }

    pub fn log_occupancy_state(
        &mut self,
        state: RoomCardinality,
        second_person: SecondPersonState,
        signal: SignalValidity,
    ) {
        self.push(move |v| v.log_occupancy_state(state, second_person, signal));
    }

    /// El frame completo. Es el dibujo caro —6,2 MB sin comprimir— y el que
    /// justificó todo esto: `rgb` se **mueve**, no se copia.
    pub fn log_frame(&mut self, header: RawFrameV1, rgb: Vec<u8>) {
        self.push(move |v| v.log_frame(&header, &rgb));
    }

    pub fn log_crop_frame(&mut self, model: &str, header: RawFrameV1, crop: CropFrameInfo) {
        let model = model.to_owned();
        self.push(move |v| v.log_crop_frame(&model, &header, crop));
    }

    pub fn log_model_depth(&mut self, model: &str, depth: Option<&DepthFrame>) {
        let (model, depth) = (model.to_owned(), depth.cloned());
        self.push(move |v| v.log_model_depth(&model, depth.as_ref()));
    }
}

/// Levanta el hilo del visor.
///
/// Es el único que toca el `VizBridge`, y el único al que le está permitido
/// bloquearse: no hay nadie aguas abajo esperándolo.
pub fn spawn(
    mut viz: VizBridge,
    batches: Arc<Slot<VizBatch>>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("viz".into())
        .spawn(move || {
            while let Some(batch) = batches.take_blocking() {
                for cmd in batch.cmds {
                    cmd(&mut viz);
                }
                // El flush también puede bloquear, y también da igual: acá
                // adentro el tiempo no es de nadie más.
                viz.tick();
            }
        })
}
