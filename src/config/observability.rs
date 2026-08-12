use serde::Deserialize;

fn default_true() -> bool {
    true
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct VizDataConfig {
    pub viz: VizDataInner,
}

impl Default for VizDataConfig {
    fn default() -> Self {
        Self {
            viz: VizDataInner::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct VizDataInner {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub rerun_addr: Option<String>,
    #[serde(default)]
    pub send: VizSendToggles,
}

impl Default for VizDataInner {
    fn default() -> Self {
        Self {
            enabled: None,
            rerun_addr: None,
            send: VizSendToggles::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct VizSendToggles {
    #[serde(default = "default_true")]
    pub frames: bool,
    #[serde(default = "default_true")]
    pub boxes: bool,
    #[serde(default)]
    pub crop_frames: bool,
    #[serde(default = "default_true")]
    pub masks: bool,
    #[serde(default)]
    pub mask_debug: bool,
    #[serde(default = "default_true")]
    pub mask_polygons: bool,
    #[serde(default)]
    pub roi_rects: bool,
    #[serde(default = "default_true")]
    pub decode_latency: bool,
    #[serde(default = "default_true")]
    pub infer_latency: bool,
    #[serde(default = "default_true")]
    pub infer_rate: bool,
    #[serde(default = "default_true")]
    pub class_counts_per_frame: bool,
    #[serde(default = "default_true")]
    pub class_confidence_per_frame: bool,
    #[serde(default = "default_true")]
    pub class_area_per_frame: bool,
    #[serde(default = "default_true")]
    pub keyframe_gap: bool,
    #[serde(default = "default_true")]
    pub keyframe_rate: bool,
    #[serde(default = "default_true")]
    pub keyframe_drops: bool,
    #[serde(default)]
    pub depth: bool,
    #[serde(default = "default_depth_viz")]
    pub depth_viz: String,
    #[serde(default = "default_true")]
    pub depth_stats: bool,
}

fn default_depth_viz() -> String {
    "disparity".into()
}

impl Default for VizSendToggles {
    fn default() -> Self {
        Self {
            frames: true,
            boxes: true,
            masks: true,
            mask_debug: false,
            mask_polygons: true,
            crop_frames: false,
            roi_rects: false,
            decode_latency: true,
            infer_latency: true,
            infer_rate: true,
            class_counts_per_frame: true,
            class_confidence_per_frame: true,
            class_area_per_frame: true,
            keyframe_gap: true,
            keyframe_rate: true,
            keyframe_drops: true,
            depth: false,
            depth_viz: default_depth_viz(),
            depth_stats: true,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct MetricsLogConfig {
    pub metrics: MetricsInner,
}

impl Default for MetricsLogConfig {
    fn default() -> Self {
        Self {
            metrics: MetricsInner::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct MetricsInner {
    #[serde(default = "default_report_interval_s")]
    pub report_interval_s: u64,
    #[serde(default)]
    pub text: MetricsTextConfig,
    #[serde(default)]
    pub jsonl: MetricsJsonlConfig,
}

impl Default for MetricsInner {
    fn default() -> Self {
        Self {
            report_interval_s: 5,
            text: MetricsTextConfig::default(),
            jsonl: MetricsJsonlConfig::default(),
        }
    }
}

fn default_report_interval_s() -> u64 {
    5
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct MetricsTextConfig {
    #[serde(default = "default_true")]
    pub ingest_line: bool,
    #[serde(default = "default_true")]
    pub infer_summary: bool,
    #[serde(default = "default_true")]
    pub per_model_lines: bool,
    /// Reporte del scan (periodo p95/min/max y overruns contra el
    /// presupuesto declarado en [health] `cycle_budget_ms`).
    #[serde(default = "default_true")]
    pub cycle_line: bool,
    /// Cumplimiento de cadencia del scan (atraso min/p95/max contra el
    /// vencimiento y cuántos vencimientos se incumplieron). Eje distinto del
    /// de `cycle_line`, que mide el periodo.
    #[serde(default = "default_true")]
    pub deadline_line: bool,
    /// Edad de la evidencia en el momento de decidir. Es la única línea del
    /// reporte que mide una magnitud clínica y no salud del motor.
    #[serde(default = "default_true")]
    pub evidence_line: bool,
    #[serde(default = "default_true")]
    pub keyframe_gap_line: bool,
    #[serde(default)]
    pub flags: MetricsTextFlags,
}

impl Default for MetricsTextConfig {
    fn default() -> Self {
        Self {
            ingest_line: true,
            infer_summary: true,
            per_model_lines: true,
            cycle_line: true,
            deadline_line: true,
            evidence_line: true,
            keyframe_gap_line: true,
            flags: MetricsTextFlags::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct MetricsTextFlags {
    #[serde(default = "default_true")]
    pub ingest_pframes: bool,
    #[serde(default = "default_true")]
    pub ingest_dup: bool,
    #[serde(default = "default_true")]
    pub ingest_keyframe_drops: bool,
    #[serde(default = "default_true")]
    pub ingest_timeouts: bool,
    #[serde(default = "default_true")]
    pub ingest_reconnect: bool,
    #[serde(default = "default_true")]
    pub ingest_ssrc: bool,
    #[serde(default = "default_true")]
    pub ingest_rtp: bool,
    #[serde(default = "default_true")]
    pub infer_skips: bool,
    #[serde(default = "default_true")]
    pub infer_empty: bool,
}

impl Default for MetricsTextFlags {
    fn default() -> Self {
        Self {
            ingest_pframes: true,
            ingest_dup: true,
            ingest_keyframe_drops: true,
            ingest_timeouts: true,
            ingest_reconnect: true,
            ingest_ssrc: true,
            ingest_rtp: true,
            infer_skips: true,
            infer_empty: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct MetricsJsonlConfig {
    #[serde(default = "default_true")]
    pub frame_events: bool,
    #[serde(default = "default_true")]
    pub detection_events: bool,
    #[serde(default = "default_true")]
    pub zone_events: bool,
    #[serde(default = "default_true")]
    pub fsm_events: bool,
    #[serde(default = "default_true")]
    pub presence_events: bool,
    #[serde(default = "default_true")]
    pub face_dwell_events: bool,
    #[serde(default = "default_true")]
    pub scene_signals_events: bool,
    #[serde(default = "default_true")]
    pub metrics_event: bool,
    #[serde(default = "default_true")]
    pub per_model_in_window: bool,
    #[serde(default = "default_true")]
    pub class_counts_in_window: bool,
    #[serde(default = "default_true")]
    pub class_per_frame_stats: bool,
    #[serde(default = "default_true")]
    pub depth_events: bool,
}

impl Default for MetricsJsonlConfig {
    fn default() -> Self {
        Self {
            frame_events: true,
            detection_events: true,
            zone_events: true,
            fsm_events: true,
            presence_events: true,
            face_dwell_events: true,
            scene_signals_events: true,
            metrics_event: true,
            per_model_in_window: true,
            class_counts_in_window: true,
            class_per_frame_stats: true,
            depth_events: true,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct RerunBlueprintConfig {
    pub rerun: RerunRoot,
}

impl Default for RerunBlueprintConfig {
    fn default() -> Self {
        Self {
            rerun: RerunRoot::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RerunRoot {
    #[serde(default = "default_rerun_app")]
    pub app: String,
    #[serde(default = "default_max_bytes")]
    pub max_bytes_in_flight_mb: usize,
    #[serde(default = "default_true")]
    pub auto_views: bool,
    #[serde(default = "default_true")]
    pub panels_expanded: bool,
    #[serde(default)]
    pub rows: Vec<RerunRow>,
}

#[allow(dead_code)]
fn default_rerun_app() -> String {
    "mana-lite".into()
}

#[allow(dead_code)]
fn default_max_bytes() -> usize {
    32
}

impl Default for RerunRoot {
    fn default() -> Self {
        Self {
            app: default_rerun_app(),
            max_bytes_in_flight_mb: default_max_bytes(),
            auto_views: default_true(),
            panels_expanded: default_true(),
            rows: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RerunRow {
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub share: f32,
    #[serde(default)]
    pub overrides: Vec<RerunOverride>,
    #[serde(default)]
    pub panels: Vec<RerunPanel>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RerunOverride {
    pub path: String,
    pub interpolation: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RerunPanel {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub contents: Vec<String>,
}
