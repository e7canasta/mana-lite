use std::collections::HashMap;
use std::time::Instant;

use crate::infer::Detection;
use ultralytics_inference::DepthMap;

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

impl Metrics {
    pub fn into_report(self, window_s: u64) -> MetricsReport {
        MetricsReport {
            window_s,
            cycles: self.cycles,
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
}

impl MetricsEngine {
    pub fn new(report_interval_s: u64) -> Self {
        Self {
            current: Metrics::default(),
            model_order: Vec::new(),
            window_start: Instant::now(),
            report_interval_s,
        }
    }

    pub fn tick_cycle(&mut self) {
        self.current.cycles += 1;
    }

    pub fn tick_keyframe(&mut self) {
        self.current.keyframes += 1;
        self.current.frames_total += 1;
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
        depth: Option<&DepthMap>,
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
            for &value in &map.data {
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
        let report = std::mem::take(&mut self.current).into_report(elapsed);
        self.window_start = Instant::now();
        Some((report, order))
    }
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
    blind: bool,
    stale: bool,
}

impl Health {
    pub fn new(data_stale_ms: u64) -> Self {
        Self {
            last_frame_at: Instant::now(),
            data_stale_ms,
            blind: false,
            stale: false,
        }
    }

    pub fn touch(&mut self) {
        self.last_frame_at = Instant::now();
        self.blind = false;
        self.stale = false;
    }

    pub fn evaluate(&mut self) -> HealthTransition {
        let stale_ms = self.last_frame_at.elapsed().as_millis() as u64;

        if stale_ms > self.data_stale_ms {
            if !self.blind {
                self.blind = true;
                return HealthTransition::Blind {
                    ms_since_frame: stale_ms,
                };
            }
        } else if stale_ms > self.data_stale_ms / 2 {
            if !self.blind && !self.stale {
                self.stale = true;
                return HealthTransition::Stale {
                    component: "ingest",
                    ms_since_frame: stale_ms,
                };
            }
        }

        if (self.blind || self.stale) && stale_ms <= self.data_stale_ms / 2 {
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

    #[test]
    fn depth_metrics_count_only_finite_positive_pixels() {
        let mut engine = MetricsEngine::new(0);
        let depth = DepthMap::new(
            array![[0.0, 1.0, 2.0], [f32::NAN, 3.0, f32::INFINITY]],
            (2, 3),
        );

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
        let mut engine = MetricsEngine::new(0);
        engine.tick_inference_depth("depth-standard", 10, None, None);
        let (report, _) = engine.take_report().expect("zero-second report");
        let metrics = &report.model_metrics["depth-standard"];

        assert_eq!(metrics.depth_empty, 1);
        assert_eq!(report.infer_empty, 0);
    }
}
