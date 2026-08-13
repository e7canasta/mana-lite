//! Cross-model evidence validation kept on the perception side.
//!
//! This module evaluates outputs that already ran in the current keyframe. It
//! never schedules a model and it never crosses raw payloads into control.

use crate::cascade::CascadeTarget;
use crate::config::{CrossModelValidationConfig, FacePoseConfig};
use crate::detection::Detection;

use super::face_pose::COCO_HEAD_JOINTS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvidenceKind {
    Detection,
    Face,
    Pose,
    Segment,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingEvidence<'a> {
    pub(crate) model_key: &'a str,
    pub(crate) kind: EvidenceKind,
    pub(crate) target: CascadeTarget,
    pub(crate) detections: &'a [Detection],
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CrossModelValidation {
    pub(crate) actor_id: u64,
    pub(crate) frame_number: u64,
    pub(crate) quality: f32,
    pub(crate) agreement: f32,
    pub(crate) freshness: f32,
    pub(crate) supporting_sources: Vec<String>,
    pub(crate) contradicting_sources: Vec<String>,
    pub(crate) reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct SourceDetection<'a> {
    model_key: &'a str,
    detection: &'a Detection,
}

#[derive(Debug, Default)]
struct ActorEvidence<'a> {
    target: Option<CascadeTarget>,
    detection: Option<SourceDetection<'a>>,
    face: Option<SourceDetection<'a>>,
    pose: Option<SourceDetection<'a>>,
    segment: Option<SourceDetection<'a>>,
}

#[derive(Debug, Clone, Copy)]
struct RelationScore<'a> {
    name: &'static str,
    left: &'a str,
    right: &'a str,
    score: f32,
}

