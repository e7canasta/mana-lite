use std::sync::Arc;

use mana_geometry::compact_mask::CompactMask;
use mana_geometry::iou::{OverlapMetric, box_overlap};

/// Region rectangular en pixeles del frame original.
///
/// Acota donde se detecta: recorte previo a la inferencia y clipping de las
/// detecciones que caen fuera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropRect {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

impl CropRect {
    pub fn from_array(a: [u32; 4]) -> Self {
        CropRect {
            x1: a[0],
            y1: a[1],
            x2: a[2],
            y2: a[3],
        }
    }

    pub fn to_array(self) -> [u32; 4] {
        [self.x1, self.y1, self.x2, self.y2]
    }
}

/// Una deteccion de un modelo en un frame, sin identidad temporal.
#[derive(Debug, Clone)]
pub struct Detection {
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
    pub keypoints: Option<Vec<[f32; 3]>>,
    pub mask: Option<DetectionMask>,
}

impl Detection {
    pub fn area_px(&self) -> f32 {
        ((self.bbox[2] - self.bbox[0]) * (self.bbox[3] - self.bbox[1])).max(0.0)
    }

    pub fn area_ratio(&self, frame_w: u32, frame_h: u32) -> f32 {
        let frame_area = (frame_w as f32) * (frame_h as f32);
        if frame_area > 0.0 {
            self.area_px() / frame_area
        } else {
            0.0
        }
    }
}

/// Instance mask attached to a detection.
///
/// The mask raster lives in *mask space* (the image passed to the model,
/// i.e. the crop for cascade models), stored as a crop-RLE `CompactMask`.
/// `origin` is the position of mask space within the original frame and
/// `mask_dims` its size, so consumers can place and rasterize the mask.
/// `polygons` are simplified contours normalized to the full frame.
///
/// `compact` and `polygons` are `Arc`-shared: the mask travels from
/// inference through consolidation and tracking without deep copies;
/// the only owned copy happens at the wire boundary, which belongs to
/// `logger` — no a este modulo.
#[derive(Debug, Clone)]
pub struct DetectionMask {
    pub compact: Arc<CompactMask>,
    pub polygons: Arc<Vec<Vec<[f32; 2]>>>,
    pub origin: [u32; 2],
    pub mask_dims: [u32; 2],
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DetectionEvidence {
    pub model: String,
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
    pub mask: Option<DetectionMask>,
}

/// Same-frame detections fused across model outputs, with no temporal identity.
#[derive(Debug, Clone)]
pub struct ConsolidatedObservation {
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
    pub primary_model: String,
    pub evidence: Vec<DetectionEvidence>,
    pub components: Vec<DetectionEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionRole {
    Primary,
    Secondary,
}

#[derive(Debug, Clone)]
pub struct ModelDetections<'a> {
    pub model: &'a str,
    pub role: DetectionRole,
    pub detections: &'a [Detection],
}

/// Stateless spatial consolidation of detections from the current cycle.
pub struct DetectionConsolidator {
    same_class_iou: f32,
    face_component_coverage: f32,
    face_max_center_y_ratio: f32,
}

impl DetectionConsolidator {
    pub const fn new(
        face_component_coverage: f32,
        face_max_center_y_ratio: f32,
        same_class_iou: f32,
    ) -> Self {
        Self {
            same_class_iou,
            face_component_coverage,
            face_max_center_y_ratio,
        }
    }

    pub fn consolidate(
        &self,
        model_outputs: &[ModelDetections<'_>],
    ) -> Vec<ConsolidatedObservation> {
        let mut observations: Vec<ConsolidatedObservation> = Vec::new();

        for output in model_outputs {
            for detection in output.detections {
                let evidence = evidence_from_detection(&output.model, detection);
                let same_class_index = observations
                    .iter()
                    .enumerate()
                    .filter_map(|(index, observation)| {
                        let iou = box_overlap(
                            (
                                observation.bbox[0],
                                observation.bbox[1],
                                observation.bbox[2],
                                observation.bbox[3],
                            ),
                            (
                                detection.bbox[0],
                                detection.bbox[1],
                                detection.bbox[2],
                                detection.bbox[3],
                            ),
                            OverlapMetric::Iou,
                        );
                        (observation.class == detection.class && iou >= self.same_class_iou)
                            .then_some((index, iou))
                    })
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(index, _)| index);
                if let Some(index) = same_class_index {
                    let observation = &mut observations[index];
                    if output.role == DetectionRole::Primary
                        && observation.primary_model != output.model
                    {
                        observation.primary_model = output.model.to_owned();
                        observation.bbox = detection.bbox;
                    }
                    observation.confidence = observation.confidence.max(detection.confidence);
                    observation.evidence.push(evidence);
                    continue;
                }

                let component_index = observations
                    .iter()
                    .enumerate()
                    .filter_map(|(index, observation)| {
                        let coverage = bbox_coverage(&detection.bbox, &observation.bbox);
                        (detection.class == "face"
                            && observation.class == "person"
                            && coverage >= self.face_component_coverage
                            && face_is_in_upper_body(
                                &detection.bbox,
                                &observation.bbox,
                                self.face_max_center_y_ratio,
                            ))
                        .then_some((index, coverage))
                    })
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(index, _)| index);
                if let Some(index) = component_index {
                    let observation = &mut observations[index];
                    observation.components.push(evidence);
                    continue;
                }

                if detection.class == "face" {
                    continue;
                }

                observations.push(ConsolidatedObservation {
                    class: detection.class.clone(),
                    confidence: detection.confidence,
                    bbox: detection.bbox,
                    primary_model: output.model.to_owned(),
                    evidence: vec![evidence],
                    components: Vec::new(),
                });
            }
        }

        observations
    }
}

