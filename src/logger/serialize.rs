use super::event::Event;

pub fn write_event(event: &Event, ts: &str, buf: &mut Vec<u8>) {
    buf.clear();
    buf.extend_from_slice(b"{\"t\":\"");
    buf.extend_from_slice(ts.as_bytes());
    buf.extend_from_slice(b"\",");
    match event {
        Event::Meta {
            event,
            detail,
            attrs,
        } => {
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
        Event::Health {
            event,
            frame_id,
            cycle_us,
            message,
        } => {
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
        Event::Frame {
            frame_id,
            is_keyframe,
            decode_ms,
            gap_ms,
        } => {
            buf.extend_from_slice(b"\"type\":\"frame\",\"frame_id\":");
            write_u64(*frame_id, buf);
            buf.extend_from_slice(b",\"is_keyframe\":");
            buf.extend_from_slice(if *is_keyframe { b"true" } else { b"false" });
            buf.extend_from_slice(b",\"decode_ms\":");
            write_u64(*decode_ms, buf);
            buf.extend_from_slice(b",\"gap_ms\":");
            write_u64(*gap_ms, buf);
        }
        Event::Detection {
            frame_id,
            model,
            infer_ms,
            pipeline_ms,
            detections,
            postprocess_rejected,
            post_nms_suppressed,
            per_class,
            crop,
        } => {
            buf.extend_from_slice(b"\"type\":\"detection\",\"frame_id\":");
            write_u64(*frame_id, buf);
            buf.extend_from_slice(b",\"model\":\"");
            write_json_string(model, buf);
            buf.extend_from_slice(b"\",\"infer_ms\":");
            write_u64(*infer_ms, buf);
            buf.extend_from_slice(b",\"pipeline_ms\":");
            write_u64(*pipeline_ms, buf);
            buf.extend_from_slice(b",\"post_rejected\":");
            write_u64(*postprocess_rejected as u64, buf);
            buf.extend_from_slice(b",\"post_nms_suppressed\":");
            write_u64(*post_nms_suppressed as u64, buf);
            if let Some([x1, y1, x2, y2]) = crop {
                buf.extend_from_slice(b",\"crop\":[");
                write_u64(*x1 as u64, buf);
                buf.push(b',');
                write_u64(*y1 as u64, buf);
                buf.push(b',');
                write_u64(*x2 as u64, buf);
                buf.push(b',');
                write_u64(*y2 as u64, buf);
                buf.push(b']');
            }
            buf.extend_from_slice(b",\"det\":[");
            for (i, d) in detections.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                buf.extend_from_slice(b"{\"class\":\"");
                write_json_string(&d.class, buf);
                buf.extend_from_slice(b"\",\"confidence\":");
                write_f32(d.confidence, buf);
                buf.extend_from_slice(b",\"bbox\":[");
                for (j, v) in d.bbox.iter().enumerate() {
                    if j > 0 {
                        buf.push(b',');
                    }
                    write_f32(*v, buf);
                }
                buf.extend_from_slice(b"]");
                if let Some(mask) = &d.mask {
                    buf.extend_from_slice(b",\"mask\":{\"rle\":[");
                    for (j, count) in mask.rle.iter().enumerate() {
                        if j > 0 {
                            buf.push(b',');
                        }
                        write_u64(*count as u64, buf);
                    }
                    buf.extend_from_slice(b"],\"bbox\":[");
                    for (j, v) in mask.bbox.iter().enumerate() {
                        if j > 0 {
                            buf.push(b',');
                        }
                        write_f32(*v, buf);
                    }
                    buf.extend_from_slice(b"],\"origin\":[");
                    for (j, v) in mask.origin.iter().enumerate() {
                        if j > 0 {
                            buf.push(b',');
                        }
                        write_u64(*v as u64, buf);
                    }
                    buf.extend_from_slice(b"],\"mask_dims\":[");
                    for (j, v) in mask.mask_dims.iter().enumerate() {
                        if j > 0 {
                            buf.push(b',');
                        }
                        write_u64(*v as u64, buf);
                    }
                    buf.extend_from_slice(b"],\"polygons\":[");
                    for (j, poly) in mask.polygons.iter().enumerate() {
                        if j > 0 {
                            buf.push(b',');
                        }
                        buf.push(b'[');
                        for (k, vertex) in poly.iter().enumerate() {
                            if k > 0 {
                                buf.push(b',');
                            }
                            buf.push(b'[');
                            write_f32(vertex[0], buf);
                            buf.push(b',');
                            write_f32(vertex[1], buf);
                            buf.push(b']');
                        }
                        buf.push(b']');
                    }
                    buf.extend_from_slice(b"]}");
                }
                buf.extend_from_slice(b"}");
            }
            buf.extend_from_slice(b"]");
            if let Some(pc) = per_class {
                if !pc.stats.is_empty() {
                    buf.extend_from_slice(b",\"per_class\":{");
                    let mut first = true;
                    for (cls, stat) in &pc.stats {
                        if !first {
                            buf.push(b',');
                        }
                        first = false;
                        buf.extend_from_slice(b"\"");
                        buf.extend_from_slice(cls.as_bytes());
                        buf.extend_from_slice(b"\":{\"count\":");
                        write_u64(stat.count, buf);
                        buf.extend_from_slice(b",\"conf_min\":");
                        write_f32(stat.conf_min, buf);
                        buf.extend_from_slice(b",\"conf_max\":");
                        write_f32(stat.conf_max, buf);
                        buf.extend_from_slice(b",\"area_min\":");
                        write_f64(stat.area_min, buf);
                        buf.extend_from_slice(b",\"area_max\":");
                        write_f64(stat.area_max, buf);
                        buf.extend_from_slice(b"}");
                    }
                    buf.extend_from_slice(b"}");
                }
            }
        }
        Event::Depth {
            version,
            frame_id,
            model,
            infer_ms,
            pipeline_ms,
            roi,
            map_width,
            map_height,
            valid_pixels,
            valid_ratio,
            min_depth_m,
            max_depth_m,
        } => {
            buf.extend_from_slice(b"\"type\":\"depth\",\"version\":");
            write_u64(*version as u64, buf);
            buf.extend_from_slice(b",\"frame_id\":");
            write_u64(*frame_id, buf);
            buf.extend_from_slice(b",\"model\":\"");
            write_json_string(model, buf);
            buf.extend_from_slice(b"\",\"infer_ms\":");
            write_u64(*infer_ms, buf);
            buf.extend_from_slice(b",\"pipeline_ms\":");
            write_u64(*pipeline_ms, buf);
            buf.extend_from_slice(b",\"roi\":");
            if let Some([x1, y1, x2, y2]) = roi {
                buf.extend_from_slice(b"[");
                write_u64(*x1 as u64, buf);
                buf.extend_from_slice(b",");
                write_u64(*y1 as u64, buf);
                buf.extend_from_slice(b",");
                write_u64(*x2 as u64, buf);
                buf.extend_from_slice(b",");
                write_u64(*y2 as u64, buf);
                buf.extend_from_slice(b"]");
            } else {
                buf.extend_from_slice(b"null");
            }
            buf.extend_from_slice(b",\"map_width\":");
            write_u64(*map_width as u64, buf);
            buf.extend_from_slice(b",\"map_height\":");
            write_u64(*map_height as u64, buf);
            buf.extend_from_slice(b",\"valid_pixels\":");
            write_u64(*valid_pixels, buf);
            buf.extend_from_slice(b",\"valid_ratio\":");
            if let Some(value) = valid_ratio {
                write_f32(*value, buf);
            } else {
                buf.extend_from_slice(b"null");
            }
            buf.extend_from_slice(b",\"min_depth_m\":");
            if let Some(value) = min_depth_m {
                write_f32(*value, buf);
            } else {
                buf.extend_from_slice(b"null");
            }
            buf.extend_from_slice(b",\"max_depth_m\":");
            if let Some(value) = max_depth_m {
                write_f32(*value, buf);
            } else {
                buf.extend_from_slice(b"null");
            }
        }
        Event::DepthRegion {
            version,
            frame_id,
            rule,
            region,
            metric,
            value,
            threshold_m,
            triggered,
            valid_pixels,
            valid_ratio,
            calibration,
        } => {
            buf.extend_from_slice(b"\"type\":\"depth_region\",\"version\":");
            write_u64(*version as u64, buf);
            buf.extend_from_slice(b",\"frame_id\":");
            write_u64(*frame_id, buf);
            buf.extend_from_slice(b",\"rule\":\"");
            write_json_string(rule, buf);
            buf.extend_from_slice(b"\",\"region\":[");
            write_u64(u64::from(region[0]), buf);
            buf.extend_from_slice(b",");
            write_u64(u64::from(region[1]), buf);
            buf.extend_from_slice(b",");
            write_u64(u64::from(region[2]), buf);
            buf.extend_from_slice(b",");
            write_u64(u64::from(region[3]), buf);
            buf.extend_from_slice(b"],\"metric\":\"");
            write_json_string(metric, buf);
            buf.extend_from_slice(b"\",\"value\":");
            if let Some(value) = value {
                write_f32(*value, buf);
            } else {
                buf.extend_from_slice(b"null");
            }
            buf.extend_from_slice(b",\"threshold_m\":");
            write_f32(*threshold_m, buf);
            buf.extend_from_slice(b",\"triggered\":");
            buf.extend_from_slice(if *triggered { b"true" } else { b"false" });
            buf.extend_from_slice(b",\"valid_pixels\":");
            write_u64(*valid_pixels, buf);
            buf.extend_from_slice(b",\"valid_ratio\":");
            if let Some(value) = valid_ratio {
                write_f32(*value, buf);
            } else {
                buf.extend_from_slice(b"null");
            }
            buf.extend_from_slice(b",\"calibration\":");
            if let Some(calibration) = calibration {
                buf.extend_from_slice(b"{\"reference_model_m\":");
                write_f32(calibration.reference_model_m, buf);
                buf.extend_from_slice(b",\"reference_scene_m\":");
                write_f32(calibration.reference_scene_m, buf);
                buf.push(b'}');
            } else {
                buf.extend_from_slice(b"null");
            }
        }
        Event::ConsolidatedDetection {
            frame_id,
            class,
            confidence,
            bbox,
            primary_model,
            sources,
        } => {
            buf.extend_from_slice(b"\"type\":\"consolidated_detection\",\"frame_id\":");
            write_u64(*frame_id, buf);
            buf.extend_from_slice(b",\"class\":\"");
            write_json_string(class, buf);
            buf.extend_from_slice(b"\",\"confidence\":");
            write_f32(*confidence, buf);
            buf.extend_from_slice(b",\"bbox\":[");
            for (i, value) in bbox.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                write_f32(*value, buf);
            }
            buf.extend_from_slice(b"],\"primary_model\":\"");
            write_json_string(primary_model, buf);
            buf.extend_from_slice(b"\",\"sources\":[");
            for (i, source) in sources.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                buf.push(b'\"');
                write_json_string(source, buf);
                buf.push(b'\"');
            }
            buf.extend_from_slice(b"]");
        }
        Event::Entity {
            track_id,
            class,
            bbox,
            sources,
            frame_id,
        } => {
            buf.extend_from_slice(b"\"type\":\"entity\",\"track_id\":");
            write_u64(*track_id, buf);
            buf.extend_from_slice(b",\"class\":\"");
            write_json_string(class, buf);
            buf.extend_from_slice(b"\",\"bbox\":[");
            for (i, value) in bbox.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                write_f32(*value, buf);
            }
            buf.extend_from_slice(b"],\"sources\":[");
            for (i, source) in sources.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                buf.push(b'\"');
                write_json_string(source, buf);
                buf.push(b'\"');
            }
            buf.extend_from_slice(b"],\"frame_id\":");
            write_u64(*frame_id, buf);
        }
        Event::Zone {
            zone,
            event,
            class,
            label,
            confidence,
            frame_id,
        } => {
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
        Event::Fsm {
            from,
            from_label,
            to,
            to_label,
            trigger,
            dwell_ms,
        } => {
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
        Event::Presence {
            frame_id,
            keyframe_gap_ms,
            source_window_ms,
            keyframes_seen,
            keyframes_dropped,
            state,
            poi_state,
            second_person,
            raw_count,
            confirmed_count,
            signal_valid,
            held,
            poi_positive_ticks,
            poi_empty_ticks,
            single_timer_ms,
            empty_timer_ms,
            multiple_candidate_timer_ms,
            multiple_exit_timer_ms,
        } => {
            buf.extend_from_slice(b"\"type\":\"presence\",\"frame_id\":");
            write_u64(*frame_id, buf);
            buf.extend_from_slice(b",\"keyframe_gap_ms\":");
            write_u64(*keyframe_gap_ms, buf);
            buf.extend_from_slice(b",\"source_window_ms\":");
            write_u64(*source_window_ms, buf);
            buf.extend_from_slice(b",\"keyframes_seen\":");
            write_u64(*keyframes_seen, buf);
            buf.extend_from_slice(b",\"keyframes_dropped\":");
            write_u64(*keyframes_dropped, buf);
            buf.extend_from_slice(b",\"state\":\"");
            write_json_string(state, buf);
            buf.extend_from_slice(b"\",\"poi_state\":\"");
            write_json_string(poi_state, buf);
            buf.extend_from_slice(b"\",\"second_person\":\"");
            write_json_string(second_person, buf);
            buf.extend_from_slice(b"\",\"raw_count\":");
            write_u64(*raw_count as u64, buf);
            buf.extend_from_slice(b",\"confirmed_count\":");
            write_u64(*confirmed_count as u64, buf);
            buf.extend_from_slice(b",\"signal_valid\":");
            buf.extend_from_slice(if *signal_valid { b"true" } else { b"false" });
            buf.extend_from_slice(b",\"held\":");
            buf.extend_from_slice(if *held { b"true" } else { b"false" });
            buf.extend_from_slice(b",\"poi_positive_ticks\":");
            write_u64(*poi_positive_ticks as u64, buf);
            buf.extend_from_slice(b",\"poi_empty_ticks\":");
            write_u64(*poi_empty_ticks as u64, buf);
            buf.extend_from_slice(b",\"single_timer_ms\":");
            write_u64(*single_timer_ms, buf);
            buf.extend_from_slice(b",\"empty_timer_ms\":");
            write_u64(*empty_timer_ms, buf);
            buf.extend_from_slice(b",\"multiple_candidate_timer_ms\":");
            write_u64(*multiple_candidate_timer_ms, buf);
            buf.extend_from_slice(b",\"multiple_exit_timer_ms\":");
            write_u64(*multiple_exit_timer_ms, buf);
        }
        Event::Metrics(r) => {
            buf.extend_from_slice(b"\"type\":\"metrics\"");
            append_field(buf, "window_s", r.window_s);
            append_field(buf, "cycles", r.cycles);
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
            buf.extend_from_slice(b",\"models\":{");
            let mut first_model = true;
            for (name, m) in &r.model_metrics {
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