/// Validates all actor-targeted model outputs available in one keyframe.
///
/// Outputs without a target id are deliberately ignored: they are frame-local
/// evidence and cannot safely become temporal actor identity. Freshness is `1.0`
/// in this first cut because all accepted inputs belong to the current keyframe.
#[must_use]
pub(crate) fn validate_pending(
    inputs: &[PendingEvidence<'_>],
    frame_number: u64,
    frame_width: u32,
    frame_height: u32,
    validation_config: &CrossModelValidationConfig,
    face_pose_config: &FacePoseConfig,
) -> Vec<CrossModelValidation> {
    if frame_width == 0 || frame_height == 0 {
        return Vec::new();
    }

    let mut actors = std::collections::BTreeMap::<u64, ActorEvidence<'_>>::new();
    for input in inputs {
        let Some(actor_id) = input.target.id else {
            continue;
        };
        let actor = actors.entry(actor_id).or_default();
        actor.target.get_or_insert(input.target);

        for detection in input.detections {
            if !matches_kind(input.kind, detection) {
                continue;
            }
            let candidate = SourceDetection {
                model_key: input.model_key,
                detection,
            };
            match input.kind {
                EvidenceKind::Detection => {
                    choose_candidate(&mut actor.detection, candidate, input.target.bbox)
                }
                EvidenceKind::Face => {
                    choose_candidate(&mut actor.face, candidate, input.target.bbox)
                }
                EvidenceKind::Pose => {
                    choose_candidate(&mut actor.pose, candidate, input.target.bbox)
                }
                EvidenceKind::Segment => {
                    choose_candidate(&mut actor.segment, candidate, input.target.bbox)
                }
            }
        }
    }

    actors
        .into_iter()
        .filter_map(|(actor_id, actor)| {
            validate_actor(
                actor_id,
                actor,
                frame_number,
                frame_width,
                frame_height,
                validation_config,
                face_pose_config,
            )
        })
        .collect()
}

fn validate_actor(
    actor_id: u64,
    actor: ActorEvidence<'_>,
    frame_number: u64,
    frame_width: u32,
    frame_height: u32,
    validation_config: &CrossModelValidationConfig,
    face_pose_config: &FacePoseConfig,
) -> Option<CrossModelValidation> {
    let target = actor.target?;
    let sources = [actor.detection, actor.face, actor.pose, actor.segment]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if sources.len() < 2 {
        return None;
    }

    let mut relations = Vec::new();
    if let (Some(face), Some(pose)) = (actor.face, actor.pose) {
        if let Some(score) = face_pose_score(
            face.detection,
            pose.detection,
            target.bbox,
            face_pose_config,
        ) {
            relations.push(RelationScore {
                name: "face_pose",
                left: face.model_key,
                right: pose.model_key,
                score,
            });
        }
    }
    if let (Some(pose), Some(segment)) = (actor.pose, actor.segment) {
        if let Some(score) = pose_segment_score(
            pose.detection,
            segment.detection,
            frame_width,
            frame_height,
            face_pose_config,
        ) {
            relations.push(RelationScore {
                name: "pose_segment",
                left: pose.model_key,
                right: segment.model_key,
                score,
            });
        }
    }
    if let (Some(face), Some(segment)) = (actor.face, actor.segment) {
        if let Some(score) =
            face_segment_score(face.detection, segment.detection, frame_width, frame_height)
        {
            relations.push(RelationScore {
                name: "face_segment",
                left: face.model_key,
                right: segment.model_key,
                score,
            });
        }
    }
    if let (Some(detection), Some(pose)) = (actor.detection, actor.pose) {
        relations.push(RelationScore {
            name: "detection_pose",
            left: detection.model_key,
            right: pose.model_key,
            score: bbox_iou(detection.detection.bbox, pose.detection.bbox),
        });
    }
    if let (Some(detection), Some(segment)) = (actor.detection, actor.segment) {
        relations.push(RelationScore {
            name: "detection_segment",
            left: detection.model_key,
            right: segment.model_key,
            score: bbox_iou(detection.detection.bbox, segment.detection.bbox),
        });
    }
    if relations.is_empty() {
        return None;
    }

    let agreement =
        relations.iter().map(|relation| relation.score).sum::<f32>() / relations.len() as f32;
    let source_quality = sources
        .iter()
        .map(|source| clamp01(source.detection.confidence))
        .sum::<f32>()
        / sources.len() as f32;
    let quality_weight =
        validation_config.source_quality_weight + validation_config.agreement_quality_weight;
    let quality = if quality_weight > 0.0 {
        clamp01(
            (validation_config.source_quality_weight * source_quality
                + validation_config.agreement_quality_weight * agreement)
                / quality_weight,
        )
    } else {
        0.0
    };
    let mut supporting_sources = Vec::new();
    let mut contradicting_sources = Vec::new();
    let reasons = relations
        .iter()
        .map(|relation| {
            if relation.score >= validation_config.relation_support_threshold {
                add_unique(&mut supporting_sources, relation.left);
                add_unique(&mut supporting_sources, relation.right);
            } else {
                add_unique(&mut contradicting_sources, relation.left);
                add_unique(&mut contradicting_sources, relation.right);
            }
            format!("{}={:.3}", relation.name, relation.score)
        })
        .collect();
    supporting_sources.sort_unstable();
    contradicting_sources.sort_unstable();

    Some(CrossModelValidation {
        actor_id,
        frame_number,
        quality,
        agreement: clamp01(agreement),
        freshness: 1.0,
        supporting_sources,
        contradicting_sources,
        reasons,
    })
}

fn matches_kind(kind: EvidenceKind, detection: &Detection) -> bool {
    match kind {
        EvidenceKind::Detection | EvidenceKind::Pose | EvidenceKind::Segment => {
            detection.class == "person"
        }
        EvidenceKind::Face => detection.class == "face",
    }
}

fn choose_candidate<'a>(
    current: &mut Option<SourceDetection<'a>>,
    candidate: SourceDetection<'a>,
    target_bbox: [f32; 4],
) {
    let candidate_score = candidate_rank(candidate, target_bbox);
    let replace = current
        .map(|existing| candidate_score > candidate_rank(existing, target_bbox))
        .unwrap_or(true);
    if replace {
        *current = Some(candidate);
    }
}

fn candidate_rank(candidate: SourceDetection<'_>, target_bbox: [f32; 4]) -> (u8, f32, f32) {
    (
        u8::from(valid_bbox(candidate.detection.bbox)),
        bbox_iou(candidate.detection.bbox, target_bbox),
        clamp01(candidate.detection.confidence),
    )
}

