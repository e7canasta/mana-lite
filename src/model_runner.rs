//! Backend-agnostic inference runner trait (ARCHITECTURE principle 5).

use crate::infer::{CropRect, Detection};
use crate::depth_map::DepthFrame;

/// Result of a single model invocation, independent of ONNX / ultralytics types.
#[derive(Debug)]
pub struct RunnerOutput {
    pub detections: Vec<Detection>,
    pub depth: Option<DepthFrame>,
    pub infer_ms: u64,
}

/// Abstraction over the concrete inference backend.
///
/// Production uses [`UltralyticsRunner`]; tests can supply a stub without ONNX.
pub trait ModelRunner {
    fn run(
        &mut self,
        model_key: &str,
        rgb: &[u8],
        width: u32,
        height: u32,
        crop: Option<CropRect>,
    ) -> Option<RunnerOutput>;

    fn model_count(&self) -> usize;
}
