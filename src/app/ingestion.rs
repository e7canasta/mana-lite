//! La ingesta: RTSP, demux y dedupe, en su propia task.
//!
//! Se queda en `async` y no en un hilo porque su trabajo **es I/O de red**
//! (ADR-033, regla 3: hilos para cómputo, async sólo para I/O). Lo que cambia
//! en la Fase 4 es que deja de compartir task con el lazo de control.
//!
//! Con eso desaparece el `tokio::select!` del camino caliente, y con él la
//! clase entera de bug de cancelación: ya no hay una rama perdedora que se
//! descarte a mitad de un drenaje. El invariante *"todo future dentro del
//! select tiene que ser cancelación-seguro"* deja de hacer falta — no se
//! necesita una regla si la estructura hace imposible la situación.

use std::sync::{Arc, Mutex};

use crate::ingest::{FrameReader, IngestEngine, RawKeyframe};
use crate::metrics::MetricsEngine;
use crate::slot::Slot;

/// Levanta la task de ingesta.
///
/// No devuelve nada más que el handle: la task no habla con el lazo de control
/// salvo por el slot. Si el lazo termina, aborta el handle; el drenaje es
/// cancelación-seguro —el keyframe drenado vive en el engine, no en la pila del
/// future (corregido en la Fase 0)— así que abortar no puede perder trabajo.
pub fn spawn<R: FrameReader>(
    mut ingest: IngestEngine<R>,
    keyframes: Arc<Slot<RawKeyframe>>,
    metrics: Arc<Mutex<MetricsEngine>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Some(kf) = ingest.poll_freshest_keyframe().await {
                // Muestra: si percepción sigue ocupada con el anterior, este lo
                // pisa. Es lo correcto — la cámara no puede ir más lento y el
                // keyframe viejo ya no vale nada.
                keyframes.put(kf);
            }
            drain_counters(&mut ingest, &metrics);
        }
    })
}

/// Los contadores de transporte los vuelca la propia etapa.
///
/// Antes los drenaba el lazo de control en cada scan, que es una lectura de
/// estado de otra etapa desde el camino caliente. Ahora el dueño de los
/// contadores es quien los produce.
fn drain_counters<R: FrameReader>(
    ingest: &mut IngestEngine<R>,
    metrics: &Arc<Mutex<MetricsEngine>>,
) {
    let c = ingest.drain_ingest_counters();
    let mut metrics = metrics
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    metrics.tick_ingest(
        c.pframes_dropped,
        c.keyframes_dup,
        c.keyframes_seen,
        c.keyframes_dropped,
    );
    if let Some(r) = c.retina {
        metrics.tick_retina_counters(
            r.timeouts,
            r.ssrc_changes,
            r.rtp_errors,
            r.stream_ends,
            r.reconnect_attempts,
        );
    }
}
