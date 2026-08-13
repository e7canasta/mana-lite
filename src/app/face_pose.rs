//! Deterministic face/pose cross-validation kept on the perception side.

use std::time::Instant;

use crate::cascade::{CascadeTarget, InferenceRequest};
use crate::config::FacePoseConfig;
use crate::detection::Detection;

pub(crate) const FACE_POSE_REQUEST_REASON: &str = "face-uncertain";

/// COCO/YOLO pose head indices used for face geometry.
pub(crate) const COCO_HEAD_JOINTS: [usize; 5] = [0, 1, 2, 3, 4];

#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingFacePoseContext {
    pub(crate) face_bbox: [f32; 4],
    pub(crate) face_confidence: f32,
    pub(crate) target: CascadeTarget,
    pub(crate) source_frame_number: u64,
    pub(crate) requested_at: Instant,
    pub(crate) expires_at: Instant,
}

impl PendingFacePoseContext {
    #[must_use]
    pub(crate) fn request(&self, config: &FacePoseConfig) -> InferenceRequest {
        InferenceRequest::new(
            config.pose_model_key.clone(),
            FACE_POSE_REQUEST_REASON,
            config.pose_request_priority,
            self.requested_at,
            self.expires_at,
        )
    }
}

#[must_use]
pub(crate) fn is_uncertain_face(detection: &Detection, config: &FacePoseConfig) -> bool {
    detection.confidence.is_finite()
        && detection.confidence >= config.min_face_confidence
        && detection.confidence <= config.uncertain_face_max_confidence
        && valid_bbox(detection.bbox)
}

#[must_use]
pub(crate) fn select_pose_detection<'a>(
    detections: &'a [Detection],
    target: CascadeTarget,
) -> Option<&'a Detection> {
    detections
        .iter()
        .filter(|detection| detection.class == "person" && valid_bbox(detection.bbox))
        .max_by(|left, right| {
            intersection_over_union(left.bbox, target.bbox)
                .total_cmp(&intersection_over_union(right.bbox, target.bbox))
                .then_with(|| left.confidence.total_cmp(&right.confidence))
                .then_with(|| left.bbox[0].total_cmp(&right.bbox[0]))
                .then_with(|| left.bbox[1].total_cmp(&right.bbox[1]))
        })
}

/// Validates one pose detection against the face context captured on the
/// previous keyframe. `None` means the evidence was unavailable; `Some(false)`
/// means sufficient evidence ran but failed the geometric gates.
#[must_use]
pub(crate) fn validate_face_pose(
    context: PendingFacePoseContext,
    pose_target: Option<CascadeTarget>,
    pose: &Detection,
    frame_number: u64,
    frame_width: u32,
    frame_height: u32,
    config: &FacePoseConfig,
) -> Option<mana_control::FacePoseValidation> {
    if frame_number <= context.source_frame_number
        || frame_width == 0
        || frame_height == 0
        || context.target.id.is_none()
        || pose_target.and_then(|target| target.id) != context.target.id
        || !valid_bbox(context.face_bbox)
        || !valid_bbox(context.target.bbox)
        || !valid_bbox(pose.bbox)
        || !context.face_confidence.is_finite()
        || !pose.confidence.is_finite()
        || context.face_confidence < config.min_face_confidence
        || pose.confidence < config.min_face_confidence
    {
        return None;
    }

    let keypoints = pose.keypoints.as_deref()?;
    let head = COCO_HEAD_JOINTS
        .iter()
        .filter_map(|&index| keypoints.get(index).copied())
        .filter(|point| {
            point[0].is_finite()
                && point[1].is_finite()
                && point[2].is_finite()
                && point[2] >= config.keypoint_min_confidence
        })
        .collect::<Vec<_>>();
    if head.len() < config.min_head_joints {
        return None;
    }

    let face_area = area(context.face_bbox);
    let face_person_coverage = intersection_area(context.face_bbox, context.target.bbox)
        .map(|intersection| intersection / face_area)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let pose_person_iou = intersection_over_union(pose.bbox, context.target.bbox);
    let head_center = mean_point(&head);
    let face_center = center(context.face_bbox);
    let face_diagonal = diagonal(context.face_bbox).max(f32::EPSILON);
    let center_distance_ratio = distance(head_center, face_center) / face_diagonal;
    let head_inside_person = contains_point(context.target.bbox, head_center);

    let joint_quality = head.iter().map(|point| point[2]).sum::<f32>() / head.len() as f32;
    let center_quality =
        (1.0 - center_distance_ratio / config.max_head_face_center_distance_ratio).clamp(0.0, 1.0);
    let geometry_quality = (face_person_coverage + pose_person_iou + center_quality) / 3.0;
    let quality = weighted_average(
        &[
            context.face_confidence,
            pose.confidence,
            joint_quality,
            geometry_quality,
        ],
        &[
            config.quality_face_weight,
            config.quality_pose_weight,
            config.quality_joint_weight,
            config.quality_geometry_weight,
        ],
    );
    let valid = face_person_coverage >= config.min_face_person_coverage
        && pose_person_iou >= config.min_pose_person_iou
        && center_distance_ratio <= config.max_head_face_center_distance_ratio
        && head_inside_person;

    Some(mana_control::FacePoseValidation {
        valid,
        quality,
        frame_number,
    })
}

