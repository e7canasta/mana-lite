pub enum Event {
    Meta {
        event: String,
        detail: String,
        attrs: Vec<(String, String)>,
    },
    Health {
        event: String,
        f: Option<u64>,
        cyc_us: Option<u64>,
        msg: Option<String>,
    },
    Frame {
        f: u64,
        kf: bool,
        dec_ms: u64,
    },
    #[allow(dead_code)]
    Detection {
        f: u64,
        m: String,
        inf_ms: u64,
        det: Vec<DetRecord>,
    },
    #[allow(dead_code)]
    Zone {
        z: String,
        e: String,
        cls: String,
        f: u64,
    },
    Fsm {
        from: String,
        to: String,
        tr: String,
        dwell: u64,
    },
    Metrics {
        window_s: u64,
        cycles: u64,
        frames_total: u64,
        keyframes: u64,
        pframes_dropped: u64,
        inferences: u64,
        infer_total_ms: u64,
        decode_total_ms: u64,
        blind_cycles: u64,
        timeouts: u64,
        ssrc_changes: u64,
        rtp_errors: u64,
        stream_ends: u64,
        reconnect_attempts: u64,
    },
}

pub struct DetRecord {
    pub c: String,
    pub conf: f32,
    pub bb: [f32; 4],
}

impl Event {
    pub fn meta_startup(version: &str, config: &str) -> Self {
        Event::Meta {
            event: "startup".into(),
            detail: "mana-lite".into(),
            attrs: vec![("v".into(), version.into()), ("config".into(), config.into())],
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

    #[allow(dead_code)]
    pub fn meta_model_load_failed(model: &str, path: &str, err: &str) -> Self {
        Event::Meta {
            event: "model_load_failed".into(),
            detail: model.into(),
            attrs: vec![("path".into(), path.into()), ("error".into(), err.into())],
        }
    }

    pub fn health_heartbeat(frame: u64, phase: &str, cycle_us: u64) -> Self {
        Event::Health {
            event: "heartbeat".into(),
            f: Some(frame),
            cyc_us: Some(cycle_us),
            msg: Some(format!("phase={phase}")),
        }
    }

    pub fn health_stale(component: &str, ms_since_frame: u64) -> Self {
        Event::Health {
            event: "stale".into(),
            f: None,
            cyc_us: None,
            msg: Some(format!("{component}: {ms_since_frame}ms since last frame")),
        }
    }

    pub fn health_blind(ms_since_frame: u64) -> Self {
        Event::Health {
            event: "blind".into(),
            f: None,
            cyc_us: None,
            msg: Some(format!("No frame for {ms_since_frame}ms, data plane silent")),
        }
    }

    #[allow(dead_code)]
    pub fn health_panic_count(count: u32, max: u32) -> Self {
        Event::Health {
            event: "panic_count".into(),
            f: None,
            cyc_us: None,
            msg: Some(format!("{count}/{max} consecutive panics")),
        }
    }

    pub fn frame_ingest(frame: u64, is_keyframe: bool, decode_ms: u64) -> Self {
        Event::Frame {
            f: frame,
            kf: is_keyframe,
            dec_ms: decode_ms,
        }
    }

    #[allow(dead_code)]
    pub fn detection(frame: u64, model: &str, infer_ms: u64, dets: Vec<DetRecord>) -> Self {
        Event::Detection {
            f: frame,
            m: model.into(),
            inf_ms: infer_ms,
            det: dets,
        }
    }

    #[allow(dead_code)]
    pub fn zone_occupied(zone: &str, by_class: &str, frame: u64) -> Self {
        Event::Zone {
            z: zone.into(),
            e: "occupied".into(),
            cls: by_class.into(),
            f: frame,
        }
    }

    #[allow(dead_code)]
    pub fn zone_vacated(zone: &str, by_class: &str, frame: u64) -> Self {
        Event::Zone {
            z: zone.into(),
            e: "vacated".into(),
            cls: by_class.into(),
            f: frame,
        }
    }

    pub fn fsm_transition(from: &str, to: &str, trigger: &str, dwell_ms: u64) -> Self {
        Event::Fsm {
            from: from.into(),
            to: to.into(),
            tr: trigger.into(),
            dwell: dwell_ms,
        }
    }

    pub fn metrics(
        window_s: u64, cycles: u64, frames_total: u64, keyframes: u64,
        pframes_dropped: u64, inferences: u64, infer_total_ms: u64,
        decode_total_ms: u64, blind_cycles: u64,
        timeouts: u64, ssrc_changes: u64, rtp_errors: u64,
        stream_ends: u64, reconnect_attempts: u64,
    ) -> Self {
        Event::Metrics {
            window_s, cycles, frames_total, keyframes,
            pframes_dropped, inferences, infer_total_ms,
            decode_total_ms, blind_cycles,
            timeouts, ssrc_changes, rtp_errors,
            stream_ends, reconnect_attempts,
        }
    }
}
