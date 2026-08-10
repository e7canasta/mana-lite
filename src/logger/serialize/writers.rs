pub(super) fn append_field(buf: &mut Vec<u8>, name: &str, value: u64) {
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

pub(super) fn write_control_stamp(
    scan_seq: u64,
    evidence_frame_id: u64,
    observations_age_ms: u64,
    depth_age_ms: Option<u64>,
    buf: &mut Vec<u8>,
) {
    buf.extend_from_slice(b",\"scan_seq\":");
    write_u64(scan_seq, buf);
    buf.extend_from_slice(b",\"evidence_frame_id\":");
    write_u64(evidence_frame_id, buf);
    buf.extend_from_slice(b",\"observations_age_ms\":");
    write_u64(observations_age_ms, buf);
    buf.extend_from_slice(b",\"depth_age_ms\":");
    match depth_age_ms {
        Some(age) => write_u64(age, buf),
        None => buf.extend_from_slice(b"null"),
    }
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
    let keep = if strip > 0 && buf.len() - strip > dot {
        buf.len() - strip
    } else {
        buf.len()
    };
    buf.truncate(keep);
    if buf.ends_with(&[b'.']) {
        buf.truncate(buf.len() - 1);
    }
}

pub fn write_f64(v: f64, buf: &mut Vec<u8>) {
    if v.is_nan() || v.is_infinite() || v.is_subnormal() {
        buf.extend_from_slice(b"null");
        return;
    }
    use std::io::Write;
    let _ = write!(buf, "{v:.6}");
    let strip = buf.iter().rev().take_while(|&&b| b == b'0').count();
    let dot = buf.iter().rposition(|&b| b == b'.').unwrap_or(buf.len());
    let keep = if strip > 0 && buf.len() - strip > dot {
        buf.len() - strip
    } else {
        buf.len()
    };
    buf.truncate(keep);
    if buf.ends_with(&[b'.']) {
        buf.truncate(buf.len() - 1);
    }
}
