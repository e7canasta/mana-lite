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
        let events: Vec<Event> = self.buffer.drain(..).collect();
        let mut buf = Vec::with_capacity(512);
        for event in &events {
            write_event(event, &now, &mut buf);
            let _ = self.write_buf(&buf);
        }
        self.flush_target();
    }

    fn flush_target(&mut self) {
        match &mut self.target {
            OutputTarget::Stdout(w) => { let _ = w.flush(); }
            OutputTarget::File(w) => { let _ = w.flush(); }
        }
    }

    fn write_buf(&mut self, buf: &[u8]) -> io::Result<()> {
        match &mut self.target {
            OutputTarget::Stdout(w) => w.write_all(buf),
            OutputTarget::File(w) => w.write_all(buf),
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

    #[doc(hidden)]
    pub fn flush_to_buffer(&mut self, out: &mut Vec<u8>) {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        for event in self.buffer.drain(..) {
            write_event(&event, &now, out);
        }
    }
}

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
    Detection {
        f: u64,
        m: String,
        inf_ms: u64,
        det: Vec<DetRecord>,
    },
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
}

fn write_event(event: &Event, ts: &str, buf: &mut Vec<u8>) {
    buf.clear();
    buf.extend_from_slice(b"{\"t\":\"");
    buf.extend_from_slice(ts.as_bytes());
    buf.extend_from_slice(b"\",");
    match event {
        Event::Meta { event, detail, attrs } => {
            buf.extend_from_slice(b"\"type\":\"meta\",\"event\":\"");
            write_json_string(event, buf);
            buf.extend_from_slice(b"\",\"detail\":\"");
            write_json_string(detail, buf);
            buf.extend_from_slice(b"\"");
            for (k, v) in attrs {
                buf.extend_from_slice(b",\"");
                buf.extend_from_slice(k.as_bytes());
                buf.extend_from_slice(b"\":\"");
                write_json_string(v, buf);
                buf.extend_from_slice(b"\"");
            }
        }
        Event::Health { event, f, cyc_us, msg } => {
            buf.extend_from_slice(b"\"type\":\"health\",\"event\":\"");
            write_json_string(event, buf);
            buf.extend_from_slice(b"\"");
            if let Some(frame) = f {
                buf.extend_from_slice(b",\"f\":");
                write_u64(*frame, buf);
            }
            if let Some(cyc) = cyc_us {
                buf.extend_from_slice(b",\"cyc_us\":");
                write_u64(*cyc, buf);
            }
            if let Some(m) = msg {
                buf.extend_from_slice(b",\"msg\":\"");
                write_json_string(m, buf);
                buf.extend_from_slice(b"\"");
            }
        }
        Event::Frame { f, kf, dec_ms } => {
            buf.extend_from_slice(b"\"type\":\"frame\",\"f\":");
            write_u64(*f, buf);
            buf.extend_from_slice(b",\"kf\":");
            buf.extend_from_slice(if *kf { b"true" } else { b"false" });
            buf.extend_from_slice(b",\"dec_ms\":");
            write_u64(*dec_ms, buf);
        }
        Event::Detection { f, m, inf_ms, det } => {
            buf.extend_from_slice(b"\"type\":\"detection\",\"f\":");
            write_u64(*f, buf);
            buf.extend_from_slice(b",\"m\":\"");
            write_json_string(m, buf);
            buf.extend_from_slice(b"\",\"inf_ms\":");
            write_u64(*inf_ms, buf);
            buf.extend_from_slice(b",\"det\":[");
            for (i, d) in det.iter().enumerate() {
                if i > 0 { buf.push(b','); }
                buf.extend_from_slice(b"{\"c\":\"");
                write_json_string(&d.c, buf);
                buf.extend_from_slice(b"\",\"conf\":");
                write_f32(d.conf, buf);
                buf.extend_from_slice(b",\"bb\":[");
                for (j, v) in d.bb.iter().enumerate() {
                    if j > 0 { buf.push(b','); }
                    write_f32(*v, buf);
                }
                buf.extend_from_slice(b"]}");
            }
            buf.extend_from_slice(b"]");
        }
        Event::Zone { z, e, cls, f } => {
            buf.extend_from_slice(b"\"type\":\"zone\",\"z\":\"");
            write_json_string(z, buf);
            buf.extend_from_slice(b"\",\"e\":\"");
            write_json_string(e, buf);
            buf.extend_from_slice(b"\",\"cls\":\"");
            write_json_string(cls, buf);
            buf.extend_from_slice(b"\",\"f\":");
            write_u64(*f, buf);
        }
        Event::Fsm { from, to, tr, dwell } => {
            buf.extend_from_slice(b"\"type\":\"fsm\",\"from\":\"");
            write_json_string(from, buf);
            buf.extend_from_slice(b"\",\"to\":\"");
            write_json_string(to, buf);
            buf.extend_from_slice(b"\",\"tr\":\"");
            write_json_string(tr, buf);
            buf.extend_from_slice(b"\",\"dwell\":");
            write_u64(*dwell, buf);
        }
    }
    buf.extend_from_slice(b"}\n");
}

