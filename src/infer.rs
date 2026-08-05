use std::collections::HashMap;
use std::path::Path;

use image::{DynamicImage, RgbImage};
use ultralytics_inference::{Device, InferenceConfig, Results, YOLOModel};

use crate::config::{ModelCatalog, ModelEntry};
use crate::error::Result;
use crate::logger::DetRecord;

pub struct InferEngine {
    models: HashMap<String, LoadedModel>,
}

struct LoadedModel {
    model: YOLOModel,
    run_count: u64,
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

            let conf = build_config(entry);

            match YOLOModel::load_with_config(&entry.path, conf) {
                Ok(model) => {
                    log::info!("model {key}: loaded ({})", model.task());
                    models.insert(key.clone(), LoadedModel {
                        model,
                        run_count: 0,
                    });
                }
                Err(e) => {
                    log::error!("model {key}: load failed — {e}");
                }
            }
        }
        Ok(Self { models })
    }

    pub fn run(
        &mut self,
        model_key: &str,
        rgb: &[u8],
        w: u32,
        h: u32,
    ) -> Option<(Vec<Detection>, u64)> {
        let loaded = self.models.get_mut(model_key)?;

        // Clone: DynamicImage toma ownership del Vec<u8>.
        // El FrameBuffer original sigue disponible para snapshots + viz.
        let img = DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, rgb.to_vec())?);
        let results = loaded.model.predict_image(&img, String::new()).ok()?;

        let infer_ms = results
            .first()
            .and_then(|r| r.speed.inference)
            .map(|ms| (ms * 1000.0) as u64)
            .unwrap_or(0);

        let detections = collect_detections(&results);
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
