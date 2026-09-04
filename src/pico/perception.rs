use std::sync::Arc;
use std::time::Duration;

use crate::pico::infer::{InferEngine, InferenceResult};
use crate::pico::ingest::DecodedFrame;
use crate::slot::Slot;

/// Spawn the perception thread. Reads decoded frames from the slot,
/// runs inference, and publishes results to the output slot.
///
/// This is a dedicated thread (not a tokio task) because:
/// 1. Inference is CPU-bound (~200ms)
/// 2. The ONNX runtime is not Send-safe across await points
/// 3. It runs at its own cadence (keyframe rate, ~1 Hz)
pub fn spawn(
    model_path: String,
    model_name: String,
    confidence: f32,
    input: Arc<Slot<DecodedFrame>>,
    output: Arc<Slot<InferenceResult>>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("perception".into())
        .spawn(move || {
            perception_loop(&model_path, &model_name, confidence, &input, &output);
        })
        .expect("failed to spawn perception thread")
}

fn perception_loop(
    model_path: &str,
    model_name: &str,
    confidence: f32,
    input: &Slot<DecodedFrame>,
    output: &Slot<InferenceResult>,
) {
    let mut engine = match InferEngine::load(model_path, model_name, confidence) {
        Ok(e) => e,
        Err(e) => {
            log::error!("perception: failed to load model: {e}");
            return;
        }
    };

    log::info!("perception: entering poll loop");

    loop {
        // Wait for a decoded frame. This blocks the thread until a frame arrives.
        // The slot's take_blocking will return None when the slot is closed.
        let frame = match input.take_blocking() {
            Some(f) => f,
            None => break, // slot closed, shutting down
        };

        // Run inference.
        if let Some(result) = engine.run(&frame) {
            log::debug!(
                "perception: frame {} → {} detections ({:?})",
                result.frame_id,
                result.detections.len(),
                Duration::from_millis(result.infer_ms),
            );

            // Publish result. If the control loop hasn't consumed the previous
            // result, this one pisar it — which is correct (ADR-034: samples,
            // not events).
            output.put(result);
        }
    }

    log::info!("perception: thread finished");
}
