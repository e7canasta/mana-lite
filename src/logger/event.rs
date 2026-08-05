use crate::metrics::{MetricsReport, PerClassFrameStats};

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
        detections: Vec<DetRecord>,
        per_class: Option<PerClassFrameStats>,
    },
    Zone {
        zone: String,
        event: String,
        class: String,
        label: Option<String>,
        confidence: Option<f32>,
        frame_id: u64,
    },
    Fsm {
        from: String,
        from_label: Option<String>,
        to: String,
        to_label: Option<String>,
        trigger: String,
        dwell_ms: u64,
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
            Event::Fsm { .. } => JsonlLevel::Info,
            Event::Metrics { .. } => JsonlLevel::Info,
            Event::Frame { .. } => JsonlLevel::Debug,
            Event::Detection { .. } => JsonlLevel::Debug,
            Event::Zone { .. } => JsonlLevel::Debug,
        }
    }
}

pub struct DetRecord {
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
}

impl Event {
    pub fn meta_startup(version: &str, config: &str) -> Self {
        Event::Meta {
            event: "startup".into(),
            detail: "mana-lite".into(),
            attrs: vec![("version".into(), version.into()), ("config".into(), config.into())],
        }
    }

    pub fn meta_model_loaded(model: &str, path: &str, task: &str, warmup_ms: u64) -> Self {
        Event::Meta {
            event: "model_loaded".into(),
            detail: model.into(),
            attrs: vec![
                ("path".into(), path.into()),
                ("task".into(), task.into()),
                ("warmup_ms".into(), warmup_ms.to_string()),
            ],
        }
    }

    pub fn health_heartbeat(frame: u64, phase: &str, cycle_us: u64) -> Self {
        Event::Health {
            event: "heartbeat".into(),
            frame_id: Some(frame),
            cycle_us: Some(cycle_us),
            message: Some(format!("phase={phase}")),
        }
    }

    pub fn health_stale(component: &str, ms_since_frame: u64) -> Self {
        Event::Health {
            event: "stale".into(),
            frame_id: None,
            cycle_us: None,
            message: Some(format!("{component}: {ms_since_frame}ms since last frame")),
        }
    }

    pub fn health_blind(ms_since_frame: u64) -> Self {
        Event::Health {
            event: "blind".into(),
            frame_id: None,
            cycle_us: None,
            message: Some(format!("No frame for {ms_since_frame}ms, data plane silent")),
        }
    }

    pub fn frame_ingest(frame_id: u64, is_keyframe: bool, decode_ms: u64, gap_ms: u64) -> Self {
        Event::Frame { frame_id, is_keyframe, decode_ms, gap_ms }
    }

    pub fn detection(frame_id: u64, model: &str, infer_ms: u64, detections: Vec<DetRecord>, per_class: Option<PerClassFrameStats>) -> Self {
        Event::Detection { frame_id, model: model.into(), infer_ms, detections, per_class }
    }

    pub fn zone_occupied(zone: &str, label: &str, by_class: &str, confidence: f32, frame_id: u64) -> Self {
        Event::Zone {
            zone: zone.into(),
            event: "occupied".into(),
            class: by_class.into(),
            label: if label == zone { None } else { Some(label.into()) },
            confidence: Some(confidence),
            frame_id,
        }
    }

    pub fn zone_vacated(zone: &str, label: &str, by_class: &str, frame_id: u64) -> Self {
        Event::Zone {
            zone: zone.into(),
            event: "vacated".into(),
            class: by_class.into(),
            label: if label == zone { None } else { Some(label.into()) },
            confidence: None,
            frame_id,
        }
    }

    pub fn fsm_transition(from: &str, from_label: Option<&str>, to: &str, to_label: Option<&str>, trigger: &str, dwell_ms: u64) -> Self {
        Event::Fsm {
            from: from.into(), from_label: from_label.map(str::to_string),
            to: to.into(), to_label: to_label.map(str::to_string),
            trigger: trigger.into(), dwell_ms,
        }
    }

    pub fn metrics(report: MetricsReport) -> Self {
        Event::Metrics(report)
    }
}
