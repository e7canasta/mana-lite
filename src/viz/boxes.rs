use rerun::datatypes::Vec2D;

use crate::app::FrameSize;
use crate::detection::{ConsolidatedObservation, CropRect, Detection};
use crate::domain::ModelRole;
use crate::track::Track;

use super::{Inner, POSE_KEYPOINT_CONFIDENCE, VizBridge, sanitize_entity_name};

/// Build a Boxes2D archetype from an axis-aligned xyxy pixel box.
pub(super) fn boxes2d_from_xyxy(
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

fn bbox_area_px(bbox: [f32; 4]) -> f32 {
    ((bbox[2] - bbox[0]) * (bbox[3] - bbox[1])).max(0.0)
}

fn bbox_area_ratio(bbox: [f32; 4], frame_w: u32, frame_h: u32) -> f32 {
    let frame_area = (frame_w as f32) * (frame_h as f32);
    if frame_area > 0.0 {
        bbox_area_px(bbox) / frame_area
    } else {
        0.0
    }
}

pub(super) fn detection_label(
    class: &str,
    confidence: f32,
    bbox: [f32; 4],
    frame_w: u32,
    frame_h: u32,
) -> String {
    format!(
        "{class} conf={confidence:.2} area={:.0}px ratio={:.4}",
        bbox_area_px(bbox),
        bbox_area_ratio(bbox, frame_w, frame_h),
    )
}

pub(super) fn bbox_in_crop(bbox: [f32; 4], rect: CropRect) -> [f32; 4] {
    [
        bbox[0] - rect.x1 as f32,
        bbox[1] - rect.y1 as f32,
        bbox[2] - rect.x1 as f32,
        bbox[3] - rect.y1 as f32,
    ]
}

pub(super) fn pose_keypoint_visible(x: f32, y: f32, confidence: f32) -> bool {
    confidence >= POSE_KEYPOINT_CONFIDENCE && x.is_finite() && y.is_finite()
}

pub(super) fn bbox_in_roi(bbox: [f32; 4], roi: CropRect) -> Option<[f32; 4]> {
    let width = (roi.x2.saturating_sub(roi.x1)) as f32;
    let height = (roi.y2.saturating_sub(roi.y1)) as f32;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let x1 = (bbox[0] - roi.x1 as f32).clamp(0.0, width);
    let y1 = (bbox[1] - roi.y1 as f32).clamp(0.0, height);
    let x2 = (bbox[2] - roi.x1 as f32).clamp(0.0, width);
    let y2 = (bbox[3] - roi.y1 as f32).clamp(0.0, height);
    (x2 > x1 && y2 > y1).then_some([x1, y1, x2, y2])
}

impl VizBridge {
    pub fn log_entity_boxes(&self, tracks: &[&Track]) {
        if !self.toggles.boxes {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = "/world/camera/entities";
        rec.log(path, &rerun::Clear::recursive()).ok();

        for track in tracks {
            let [x1, y1, x2, y2] = track.bbox;
            let label = format!("{} #{}", track.class, track.id);
            let color = if track.misses == 0 {
                rerun::Color::from_unmultiplied_rgba(0, 255, 0, 255)
            } else {
                rerun::Color::from_unmultiplied_rgba(255, 180, 0, 220)
            };
            let bbox = boxes2d_from_xyxy([x1, y1, x2, y2], color, Some(&label), 2.0);
            let entity = format!("{path}/{}", track.id);
            if let Err(e) = rec.log(entity.as_str(), &bbox) {
                log::warn!("viz entity bbox {entity} failed: {e}");
            }
        }
    }

    pub fn log_consolidated_observations(
        &self,
        observations: &[ConsolidatedObservation],
        frame: FrameSize,
    ) {
        if !self.toggles.boxes {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = "/world/camera/observations";
        rec.log(path, &rerun::Clear::recursive()).ok();

        for (index, observation) in observations.iter().enumerate() {
            let [x1, y1, x2, y2] = observation.bbox;
            let label = format!(
                "{} model={}",
                detection_label(
                    &observation.class,
                    observation.confidence,
                    observation.bbox,
                    frame.w,
                    frame.h,
                ),
                observation.primary_model,
            );
            let bbox = boxes2d_from_xyxy(
                [x1, y1, x2, y2],
                rerun::Color::from_unmultiplied_rgba(0, 180, 255, 255),
                Some(&label),
                2.0,
            );
            let entity = format!("{path}/{index}");
            if let Err(e) = rec.log(entity.as_str(), &bbox) {
                log::warn!("viz consolidated observation {entity} failed: {e}");
            }
        }
    }

    pub fn log_model_detections(
        &self,
        model: &str,
        detections: &[Detection],
        crop_rect: Option<CropRect>,
        frame: FrameSize,
    ) {
        if !self.toggles.boxes {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let is_face_model = self.is_face_model(model);
        let model = sanitize_entity_name(model);
        let path = format!("/world/camera/detections/{model}");
        let crop_path = format!("/world/camera/crops/{model}/detections");
        if !(is_face_model && detections.is_empty()) {
            rec.log(path.as_str(), &rerun::Clear::recursive()).ok();
            rec.log(crop_path.as_str(), &rerun::Clear::recursive()).ok();
        }

        for (index, detection) in detections.iter().enumerate() {
            let label = detection_label(
                &detection.class,
                detection.confidence,
                detection.bbox,
                frame.w,
                frame.h,
            );
            let log_box = |entity: String, [x1, y1, x2, y2]: [f32; 4]| {
                let bbox = boxes2d_from_xyxy(
                    [x1, y1, x2, y2],
                    rerun::Color::from_unmultiplied_rgba(255, 80, 80, 255),
                    Some(&label),
                    2.0,
                );
                if let Err(e) = rec.log(entity.as_str(), &bbox) {
                    log::warn!("viz raw detection {entity} failed: {e}");
                }
            };
            log_box(format!("{path}/{index}"), detection.bbox);
            if let Some(rect) = crop_rect {
                log_box(
                    format!("{crop_path}/{index}"),
                    bbox_in_crop(detection.bbox, rect),
                );
            }
        }
    }

    pub fn log_model_pose(&self, model: &str, detections: &[Detection]) {
        if !self.toggles.boxes || self.role_of(model) != ModelRole::Skeleton {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let base = format!(
            "/world/camera/detections/{}/pose",
            sanitize_entity_name(model)
        );
        rec.log(base.as_str(), &rerun::Clear::recursive()).ok();

        use ultralytics_inference::visualizer::color::POSE_COLORS;
        use ultralytics_inference::visualizer::skeleton::{
            KPT_COLOR_INDICES, LIMB_COLOR_INDICES, SKELETON,
        };

        for (person_index, detection) in detections.iter().enumerate() {
            let Some(keypoints) = detection.keypoints.as_ref() else {
                continue;
            };
            let mut points = Vec::new();
            let mut point_ids = Vec::new();
            let mut point_colors = Vec::new();
            let mut skeleton = Vec::new();
            let mut skeleton_colors = Vec::new();

            for (keypoint_index, &[x, y, confidence]) in keypoints.iter().enumerate() {
                if !pose_keypoint_visible(x, y, confidence) {
                    continue;
                }
                points.push([x, y]);
                point_ids.push(keypoint_index as u16);
                let color_index = KPT_COLOR_INDICES[keypoint_index % KPT_COLOR_INDICES.len()];
                let [r, g, b] = POSE_COLORS[color_index];
                point_colors.push(rerun::Color::from_rgb(r, g, b));
            }

            for (limb_index, &[a, b]) in SKELETON.iter().enumerate() {
                let (Some(&[x1, y1, c1]), Some(&[x2, y2, c2])) =
                    (keypoints.get(a), keypoints.get(b))
                else {
                    continue;
                };
                if !pose_keypoint_visible(x1, y1, c1) || !pose_keypoint_visible(x2, y2, c2) {
                    continue;
                }
                skeleton.push(vec![[x1, y1], [x2, y2]]);
                let color_index = LIMB_COLOR_INDICES[limb_index % LIMB_COLOR_INDICES.len()];
                let [r, g, b] = POSE_COLORS[color_index];
                skeleton_colors.push(rerun::Color::from_unmultiplied_rgba(r, g, b, 220));
            }

            if !points.is_empty() {
                let path = format!("{base}/{person_index}/keypoints");
                let points = rerun::Points2D::new(points)
                    .with_keypoint_ids(point_ids)
                    .with_colors(point_colors)
                    .with_radii([rerun::Radius::new_ui_points(5.0)]);
                if let Err(e) = rec.log(path.as_str(), &points) {
                    log::warn!("viz pose keypoints {path} failed: {e}");
                }
            }
            if !skeleton.is_empty() {
                let path = format!("{base}/{person_index}/skeleton");
                let strips = rerun::LineStrips2D::new(skeleton)
                    .with_colors(skeleton_colors)
                    .with_radii([rerun::Radius::new_ui_points(3.0)]);
                if let Err(e) = rec.log(path.as_str(), &strips) {
                    log::warn!("viz pose skeleton {path} failed: {e}");
                }
            }
        }
    }

    pub fn log_depth_context_boxes(
        &self,
        model: &str,
        detections: &[Detection],
        depth_context_roi: Option<CropRect>,
    ) {
        if !self.toggles.boxes || self.role_of(model) != ModelRole::Boxes {
            return;
        }
        let Some(depth_context_roi) = depth_context_roi else {
            return;
        };
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let is_face_model = self.is_face_model(model);
        let model = sanitize_entity_name(model);
        let path = format!("/world/camera/crops/depth-standard/depth/context/{model}");
        if !(is_face_model && detections.is_empty()) {
            rec.log(path.as_str(), &rerun::Clear::recursive()).ok();
        }

        let color = if is_face_model {
            rerun::Color::from_unmultiplied_rgba(255, 220, 0, 165)
        } else {
            rerun::Color::from_unmultiplied_rgba(0, 255, 100, 165)
        };
        let label_prefix = if is_face_model { "face" } else { "body" };

        for (index, detection) in detections.iter().enumerate() {
            let Some([x1, y1, x2, y2]) = bbox_in_roi(detection.bbox, depth_context_roi) else {
                continue;
            };
            let label = format!(
                "{label_prefix} {} {:.2}",
                detection.class, detection.confidence
            );
            let bbox = boxes2d_from_xyxy(
                [x1, y1, x2, y2],
                color,
                (!is_face_model).then_some(label.as_str()),
                3.0,
            );
            let entity = format!("{path}/{index}");
            if let Err(e) = rec.log(entity.as_str(), &bbox) {
                log::warn!("viz depth context box {entity} failed: {e}");
            }
        }
    }

    pub fn clear_depth_context_boxes(&self) {
        if !self.toggles.boxes {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        for (model, role) in &self.roles {
            if *role == ModelRole::Boxes {
                let model = sanitize_entity_name(model);
                let path = format!("/world/camera/crops/depth-standard/depth/context/{model}");
                rec.log(path.as_str(), &rerun::Clear::recursive()).ok();
            }
        }
    }
}