fn face_pose_score(
    face: &Detection,
    pose: &Detection,
    target: [f32; 4],
    config: &FacePoseConfig,
) -> Option<f32> {
    if !valid_bbox(face.bbox) || !valid_bbox(pose.bbox) {
        return None;
    }
    let keypoints = pose.keypoints.as_deref()?;
    let head = COCO_HEAD_JOINTS
        .into_iter()
        .filter_map(|index| keypoints.get(index).copied())
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

    let head_center = mean_point(&head);
    let face_center = center(face.bbox);
    let distance_ratio = distance(head_center, face_center) / diagonal(face.bbox).max(f32::EPSILON);
    let center_quality =
        (1.0 - distance_ratio / config.max_head_face_center_distance_ratio).clamp(0.0, 1.0);
    let joint_quality = head.iter().map(|point| point[2]).sum::<f32>() / head.len() as f32;
    let face_coverage = bbox_coverage(face.bbox, target);
    let pose_iou = bbox_iou(pose.bbox, target);
    Some(weighted_average(
        &[
            face_coverage,
            pose_iou,
            center_quality,
            clamp01(joint_quality),
        ],
        &[
            config.quality_face_weight,
            config.quality_pose_weight,
            config.quality_geometry_weight,
            config.quality_joint_weight,
        ],
    ))
}

fn pose_segment_score(
    pose: &Detection,
    segment: &Detection,
    frame_width: u32,
    frame_height: u32,
    config: &FacePoseConfig,
) -> Option<f32> {
    let keypoints = pose.keypoints.as_deref()?;
    let polygons = segment.mask.as_ref()?.polygons.as_ref();
    if polygons.is_empty() {
        return None;
    }
    let mut total_weight = 0.0;
    let mut inside_weight = 0.0;
    for point in keypoints.iter().filter(|point| {
        point[0].is_finite()
            && point[1].is_finite()
            && point[2].is_finite()
            && point[2] >= config.keypoint_min_confidence
    }) {
        let weight = clamp01(point[2]);
        let normalized = [
            point[0] / frame_width as f32,
            point[1] / frame_height as f32,
        ];
        total_weight += weight;
        if polygons
            .iter()
            .any(|polygon| point_in_polygon(normalized, polygon))
        {
            inside_weight += weight;
        }
    }
    (total_weight > 0.0).then(|| clamp01(inside_weight / total_weight))
}

fn face_segment_score(
    face: &Detection,
    segment: &Detection,
    frame_width: u32,
    frame_height: u32,
) -> Option<f32> {
    let polygons = segment.mask.as_ref()?.polygons.as_ref();
    if polygons.is_empty() || !valid_bbox(face.bbox) {
        return None;
    }
    let center = center(face.bbox);
    let points = [
        center,
        [face.bbox[0], face.bbox[1]],
        [face.bbox[2], face.bbox[1]],
        [face.bbox[0], face.bbox[3]],
        [face.bbox[2], face.bbox[3]],
    ];
    let inside = points
        .into_iter()
        .filter(|point| {
            let normalized = [
                point[0] / frame_width as f32,
                point[1] / frame_height as f32,
            ];
            polygons
                .iter()
                .any(|polygon| point_in_polygon(normalized, polygon))
        })
        .count();
    Some(inside as f32 / points.len() as f32)
}

