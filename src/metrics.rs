use std::collections::HashMap;
use std::time::Instant;

use crate::infer::Detection;
use crate::depth_map::DepthFrame;

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
        metrics.keyframe_gap_samples = Vec::with_capacity(cycle_capacity);
        metrics
    }

    pub fn into_report(mut self, window_s: u64, cycle_budget_ms: u64) -> MetricsReport {
        self.cycle_samples.sort_unstable();
        let cycle_p95_us = percentile_us(&self.cycle_samples, 95);
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
    /// la tesis del PLC vuelta señal verificable. `processed` marca los
    /// ciclos con trabajo real — un ciclo que solo esperó el `poll` nunca
    /// declara overrun, porque el ocio cumple su presupuesto por
    /// construcción (duerme a timeout).
    pub fn tick_cycle_at(&mut self, now: Instant, processed: bool) {
        self.current.cycles += 1;
        let delta_us = u64::try_from(now.saturating_duration_since(self.last_cycle_start).as_micros())
            .unwrap_or(u64::MAX);
        self.last_cycle_start = now;
        self.current.cycle_min_us = self.current.cycle_min_us.min(delta_us);
        self.current.cycle_max_us = self.current.cycle_max_us.max(delta_us);
        self.current.cycle_samples.push(delta_us);
        if processed && delta_us > self.cycle_budget_us {
            self.current.cycle_overruns += 1;
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

#[derive(Debug, Clone, PartialEq)]
pub enum HealthTransition {
    Stale {
        component: &'static str,
        ms_since_frame: u64,
    },
    Blind {
        ms_since_frame: u64,
    },
    Recovered,
    None,
}

pub struct Health {
    last_frame_at: Instant,
    data_stale_ms: u64,
    stale_warn_ms: u64,
    blind: bool,
    stale: bool,
}

impl Health {
    pub fn new(data_stale_ms: u64, stale_warn_ms: u64) -> Self {
        Self::new_at(data_stale_ms, stale_warn_ms, Instant::now())
    }

    pub fn new_at(data_stale_ms: u64, stale_warn_ms: u64, now: Instant) -> Self {
        Self {
            last_frame_at: now,
            data_stale_ms,
            stale_warn_ms,
            blind: false,
            stale: false,
        }
    }

    #[allow(dead_code)]
    pub fn touch(&mut self) {
        self.touch_at(Instant::now());
    }

    /// Marca senal fresca. Devuelve si se venia de blind: en el superloop la
    /// recuperacion real ocurre aca (el keyframe llega), no en `evaluate_at`,
    /// que ya encuentra las banderas limpias. El evento de heartbeat debe
    /// emitirse cuando esto devuelve true.
    pub fn touch_at(&mut self, now: Instant) -> bool {
        let was_blind = self.blind;
        self.last_frame_at = now;
        self.blind = false;
        self.stale = false;
        was_blind
    }

    #[allow(dead_code)]
    pub fn evaluate(&mut self) -> HealthTransition {
        self.evaluate_at(Instant::now())
    }

    pub fn evaluate_at(&mut self, now: Instant) -> HealthTransition {
        let stale_ms = now.saturating_duration_since(self.last_frame_at).as_millis() as u64;

        if stale_ms > self.data_stale_ms {
            if !self.blind {
                self.blind = true;
                return HealthTransition::Blind {
                    ms_since_frame: stale_ms,
                };
            }
        } else if stale_ms > self.stale_warn_ms {
            if !self.blind && !self.stale {
                self.stale = true;
                return HealthTransition::Stale {
                    component: "ingest",
                    ms_since_frame: stale_ms,
                };
            }
        }

        if (self.blind || self.stale) && stale_ms <= self.stale_warn_ms {
            let was_blind = self.blind;
            self.blind = false;
            self.stale = false;
            if was_blind {
                return HealthTransition::Recovered;
            }
        }

        HealthTransition::None
    }

    pub fn is_blind(&self) -> bool {
        self.blind
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;
    use ultralytics_inference::DepthMap;

    #[test]
    fn depth_metrics_count_only_finite_positive_pixels() {
        let mut engine = MetricsEngine::new(0, 50);
        let depth = DepthFrame::from_ultralytics(DepthMap::new(
            array![[0.0, 1.0, 2.0], [f32::NAN, 3.0, f32::INFINITY]],
            (2, 3),
        ));

        engine.tick_inference_depth("depth-standard", 190_000, Some(&depth), None);
        let (report, order) = engine.take_report().expect("zero-second report");
        let metrics = &report.model_metrics["depth-standard"];

        assert_eq!(order, vec!["depth-standard"]);
        assert_eq!(metrics.depth_frames, 1);
        assert_eq!(metrics.depth_valid_pixels, 3);
        assert_eq!(metrics.depth_empty, 0);
        assert_eq!(metrics.depth_min_m, 1.0);
        assert_eq!(metrics.depth_max_m, 3.0);
        assert_eq!(report.infer_empty, 0);
    }

    #[test]
    fn empty_depth_does_not_count_as_detection_empty() {
        let mut engine = MetricsEngine::new(0, 50);
        engine.tick_inference_depth("depth-standard", 10, None, None);
        let (report, _) = engine.take_report().expect("zero-second report");
        let metrics = &report.model_metrics["depth-standard"];

        assert_eq!(metrics.depth_empty, 1);
        assert_eq!(report.infer_empty, 0);
    }

    /// Aceptación del item "presupuesto de ciclo": un ciclo sintético por
    /// encima del presupuesto incrementa cycle_overruns y sale en el
    /// reporte con p95 y max.
    #[test]
    fn cycle_overruns_trip_above_budget_and_show_in_report() {
        let start = Instant::now();
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);
        let mut engine = MetricsEngine::new_at(0, 50, start);

        engine.tick_cycle_at(at(40), false); // ocioso 40ms: nada
        engine.tick_cycle_at(at(91), true); // trabajo 51ms > 50: overrun
        engine.tick_cycle_at(at(111), true); // trabajo 20ms: dentro de budget
        engine.tick_cycle_at(at(171), true); // trabajo 60ms: overrun
        engine.tick_cycle_at(at(221), false); // ocioso 50ms: ocio nunca overruns

        let (report, _) = engine.take_report().expect("zero-second report");
        assert_eq!(report.cycles, 5);
        assert_eq!(report.cycle_overruns, 2);
        assert_eq!(report.cycle_min_ms, 20);
        assert_eq!(report.cycle_max_ms, 60);
        assert_eq!(report.cycle_p95_ms, 60, "p95 de [40,51,20,60,50]");
        assert_eq!(report.cycle_budget_ms, 50);
    }

    #[test]
    fn keyframe_gap_distribution_is_reported() {
        let start = Instant::now();
        let mut engine = MetricsEngine::new_at(0, 50, start);

        for gap_ms in [40, 80, 120, 200] {
            engine.tick_keyframe(gap_ms);
        }

        let (report, _) = engine.take_report().expect("zero-second report");
        assert_eq!(report.keyframe_gap_min_ms, 40);
        assert_eq!(report.keyframe_gap_p50_ms, 80);
        assert_eq!(report.keyframe_gap_p95_ms, 200);
        assert_eq!(report.keyframe_gap_max_ms, 200);
    }

    #[test]
    fn idle_cycles_never_trip_the_budget() {
        let start = Instant::now();
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);
        let mut engine = MetricsEngine::new_at(0, 50, start);

        engine.tick_cycle_at(at(50), false);
        engine.tick_cycle_at(at(102), false); // 52ms ociosos: espera
        let (report, _) = engine.take_report().expect("zero-second report");
        assert_eq!(report.cycle_overruns, 0, "el ocio nunca declara overrun");
    }

    #[test]
    fn health_enters_stale_at_half_threshold() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        assert_eq!(health.evaluate_at(at(5_000)), HealthTransition::None);
        assert_eq!(
            health.evaluate_at(at(5_001)),
            HealthTransition::Stale {
                component: "ingest",
                ms_since_frame: 5_001,
            }
        );
    }

    #[test]
    fn health_uses_configured_stale_warning_threshold() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 2_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        assert_eq!(health.evaluate_at(at(2_000)), HealthTransition::None);
        assert!(matches!(
            health.evaluate_at(at(2_001)),
            HealthTransition::Stale {
                ms_since_frame: 2_001,
                ..
            }
        ));
    }

    #[test]
    fn health_enters_blind_at_full_threshold() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        assert_eq!(
            health.evaluate_at(at(10_001)),
            HealthTransition::Blind {
                ms_since_frame: 10_001,
            }
        );
        assert!(health.is_blind());
    }

    #[test]
    fn blind_fires_once_while_condition_holds() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        assert_eq!(
            health.evaluate_at(at(10_001)),
            HealthTransition::Blind {
                ms_since_frame: 10_001,
            }
        );
        assert_eq!(health.evaluate_at(at(12_000)), HealthTransition::None);
        assert_eq!(health.evaluate_at(at(60_000)), HealthTransition::None);
        assert!(health.is_blind());
    }

    #[test]
    fn blind_persists_across_intermediate_hysteresis_band() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        let _ = health.evaluate_at(at(10_001));
        assert_eq!(health.evaluate_at(at(7_500)), HealthTransition::None);
        assert!(health.is_blind(), "still blind inside the band");
        assert_eq!(health.evaluate_at(at(5_000)), HealthTransition::Recovered);
        assert!(!health.is_blind());
    }

    #[test]
    fn recovered_fires_once_then_stays_silent() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        let _ = health.evaluate_at(at(10_001));
        assert_eq!(health.evaluate_at(at(4_000)), HealthTransition::Recovered);
        assert_eq!(health.evaluate_at(at(3_000)), HealthTransition::None);
        assert!(!health.is_blind());
    }

    #[test]
    fn superloop_recovery_is_reported_by_touch_not_evaluate() {
        // En el ciclo real el keyframe que vuelve hace touch_at (limpia
        // blind/stale) antes de que evaluate_at corra: la rama Recovered de
        // evaluate jamas se cumple en vivo. La recuperacion la reporta
        // touch_at, y el heartbeat se emite en el touch del frame valido.
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        let _ = health.evaluate_at(at(10_001));
        assert!(health.touch_at(at(11_000)), "frame marks the recovery");
        assert!(!health.is_blind());
        assert_eq!(health.evaluate_at(at(11_000)), HealthTransition::None);
    }

    #[test]
    fn evaluate_at_saturates_when_clock_goes_backwards() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        assert_eq!(
            health.evaluate_at(start - std::time::Duration::from_millis(100)),
            HealthTransition::None
        );
    }
}