fn write_json_string(s: &str, buf: &mut Vec<u8>) {
    let bytes = s.as_bytes();
    let mut last = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let esc: Option<&[u8]> = match b {
            b'"' => Some(b"\\\""),
            b'\\' => Some(b"\\\\"),
            b'\n' => Some(b"\\n"),
            b'\r' => Some(b"\\r"),
            b'\t' => Some(b"\\t"),
            c if c < 0x20 => {
                buf.extend_from_slice(&bytes[last..i]);
                buf.extend_from_slice(format!("\\u{:04x}", c).as_bytes());
                last = i + 1;
                continue;
            }
            _ => None,
        };
        if let Some(escaped) = esc {
            buf.extend_from_slice(&bytes[last..i]);
            buf.extend_from_slice(escaped);
            last = i + 1;
        }
    }
    buf.extend_from_slice(&bytes[last..]);
}

fn write_u64(n: u64, buf: &mut Vec<u8>) {
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

fn write_f32(v: f32, buf: &mut Vec<u8>) {
    if v.is_nan() || v.is_infinite() {
        buf.extend_from_slice(b"null");
        return;
    }
    let neg = v < 0.0;
    let abs = if neg { -v } else { v };
    let scaled = (abs * 100.0 + 0.5) as u64;
    let int_part = scaled / 100;
    let frac_part = scaled % 100;
    if neg { buf.push(b'-'); }
    write_u64(int_part, buf);
    buf.push(b'.');
    if frac_part < 10 { buf.push(b'0'); }
    write_u64(frac_part, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(logger: &mut Logger) -> String {
        let mut buf = Vec::new();
        logger.flush_to_buffer(&mut buf);
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn meta_startup_has_type_and_version() {
        let mut log = Logger::new();
        log.emit(Event::meta_startup("0.1.0", "mana.toml"));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"meta\""));
        assert!(out.contains("\"event\":\"startup\""));
        assert!(out.contains("\"v\":\"0.1.0\""));
    }

    #[test]
    fn detection_emits_class_and_bbox() {
        let det = vec![DetRecord { c: "person".into(), conf: 0.87, bb: [100.0, 200.0, 300.0, 500.0] }];
        let mut log = Logger::new();
        log.emit(Event::detection(1, "detect-fast", 52, det));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"detection\""));
        assert!(out.contains("\"c\":\"person\""));
        assert!(out.contains("\"conf\":0.87"));
        assert!(out.contains("\"bb\":[100.00,200.00,300.00,500.00]"));
    }

    #[test]
    fn fsm_transition_has_from_to_trigger() {
        let mut log = Logger::new();
        log.emit(Event::fsm_transition("idle", "monitoring", "bed_occupied", 0));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"fsm\""));
        assert!(out.contains("\"from\":\"idle\""));
        assert!(out.contains("\"to\":\"monitoring\""));
        assert!(out.contains("\"tr\":\"bed_occupied\""));
    }

    #[test]
    fn health_blind_has_message() {
        let mut log = Logger::new();
        log.emit(Event::health_blind(10_000));
        let out = collect(&mut log);
        assert!(out.contains("\"type\":\"health\""));
        assert!(out.contains("\"event\":\"blind\""));
        assert!(out.contains("10000ms"));
    }

    #[test]
    fn escape_json_string_quotes_and_backslash() {
        let mut log = Logger::new();
        log.emit(Event::Meta {
            event: "test".into(),
            detail: "say \"hello\"".into(),
            attrs: vec![("path".into(), "C:\\Users\\test".into())],
        });
        let out = collect(&mut log);
        assert!(out.contains("say \\\"hello\\\""));
        assert!(out.contains("C:\\\\Users\\\\test"));
    }

    #[test]
    fn escape_json_control_chars() {
        let mut log = Logger::new();
        log.emit(Event::Meta {
            event: "test".into(),
            detail: "line1\nline2".into(),
            attrs: vec![],
        });
        let out = collect(&mut log);
        assert!(out.contains("line1\\nline2"));
    }

    #[test]
    fn negative_float_is_valid_json() {
        let det = vec![DetRecord { c: "x".into(), conf: 0.5, bb: [-10.5, 0.0, 100.0, 200.3] }];
        let mut log = Logger::new();
        log.emit(Event::detection(1, "m", 10, det));
        let out = collect(&mut log);
        assert!(out.contains("\"bb\":[-10.50,0.00,100.00,200.30]"));
    }
}
