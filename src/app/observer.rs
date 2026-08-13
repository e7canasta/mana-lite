//! Observador de la etapa de percepción: dibujo directo, eventos por cola.

#[cfg(feature = "rerun")]
use super::viz_relay::VizHandle;
use crate::logger::Event;
use crate::occupancy::{RoomCardinality, SecondPersonState, SignalValidity};

/// Observador del hilo de percepción.
///
/// Dibuja directo —es el dueño único del [`VizBridge`]— pero **no escribe
/// JSONL**: acumula sus eventos y el hilo los entrega por cola al lazo de
/// control, que es el único que toca el sink. Dos hilos escribiendo el mismo
/// archivo intercalarían líneas y romperían el orden del registro.
///
/// La asimetría es deliberada y sigue ADR-034: el frame es una **muestra** y se
/// puede pisar; una detección del JSONL es un **evento** y perderla es un bug
/// de auditoría.
///
/// Antes de la Fase 3 esto era un trait `PipelineObserver` con tres
/// implementaciones —fanout, null y este—. Al partir las etapas quedó un solo
/// implementador y ningún doble de test que lo usara: un trait que no vuelve
/// imposible ningún error es un módulo con pasos de más, así que se colapsó a
/// métodos inherentes.
///
/// Desde la Fase 2 el campo `viz` ya no es el bridge sino un [`VizHandle`]: los
/// dibujos se encolan y los aplica el hilo del visor. Ni siquiera esta etapa
/// puede quedar bloqueada por el enlace.
pub struct PerceptionObserver {
    #[cfg(feature = "rerun")]
    pub viz: VizHandle,
    pending: Vec<Event>,
}

impl PerceptionObserver {
    #[cfg(feature = "rerun")]
    pub fn new(viz: VizHandle) -> Self {
        Self {
            viz,
            pending: Vec::with_capacity(32),
        }
    }

    #[cfg(not(feature = "rerun"))]
    pub fn new() -> Self {
        Self {
            pending: Vec::with_capacity(32),
        }
    }

    /// Encola un evento para el lazo de control. No toca disco.
    pub fn emit(&mut self, event: Event) {
        self.pending.push(event);
    }

    /// Se lleva los eventos del keyframe para mandarlos por la cola.
    pub fn drain_pending(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    pub fn on_occupancy(
        &mut self,
        state: RoomCardinality,
        second_person: SecondPersonState,
        signal: SignalValidity,
    ) {
        #[cfg(feature = "rerun")]
        self.viz.log_occupancy_state(state, second_person, signal);
        #[cfg(not(feature = "rerun"))]
        let _ = (state, second_person, signal);
    }

    /// Publica el lote de dibujo del keyframe. **No dibuja ni espera**: deja el
    /// lote en un slot y sigue. El borde que bloqueó el pipeline 41 s en la
    /// Fase 0 quedó del otro lado de ese slot.
    pub fn flush(&mut self) {
        #[cfg(feature = "rerun")]
        self.viz.commit();
    }
}

#[cfg(not(feature = "rerun"))]
impl Default for PerceptionObserver {
    fn default() -> Self {
        Self::new()
    }
}