fn valid_bbox(bbox: [f32; 4]) -> bool {
    bbox.iter().all(|value| value.is_finite()) && bbox[2] > bbox[0] && bbox[3] > bbox[1]
}

fn area(bbox: [f32; 4]) -> f32 {
    (bbox[2] - bbox[0]) * (bbox[3] - bbox[1])
}

fn intersection_area(left: [f32; 4], right: [f32; 4]) -> Option<f32> {
    let x1 = left[0].max(right[0]);
    let y1 = left[1].max(right[1]);
    let x2 = left[2].min(right[2]);
    let y2 = left[3].min(right[3]);
    (x2 > x1 && y2 > y1).then_some((x2 - x1) * (y2 - y1))
}

fn intersection_over_union(left: [f32; 4], right: [f32; 4]) -> f32 {
    let intersection = intersection_area(left, right).unwrap_or(0.0);
    let union = area(left) + area(right) - intersection;
    if union > 0.0 {
        (intersection / union).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn center(bbox: [f32; 4]) -> [f32; 2] {
    [(bbox[0] + bbox[2]) / 2.0, (bbox[1] + bbox[3]) / 2.0]
}

fn diagonal(bbox: [f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).hypot(bbox[3] - bbox[1])
}

fn mean_point(points: &[[f32; 3]]) -> [f32; 2] {
    let count = points.len() as f32;
    [
        points.iter().map(|point| point[0]).sum::<f32>() / count,
        points.iter().map(|point| point[1]).sum::<f32>() / count,
    ]
}

fn distance(left: [f32; 2], right: [f32; 2]) -> f32 {
    (left[0] - right[0]).hypot(left[1] - right[1])
}

fn contains_point(bbox: [f32; 4], point: [f32; 2]) -> bool {
    point[0] >= bbox[0] && point[0] <= bbox[2] && point[1] >= bbox[1] && point[1] <= bbox[3]
}

fn clamp01(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn weighted_average(values: &[f32], weights: &[f32]) -> f32 {
    let total_weight = weights.iter().sum::<f32>();
    if total_weight <= 0.0 || values.len() != weights.len() {
        return 0.0;
    }
    clamp01(
        values
            .iter()
            .zip(weights)
            .map(|(value, weight)| value * weight)
            .sum::<f32>()
            / total_weight,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn context() -> PendingFacePoseContext {
        PendingFacePoseContext {
            face_bbox: [150.0, 120.0, 210.0, 190.0],
            face_confidence: 0.45,
            target: CascadeTarget {
                id: Some(7),
                bbox: [100.0, 100.0, 300.0, 500.0],
            },
            source_frame_number: 10,
            requested_at: Instant::now(),
            expires_at: Instant::now() + Duration::from_secs(4),
        }
    }

    fn pose() -> Detection {
        Detection {
            class: "person".into(),
            confidence: 0.9,
            bbox: [100.0, 100.0, 300.0, 500.0],
            keypoints: Some(vec![
                [180.0, 155.0, 0.95],
                [170.0, 145.0, 0.90],
                [190.0, 145.0, 0.90],
                [160.0, 155.0, 0.85],
                [200.0, 155.0, 0.85],
            ]),
            mask: None,
        }
    }

    #[test]
    fn coco_head_joint_map_matches_yolo26_pose_contract() {
        assert_eq!(COCO_HEAD_JOINTS, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn positive_validation_is_deterministic() {
        let config = FacePoseConfig::default();
        let first = validate_face_pose(
            context(),
            Some(CascadeTarget {
                id: Some(7),
                bbox: [100.0, 100.0, 300.0, 500.0],
            }),
            &pose(),
            11,
            640,
            480,
            &config,
        )
        .expect("sufficient evidence");
        let second = validate_face_pose(
            context(),
            Some(CascadeTarget {
                id: Some(7),
                bbox: [100.0, 100.0, 300.0, 500.0],
            }),
            &pose(),
            11,
            640,
            480,
            &config,
        )
        .expect("sufficient evidence");
        assert_eq!(first, second);
        assert!(first.valid);
        assert!((0.0..=1.0).contains(&first.quality));
        assert_eq!(first.frame_number, 11);
    }

    #[test]
    fn geometry_failure_is_negative_but_has_finite_quality() {
        let mut bad = pose();
        bad.keypoints.as_mut().unwrap()[0] = [500.0, 500.0, 0.95];
        bad.keypoints.as_mut().unwrap()[1] = [500.0, 500.0, 0.90];
        bad.keypoints.as_mut().unwrap()[2] = [500.0, 500.0, 0.90];
        let result = validate_face_pose(
            context(),
            Some(CascadeTarget {
                id: Some(7),
                bbox: [100.0, 100.0, 300.0, 500.0],
            }),
            &bad,
            11,
            640,
            480,
            &FacePoseConfig::default(),
        )
        .expect("enough joints still provide negative evidence");
        assert!(!result.valid);
        assert!(result.quality.is_finite());
    }

    #[test]
    fn insufficient_or_mismatched_evidence_is_absent() {
        let mut insufficient = pose();
        insufficient.keypoints.as_mut().unwrap()[1][2] = 0.1;
        insufficient.keypoints.as_mut().unwrap()[2][2] = 0.1;
        insufficient.keypoints.as_mut().unwrap()[3][2] = 0.1;
        insufficient.keypoints.as_mut().unwrap()[4][2] = 0.1;
        assert!(
            validate_face_pose(
                context(),
                Some(CascadeTarget {
                    id: Some(7),
                    bbox: [100.0, 100.0, 300.0, 500.0],
                }),
                &insufficient,
                11,
                640,
                480,
                &FacePoseConfig::default(),
            )
            .is_none()
        );
        assert!(
            validate_face_pose(
                context(),
                Some(CascadeTarget {
                    id: Some(8),
                    bbox: [100.0, 100.0, 300.0, 500.0],
                }),
                &pose(),
                11,
                640,
                480,
                &FacePoseConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn face_uncertainty_requires_finite_confidence_and_valid_bbox() {
        let mut face = pose();
        face.class = "face".into();
        face.confidence = 0.45;
        let config = FacePoseConfig::default();
        assert!(is_uncertain_face(&face, &config));
        face.confidence = f32::NAN;
        assert!(!is_uncertain_face(&face, &config));
    }

    #[test]
    fn request_has_stable_reason_and_bounded_ttl() {
        let now = Instant::now();
        let context = PendingFacePoseContext {
            requested_at: now,
            expires_at: now + Duration::from_secs(4),
            ..context()
        };
        let request = context.request(&FacePoseConfig::default());
        assert_eq!(request.model_key, "pose-standard");
        assert_eq!(request.reason, FACE_POSE_REQUEST_REASON);
        assert_eq!(
            request.expires_at.duration_since(request.requested_at),
            Duration::from_secs(4)
        );
    }
}
