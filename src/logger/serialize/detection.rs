use super::Event;
use super::writers::{write_f32, write_f64, write_json_string, write_u64};
use crate::logger::event::{DetRecord, MaskRecord};
use crate::metrics::PerClassFrameStats;

pub(super) fn write_detection_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Detection {
        frame_id,
        model,
        infer_ms,
        pipeline_ms,
        detections,
        postprocess_rejected,
        post_nms_suppressed,
        per_class,
        crop,
    } = event
    else {
        unreachable!()
    };
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
        write_det_bbox(d, buf);
    }
    buf.extend_from_slice(b"]");
    write_detection_per_class(per_class, buf);
}

pub(super) fn write_det_bbox(d: &DetRecord, buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"{\"class\":\"");
    write_json_string(&d.class, buf);
    buf.extend_from_slice(b"\",\"confidence\":");
    write_f32(d.confidence, buf);
    buf.extend_from_slice(b",\"area_px\":");
    write_f32(d.area_px, buf);
    buf.extend_from_slice(b",\"area_ratio\":");
    write_f32(d.area_ratio, buf);
    buf.extend_from_slice(b",\"bbox\":[");
    for (j, v) in d.bbox.iter().enumerate() {
        if j > 0 {
            buf.push(b',');
        }
        write_f32(*v, buf);
    }
    buf.extend_from_slice(b"]");
    if let Some(mask) = &d.mask {
        write_det_mask(mask, buf);
    }
    buf.extend_from_slice(b"}");
}

pub(super) fn write_det_mask(mask: &MaskRecord, buf: &mut Vec<u8>) {
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

pub(super) fn write_detection_per_class(per_class: &Option<PerClassFrameStats>, buf: &mut Vec<u8>) {
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

pub(super) fn write_depth_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Depth {
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
    } = event
    else {
        unreachable!()
    };
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

pub(super) fn write_depth_region_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::DepthRegion {
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
    } = event
    else {
        unreachable!()
    };
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

pub(super) fn write_consolidated_detection_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::ConsolidatedDetection {
        frame_id,
        class,
        confidence,
        bbox,
        primary_model,
        sources,
    } = event
    else {
        unreachable!()
    };
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
