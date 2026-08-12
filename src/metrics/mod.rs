use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::depth_map::DepthFrame;
use crate::detection::Detection;

/// Piso de medición del atraso de vencimiento, en microsegundos.
///
/// El temporizador de tokio es una rueda jerárquica: un `sleep_until` nunca se
/// despierta antes del vencimiento, pero se despierta sistemáticamente algo
/// después aunque el lazo esté completamente ocioso. Contar eso como
/// incumplimiento haría que **todos** los vencimientos figuraran incumplidos
/// siempre, y un contador que vale 100% en reposo no distingue nada.
///
/// **El valor es medido, no elegido.** Corrida de 180 s del escenario 01
/// —ingesta sola, nada que pueda bloquear el lazo— sobre 487 vencimientos:
///
/// ```text
/// late_min_us   800 … 1_145
/// late_p95_us 1_879 … 2_083
/// late_max_us 1_957 … 2_117   ← ningún vencimiento de la corrida lo superó
/// ```
///
/// El piso es ~2,1 ms y es muy estable. La primera versión de esta constante
/// valía 1 ms —debajo del piso— y producía 470 incumplimientos de 487 en el
/// escenario *de control*, contra 321 de 332 en el escenario con inferencia
/// bloqueando 188 ms: el contador no distinguía un lazo sano de uno roto. Está
/// en 5 ms, 2,4× el piso medido y 2,5% del periodo de 200 ms.
///
/// **Compuerta que lo mantiene honesto:** el escenario 01 debe informar
/// `missed 0`. Si vuelve a informar incumplimientos sin que nada bloquee el
/// lazo, el piso se movió y esta constante hay que volver a medirla — no
/// subirla hasta que el número quede lindo.
///
/// Si alguien baja `scan.period_ms` a menos de ~20 ms, esta tolerancia deja de
/// ser despreciable frente al periodo y hay que revisarla.
///
/// La tolerancia gobierna sólo el **contador**. La distribución de atraso
/// (`min`/`p50`/`p95`/`max`) se publica en microsegundos y sin recortar: el
/// piso del instrumento queda a la vista en el reporte en vez de esconderse
/// detrás del umbral.
pub const SCAN_DEADLINE_TOLERANCE_US: u64 = 5_000;

// ── Per-frame per-class stats (transient, computed each keyframe) ──

#[derive(Debug, Clone)]
pub struct ClassFrameStat {
    pub count: u64,
    pub conf_min: f32,
    pub conf_max: f32,
    pub area_min: f64,
    pub area_max: f64,
}