fn point_in_polygon(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        let crosses = (current[1] > point[1]) != (previous[1] > point[1]);
        if crosses {
            let x_at_y = (previous[0] - current[0]) * (point[1] - current[1])
                / (previous[1] - current[1])
                + current[0];
            if point[0] < x_at_y {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

fn add_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

fn valid_bbox(bbox: [f32; 4]) -> bool {
    bbox.iter().all(|value| value.is_finite()) && bbox[2] > bbox[0] && bbox[3] > bbox[1]
}

fn bbox_coverage(inner: [f32; 4], outer: [f32; 4]) -> f32 {
    let x1 = inner[0].max(outer[0]);
    let y1 = inner[1].max(outer[1]);
    let x2 = inner[2].min(outer[2]);
    let y2 = inner[3].min(outer[3]);
    let area = area(inner);
    if area <= 0.0 {
        0.0
    } else {
        ((x2 - x1).max(0.0) * (y2 - y1).max(0.0) / area).clamp(0.0, 1.0)
    }
}

fn bbox_iou(left: [f32; 4], right: [f32; 4]) -> f32 {
    let intersection = bbox_coverage(left, right) * area(left);
    let union = area(left) + area(right) - intersection;
    if union > 0.0 {
        (intersection / union).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn area(bbox: [f32; 4]) -> f32 {
    ((bbox[2] - bbox[0]).max(0.0)) * ((bbox[3] - bbox[1]).max(0.0))
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
    use std::sync::Arc;

    fn detection(class: &str, bbox: [f32; 4], confidence: f32) -> Detection {
        Detection {
            class: class.into(),
            confidence,
            bbox,
            keypoints: None,
            mask: None,
        }
    }

    fn pose() -> Detection {
        Detection {
            keypoints: Some(vec![
                [200.0, 150.0, 0.95],
                [190.0, 140.0, 0.90],
                [210.0, 140.0, 0.90],
                [180.0, 150.0, 0.85],
                [220.0, 150.0, 0.85],
                [150.0, 230.0, 0.90],
                [250.0, 230.0, 0.90],
            ]),
            ..detection("person", [100.0, 100.0, 300.0, 500.0], 0.90)
        }
    }

    fn target(id: u64) -> CascadeTarget {
        CascadeTarget {
            id: Some(id),
            bbox: [100.0, 100.0, 300.0, 500.0],
        }
    }

    fn validation_config() -> crate::config::CrossModelValidationConfig {
        crate::config::CrossModelValidationConfig {
            relation_support_threshold: 0.50,
            source_quality_weight: 0.40,
            agreement_quality_weight: 0.60,
        }
    }

    fn face_pose_config() -> crate::config::FacePoseConfig {
        crate::config::FacePoseConfig::default()
    }

    fn segment(mask: Vec<Vec<[f32; 2]>>) -> Detection {
        Detection {
            mask: Some(crate::detection::DetectionMask {
                compact: Arc::new(
                    mana_geometry::compact_mask::CompactMask::from_dense(
                        &[1],
                        1,
                        1,
                        (0, 0),
                        (1000, 1000),
                    )
                    .expect("valid compact mask"),
                ),
                polygons: Arc::new(mask),
                origin: [0, 0],
                mask_dims: [1000, 1000],
            }),
            ..detection("person", [100.0, 100.0, 300.0, 500.0], 0.88)
        }
    }

    #[test]
    fn different_target_ids_are_not_cross_validated() {
        let face = detection("face", [170.0, 120.0, 230.0, 190.0], 0.8);
        let pose = pose();
        let inputs = [
            PendingEvidence {
                model_key: "face-yolo",
                kind: EvidenceKind::Face,
                target: target(1),
                detections: std::slice::from_ref(&face),
            },
            PendingEvidence {
                model_key: "pose-standard",
                kind: EvidenceKind::Pose,
                target: target(2),
                detections: std::slice::from_ref(&pose),
            },
        ];
        assert!(
            validate_pending(
                &inputs,
                10,
                1000,
                1000,
                &validation_config(),
                &face_pose_config(),
            )
            .is_empty()
        );
    }

    #[test]
    fn face_pose_validation_produces_continuous_quality() {
        let face = detection("face", [170.0, 120.0, 230.0, 190.0], 0.8);
        let pose = pose();
        let inputs = [
            PendingEvidence {
                model_key: "face-yolo",
                kind: EvidenceKind::Face,
                target: target(1),
                detections: std::slice::from_ref(&face),
            },
            PendingEvidence {
                model_key: "pose-standard",
                kind: EvidenceKind::Pose,
                target: target(1),
                detections: std::slice::from_ref(&pose),
            },
        ];
        let result = validate_pending(
            &inputs,
            10,
            1000,
            1000,
            &validation_config(),
            &face_pose_config(),
        )
        .into_iter()
        .next()
        .expect("face and pose should be comparable");
        assert_eq!(result.actor_id, 1);
        assert_eq!(result.frame_number, 10);
        assert!((0.0..=1.0).contains(&result.quality));
        assert!((0.0..=1.0).contains(&result.agreement));
        assert_eq!(result.freshness, 1.0);
        assert!(result.contradicting_sources.is_empty());
    }

    #[test]
    fn pose_segment_validation_marks_keypoints_outside_mask_as_contradiction() {
        let pose = pose();
        let segment = segment(vec![vec![
            [0.195, 0.13],
            [0.205, 0.13],
            [0.205, 0.16],
            [0.195, 0.16],
        ]]);
        let inputs = [
            PendingEvidence {
                model_key: "pose-standard",
                kind: EvidenceKind::Pose,
                target: target(1),
                detections: std::slice::from_ref(&pose),
            },
            PendingEvidence {
                model_key: "seg-standard",
                kind: EvidenceKind::Segment,
                target: target(1),
                detections: std::slice::from_ref(&segment),
            },
        ];
        let result = validate_pending(
            &inputs,
            10,
            1000,
            1000,
            &validation_config(),
            &face_pose_config(),
        )
        .into_iter()
        .next()
        .expect("pose and segment should be comparable");
        assert!(result.quality < 0.8);
        assert_eq!(
            result.contradicting_sources,
            vec!["pose-standard", "seg-standard"]
        );
    }
}
