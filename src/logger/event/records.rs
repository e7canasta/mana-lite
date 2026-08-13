use crate::detection::{Detection, DetectionMask};

#[derive(Debug, Clone)]
pub struct DetRecord {
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
    /// Bounding-box area in source-frame pixels and as a fraction of the full frame.
    pub area_px: f32,
    pub area_ratio: f32,
    /// Pose keypoints remapped to the source frame, one `[x, y, confidence]`
    /// per point. Present only for pose models.
    pub keypoints: Option<Vec<[f32; 3]>>,
    pub mask: Option<MaskRecord>,
}

#[derive(Debug, Clone)]
pub struct BodyPartRecord {
    pub part: String,
    pub geometry: BodyGeometryRecord,
    pub support: Vec<String>,
    pub source_models: Vec<String>,
    pub quality: f32,
    pub mask_coverage: Option<f32>,
    pub depth: Option<BodyPartDepthRecord>,
    pub source_frame_numbers: Vec<u64>,
    pub stale: bool,
}

#[derive(Debug, Clone)]
pub struct BodyPartDepthRecord {
    pub source_model: String,
    pub roi: [u32; 4],
    pub map_width: u32,
    pub map_height: u32,
    pub sampled_pixels: u64,
    pub valid_pixels: u64,
    pub valid_ratio: Option<f32>,
    pub min_depth_m: Option<f32>,
    pub median_depth_m: Option<f32>,
    pub p10_depth_m: Option<f32>,
    pub p90_depth_m: Option<f32>,
    pub max_depth_m: Option<f32>,
    pub relative_to_torso_m: Option<f32>,
}

#[derive(Debug, Clone)]
pub enum BodyGeometryRecord {
    Bbox([f32; 4]),
    Polygon(Vec<[f32; 2]>),
    Polyline { points: Vec<[f32; 2]>, radius: f32 },
}

#[derive(Debug, Clone)]
pub struct FaceDwellTimerRecord {
    pub trigger: String,
    pub elapsed_ms: u64,
    pub required_ms: u64,
}

/// JSONL wire record for an instance mask (Spec-003).
///
/// `rle` are column-major run-length counts of the mask crop; `bbox` is the
/// detection box inside mask space (`bbox[2]-bbox[0]` = RLE width,
/// `bbox[3]-bbox[1]` = RLE height). `origin` positions mask space in the
/// original frame; `mask_dims` is its size. `polygons` are contours
/// normalized to the full frame.
///
/// Built by [`MaskRecord::from_mask`]: el formato de cable lo posee este
/// modulo, no el tipo de dominio.
#[derive(Debug, Clone)]
pub struct MaskRecord {
    pub rle: Vec<u32>,
    pub bbox: [f32; 4],
    pub origin: [u32; 2],
    pub mask_dims: [u32; 2],
    pub polygons: Vec<Vec<[f32; 2]>>,
}

impl MaskRecord {
    /// JSONL wire record (Spec-003). `rle` are column-major run-length
    /// counts of the mask crop; `bbox` is the detection box inside mask
    /// space; `origin`/`mask_dims` place mask space in the frame and
    /// `polygons` are contours normalized to the full frame.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn from_mask(mask: &DetectionMask) -> Self {
        let (bbox_h, bbox_w) = if let Some(rle) = mask.compact.rles.first() {
            (rle.h, rle.w)
        } else {
            (0, 0)
        };
        let (off_x, off_y) = mask.compact.offsets.first().copied().unwrap_or((0, 0));
        MaskRecord {
            rle: mask
                .compact
                .rles
                .first()
                .map(|rle| rle.counts.to_vec())
                .unwrap_or_default(),
            bbox: [
                off_x as f32,
                off_y as f32,
                (off_x + bbox_w) as f32,
                (off_y + bbox_h) as f32,
            ],
            origin: mask.origin,
            mask_dims: mask.mask_dims,
            polygons: mask.polygons.as_ref().clone(),
        }
    }
}

impl DetRecord {
    pub fn from_detection(detection: &Detection, frame_w: u32, frame_h: u32) -> Self {
        DetRecord {
            class: detection.class.clone(),
            confidence: detection.confidence,
            bbox: detection.bbox,
            area_px: detection.area_px(),
            area_ratio: detection.area_ratio(frame_w, frame_h),
            keypoints: detection.keypoints.clone(),
            mask: detection.mask.as_ref().map(MaskRecord::from_mask),
        }
    }
}
