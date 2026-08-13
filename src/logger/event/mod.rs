mod constructors;
mod records;
mod scene;

pub use records::{
    BodyGeometryRecord, BodyPartRecord, DetRecord, FaceDwellTimerRecord, MaskRecord,
};
pub use scene::{scene_events_to_log, track_event_to_log, zone_event_to_log};

use crate::metrics::{MetricsReport, PerClassFrameStats};
use crate::scan::ControlStamp;
use mana_control::signals::SceneSignalsSnapshot;

/// Version del esquema del evento `depth`. v2 agrega `roi`, `map_width`,
/// `map_height` y `valid_ratio` (contrato `DepthRoiMap`, spec §8/§10).
pub const DEPTH_EVENT_VERSION: u8 = 2;
/// Version of the top-level JSONL event stream schema.
/// v2: control events carry scan_seq / evidence_frame_id / observations_age_ms /
/// depth_age_ms instead of perception coordinates (frame_id, keyframe_gap_ms, …).
pub const JSONL_SCHEMA_VERSION: u8 = 2;

#[derive(Debug, Clone)]
pub enum Event {
    Meta {
        event: String,
        detail: String,
        attrs: Vec<(String, String)>,
    },
    Health {
        event: String,
        frame_id: Option<u64>,
        cycle_us: Option<u64>,
        message: Option<String>,
    },
    /// Cumplimiento de cadencia del lazo sobre una ventana de reporte.
    ///
    /// Es un evento de salud y no de métricas: la pregunta que contesta en una
    /// revisión de incidente —*¿el lazo estaba corriendo a tiempo cuando pasó
    /// esto?*— es del mismo orden que `stale` o `blind`, y como ellos sale
    /// siempre, sin depender de que la telemetría verbosa esté encendida.
    ///
    /// Los números van como campos y no dentro de `message`: un atraso metido
    /// en una cadena de prosa no se puede consultar.
    ScanDeadline {
        window_s: u64,
        deadlines: u64,
        missed: u64,
        late_min_us: u64,
        late_p50_us: u64,
        late_p95_us: u64,
        late_max_us: u64,
        tolerance_us: u64,
    },
    /// Edad de la evidencia sobre la que se decidió, por ventana.
    ///
    /// Sale como `health` y no dentro de `metrics` por la misma razón que
    /// [`Event::ScanDeadline`], sólo que acá pesa más: `metrics_event` está
    /// apagado en los despliegues reales, y ésta es **la única magnitud del
    /// sistema con consecuencia clínica directa**. Una revisión de incidente
    /// que no pueda contestar *"¿de cuándo era lo que vio?"* no puede concluir
    /// nada.
    EvidenceAge {
        window_s: u64,
        scans: u64,
        min_ms: u64,
        p50_ms: u64,
        p95_ms: u64,
        max_ms: u64,
    },
    Frame {
        frame_id: u64,
        is_keyframe: bool,
        decode_ms: u64,
        gap_ms: u64,
    },
    Detection {
        frame_id: u64,
        model: String,
        infer_ms: u64,
        pipeline_ms: u64,
        detections: Vec<DetRecord>,
        postprocess_rejected: usize,
        post_nms_suppressed: usize,
        per_class: Option<PerClassFrameStats>,
        crop: Option<[u32; 4]>,
    },
    Depth {
        version: u8,
        frame_id: u64,
        model: String,
        infer_ms: u64,
        pipeline_ms: u64,
        roi: Option<[u32; 4]>,
        map_width: u32,
        map_height: u32,
        valid_pixels: u64,
        valid_ratio: Option<f32>,
        min_depth_m: Option<f32>,
        max_depth_m: Option<f32>,
    },
    DepthRegion {
        version: u8,
        frame_id: u64,
        rule: String,
        region: [u32; 4],
        metric: String,
        value: Option<f32>,
        threshold_m: f32,
        triggered: bool,
        valid_pixels: u64,
        valid_ratio: Option<f32>,
        calibration: Option<mana_control::DepthCalibration>,
    },
    ConsolidatedDetection {
        frame_id: u64,
        class: String,
        confidence: f32,
        bbox: [f32; 4],
        primary_model: String,
        sources: Vec<String>,
    },
    CrossModelValidation {
        frame_id: u64,
        actor_id: u64,
        quality: f32,
        agreement: f32,
        freshness: f32,
        supporting_sources: Vec<String>,
        contradicting_sources: Vec<String>,
        reasons: Vec<String>,
    },
    BodyParts {
        frame_id: u64,
        actor_id: Option<u64>,
        frame_local_index: Option<usize>,
        overall_quality: f32,
        parts: Vec<BodyPartRecord>,
    },
    Entity {
        track_id: u64,
        class: String,
        bbox: [f32; 4],
        sources: Vec<String>,
        scan_seq: u64,
        evidence_frame_id: u64,
        observations_age_ms: u64,
        depth_age_ms: Option<u64>,
    },
    Zone {
        zone: String,
        event: String,
        class: String,
        label: Option<String>,
        confidence: Option<f32>,
        scan_seq: u64,
        evidence_frame_id: u64,
        observations_age_ms: u64,
        depth_age_ms: Option<u64>,
    },
    Fsm {
        from: String,
        from_label: Option<String>,
        to: String,
        to_label: Option<String>,
        trigger: String,
        dwell_ms: u64,
    },
    Presence {
        scan_seq: u64,
        evidence_frame_id: u64,
        observations_age_ms: u64,
        depth_age_ms: Option<u64>,
        state: String,
        poi_state: String,
        second_person: String,
        raw_count: usize,
        confirmed_count: usize,
        signal_valid: bool,
        held: bool,
        poi_positive_ms: u64,
        poi_empty_ms: u64,
        single_timer_ms: u64,
        empty_timer_ms: u64,
        multiple_candidate_timer_ms: u64,
        multiple_exit_timer_ms: u64,
    },
    SceneSignals {
        stamp: ControlStamp,
        snapshot: SceneSignalsSnapshot,
    },
    FaceDwell {
        scan_seq: u64,
        evidence_frame_id: u64,
        observations_age_ms: u64,
        depth_age_ms: Option<u64>,
        source: String,
        state: String,
        state_label: Option<String>,
        state_dwell_ms: u64,
        state_dwell_required_ms: Option<u64>,
        cardinality: Option<String>,
        person_present: bool,
        face_present: bool,
        face_confidence: Option<f32>,
        face_in_dwell: Option<bool>,
        at_edge: bool,
        face_was_inside: bool,
        face_model_ran: bool,
        active_timers: Vec<FaceDwellTimerRecord>,
    },
    Metrics(MetricsReport),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JsonlLevel {
    Debug = 0,
    Info = 1,
    Quiet = 2,
}

impl JsonlLevel {
    pub fn from_str(s: &str) -> Self {
        match s {
            "debug" => JsonlLevel::Debug,
            "info" => JsonlLevel::Info,
            "quiet" => JsonlLevel::Quiet,
            _ => JsonlLevel::Info,
        }
    }

