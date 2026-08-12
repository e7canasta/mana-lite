use super::Event;
use super::writers::{append_field, write_f64, write_json_string, write_u64};
use crate::metrics::PerModelMetrics;
use std::collections::HashMap;

pub(super) fn write_meta_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Meta {
        event,
        detail,
        attrs,
    } = event
    else {
        unreachable!()
    };
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

pub(super) fn write_health_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Health {
        event,
        frame_id,
        cycle_us,
        message,
    } = event
    else {
        unreachable!()
    };
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

/// Se emite bajo `"type":"health"` a propósito: el atraso del lazo se lee
/// junto a `stale` y `blind`, no en la telemetría de rendimiento.
pub(super) fn write_scan_deadline_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::ScanDeadline {
        window_s,
        deadlines,
        missed,
        late_min_us,
        late_p50_us,
        late_p95_us,
        late_max_us,
        tolerance_us,
    } = event
    else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"health\",\"event\":\"scan_deadline\"");
    append_field(buf, "window_s", *window_s);
    append_field(buf, "deadlines", *deadlines);
    append_field(buf, "missed", *missed);
    append_field(buf, "late_min_us", *late_min_us);
    append_field(buf, "late_p50_us", *late_p50_us);
    append_field(buf, "late_p95_us", *late_p95_us);
    append_field(buf, "late_max_us", *late_max_us);
    append_field(buf, "tolerance_us", *tolerance_us);
}

pub(super) fn write_frame_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Frame {
        frame_id,
        is_keyframe,
        decode_ms,
        gap_ms,
    } = event
    else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"frame\",\"frame_id\":");
    write_u64(*frame_id, buf);
    buf.extend_from_slice(b",\"is_keyframe\":");
    buf.extend_from_slice(if *is_keyframe { b"true" } else { b"false" });
    buf.extend_from_slice(b",\"decode_ms\":");
    write_u64(*decode_ms, buf);
    buf.extend_from_slice(b",\"gap_ms\":");
    write_u64(*gap_ms, buf);
}

pub(super) fn write_metrics_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Metrics(r) = event else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"metrics\"");
    append_field(buf, "window_s", r.window_s);
    append_field(buf, "cycles", r.cycles);
    append_field(buf, "cycle_min_ms", r.cycle_min_ms);
    append_field(buf, "cycle_max_ms", r.cycle_max_ms);
    append_field(buf, "cycle_p95_ms", r.cycle_p95_ms);
    append_field(buf, "cycle_overruns", r.cycle_overruns);
    append_field(buf, "cycle_budget_ms", r.cycle_budget_ms);
    append_field(buf, "scan_deadlines", r.scan_deadlines);
    append_field(buf, "scan_late_min_us", r.scan_late_min_us);
    append_field(buf, "scan_late_p50_us", r.scan_late_p50_us);
    append_field(buf, "scan_late_p95_us", r.scan_late_p95_us);
    append_field(buf, "scan_late_max_us", r.scan_late_max_us);
    append_field(buf, "scan_deadlines_missed", r.scan_deadlines_missed);
    append_field(buf, "slot_keyframes_dropped", r.slot_keyframes_dropped);
    append_field(buf, "slot_images_dropped", r.slot_images_dropped);
    append_field(buf, "slot_viz_dropped", r.slot_viz_dropped);
    append_field(buf, "evidence_scans", r.evidence_scans);
    append_field(buf, "evidence_age_min_ms", r.evidence_age_min_ms);
    append_field(buf, "evidence_age_p50_ms", r.evidence_age_p50_ms);
    append_field(buf, "evidence_age_p95_ms", r.evidence_age_p95_ms);
    append_field(buf, "evidence_age_max_ms", r.evidence_age_max_ms);
    append_field(buf, "keyframe_gap_min_ms", r.keyframe_gap_min_ms);
    append_field(buf, "keyframe_gap_p50_ms", r.keyframe_gap_p50_ms);
    append_field(buf, "keyframe_gap_p95_ms", r.keyframe_gap_p95_ms);
    append_field(buf, "keyframe_gap_max_ms", r.keyframe_gap_max_ms);
    append_field(buf, "frames_total", r.frames_total);
    append_field(buf, "keyframes", r.keyframes);
    append_field(buf, "keyframes_seen", r.keyframes_seen);
    append_field(buf, "keyframes_dropped", r.keyframes_dropped);
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
    write_metrics_models(&r.model_metrics, buf);
}

pub(super) fn write_metrics_models(
    model_metrics: &HashMap<String, PerModelMetrics>,
    buf: &mut Vec<u8>,
) {
    buf.extend_from_slice(b",\"models\":{");
    let mut first_model = true;
    for (name, m) in model_metrics {
        if !first_model {
            buf.push(b',');
        }
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
        if m.depth_frames > 0 {
            append_field(buf, "depth_frames", m.depth_frames);
            append_field(buf, "depth_valid_pixels", m.depth_valid_pixels);
            append_field(buf, "depth_empty", m.depth_empty);
            buf.extend_from_slice(b",\"depth_min_m\":");
            write_f64(m.depth_min_m, buf);
            buf.extend_from_slice(b",\"depth_max_m\":");
            write_f64(m.depth_max_m, buf);
        }
        if let Some([x1, y1, x2, y2]) = m.roi {
            buf.extend_from_slice(b",\"roi\":[");
            write_u64(x1 as u64, buf);
            buf.push(b',');
            write_u64(y1 as u64, buf);
            buf.push(b',');
            write_u64(x2 as u64, buf);
            buf.push(b',');
            write_u64(y2 as u64, buf);
            buf.push(b']');
        }
        if !m.class_counts.is_empty() {
            buf.extend_from_slice(b",\"classes\":{");
            let mut first_class = true;
            for (cls, count) in &m.class_counts {
                if !first_class {
                    buf.push(b',');
                }
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
