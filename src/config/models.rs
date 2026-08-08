use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ModelTask {
    Detect,
    Pose,
    Segment,
    Depth,
    Classify,
    Obb,
    Semantic,
}

impl ModelTask {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Detect => "detect",
            Self::Pose => "pose",
            Self::Segment => "segment",
            Self::Depth => "depth",
            Self::Classify => "classify",
            Self::Obb => "obb",
            Self::Semantic => "semantic",
        }
    }
}

impl fmt::Display for ModelTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelCatalog {
    pub models: HashMap<String, ModelEntry>,
}

#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub path: PathBuf,
    pub task: ModelTask,
    pub enabled: bool,
    pub confidence: f32,
    pub iou: f32,
    pub max_det: u32,
    pub imgsz: Option<u32>,
    pub device: String,
    pub half: bool,
    pub rect: bool,
    pub polygon_simplify: f64,
    pub postprocess: PostprocessConfig,
    pub crop: Option<CropConfig>,
}

impl ModelEntry {
    pub fn is_valid(&self) -> bool {
        self.confidence.is_finite()
            && (0.0..=1.0).contains(&self.confidence)
            && self.iou.is_finite()
            && (0.0..=1.0).contains(&self.iou)
            && self.max_det > 0
            && self.imgsz.is_none_or(|size| size > 0)
            && self.polygon_simplify.is_finite()
            && self.polygon_simplify > 0.0
    }
}

#[derive(Debug, Clone)]
pub struct PostprocessConfig {
    pub allow_classes: Vec<String>,
    pub min_confidence: f32,
    pub min_area_ratio: f32,
    pub max_area_ratio: f32,
    pub min_component_area_ratio: f32,
    pub mask_threshold: f32,
    pub nms_iou: f32,
    pub max_detections: Option<usize>,
}

impl PostprocessConfig {
    pub fn is_valid(&self) -> bool {
        self.allow_classes
            .iter()
            .all(|class| !class.trim().is_empty())
            && self.min_confidence.is_finite()
            && (0.0..=1.0).contains(&self.min_confidence)
            && self.min_area_ratio.is_finite()
            && self.min_area_ratio >= 0.0
            && self.max_area_ratio.is_finite()
            && self.max_area_ratio <= 1.0
            && self.max_area_ratio >= self.min_area_ratio
            && self.min_component_area_ratio.is_finite()
            && (0.0..=1.0).contains(&self.min_component_area_ratio)
            && self.mask_threshold.is_finite()
            && (0.0..=1.0).contains(&self.mask_threshold)
            && self.nms_iou.is_finite()
            && (0.0..=1.0).contains(&self.nms_iou)
            && self.max_detections.is_none_or(|max| max > 0)
    }

    pub fn accepts(
        &self,
        class: &str,
        confidence: f32,
        bbox: [f32; 4],
        frame_w: u32,
        frame_h: u32,
    ) -> bool {
        self.rejection_reason(class, confidence, bbox, frame_w, frame_h)
            .is_none()
    }

    pub fn rejection_reason(
        &self,
        class: &str,
        confidence: f32,
        bbox: [f32; 4],
        frame_w: u32,
        frame_h: u32,
    ) -> Option<&'static str> {
        let [x1, y1, x2, y2] = bbox;
        let width = x2 - x1;
        let height = y2 - y1;
        let frame_area = (frame_w as f32) * (frame_h as f32);
        let area_ratio = if frame_area > 0.0 {
            (width * height) / frame_area
        } else {
            0.0
        };

        if !self.allow_classes.is_empty()
            && !self.allow_classes.iter().any(|allowed| allowed == class)
        {
            return Some("class");
        }
        if !confidence.is_finite() || confidence < self.min_confidence {
            return Some("confidence");
        }
        if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite()) {
            return Some("non_finite_bbox");
        }
        if x1 < 0.0 || y1 < 0.0 || x2 > frame_w as f32 || y2 > frame_h as f32 {
            return Some("bbox_outside_frame");
        }
        if width <= 0.0 || height <= 0.0 {
            return Some("invalid_bbox");
        }
        if frame_area <= 0.0 {
            return Some("invalid_frame");
        }
        if area_ratio < self.min_area_ratio {
            return Some("min_area_ratio");
        }
        if area_ratio > self.max_area_ratio {
            return Some("max_area_ratio");
        }
        None
    }
}

impl Default for PostprocessConfig {
    fn default() -> Self {
        Self {
            allow_classes: Vec::new(),
            min_confidence: default_postprocess_min_confidence(),
            min_area_ratio: default_postprocess_min_area_ratio(),
            max_area_ratio: default_postprocess_max_area_ratio(),
            min_component_area_ratio: default_postprocess_min_component_area_ratio(),
            mask_threshold: default_postprocess_mask_threshold(),
            nms_iou: default_postprocess_nms_iou(),
            max_detections: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CropConfig {
    pub crop_type: CropType,
    #[allow(dead_code)]
    pub class: Option<String>,
    pub margin: f32,
    pub region: Option<[u32; 4]>,
    pub min_region: Option<[u32; 4]>,
    pub max_region: Option<[u32; 4]>,
    pub square_size: Option<u32>,
    pub upper_fraction: Option<f32>,
    #[allow(dead_code)]
    pub fallback: FallbackMode,
}

impl CropConfig {
    pub fn is_valid(&self) -> bool {
        self.square_size != Some(0)
            && self
                .upper_fraction
                .is_none_or(|fraction| fraction.is_finite() && (0.0..=1.0).contains(&fraction))
    }

    #[allow(dead_code)]
    pub fn always_run(&self) -> bool {
        self.min_region.is_some() || self.fallback == FallbackMode::Full
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CropType {
    Static,
    LargestClass,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FallbackMode {
    #[default]
    Skip,
    Full,
}

pub(crate) fn default_postprocess_min_confidence() -> f32 {
    0.0
}

pub(crate) fn default_postprocess_min_area_ratio() -> f32 {
    0.0
}

pub(crate) fn default_postprocess_max_area_ratio() -> f32 {
    1.0
}

pub(crate) fn default_postprocess_min_component_area_ratio() -> f32 {
    0.0
}

pub(crate) fn default_postprocess_mask_threshold() -> f32 {
    0.5
}

pub(crate) fn default_postprocess_nms_iou() -> f32 {
    0.5
}

pub(crate) fn default_crop_margin() -> f32 {
    0.15
}

pub(crate) fn default_confidence() -> f32 {
    0.25
}

pub(crate) fn default_iou() -> f32 {
    0.7
}

pub(crate) fn default_max_det() -> u32 {
    300
}

pub(crate) fn default_device() -> String {
    "cpu".into()
}

pub(crate) fn default_rect() -> bool {
    true
}

pub(crate) fn default_polygon_simplify() -> f64 {
    0.75
}
