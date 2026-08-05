use super::event::Event;

pub fn write_event(event: &Event, ts: &str, buf: &mut Vec<u8>) {
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
        Event::Health { event, frame_id, cycle_us, message } => {
            buf.extend_from_slice(b"\"type\":\"health\",\"event\":\"");
            write_json_string(event, buf);
            buf.extend_from_slice(b"\"");
            if let Some(f) = frame_id {
                buf.extend_from_slice(b",\"frame_id\":");
                write_u64(*f, buf);
            }
            if let Some(cyc) = cycle_us {
                buf.extend_from_slice(b",\"cyc_us\":");
                write_u64(*cyc, buf);
            }
            if let Some(msg) = message {
                buf.extend_from_slice(b",\"msg\":\"");
                write_json_string(msg, buf);
                buf.extend_from_slice(b"\"");
            }
        }
        Event::Frame { frame_id, is_keyframe, decode_ms } => {
            buf.extend_from_slice(b"\"type\":\"frame\",\"frame_id\":");
            write_u64(*frame_id, buf);
            buf.extend_from_slice(b",\"is_keyframe\":");
            buf.extend_from_slice(if *is_keyframe { b"true" } else { b"false" });
            buf.extend_from_slice(b",\"decode_ms\":");
            write_u64(*decode_ms, buf);
        }
        Event::Detection { frame_id, model, infer_ms, detections } => {
            buf.extend_from_slice(b"\"type\":\"detection\",\"frame_id\":");
            write_u64(*frame_id, buf);
            buf.extend_from_slice(b",\"model\":\"");
            write_json_string(model, buf);
            buf.extend_from_slice(b"\",\"infer_ms\":");
            write_u64(*infer_ms, buf);
            buf.extend_from_slice(b",\"det\":[");
            for (i, d) in detections.iter().enumerate() {
                if i > 0 { buf.push(b','); }
                buf.extend_from_slice(b"{\"class\":\"");
                write_json_string(&d.class, buf);
                buf.extend_from_slice(b"\",\"confidence\":");
                write_f32(d.confidence, buf);
                buf.extend_from_slice(b",\"bbox\":[");
                for (j, v) in d.bbox.iter().enumerate() {
                    if j > 0 { buf.push(b','); }
                    write_f32(*v, buf);
                }
                buf.extend_from_slice(b"]}");
            }
            buf.extend_from_slice(b"]");
        }
        Event::Zone { zone, event, class, label, confidence, frame_id } => {
            buf.extend_from_slice(b"\"type\":\"zone\",\"zone\":\"");
            write_json_string(zone, buf);
            buf.extend_from_slice(b"\",\"event\":\"");
            write_json_string(event, buf);
            buf.extend_from_slice(b"\",\"class\":\"");
            write_json_string(class, buf);
            buf.extend_from_slice(b"\"");
            if let Some(l) = label {
                buf.extend_from_slice(b",\"label\":\"");
                write_json_string(l, buf);
                buf.extend_from_slice(b"\"");
            }
            if let Some(c) = confidence {
                buf.extend_from_slice(b",\"confidence\":");
                write_f32(*c, buf);
            }
            buf.extend_from_slice(b",\"frame_id\":");
            write_u64(*frame_id, buf);
        }
        Event::Fsm { from, from_label, to, to_label, trigger, dwell_ms } => {
            buf.extend_from_slice(b"\"type\":\"fsm\",\"from\":\"");
            write_json_string(from, buf);
            buf.extend_from_slice(b"\"");
            if let Some(l) = from_label {
                buf.extend_from_slice(b",\"from_label\":\"");
                write_json_string(l, buf);
                buf.extend_from_slice(b"\"");
            }
            buf.extend_from_slice(b",\"to\":\"");
            write_json_string(to, buf);
            buf.extend_from_slice(b"\"");
            if let Some(l) = to_label {
                buf.extend_from_slice(b",\"to_label\":\"");
                write_json_string(l, buf);
                buf.extend_from_slice(b"\"");
            }
            buf.extend_from_slice(b",\"trigger\":\"");
            write_json_string(trigger, buf);
            buf.extend_from_slice(b"\",\"dwell_ms\":");
            write_u64(*dwell_ms, buf);
        }
        Event::Metrics(r) => {
            buf.extend_from_slice(b"\"type\":\"metrics\"");
            append_field(buf, "window_s", r.window_s);
            append_field(buf, "cycles", r.cycles);
            append_field(buf, "frames_total", r.frames_total);
            append_field(buf, "keyframes", r.keyframes);
            append_field(buf, "pframes_dropped", r.pframes_dropped);
            append_field(buf, "inferences", r.inferences);
            append_field(buf, "infer_total_ms", r.infer_total_ms);
            append_field(buf, "infer_min_ms", r.infer_min_ms);
            append_field(buf, "infer_max_ms", r.infer_max_ms);
            append_field(buf, "decode_total_ms", r.decode_total_ms);
            append_field(buf, "blind_cycles", r.blind_cycles);
            append_field(buf, "timeouts", r.timeouts);
            append_field(buf, "ssrc_changes", r.ssrc_changes);
            append_field(buf, "rtp_errors", r.rtp_errors);
            append_field(buf, "stream_ends", r.stream_ends);
            append_field(buf, "reconnect_attempts", r.reconnect_attempts);
            buf.extend_from_slice(b",\"models\":{");
            let mut first_model = true;
            for (name, m) in &r.model_metrics {
                if !first_model { buf.push(b','); }
                first_model = false;
                buf.extend_from_slice(b"\"");
                buf.extend_from_slice(name.as_bytes());
                buf.extend_from_slice(b"\":{");
                append_field(buf, "calls", m.inferences);
                append_field(buf, "total_ms", m.infer_total_us / 1000);
                append_field(buf, "min_ms", m.infer_min_us / 1000);
                append_field(buf, "max_ms", m.infer_max_us / 1000);
                append_field(buf, "dets", m.total_dets);
                append_field(buf, "skips", m.skips);
                append_field(buf, "empty", m.empty);
                if !m.class_counts.is_empty() {
                    buf.extend_from_slice(b",\"classes\":{");
                    let mut first_class = true;
                    for (cls, count) in &m.class_counts {
                        if !first_class { buf.push(b','); }
                        first_class = false;
                        buf.extend_from_slice(b"\"");
                        buf.extend_from_slice(cls.as_bytes());
                        buf.extend_from_slice(b"\":");
                        write_u64(*count, buf);
                    }
                    buf.extend_from_slice(b"}");
                }
                buf.extend_from_slice(b"}");
            }
            buf.extend_from_slice(b"}");
        }
    }
    buf.extend_from_slice(b"}\n");
}

fn append_field(buf: &mut Vec<u8>, name: &str, value: u64) {
    buf.extend_from_slice(b",\"");
    buf.extend_from_slice(name.as_bytes());
    buf.extend_from_slice(b"\":");
    write_u64(value, buf);
}

pub fn write_json_string(s: &str, buf: &mut Vec<u8>) {
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

pub fn write_u64(n: u64, buf: &mut Vec<u8>) {
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

pub fn write_f32(v: f32, buf: &mut Vec<u8>) {
    if v.is_nan() || v.is_infinite() || v.is_subnormal() {
        buf.extend_from_slice(b"null");
        return;
    }
    use std::io::Write;
    let _ = write!(buf, "{v:.6}");
    let strip = buf.iter().rev().take_while(|&&b| b == b'0').count();
    let dot = buf.iter().rposition(|&b| b == b'.').unwrap_or(buf.len());
    let keep = if strip > 0 && buf.len() - strip > dot { buf.len() - strip } else { buf.len() };
    buf.truncate(keep);
    if buf.ends_with(&[b'.']) {
        buf.truncate(buf.len() - 1);
    }
}
