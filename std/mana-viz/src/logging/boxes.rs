use anyhow::Result;
use mana_types::bbox;
use mana_types::{DetectionBatchV1, RoiCommandV1, SceneMsgV1};

use rerun::datatypes::Vec2D;

use super::util::{FrameSize, log_archetype, log_many};

pub fn log_detections_2d(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    batch: &DetectionBatchV1,
    frame: FrameSize,
) -> Result<()> {
    let dets = batch.valid();
    rec.set_timestamp_nanos_since_epoch("frame_time", batch.timestamp_ns);
    rec.log(entity_path, &rerun::Clear::recursive()).ok();
    if dets.is_empty() {
        return Ok(());
    }
    let mut class_idx = std::collections::HashMap::with_capacity(4);
    let items = dets.iter().map(|d| {
        let e = class_idx.entry(d.class_id).or_insert(0);
        let i = *e;
        *e += 1;
        let (cx, cy, hw, hh) = bbox::box_halfsize_to_pixels(d.cx, d.cy, d.w, d.h, frame.w, frame.h);
        let label = format!("cls:{} cf:{:.2}", d.class_id, d.confidence);
        let boxes = single_box([cx, cy], [hw, hh], d.class_id, label);
        (format!("{entity_path}/{}/{}", d.class_id, i), boxes)
    });
    log_many(rec, batch.timestamp_ns, items, "detection");
    Ok(())
}

pub fn log_zones_2d(rec: &rerun::RecordingStream, entity_path_prefix: &str, msg: &SceneMsgV1) {
    let items = msg.valid_zones().iter().map(|z| {
        let name = z.name_str().to_string();
        let boxes = rerun::Boxes2D::from_centers_and_half_sizes(
            [Vec2D([z.cx, z.cy])],
            [Vec2D([z.w / 2.0, z.h / 2.0])],
        )
        .with_labels([name.clone()]);
        (format!("{entity_path_prefix}/{name}"), boxes)
    });
    log_many(rec, msg.timestamp_ns, items, "zone")
}

pub fn log_roi_2d(
    rec: &rerun::RecordingStream,
    parent_path: &str,
    cmd: &RoiCommandV1,
    frame: FrameSize,
) -> Result<()> {
    let fw = frame.w as f32;
    let fh = frame.h as f32;

    let obj = cmd.target_name_str();
    let obj = if obj.is_empty() || obj == "<invalid>" {
        "attention"
    } else {
        obj
    };
    let scope = format!("{parent_path}/{obj}");

    match cmd.mode {
        RoiCommandV1::FULL => {
            rec.log(parent_path, &rerun::Clear::recursive()).ok();
        }
        RoiCommandV1::CENTER_SQUARE | RoiCommandV1::RECT => {
            let (label, cx, cy, hw, hh) = if cmd.mode == RoiCommandV1::CENTER_SQUARE {
                let side = fw.min(fh);
                ("CenterSquare", fw / 2.0, fh / 2.0, side / 2.0, side / 2.0)
            } else {
                (
                    "Rect",
                    (cmd.x + cmd.w / 2.0) * fw,
                    (cmd.y + cmd.h / 2.0) * fh,
                    (cmd.w * fw) / 2.0,
                    (cmd.h * fh) / 2.0,
                )
            };
            let roi =
                rerun::Boxes2D::from_centers_and_half_sizes([Vec2D([cx, cy])], [Vec2D([hw, hh])])
                    .with_colors([roi_fill(255, 255, 0)])
                    .with_labels([label]);
            log_archetype(rec, &format!("{scope}/box"), &roi, || "roi_box".into())?;
        }
        RoiCommandV1::BED => {
            rec.log(format!("{scope}/trapezoid").as_str(), &rerun::Clear::flat())
                .ok();
            rec.log(format!("{scope}/bbox").as_str(), &rerun::Clear::flat())
                .ok();

            let cx = cmd.x * fw;
            let ty = cmd.y * fh;
            let tw = cmd.w * fw;
            let th = cmd.h * fh;
            let bw = cmd.base_width * fw;

            let tl = Vec2D([cx - tw / 2.0, ty]);
            let tr = Vec2D([cx + tw / 2.0, ty]);
            let br = Vec2D([cx + bw / 2.0, ty + th]);
            let bl = Vec2D([cx - bw / 2.0, ty + th]);
            let strip = rerun::LineStrips2D::new([vec![tl, tr, br, bl, tl]]).with_colors([
                rerun::datatypes::Rgba32::from_unmultiplied_rgba(255, 80, 80, 200),
            ]);
            log_archetype(rec, &format!("{scope}/trapezoid"), &strip, || {
                "roi_trap".into()
            })?;

            let bbox = rerun::Boxes2D::from_centers_and_half_sizes(
                [Vec2D([cx, ty + th / 2.0])],
                [Vec2D([bw / 2.0, th / 2.0])],
            )
            .with_colors([roi_fill(255, 255, 0)])
            .with_labels(["Bed bbox"]);
            log_archetype(rec, &format!("{scope}/bbox"), &bbox, || "roi_bbox".into())?;
        }
        _ => {}
    }
    Ok(())
}

/// Build a Boxes2D archetype from an axis-aligned xyxy pixel box.
#[must_use]
pub fn boxes2d_from_xyxy(
    bbox: [f32; 4],
    color: rerun::Color,
    label: Option<&str>,
    radius: f32,
) -> rerun::Boxes2D {
    let [x1, y1, x2, y2] = bbox;
    let boxes = rerun::Boxes2D::from_centers_and_half_sizes(
        [Vec2D([(x1 + x2) / 2.0, (y1 + y2) / 2.0])],
        [Vec2D([(x2 - x1).abs() / 2.0, (y2 - y1).abs() / 2.0])],
    )
    .with_colors([color])
    .with_radii([radius]);
    match label {
        Some(label) => boxes.with_labels([label]),
        None => boxes,
    }
}

fn single_box(
    center: [f32; 2],
    half_size: [f32; 2],
    class_id: u16,
    label: String,
) -> rerun::Boxes2D {
    rerun::Boxes2D::from_centers_and_half_sizes([Vec2D(center)], [Vec2D(half_size)])
        .with_class_ids([class_id])
        .with_labels([label])
}

fn roi_fill(r: u8, g: u8, b: u8) -> rerun::datatypes::Rgba32 {
    rerun::datatypes::Rgba32::from_unmultiplied_rgba(r, g, b, 60)
}
