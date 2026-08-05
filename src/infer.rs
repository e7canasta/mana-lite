use std::collections::HashMap;
use std::path::Path;

use image::{DynamicImage, RgbImage};
use ultralytics_inference::{Device, InferenceConfig, Results, YOLOModel};

use crate::config::{CropConfig, CropType, FallbackMode, ModelCatalog, ModelEntry};
use crate::error::Result;
use crate::logger::DetRecord;

pub struct InferEngine {
    models: HashMap<String, LoadedModel>,
}

struct LoadedModel {
    model: YOLOModel,
    run_count: u64,
    crop_config: Option<CropConfig>,
}

pub struct Detection {
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
}

impl From<&Detection> for DetRecord {
    fn from(d: &Detection) -> Self {
        DetRecord {
            class: d.class.clone(),
            confidence: d.confidence,
            bbox: d.bbox,
        }
    }
}

impl InferEngine {
    pub fn from_catalog(catalog: &ModelCatalog) -> Result<Self> {
        let mut models = HashMap::new();
        for (key, entry) in &catalog.models {
            if !Path::new(&entry.path).exists() {
                log::warn!("model {key}: file not found at {}, skipping", entry.path.display());
                continue;
            }

            let mut conf = build_config(entry);

            if let Some(ref crop) = entry.crop {
                if crop.crop_type == CropType::Static {
                    if let Some([x1, y1, x2, y2]) = crop.region {
                        conf = conf.with_roi(x1, y1, x2, y2);
                        log::info!("model {key}: static ROI [{x1},{y1} {x2},{y2}]");
                    }
                }
            }

            match YOLOModel::load_with_config(&entry.path, conf) {
                Ok(model) => {
                    log::info!("model {key}: loaded ({})", model.task());
                    models.insert(key.clone(), LoadedModel {
                        model,
                        run_count: 0,
                        crop_config: entry.crop.as_ref()
                            .filter(|c| c.crop_type == CropType::LargestClass)
                            .cloned(),
                    });
                }
                Err(e) => {
                    log::error!("model {key}: load failed — {e}");
                }
            }
        }
        Ok(Self { models })
    }

    pub fn crop_info(&self, model_key: &str) -> Option<&CropConfig> {
        self.models.get(model_key)?.crop_config.as_ref()
    }

    pub fn force_run(&self, model_key: &str) -> bool {
        match self.crop_info(model_key) {
            Some(c) => c.min_region.is_some() || c.fallback == FallbackMode::Full,
            None => false,
        }
    }

    pub fn run(
        &mut self,
        model_key: &str,
        rgb: &[u8],
        w: u32,
        h: u32,
        crop_rect: Option<(u32, u32, u32, u32)>,
    ) -> Option<(Vec<Detection>, u64)> {
        let loaded = self.models.get_mut(model_key)?;

        let (img, offset_x, offset_y) = if let Some((x1, y1, x2, y2)) = crop_rect {
            let crop_w = x2 - x1;
            let crop_h = y2 - y1;
            if crop_w == 0 || crop_h == 0 {
                return None;
            }
            let mut cropped = vec![0u8; (crop_w * crop_h * 3) as usize];
            for row in y1..y2 {
                let src_off = (row * w + x1) as usize * 3;
                let dst_off = ((row - y1) * crop_w) as usize * 3;
                cropped[dst_off..dst_off + (crop_w as usize * 3)]
                    .copy_from_slice(&rgb[src_off..src_off + (crop_w as usize * 3)]);
            }
            let img = DynamicImage::ImageRgb8(RgbImage::from_raw(crop_w, crop_h, cropped)?);
            (img, x1 as f32, y1 as f32)
        } else {
            let img = DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, rgb.to_vec())?);
            (img, 0.0, 0.0)
        };

        let results = loaded.model.predict_image(&img, String::new()).ok()?;

        let infer_ms = results
            .first()
            .and_then(|r| r.speed.inference)
            .map(|ms| (ms * 1000.0) as u64)
            .unwrap_or(0);

        let mut detections = collect_detections(&results);
        for d in &mut detections {
            d.bbox[0] += offset_x;
            d.bbox[1] += offset_y;
            d.bbox[2] += offset_x;
            d.bbox[3] += offset_y;
        }
        loaded.run_count += 1;

        Some((detections, infer_ms))
    }

    pub fn model_count(&self) -> usize {
        self.models.len()
    }
}

fn build_config(entry: &ModelEntry) -> InferenceConfig {
    let mut c = InferenceConfig::default()
        .with_confidence(entry.confidence)
        .with_iou(entry.iou)
        .with_max_det(entry.max_det as usize)
        .with_rect(entry.rect);

    if entry.half {
        c = c.with_half(true);
    }

    if let Some(imgsz) = entry.imgsz {
        c = c.with_imgsz(imgsz as usize, imgsz as usize);
    }

    if entry.device != "cpu" {
        match entry.device.parse::<Device>() {
            Ok(dev) => c = c.with_device(dev),
            Err(e) => log::warn!("invalid device '{}': {e}, using cpu", entry.device),
        }
    }

    c
}

