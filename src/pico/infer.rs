use std::path::Path;
use std::time::Instant;

use ultralytics_inference::{InferenceConfig, YOLOModel};

use crate::pico::ingest::DecodedFrame;

/// Detection from a single model.
#[derive(Debug, Clone)]
pub struct Detection {
    pub class_id: usize,
    pub class_name: String,
    pub confidence: f32,
    pub bbox: [f32; 4], // [x1, y1, x2, y2] normalized
}

/// Result of running inference on a frame.
pub struct InferenceResult {
    pub detections: Vec<Detection>,
    pub infer_ms: u64,
    pub frame_id: u64,
}

/// Minimal inference engine: loads one model and runs it.
pub struct InferEngine {
    model: YOLOModel,
    model_name: String,
    confidence: f32,
}

impl InferEngine {
    pub fn load(model_path: &str, model_name: &str, confidence: f32) -> Result<Self, String> {
        let path = Path::new(model_path);
        if !path.exists() {
            return Err(format!("model not found: {model_path}"));
        }

        let model = YOLOModel::load(model_path).map_err(|e| format!("load model: {e}"))?;

        log::info!(
            "infer: loaded '{}' from {} (conf={})",
            model_name,
            model_path,
            confidence
        );

        Ok(Self {
            model,
            model_name: model_name.to_string(),
            confidence,
        })
    }

    /// Run inference on a decoded frame.
    pub fn run(&mut self, frame: &DecodedFrame) -> Option<InferenceResult> {
        let t0 = Instant::now();

        // Convert RGB to image for ultralytics.
        let img = image::RgbImage::from_raw(frame.width, frame.height, frame.rgb.clone())?;
        let dynamic = image::DynamicImage::ImageRgb8(img);

        // Run inference.
        let config = InferenceConfig {
            confidence_threshold: self.confidence,
            ..Default::default()
        };

        let results = self
            .model
            .predict_image(&dynamic, format!("frame_{}", frame.frame_id))
            .ok()?;

        let mut detections = Vec::new();
        for result in &results {
            if let Some(ref boxes) = result.boxes {
                for i in 0..boxes.len() {
                    let cls = boxes.cls()[i] as usize;
                    let conf = boxes.conf()[i];
                    let name = result
                        .names
                        .get(&cls)
                        .map(|s| s.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let bbox = boxes.xyxy();
                    detections.push(Detection {
                        class_id: cls,
                        class_name: name,
                        confidence: conf,
                        bbox: [bbox[[i, 0]], bbox[[i, 1]], bbox[[i, 2]], bbox[[i, 3]]],
                    });
                }
            }
        }

        let infer_ms = t0.elapsed().as_millis() as u64;

        Some(InferenceResult {
            detections,
            infer_ms,
            frame_id: frame.frame_id,
        })
    }
}