    pub fn allows(&self, event_level: JsonlLevel) -> bool {
        *self <= event_level
    }
}

impl Event {
    pub fn min_level(&self) -> JsonlLevel {
        match self {
            Event::Meta { .. } => JsonlLevel::Info,
            Event::Health { .. } => JsonlLevel::Info,
            Event::ScanDeadline { .. } => JsonlLevel::Info,
            Event::EvidenceAge { .. } => JsonlLevel::Info,
            Event::Fsm { .. } => JsonlLevel::Info,
            Event::Presence { .. } => JsonlLevel::Debug,
            Event::SceneSignals { .. } => JsonlLevel::Info,
            Event::FaceDwell { .. } => JsonlLevel::Debug,
            Event::Metrics { .. } => JsonlLevel::Info,
            Event::Frame { .. } => JsonlLevel::Debug,
            Event::Detection { .. } => JsonlLevel::Debug,
            Event::Depth { .. } => JsonlLevel::Debug,
            Event::DepthRegion { .. } => JsonlLevel::Debug,
            Event::ConsolidatedDetection { .. } => JsonlLevel::Debug,
            Event::CrossModelValidation { .. } => JsonlLevel::Debug,
            Event::BodyParts { .. } => JsonlLevel::Debug,
            Event::Entity { .. } => JsonlLevel::Debug,
            Event::Zone { .. } => JsonlLevel::Debug,
        }
    }
}