impl Default for DetectionConsolidator {
    fn default() -> Self {
        Self::new(0.7, 0.65, 0.5)
    }
}

fn evidence_from_detection(model: &str, detection: &Detection) -> DetectionEvidence {
    DetectionEvidence {
        model: model.into(),
        class: detection.class.clone(),
        confidence: detection.confidence,
        bbox: detection.bbox,
        mask: detection.mask.clone(),
    }
}

fn bbox_coverage(inner: &[f32; 4], outer: &[f32; 4]) -> f32 {
    let ix1 = inner[0].max(outer[0]);
    let iy1 = inner[1].max(outer[1]);
    let ix2 = inner[2].min(outer[2]);
    let iy2 = inner[3].min(outer[3]);
    let intersection = (ix2 - ix1).max(0.0) * (iy2 - iy1).max(0.0);
    let inner_area = bbox_area(inner);
    if inner_area <= 0.0 {
        0.0
    } else {
        intersection / inner_area
    }
}

fn bbox_area(bbox: &[f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).max(0.0) * (bbox[3] - bbox[1]).max(0.0)
}

fn face_is_in_upper_body(face: &[f32; 4], person: &[f32; 4], max_center_y_ratio: f32) -> bool {
    let person_height = (person[3] - person[1]).max(0.0);
    if person_height <= 0.0 {
        return false;
    }
    let face_center_y = (face[1] + face[3]) / 2.0;
    let relative_center_y = (face_center_y - person[1]) / person_height;
    relative_center_y <= max_center_y_ratio
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detection(class: &str, bbox: [f32; 4]) -> Detection {
        Detection {
            class: class.into(),
            confidence: 0.9,
            bbox,
            keypoints: None,
            mask: None,
        }
    }

    #[test]
    fn detection_area_and_ratio_use_frame_coordinates() {
        let detection = detection("person", [10.0, 20.0, 110.0, 220.0]);
        assert_eq!(detection.area_px(), 20_000.0);
        assert!((detection.area_ratio(1_000, 1_000) - 0.02).abs() < f32::EPSILON);
    }

    #[test]
    fn same_class_models_fuse_into_one_observation() {
        let consolidator = DetectionConsolidator::default();
        let detect = [detection("person", [0.0, 0.0, 100.0, 100.0])];
        let pose = [detection("person", [5.0, 5.0, 95.0, 95.0])];
        let entities = consolidator.consolidate(&[
            ModelDetections {
                model: "detect-fast",
                role: DetectionRole::Primary,
                detections: &detect,
            },
            ModelDetections {
                model: "pose-standard",
                role: DetectionRole::Secondary,
                detections: &pose,
            },
        ]);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].evidence.len(), 2);
        assert_eq!(entities[0].primary_model, "detect-fast");
    }

    #[test]
    fn face_becomes_component_of_person() {
        let consolidator = DetectionConsolidator::default();
        let person = [detection("person", [0.0, 0.0, 100.0, 100.0])];
        let face = [detection("face", [20.0, 20.0, 40.0, 40.0])];
        let entities = consolidator.consolidate(&[
            ModelDetections {
                model: "detect-fast",
                role: DetectionRole::Primary,
                detections: &person,
            },
            ModelDetections {
                model: "face-v12",
                role: DetectionRole::Secondary,
                detections: &face,
            },
        ]);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].components.len(), 1);
    }

    #[test]
    fn face_does_not_attach_to_non_person_observation() {
        let consolidator = DetectionConsolidator::default();
        let wheelchair = [detection("wheelchair", [0.0, 0.0, 100.0, 100.0])];
        let face = [detection("face", [20.0, 20.0, 40.0, 40.0])];
        let entities = consolidator.consolidate(&[
            ModelDetections {
                model: "detect-fast",
                role: DetectionRole::Primary,
                detections: &wheelchair,
            },
            ModelDetections {
                model: "face-yolo",
                role: DetectionRole::Secondary,
                detections: &face,
            },
        ]);
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn face_does_not_attach_to_lower_body() {
        let consolidator = DetectionConsolidator::default();
        let person = [detection("person", [0.0, 0.0, 100.0, 100.0])];
        let face = [detection("face", [20.0, 70.0, 40.0, 90.0])];
        let entities = consolidator.consolidate(&[
            ModelDetections {
                model: "detect-fast",
                role: DetectionRole::Primary,
                detections: &person,
            },
            ModelDetections {
                model: "face-yolo",
                role: DetectionRole::Secondary,
                detections: &face,
            },
        ]);
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn different_primary_classes_remain_separate() {
        let consolidator = DetectionConsolidator::default();
        let detections = [
            detection("person", [0.0, 0.0, 100.0, 100.0]),
            detection("wheelchair", [0.0, 0.0, 100.0, 100.0]),
        ];
        let entities = consolidator.consolidate(&[ModelDetections {
            model: "detect-fast",
            role: DetectionRole::Primary,
            detections: &detections,
        }]);
        assert_eq!(entities.len(), 2);
    }

    #[test]
    fn enrichment_uses_best_spatial_match() {
        let consolidator = DetectionConsolidator::default();
        let detect = [
            detection("person", [0.0, 0.0, 100.0, 100.0]),
            detection("person", [120.0, 0.0, 220.0, 100.0]),
        ];
        let pose = [detection("person", [125.0, 5.0, 215.0, 95.0])];
        let entities = consolidator.consolidate(&[
            ModelDetections {
                model: "detect-fast",
                role: DetectionRole::Primary,
                detections: &detect,
            },
            ModelDetections {
                model: "pose-standard",
                role: DetectionRole::Secondary,
                detections: &pose,
            },
        ]);
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].evidence.len(), 1);
        assert_eq!(entities[1].evidence.len(), 2);
    }
}
