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
        Event::Metrics { window_s, cycles, frames_total, keyframes,
            pframes_dropped, inferences, infer_total_ms, decode_total_ms, blind_cycles,
            timeouts, ssrc_changes, rtp_errors, stream_ends, reconnect_attempts } => {
            buf.extend_from_slice(b"\"type\":\"metrics\"");
            buf.extend_from_slice(b",\"window_s\":");
            write_u64(*window_s, buf);
            buf.extend_from_slice(b",\"cycles\":");
            write_u64(*cycles, buf);
            buf.extend_from_slice(b",\"frames_total\":");
            write_u64(*frames_total, buf);
            buf.extend_from_slice(b",\"keyframes\":");
            write_u64(*keyframes, buf);
            buf.extend_from_slice(b",\"pframes_dropped\":");
            write_u64(*pframes_dropped, buf);
            buf.extend_from_slice(b",\"inferences\":");
            write_u64(*inferences, buf);
            buf.extend_from_slice(b",\"infer_total_ms\":");
            write_u64(*infer_total_ms, buf);
            buf.extend_from_slice(b",\"decode_total_ms\":");
            write_u64(*decode_total_ms, buf);
            buf.extend_from_slice(b",\"blind_cycles\":");
            write_u64(*blind_cycles, buf);
            buf.extend_from_slice(b",\"timeouts\":");
            write_u64(*timeouts, buf);
            buf.extend_from_slice(b",\"ssrc_changes\":");
            write_u64(*ssrc_changes, buf);
            buf.extend_from_slice(b",\"rtp_errors\":");
            write_u64(*rtp_errors, buf);
            buf.extend_from_slice(b",\"stream_ends\":");
            write_u64(*stream_ends, buf);
            buf.extend_from_slice(b",\"reconnect_attempts\":");
            write_u64(*reconnect_attempts, buf);
        }
    }
    buf.extend_from_slice(b"}\n");
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
