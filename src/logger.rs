use serde::Serialize;
use std::io::{self, BufWriter, Write};
use std::time::Instant;

pub struct Logger {
    buffer: Vec<Event>,
    started_at: Instant,
    frame_count: u64,
    target: OutputTarget,
}

enum OutputTarget {
    Stdout(BufWriter<io::Stdout>),
    #[allow(dead_code)]
    File(BufWriter<std::fs::File>),
}

impl Logger {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(32),
            started_at: Instant::now(),
            frame_count: 0,
            target: OutputTarget::Stdout(BufWriter::new(io::stdout())),
        }
    }

    pub fn with_file(path: &str) -> io::Result<Self> {
        Ok(Self {
            buffer: Vec::with_capacity(32),
            started_at: Instant::now(),
            frame_count: 0,
            target: OutputTarget::File(BufWriter::new(std::fs::File::create(path)?)),
        })
    }

    pub fn emit(&mut self, event: Event) {
        self.buffer.push(event);
    }

    pub fn set_frame(&mut self, frame: u64) {
        self.frame_count = frame;
    }

    pub fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        match &mut self.target {
            OutputTarget::Stdout(w) => {
                for event in self.buffer.drain(..) {
                    let _ = event.write_line(w, &now);
                }
                let _ = w.flush();
            }
            OutputTarget::File(w) => {
                for event in self.buffer.drain(..) {
                    let _ = event.write_line(w, &now);
                }
                let _ = w.flush();
            }
        }
    }

    pub fn shutdown(&mut self, reason: &str) -> io::Result<()> {
        self.emit(Event::Meta {
            event: "shutdown".into(),
            detail: reason.into(),
            attrs: vec![(
                "uptime".into(),
                self.started_at.elapsed().as_secs().to_string(),
            )],
        });
        self.flush();
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "meta")]
    Meta {
        event: String,
        detail: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        attrs: Vec<(String, String)>,
    },

    #[serde(rename = "health")]
    Health {
        event: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        f: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cyc_us: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        msg: Option<String>,
    },

    #[serde(rename = "frame")]
    Frame {
        f: u64,
        kf: bool,
        dec_ms: u64,
    },

    #[serde(rename = "detection")]
    Detection {
        f: u64,
        m: String,
        inf_ms: u64,
        det: Vec<DetRecord>,
    },

    #[serde(rename = "zone")]
    Zone {
        z: String,
        e: String,
        cls: String,
        f: u64,
    },

    #[serde(rename = "fsm")]
    Fsm {
        from: String,
        to: String,
        tr: String,
        dwell: u64,
    },
}

