use super::Event;
use super::writers::{write_f32, write_f64, write_json_string, write_u64};
use crate::logger::event::{BodyGeometryRecord, BodyPartRecord, DetRecord, MaskRecord};
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
    if let Some(keypoints) = &d.keypoints {
        write_det_keypoints(keypoints, buf);
    }
    if let Some(mask) = &d.mask {
        write_det_mask(mask, buf);
    }
    buf.extend_from_slice(b"}");
}

pub(super) fn write_det_keypoints(keypoints: &[[f32; 3]], buf: &mut Vec<u8>) {
    buf.extend_from_slice(b",\"keypoints\":[");
    for (j, [x, y, confidence]) in keypoints.iter().enumerate() {
        if j > 0 {
            buf.push(b',');
        }
        buf.push(b'[');
        write_f32(*x, buf);
        buf.push(b',');
        write_f32(*y, buf);
        buf.push(b',');
        write_f32(*confidence, buf);
        buf.push(b']');
    }
    buf.push(b']');
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

pub(super) fn write_cross_model_validation_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::CrossModelValidation {
        frame_id,
        actor_id,
        quality,
        agreement,
        freshness,
        supporting_sources,
        contradicting_sources,
        reasons,
    } = event
    else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"cross_model_validation\",\"frame_id\":");
    write_u64(*frame_id, buf);
    buf.extend_from_slice(b",\"actor_id\":");
    write_u64(*actor_id, buf);
    buf.extend_from_slice(b",\"quality\":");
    write_f32(*quality, buf);
    buf.extend_from_slice(b",\"agreement\":");
    write_f32(*agreement, buf);
    buf.extend_from_slice(b",\"freshness\":");
    write_f32(*freshness, buf);
    write_string_array("supporting_sources", supporting_sources, buf);
    write_string_array("contradicting_sources", contradicting_sources, buf);
    write_string_array("reasons", reasons, buf);
}

pub(super) fn write_body_parts_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::BodyParts {
        frame_id,
        actor_id,
        frame_local_index,
        overall_quality,
        parts,
    } = event
    else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"body_parts\",\"frame_id\":");
    write_u64(*frame_id, buf);
    buf.extend_from_slice(b",\"actor_id\":");
    if let Some(actor_id) = actor_id {
        write_u64(*actor_id, buf);
    } else {
        buf.extend_from_slice(b"null");
    }
    buf.extend_from_slice(b",\"frame_local_index\":");
    if let Some(index) = frame_local_index {
        write_u64(*index as u64, buf);
    } else {
        buf.extend_from_slice(b"null");
    }
    buf.extend_from_slice(b",\"overall_quality\":");
    write_f32(*overall_quality, buf);
    buf.extend_from_slice(b",\"parts\":[");
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            buf.push(b',');
        }
        write_body_part(part, buf);
    }
    buf.push(b']');
}

fn write_body_part(part: &BodyPartRecord, buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"{\"part\":\"");
    write_json_string(&part.part, buf);
    buf.extend_from_slice(b"\",\"geometry\":");
    write_body_geometry(&part.geometry, buf);
    write_string_array("support", &part.support, buf);
    write_string_array("source_models", &part.source_models, buf);
    buf.extend_from_slice(b",\"quality\":");
    write_f32(part.quality, buf);
    buf.extend_from_slice(b",\"mask_coverage\":");
    if let Some(coverage) = part.mask_coverage {
        write_f32(coverage, buf);
    } else {
        buf.extend_from_slice(b"null");
    }
    buf.extend_from_slice(b",\"source_frame_numbers\":[");
    for (index, frame) in part.source_frame_numbers.iter().enumerate() {
        if index > 0 {
            buf.push(b',');
        }
        write_u64(*frame, buf);
    }
    buf.extend_from_slice(b"],\"stale\":");
    buf.extend_from_slice(if part.stale { b"true" } else { b"false" });
    buf.push(b'}');
}

fn write_body_geometry(geometry: &BodyGeometryRecord, buf: &mut Vec<u8>) {
    match geometry {
        BodyGeometryRecord::Bbox(bbox) => {
            buf.extend_from_slice(b"{\"kind\":\"bbox\",\"bbox\":[");
            write_f32_array(bbox, buf);
            buf.extend_from_slice(b"]}");
        }
        BodyGeometryRecord::Polygon(points) => {
            buf.extend_from_slice(b"{\"kind\":\"polygon\",\"points\":[");
            write_point_array(points, buf);
            buf.extend_from_slice(b"]}");
        }
        BodyGeometryRecord::Polyline { points, radius } => {
            buf.extend_from_slice(b"{\"kind\":\"polyline\",\"radius\":");
            write_f32(*radius, buf);
            buf.extend_from_slice(b",\"points\":[");
            write_point_array(points, buf);
            buf.extend_from_slice(b"]}");
        }
    }
}

fn write_f32_array(values: &[f32], buf: &mut Vec<u8>) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            buf.push(b',');
        }
        write_f32(*value, buf);
    }
}

fn write_point_array(points: &[[f32; 2]], buf: &mut Vec<u8>) {
    for (index, [x, y]) in points.iter().enumerate() {
        if index > 0 {
            buf.push(b',');
        }
        buf.push(b'[');
        write_f32(*x, buf);
        buf.push(b',');
        write_f32(*y, buf);
        buf.push(b']');
    }
}

fn write_string_array(name: &str, values: &[String], buf: &mut Vec<u8>) {
    buf.extend_from_slice(b",\"");
    write_json_string(name, buf);
    buf.extend_from_slice(b"\":[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            buf.push(b',');
        }
        buf.push(b'"');
        write_json_string(value, buf);
        buf.push(b'"');
    }
    buf.push(b']');
}