impl Default for ClassFrameStat {
    fn default() -> Self {
        Self {
            count: 0,
            conf_min: f32::MAX,
            conf_max: 0.0,
            area_min: f64::MAX,
            area_max: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PerClassFrameStats {
    pub stats: HashMap<String, ClassFrameStat>,
}

impl PerClassFrameStats {
    pub fn from_detections(detections: &[Detection]) -> Self {
        let mut stats: HashMap<String, ClassFrameStat> = HashMap::new();
        for det in detections {
            let entry = stats.entry(det.class.clone()).or_default();
            entry.count += 1;
            let c = det.confidence;
            entry.conf_min = entry.conf_min.min(c);
            entry.conf_max = entry.conf_max.max(c);
            let area = ((det.bbox[2] - det.bbox[0]) * (det.bbox[3] - det.bbox[1])).max(1.0);
            let a = area as f64;
            entry.area_min = entry.area_min.min(a);
            entry.area_max = entry.area_max.max(a);
        }
        Self { stats }
    }
}

#[derive(Debug, Clone)]
pub struct PerModelMetrics {
    pub inferences: u64,
    pub infer_total_us: u64,
    pub infer_min_us: u64,
    pub infer_max_us: u64,
    pub total_dets: u64,
    pub skips: u64,
    /// Veces que el estado del FSM no pidió este modelo. Distinto de `skips`:
    /// el modelo no llegó a mirar la escena.
    pub gated: u64,
    pub empty: u64,
    pub conf_sum: f64,
    pub conf_min: f64,
    pub bbox_area_sum: f64,
    pub class_counts: HashMap<String, u64>,
    pub roi: Option<[u32; 4]>,
    pub depth_frames: u64,
    pub depth_valid_pixels: u64,
    pub depth_empty: u64,
    pub depth_min_m: f64,
    pub depth_max_m: f64,
}

impl Default for PerModelMetrics {
    fn default() -> Self {
        Self {
            inferences: 0,
            infer_total_us: 0,
            infer_min_us: u64::MAX,
            infer_max_us: 0,
            total_dets: 0,
            skips: 0,
            gated: 0,
            empty: 0,
            conf_sum: 0.0,
            conf_min: 0.0,
            bbox_area_sum: 0.0,
            class_counts: HashMap::new(),
            roi: None,
            depth_frames: 0,
            depth_valid_pixels: 0,
            depth_empty: 0,
            depth_min_m: f64::INFINITY,
            depth_max_m: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Metrics {
    pub cycles: u64,
    pub cycle_min_us: u64,
    pub cycle_max_us: u64,
    pub cycle_overruns: u64,
    pub cycle_samples: Vec<u64>,
    pub scan_late_min_us: u64,
    pub scan_late_max_us: u64,
    pub scan_late_samples: Vec<u64>,
    pub scan_deadlines_missed: u64,
    pub slot_keyframes_dropped: u64,
    pub slot_images_dropped: u64,
    pub slot_viz_dropped: u64,
    pub evidence_age_min_ms: u64,
    pub evidence_age_max_ms: u64,
    pub evidence_age_samples: Vec<u64>,
    pub keyframe_gap_min_us: u64,
    pub keyframe_gap_max_us: u64,
    pub keyframe_gap_samples: Vec<u64>,
    pub frames_total: u64,
    pub keyframes: u64,
    pub keyframes_seen: u64,
    pub keyframes_dropped: u64,
    pub pframes_dropped: u64,
    pub inferences: u64,
    pub infer_total_us: u64,
    pub infer_min_us: u64,
    pub infer_max_us: u64,
    pub decode_total_us: u64,
    pub blind_cycles: u64,
    pub timeouts: u64,
    pub ssrc_changes: u64,
    pub rtp_errors: u64,
    pub stream_ends: u64,
    pub reconnect_attempts: u64,
    pub ingest_pframes: u64,
    pub ingest_dup_keyframes: u64,
    pub infer_skips: u64,
    pub infer_gated: u64,
    pub infer_empty: u64,
    pub infer_total_dets: u64,
    pub model_metrics: HashMap<String, PerModelMetrics>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            cycles: 0,
            cycle_min_us: u64::MAX,
            cycle_max_us: 0,
            cycle_overruns: 0,
            cycle_samples: Vec::new(),
            scan_late_min_us: u64::MAX,
            scan_late_max_us: 0,
            scan_late_samples: Vec::new(),
            scan_deadlines_missed: 0,
            slot_keyframes_dropped: 0,
            slot_images_dropped: 0,
            slot_viz_dropped: 0,
            evidence_age_min_ms: u64::MAX,
            evidence_age_max_ms: 0,
            evidence_age_samples: Vec::new(),
            keyframe_gap_min_us: u64::MAX,
            keyframe_gap_max_us: 0,
            keyframe_gap_samples: Vec::new(),
            frames_total: 0,
            keyframes: 0,
            keyframes_seen: 0,
            keyframes_dropped: 0,
            pframes_dropped: 0,
            inferences: 0,
            infer_total_us: 0,
            infer_min_us: u64::MAX,
            infer_max_us: 0,
            decode_total_us: 0,
            blind_cycles: 0,
            timeouts: 0,
            ssrc_changes: 0,
            rtp_errors: 0,
            stream_ends: 0,
            reconnect_attempts: 0,
            ingest_pframes: 0,
            ingest_dup_keyframes: 0,
            infer_skips: 0,
            infer_gated: 0,
            infer_empty: 0,
            infer_total_dets: 0,
            model_metrics: HashMap::new(),
        }
    }
}

/// Percentil entero de una muestra ya ordenada, sin f64 ni copias temporales.
fn percentile_us(samples: &[u64], percentile: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let idx = samples
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1);
    samples[idx.min(samples.len() - 1)]
}

impl Metrics {
    fn with_cycle_capacity(cycle_capacity: usize) -> Self {
        let mut metrics = Self::default();
        metrics.cycle_samples = Vec::with_capacity(cycle_capacity);
        metrics.scan_late_samples = Vec::with_capacity(cycle_capacity);
        metrics.evidence_age_samples = Vec::with_capacity(cycle_capacity);
        metrics.keyframe_gap_samples = Vec::with_capacity(cycle_capacity);
        metrics
    }

    pub fn into_report(mut self, window_s: u64, cycle_budget_ms: u64) -> MetricsReport {
        self.cycle_samples.sort_unstable();
        let cycle_p95_us = percentile_us(&self.cycle_samples, 95);
        self.scan_late_samples.sort_unstable();
        let scan_deadlines = self.scan_late_samples.len() as u64;
        // p50 junto a p95 porque el atraso es **bimodal** por construcción: los
        // ciclos que no chocan con trabajo se quedan en el piso del
        // temporizador y los que sí, saltan a la latencia de la etapa que los
        // bloqueó. Sin la mediana, un p95 de 115 ms parece un lazo degradado en
        // vez de uno que cumple el 80% de las veces y se bloquea el resto.
        let scan_late_p50_us = percentile_us(&self.scan_late_samples, 50);
        let scan_late_p95_us = percentile_us(&self.scan_late_samples, 95);
        self.evidence_age_samples.sort_unstable();
        let evidence_scans = self.evidence_age_samples.len() as u64;
        let evidence_age_p50_ms = percentile_us(&self.evidence_age_samples, 50);
        let evidence_age_p95_ms = percentile_us(&self.evidence_age_samples, 95);
        self.keyframe_gap_samples.sort_unstable();
        let keyframe_gap_count = self.keyframe_gap_samples.len();
        let keyframe_gap_p50_us = percentile_us(&self.keyframe_gap_samples, 50);
        let keyframe_gap_p95_us = percentile_us(&self.keyframe_gap_samples, 95);
        MetricsReport {
            window_s,
            cycles: self.cycles,
            cycle_min_ms: if self.cycles > 0 {
                self.cycle_min_us / 1000
            } else {
                0
            },
            cycle_max_ms: if self.cycles > 0 {
                self.cycle_max_us / 1000
            } else {
                0
            },
            cycle_p95_ms: cycle_p95_us / 1000,
            cycle_overruns: self.cycle_overruns,
            cycle_budget_ms,
            scan_deadlines,
            // En microsegundos y sin dividir: en un lazo sano el atraso vive
            // por debajo del milisegundo, y en ms el reporte diría 0 tanto
            // cuando el lazo cumple como cuando el instrumento está roto.
            scan_late_min_us: if scan_deadlines > 0 {
                self.scan_late_min_us
            } else {
                0
            },
            scan_late_p50_us,
            scan_late_p95_us,
            scan_late_max_us: self.scan_late_max_us,
            scan_deadlines_missed: self.scan_deadlines_missed,
            scan_late_tolerance_us: SCAN_DEADLINE_TOLERANCE_US,
            slot_keyframes_dropped: self.slot_keyframes_dropped,
            slot_images_dropped: self.slot_images_dropped,
            slot_viz_dropped: self.slot_viz_dropped,
            evidence_scans,
            evidence_age_min_ms: if evidence_scans > 0 {
                self.evidence_age_min_ms
            } else {
                0
            },
            evidence_age_p50_ms,
            evidence_age_p95_ms,
            evidence_age_max_ms: self.evidence_age_max_ms,
            keyframe_gap_min_ms: if keyframe_gap_count > 0 {
                self.keyframe_gap_min_us / 1000
            } else {
                0
            },
            keyframe_gap_p50_ms: keyframe_gap_p50_us / 1000,
            keyframe_gap_p95_ms: keyframe_gap_p95_us / 1000,
            keyframe_gap_max_ms: if keyframe_gap_count > 0 {
                self.keyframe_gap_max_us / 1000
            } else {
                0
            },
            frames_total: self.frames_total,
            keyframes: self.keyframes,
            keyframes_seen: self.keyframes_seen,
            keyframes_dropped: self.keyframes_dropped,
            pframes_dropped: self.pframes_dropped,
            inferences: self.inferences,
            infer_total_ms: self.infer_total_us / 1000,
            infer_min_ms: if self.inferences > 0 {
                self.infer_min_us / 1000
            } else {
                0
            },
            infer_max_ms: if self.inferences > 0 {
                self.infer_max_us / 1000
            } else {
                0
            },
            decode_total_ms: self.decode_total_us / 1000,
            blind_cycles: self.blind_cycles,
            timeouts: self.timeouts,
            ssrc_changes: self.ssrc_changes,
            rtp_errors: self.rtp_errors,
            stream_ends: self.stream_ends,
            reconnect_attempts: self.reconnect_attempts,
            ingest_pframes: self.ingest_pframes,
            ingest_dup_keyframes: self.ingest_dup_keyframes,
            infer_skips: self.infer_skips,
            infer_gated: self.infer_gated,
            infer_empty: self.infer_empty,
            infer_total_dets: self.infer_total_dets,
            model_metrics: self.model_metrics,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetricsReport {
    pub window_s: u64,
    pub cycles: u64,
    pub cycle_min_ms: u64,
    pub cycle_max_ms: u64,
    pub cycle_p95_ms: u64,
    pub cycle_overruns: u64,
    pub cycle_budget_ms: u64,
    pub scan_deadlines: u64,
    pub scan_late_min_us: u64,
    pub scan_late_p50_us: u64,
    pub scan_late_p95_us: u64,
    pub scan_late_max_us: u64,
    pub scan_deadlines_missed: u64,
    pub scan_late_tolerance_us: u64,
    pub slot_keyframes_dropped: u64,
    pub slot_images_dropped: u64,
    pub slot_viz_dropped: u64,
    pub evidence_scans: u64,
    pub evidence_age_min_ms: u64,
    pub evidence_age_p50_ms: u64,
    pub evidence_age_p95_ms: u64,
    pub evidence_age_max_ms: u64,
    pub keyframe_gap_min_ms: u64,
    pub keyframe_gap_p50_ms: u64,
    pub keyframe_gap_p95_ms: u64,
    pub keyframe_gap_max_ms: u64,
    pub frames_total: u64,
    pub keyframes: u64,
    pub keyframes_seen: u64,
    pub keyframes_dropped: u64,
    pub pframes_dropped: u64,
    pub inferences: u64,
    pub infer_total_ms: u64,
    pub infer_min_ms: u64,
    pub infer_max_ms: u64,
    pub decode_total_ms: u64,
    pub blind_cycles: u64,
    pub timeouts: u64,
    pub ssrc_changes: u64,
    pub rtp_errors: u64,
    pub stream_ends: u64,
    pub reconnect_attempts: u64,
    pub ingest_pframes: u64,
    pub ingest_dup_keyframes: u64,
    pub infer_skips: u64,
    pub infer_gated: u64,
    pub infer_empty: u64,
    pub infer_total_dets: u64,
    pub model_metrics: HashMap<String, PerModelMetrics>,
}

pub struct MetricsEngine {
    current: Metrics,
    model_order: Vec<String>,
    window_start: Instant,
    report_interval_s: u64,
    cycle_budget_us: u64,
    cycle_sample_capacity: usize,
    last_cycle_start: Instant,
}

impl MetricsEngine {
    pub fn new(report_interval_s: u64, cycle_budget_ms: u64) -> Self {
        Self::new_at(report_interval_s, cycle_budget_ms, Instant::now())
    }

    pub fn new_at(report_interval_s: u64, cycle_budget_ms: u64, now: Instant) -> Self {
        let cycle_sample_capacity = cycle_sample_capacity(report_interval_s, cycle_budget_ms);
        Self {
            current: Metrics::with_cycle_capacity(cycle_sample_capacity),
            model_order: Vec::new(),
            window_start: now,
            report_interval_s,
            cycle_budget_us: cycle_budget_ms * 1000,
            cycle_sample_capacity,
            last_cycle_start: now,
        }
    }

    /// Mide el periodo real del scan contra el inicio del ciclo anterior y lo
    /// enfrenta al presupuesto declarado ([health] `cycle_budget_ms`):
    /// la tesis del PLC vuelta señal verificable.
    ///
    /// Lo que se mide es el **periodo**, no el trabajo. Un ciclo ocioso duerme
    /// hasta el tick del scan y por construcción cae muy por debajo del
    /// presupuesto, así que no necesita exención: si el periodo se pasa, algo
    /// bloqueó el lazo, y eso es precisamente lo que el presupuesto existe para
    /// delatar.
    ///
    /// Antes había un parámetro `processed` que exigía "trabajo real" para
    /// declarar overrun. Confundía *no hizo trabajo* con *estuvo ocioso*, y el
    /// único llamador de producción lo pasaba en `false` de forma
    /// incondicional — de modo que `cycle_overruns` no podía ser distinto de
    /// cero nunca y `cycle_budget_ms` era un knob que no gobernaba nada. El
    /// caso que ocultaba es el importante: un scan bloqueado 8 s drenando un
    /// sink saturado no procesa keyframes, así que quedaba exento del
    /// presupuesto que debía denunciarlo.
    pub fn tick_cycle_at(&mut self, now: Instant) {
        self.current.cycles += 1;
        let delta_us = u64::try_from(
            now.saturating_duration_since(self.last_cycle_start)
                .as_micros(),
        )
        .unwrap_or(u64::MAX);
        self.last_cycle_start = now;
        self.current.cycle_min_us = self.current.cycle_min_us.min(delta_us);
        self.current.cycle_max_us = self.current.cycle_max_us.max(delta_us);
        self.current.cycle_samples.push(delta_us);
        if delta_us > self.cycle_budget_us {
            self.current.cycle_overruns += 1;
        }
    }

    /// Registra cuánto después de su vencimiento arrancó un scan.
    ///
    /// Mide un eje distinto del de [`tick_cycle_at`](Self::tick_cycle_at) y las
    /// dos conviven porque responden preguntas distintas:
    ///
    /// - **periodo**: cuánto pasó entre dos scans. Con recuperación en ráfaga
    ///   se autocorrige — un scan que arrancó tarde se compensa con el
    ///   siguiente, que arranca inmediatamente — así que el promedio se ve sano
    ///   incluso cuando el lazo incumplió.
    /// - **atraso**: cuánto después de su vencimiento arrancó cada scan. Eso no
    ///   se autocorrige, y es lo que en un PLC se llama incumplimiento.
    ///
    /// Por eso `cycle_budget_ms` no sirve para esto: es un umbral sobre el
    /// periodo, y el periodo es justamente la magnitud que la ráfaga repara.
    pub fn tick_scan_deadline(&mut self, late: Duration) {
        let late_us = u64::try_from(late.as_micros()).unwrap_or(u64::MAX);
        self.current.scan_late_min_us = self.current.scan_late_min_us.min(late_us);
        self.current.scan_late_max_us = self.current.scan_late_max_us.max(late_us);
        self.current.scan_late_samples.push(late_us);
        if late_us > SCAN_DEADLINE_TOLERANCE_US {
            self.current.scan_deadlines_missed += 1;
        }
    }

    pub fn tick_keyframe(&mut self, gap_ms: u64) {
        self.current.keyframes += 1;
        self.current.frames_total += 1;
        let sample_us = gap_ms.saturating_mul(1000);
        self.current.keyframe_gap_min_us = self.current.keyframe_gap_min_us.min(sample_us);
        self.current.keyframe_gap_max_us = self.current.keyframe_gap_max_us.max(sample_us);
        self.current.keyframe_gap_samples.push(sample_us);
    }

    pub fn tick_inference_model(
        &mut self,
        model_key: &str,
        elapsed_us: u64,
        detections: &[Detection],
        crop_rect: Option<[u32; 4]>,
    ) {
        self.current.inferences += 1;
        self.current.infer_total_us += elapsed_us;
        self.current.infer_min_us = self.current.infer_min_us.min(elapsed_us);
        self.current.infer_max_us = self.current.infer_max_us.max(elapsed_us);
        self.current.infer_total_dets += detections.len() as u64;
        if detections.is_empty() {
            self.current.infer_empty += 1;
        }

        let m = self
            .current
            .model_metrics
            .entry(model_key.to_string())
            .or_default();
        m.inferences += 1;
        m.infer_total_us += elapsed_us;
        m.infer_min_us = m.infer_min_us.min(elapsed_us);
        m.infer_max_us = m.infer_max_us.max(elapsed_us);
        m.total_dets += detections.len() as u64;
        m.roi = crop_rect;
        if detections.is_empty() {
            m.empty += 1;
        }
        for det in detections {
            m.conf_sum += det.confidence as f64;
            if m.conf_min == 0.0 || (det.confidence as f64) < m.conf_min {
                m.conf_min = det.confidence as f64;
            }
            let area = ((det.bbox[2] - det.bbox[0]) * (det.bbox[3] - det.bbox[1])).max(1.0);
            m.bbox_area_sum += area as f64;
            *m.class_counts.entry(det.class.clone()).or_default() += 1;
        }

        if !self.model_order.iter().any(|n| n == model_key) {
            self.model_order.push(model_key.to_string());
        }
    }

    pub fn tick_inference_depth(
        &mut self,
        model_key: &str,
        elapsed_us: u64,
        depth: Option<&DepthFrame>,
        crop_rect: Option<[u32; 4]>,
    ) {
        self.current.inferences += 1;
        self.current.infer_total_us += elapsed_us;
        self.current.infer_min_us = self.current.infer_min_us.min(elapsed_us);
        self.current.infer_max_us = self.current.infer_max_us.max(elapsed_us);

        let m = self
            .current
            .model_metrics
            .entry(model_key.to_string())
            .or_default();
        m.inferences += 1;
        m.infer_total_us += elapsed_us;
        m.infer_min_us = m.infer_min_us.min(elapsed_us);
        m.infer_max_us = m.infer_max_us.max(elapsed_us);
        m.roi = crop_rect;
        m.depth_frames += 1;

        let mut valid_pixels = 0;
        let mut min_depth = f64::INFINITY;
        let mut max_depth: f64 = 0.0;
        if let Some(map) = depth {
            for value in map.iter_values() {
                if value.is_finite() && value > 0.0 {
                    let value = f64::from(value);
                    valid_pixels += 1;
                    min_depth = min_depth.min(value);
                    max_depth = max_depth.max(value);
                }
            }
        }
        m.depth_valid_pixels += valid_pixels;
        if valid_pixels == 0 {
            m.depth_empty += 1;
        } else {
            m.depth_min_m = m.depth_min_m.min(min_depth);
            m.depth_max_m = m.depth_max_m.max(max_depth);
        }

        if !self.model_order.iter().any(|n| n == model_key) {
            self.model_order.push(model_key.to_string());
        }
    }

    pub fn tick_decode(&mut self, elapsed_us: u64) {
        self.current.decode_total_us += elapsed_us;
    }

    pub fn tick_blind(&mut self) {
        self.current.blind_cycles += 1;
    }

    /// Cuán vieja era la evidencia sobre la que el control acaba de decidir.
    ///
    /// **Es la única magnitud del sistema con consecuencia clínica directa.**
    /// Todo lo demás que se mide acá —periodo, atraso, latencia de inferencia,
    /// descartes— es salud del motor: dice si la máquina está sana, no si la
    /// decisión fue tomada sobre algo actual.
    ///
    /// La pregunta que contesta es la que hace un revisor de incidente: *cuando
    /// el FSM dijo `bed_alert`, ¿de cuándo era lo que vio?* El dato viajaba por
    /// evento en el JSONL desde el esquema v2, pero sin agregado había que
    /// reconstruirlo parseando evento por evento.
    ///
    /// Se mide contra el reloj de control —`ScanTimeline`— y no contra el de
    /// pared, porque es la edad tal como la percibió la decisión.
    ///
    /// Sólo se registra cuando hay evidencia: sin observaciones la edad es
    /// `u64::MAX`, y meter ese centinela en una distribución la arruina. La
    /// ausencia de evidencia ya la cuenta `blind_cycles`.
    pub fn tick_evidence_age(&mut self, age_ms: u64) {
        self.current.evidence_age_min_ms = self.current.evidence_age_min_ms.min(age_ms);
        self.current.evidence_age_max_ms = self.current.evidence_age_max_ms.max(age_ms);
        self.current.evidence_age_samples.push(age_ms);
    }

    /// Muestras pisadas en los bordes entre etapas (ADR-034).
    ///
    /// `keyframes` pisados significa que percepción no llegó a tomar el
    /// anterior — la inferencia va más lenta que la cámara. `images` pisadas
    /// significa que percepción produjo dos evidencias entre dos scans, que a
    /// las cadencias reales no debería pasar nunca. `viz` pisados es el enlace
    /// del visor sin dar abasto, y es el contador que **debe** subir cuando el
    /// enlace satura: es la prueba de que se descarta en vez de bloquear.
    ///
    /// Descartar es la degradación correcta para una muestra; **descartarla en
    /// silencio no lo es**, y un borde sin instrumentar es un borde sobre el
    /// que no se puede razonar cuando algo va mal.
    pub fn tick_slot_drops(&mut self, keyframes: u64, images: u64, viz: u64) {
        self.current.slot_keyframes_dropped += keyframes;
        self.current.slot_images_dropped += images;
        self.current.slot_viz_dropped += viz;
    }

    /// El estado del FSM no pidió este modelo en este keyframe.
    pub fn tick_infer_gated(&mut self, model_key: &str) {
        self.current.infer_gated += 1;
        let m = self
            .current
            .model_metrics
            .entry(model_key.to_string())
            .or_default();
        m.gated += 1;
        if !self.model_order.iter().any(|n| n == model_key) {
            self.model_order.push(model_key.to_string());
        }
    }

    pub fn tick_infer_skip(&mut self, model_key: &str) {
        self.current.infer_skips += 1;
        let m = self
            .current
            .model_metrics
            .entry(model_key.to_string())
            .or_default();
        m.skips += 1;
        if !self.model_order.iter().any(|n| n == model_key) {
            self.model_order.push(model_key.to_string());
        }
    }

    pub fn tick_ingest(
        &mut self,
        pframes: u64,
        dup_keyframes: u64,
        keyframes_seen: u64,
        keyframes_dropped: u64,
    ) {
        self.current.ingest_pframes += pframes;
        self.current.ingest_dup_keyframes += dup_keyframes;
        self.current.keyframes_seen += keyframes_seen;
        self.current.keyframes_dropped += keyframes_dropped;
    }

    pub fn tick_retina_counters(
        &mut self,
        timeouts: u64,
        ssrc_changes: u64,
        rtp_errors: u64,
        stream_ends: u64,
        reconnect_attempts: u64,
    ) {
        self.current.timeouts = self.current.timeouts.saturating_add(timeouts);
        self.current.ssrc_changes = self.current.ssrc_changes.saturating_add(ssrc_changes);
        self.current.rtp_errors = self.current.rtp_errors.saturating_add(rtp_errors);
        self.current.stream_ends = self.current.stream_ends.saturating_add(stream_ends);
        self.current.reconnect_attempts = self
            .current
            .reconnect_attempts
            .saturating_add(reconnect_attempts);
    }

    pub fn take_report(&mut self) -> Option<(MetricsReport, Vec<String>)> {
        let elapsed = self.window_start.elapsed().as_secs();
        if elapsed < self.report_interval_s {
            return None;
        }
        let order = std::mem::take(&mut self.model_order);
        let current = std::mem::replace(
            &mut self.current,
            Metrics::with_cycle_capacity(self.cycle_sample_capacity),
        );
        let report = current.into_report(elapsed, self.cycle_budget_us / 1000);
        self.window_start = Instant::now();
        Some((report, order))
    }
}

fn cycle_sample_capacity(report_interval_s: u64, cycle_budget_ms: u64) -> usize {
    let budget_ms = cycle_budget_ms.max(1);
    let samples = report_interval_s.saturating_mul(1_000).div_ceil(budget_ms);
    usize::try_from(samples).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests;