#[derive(Serialize)]
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

    pub fn detection(frame: u64, model: &str, infer_ms: u64, dets: Vec<DetRecord>) -> Self {
        Event::Detection {
            f: frame,
            m: model.into(),
            inf_ms: infer_ms,
            det: dets,
        }
    }

    pub fn zone_occupied(zone: &str, by_class: &str, frame: u64) -> Self {
        Event::Zone {
            z: zone.into(),
            e: "occupied".into(),
            cls: by_class.into(),
            f: frame,
        }
    }

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

    fn write_line(&self, w: &mut impl Write, ts: &str) -> io::Result<()> {
        let mut buf = Vec::with_capacity(512);
        self.write_prefixed(w, ts, &mut buf)
    }

    fn write_prefixed(&self, w: &mut impl Write, ts: &str, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.clear();
        buf.extend_from_slice(b"{\"t\":\"");
        buf.extend_from_slice(ts.as_bytes());
        buf.extend_from_slice(b"\",");

        match self {
            Event::Meta {
                event,
                detail,
                attrs,
            } => {
                buf.extend_from_slice(b"\"type\":\"meta\",\"event\":\"");
                buf.extend_from_slice(event.as_bytes());
                buf.extend_from_slice(b"\",\"detail\":\"");
                buf.extend_from_slice(detail.as_bytes());
                buf.extend_from_slice(b"\"");
                for (k, v) in *attrs {
                    buf.extend_from_slice(b",\"");
                    buf.extend_from_slice(k.as_bytes());
                    buf.extend_from_slice(b"\":\"");
                    buf.extend_from_slice(v.as_bytes());
                    buf.extend_from_slice(b"\"");
                }
            }
            Event::Health {
                event,
                f,
                cyc_us,
                msg,
            } => {
                buf.extend_from_slice(b"\"type\":\"health\",\"event\":\"");
                buf.extend_from_slice(event.as_bytes());
                buf.extend_from_slice(b"\"");
                if let Some(frame) = f {
                    buf.extend_from_slice(b",\"f\":");
                    itoa_fast(*frame, buf);
                }
                if let Some(cyc) = cyc_us {
                    buf.extend_from_slice(b",\"cyc_us\":");
                    itoa_fast(*cyc, buf);
                }
                if let Some(m) = msg {
                    buf.extend_from_slice(b",\"msg\":\"");
                    buf.extend_from_slice(m.as_bytes());
                    buf.extend_from_slice(b"\"");
                }
            }
            Event::Frame { f, kf, dec_ms } => {
                buf.extend_from_slice(b"\"type\":\"frame\",\"f\":");
                itoa_fast(*f, buf);
                buf.extend_from_slice(b",\"kf\":");
                buf.extend_from_slice(if *kf { b"true" } else { b"false" });
                buf.extend_from_slice(b",\"dec_ms\":");
                itoa_fast(*dec_ms, buf);
            }
            Event::Detection {
                f,
                m,
                inf_ms,
                det,
            } => {
                buf.extend_from_slice(b"\"type\":\"detection\",\"f\":");
                itoa_fast(*f, buf);
                buf.extend_from_slice(b",\"m\":\"");
                buf.extend_from_slice(m.as_bytes());
                buf.extend_from_slice(b"\",\"inf_ms\":");
                itoa_fast(*inf_ms, buf);
                buf.extend_from_slice(b",\"det\":[");
                for (i, d) in det.iter().enumerate() {
                    if i > 0 {
                        buf.push(b',');
                    }
                    buf.extend_from_slice(b"{\"c\":\"");
                    buf.extend_from_slice(d.c.as_bytes());
                    buf.extend_from_slice(b"\",\"conf\":");
                    write_f32_short(d.conf, buf);
                    buf.extend_from_slice(b",\"bb\":[");
                    for (j, v) in d.bb.iter().enumerate() {
                        if j > 0 {
                            buf.push(b',');
                        }
                        write_f32_short(*v, buf);
                    }
                    buf.extend_from_slice(b"]}");
                }
                buf.extend_from_slice(b"]");
            }
            Event::Zone { z, e, cls, f } => {
                buf.extend_from_slice(b"\"type\":\"zone\",\"z\":\"");
                buf.extend_from_slice(z.as_bytes());
                buf.extend_from_slice(b"\",\"e\":\"");
                buf.extend_from_slice(e.as_bytes());
                buf.extend_from_slice(b"\",\"cls\":\"");
                buf.extend_from_slice(cls.as_bytes());
                buf.extend_from_slice(b"\",\"f\":");
                itoa_fast(*f, buf);
            }
            Event::Fsm {
                from,
                to,
                tr,
                dwell,
            } => {
                buf.extend_from_slice(b"\"type\":\"fsm\",\"from\":\"");
                buf.extend_from_slice(from.as_bytes());
                buf.extend_from_slice(b"\",\"to\":\"");
                buf.extend_from_slice(to.as_bytes());
                buf.extend_from_slice(b"\",\"tr\":\"");
                buf.extend_from_slice(tr.as_bytes());
                buf.extend_from_slice(b"\",\"dwell\":");
                itoa_fast(*dwell, buf);
            }
        }
        buf.extend_from_slice(b"}\n");
        w.write_all(buf)
    }
}

fn itoa_fast(n: u64, buf: &mut Vec<u8>) {
    let mut temp = [0u8; 20];
    let mut i = 20;
    let mut v = n;
    if v == 0 {
        buf.push(b'0');
        return;
    }
    while v > 0 {
        i -= 1;
        temp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    buf.extend_from_slice(&temp[i..]);
}

fn write_f32_short(v: f32, buf: &mut Vec<u8>) {
    if v.fract() == 0.0 && v.abs() < 1e7 {
        itoa_fast(v as u64, buf);
    } else {
        let s = format!("{v:.2}");
        buf.extend_from_slice(s.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meta_startup() {
        let mut logger = Logger::new();
        logger.emit(Event::meta_startup("0.1.0", "mana.toml"));
        logger.flush();
    }

    #[test]
    fn test_detection_roundtrip() {
        let det = vec![DetRecord {
            c: "person".into(),
            conf: 0.87,
            bb: [100.0, 200.0, 300.0, 500.0],
        }];
        let mut logger = Logger::new();
        logger.emit(Event::detection(1, "detect-fast", 52, det));
        logger.flush();
    }

    #[test]
    fn test_fsm_transition() {
        let mut logger = Logger::new();
        logger.emit(Event::fsm_transition("idle", "monitoring", "bed_occupied", 0));
        logger.flush();
    }

    #[test]
    fn test_serialize_health_blind() {
        let mut logger = Logger::new();
        logger.emit(Event::health_blind(10_000));
        logger.flush();
    }
}