fn collect_detections(results: &[Results]) -> Vec<Detection> {
    let mut dets = Vec::new();

    for r in results {
        let Some(ref boxes) = r.boxes else { continue };
        let xyxy = boxes.xyxy();

        for i in 0..boxes.len() {
            if i >= xyxy.nrows() {
                break;
            }
            let cls_id_raw = boxes.cls().get(i).copied().unwrap_or(-1.0);
            if cls_id_raw < 0.0 {
                continue;
            }
            let cls = cls_id_raw as usize;
            dets.push(Detection {
                class: r.names.get(&cls).cloned().unwrap_or_else(|| "unknown".into()),
                confidence: boxes.conf().get(i).copied().unwrap_or(0.0),
                bbox: [xyxy[[i, 0]], xyxy[[i, 1]], xyxy[[i, 2]], xyxy[[i, 3]]],
            });
        }
    }

    dets
}

pub fn compute_largest_class_roi(
    detections: &[Detection],
    target_class: &str,
    margin: f32,
    frame_w: u32,
    frame_h: u32,
    min_region: Option<[u32; 4]>,
    max_region: Option<[u32; 4]>,
) -> Option<(u32, u32, u32, u32)> {
    let class_rect = detections
        .iter()
        .filter(|d| d.class == target_class)
        .max_by(|a, b| {
            let area_a = (a.bbox[2] - a.bbox[0]) * (a.bbox[3] - a.bbox[1]);
            let area_b = (b.bbox[2] - b.bbox[0]) * (b.bbox[3] - b.bbox[1]);
            area_a.partial_cmp(&area_b).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|best| {
            let [bx1, by1, bx2, by2] = best.bbox;
            let bw = bx2 - bx1;
            let bh = by2 - by1;
            let expand_w = bw * margin;
            let expand_h = bh * margin;
            (
                (bx1 - expand_w).max(0.0) as u32,
                (by1 - expand_h).max(0.0) as u32,
                ((bx2 + expand_w) as u32).min(frame_w),
                ((by2 + expand_h) as u32).min(frame_h),
            )
        });

    let mut result = match (class_rect, min_region) {
        (Some((cx1, cy1, cx2, cy2)), Some([mx1, my1, mx2, my2])) => (
            cx1.min(mx1), cy1.min(my1),
            cx2.max(mx2), cy2.max(my2),
        ),
        (Some(cr), None) => cr,
        (None, Some([mx1, my1, mx2, my2])) => (mx1, my1, mx2, my2),
        (None, None) => return None,
    };

    if let Some([mx1, my1, mx2, my2]) = max_region {
        result.0 = result.0.max(mx1);
        result.1 = result.1.max(my1);
        result.2 = result.2.min(mx2);
        result.3 = result.3.min(my2);
    }

    if result.2 <= result.0 || result.3 <= result.1 {
        return None;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(x1: f32, y1: f32, x2: f32, y2: f32) -> Detection {
        Detection { class: "person".into(), confidence: 0.9, bbox: [x1, y1, x2, y2] }
    }

    #[test]
    fn roi_largest_class_simple() {
        let dets = vec![person(100.0, 100.0, 200.0, 300.0)];
        let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, None, None).unwrap();
        assert_eq!(r, (100, 100, 200, 300));
    }

    #[test]
    fn roi_min_region_union() {
        let dets = vec![person(300.0, 100.0, 400.0, 200.0)];
        let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, Some([100, 200, 500, 450]), None).unwrap();
        assert_eq!(r, (100, 100, 500, 450));
    }

    #[test]
    fn roi_min_region_fallback() {
        let dets: Vec<Detection> = vec![];
        let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, Some([100, 200, 500, 450]), None).unwrap();
        assert_eq!(r, (100, 200, 500, 450));
    }

    #[test]
    fn roi_max_region_clamps() {
        let dets = vec![person(0.0, 0.0, 640.0, 480.0)];
        let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, None, Some([50, 50, 400, 300])).unwrap();
        assert_eq!(r, (50, 50, 400, 300));
    }

    #[test]
    fn roi_min_max_together() {
        let dets = vec![person(200.0, 100.0, 300.0, 200.0)];
        let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, Some([50, 50, 500, 400]), Some([0, 0, 350, 300])).unwrap();
        assert_eq!(r, (50, 50, 350, 300));
    }

    #[test]
    fn roi_no_class_no_min() {
        let dets: Vec<Detection> = vec![];
        let r = compute_largest_class_roi(&dets, "person", 0.0, 640, 480, None, None);
        assert!(r.is_none());
    }
}
