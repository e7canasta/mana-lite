//! Record inference outputs: metrics, viz, JSONL detection/depth events.

use crate::detection::CropRect;
use crate::infer::InferenceResult;
use crate::logger::{DetRecord, Event};
use crate::metrics::PerClassFrameStats;
use std::time::Instant;

use super::perception::PerceptionStage;
use super::{CropFrameQueue, FrameSize};

impl PerceptionStage {
    pub(super) fn record_model_result(
        &mut self,
        model_key: &str,
        output: &InferenceResult,
        crop_frame: Option<crate::infer::CropFrameInfo>,
        crop_rect: Option<CropRect>,
        frame: FrameSize,
        now: Instant,
    ) {
        if self.models.is_depth(model_key) {
            self.record_depth_result(model_key, output, crop_rect, frame.w, frame.h, now);
            return;
        }
        if output.postprocess_rejected > 0 || output.post_nms_suppressed > 0 {
            log::info!(
                "model {model_key}: postprocess rejected={} nms_suppressed={}",
                output.postprocess_rejected,
                output.post_nms_suppressed,
            );
        }
        let per_class = PerClassFrameStats::from_detections(&output.detections);
        self.lock_metrics().tick_inference_model(
            model_key,
            output.pipeline_us,
            &output.detections,
            crop_rect.map(|r| r.to_array()),
        );
        self.log_model_viz(model_key, output, crop_rect, &per_class, frame);
        if crop_rect.is_some()
            && !(self.models.is_face_model(model_key) && output.detections.is_empty())
        {
            self.crop_frames_pending.push(CropFrameQueue {
                model: model_key.to_string(),
                crop_frame,
            });
        }
        self.observer.emit(Event::detection(
            self.frame_number(),
            model_key,
            output.infer_ms,
            output.pipeline_us / 1000,
            output
                .detections
                .iter()
                .map(|detection| DetRecord::from_detection(detection, frame.w, frame.h))
                .collect(),
            output.postprocess_rejected,
            output.post_nms_suppressed,
            Some(per_class),
            crop_rect.map(|r| r.to_array()),
        ));
    }

    #[cfg(feature = "rerun")]
    fn log_model_viz(
        &mut self,
        model_key: &str,
        output: &InferenceResult,
        crop_rect: Option<CropRect>,
        per_class: &PerClassFrameStats,
        frame: FrameSize,
    ) {
        self.observer
            .viz
            .log_infer_latency(model_key, output.infer_ms * 1000, output.pipeline_us);
        if let Some(rect) = crop_rect {
            self.observer.viz.log_roi_boxes(model_key, rect);
        }
        self.observer
            .viz
            .log_per_frame_class_stats(model_key, per_class);
        self.observer
            .viz
            .log_model_detections(model_key, &output.detections, crop_rect, frame);
        self.observer
            .viz
            .log_model_pose(model_key, &output.detections);
        self.observer.viz.log_depth_context_boxes(
            model_key,
            &output.detections,
            self.depth_context_roi,
        );
        self.observer.viz.log_depth_context_polygons(
            model_key,
            &output.detections,
            self.depth_context_roi,
            frame,
        );
        if output.detections.iter().any(|d| d.mask.is_some()) {
            self.observer
                .viz
                .log_model_masks(model_key, &output.detections, frame);
        }
    }

    #[cfg(not(feature = "rerun"))]
    fn log_model_viz(
        &mut self,
        _model_key: &str,
        _output: &InferenceResult,
        _crop_rect: Option<CropRect>,
        _per_class: &PerClassFrameStats,
        _frame: FrameSize,
    ) {
    }

    fn record_depth_result(
        &mut self,
        model_key: &str,
        output: &InferenceResult,
        crop_rect: Option<CropRect>,
        frame_w: u32,
        frame_h: u32,
        now: Instant,
    ) {
        let (width, height, valid_pixels, min_depth_m, max_depth_m) =
            depth_summary(output.depth.as_ref(), frame_w, frame_h);
        let map_area = (width as u64) * (height as u64);
        let valid_ratio = (map_area > 0).then(|| valid_pixels as f32 / map_area as f32);
        self.lock_metrics().tick_inference_depth(
            model_key,
            output.pipeline_us,
            output.depth.as_ref(),
            crop_rect.map(|rect| rect.to_array()),
        );
        #[cfg(feature = "rerun")]
        {
            self.observer.viz.log_infer_latency(
                model_key,
                output.infer_ms * 1000,
                output.pipeline_us,
            );
            if let Some(rect) = crop_rect {
                self.observer.viz.log_roi_boxes(model_key, rect);
            }
            self.observer
                .viz
                .log_model_depth(model_key, output.depth.as_ref());
        }
        self.observer.emit(Event::depth(
            self.frame_number(),
            model_key,
            output.infer_ms,
            output.pipeline_us / 1000,
            crop_rect.map(|rect| rect.to_array()),
            width,
            height,
            valid_pixels,
            valid_ratio,
            min_depth_m,
            max_depth_m,
        ));
        self.evaluate_depth_rules(output, crop_rect, now);
    }

    pub(crate) fn evaluate_depth_rules(
        &mut self,
        output: &InferenceResult,
        crop_rect: Option<CropRect>,
        now: Instant,
    ) {
        let Some(depth) = output.depth.as_ref() else {
            return;
        };
        let Some(roi) = crop_rect
            .or(self.depth_context_roi)
            .map(|rect| rect.to_array())
        else {
            return;
        };
        let measurements: Vec<mana_control::DepthRegionStats> = self
            .depth_rules
            .rules
            .iter()
            .filter_map(|rule| {
                let stats = mana_perception::region_stats(depth, roi, rule.region)?;
                Some(mana_control::DepthRegionStats {
                    region: stats.region,
                    valid_pixels: stats.valid_pixels,
                    valid_ratio: stats.valid_ratio,
                    min_depth_m: stats.min_depth_m,
                    median_depth_m: stats.median_depth_m,
                    p10_depth_m: stats.p10_depth_m,
                    p90_depth_m: stats.p90_depth_m,
                    max_depth_m: stats.max_depth_m,
                })
            })
            .collect();
        let results = self.depth_rules.evaluate(&measurements);
        wire_depth_evidence(&mut self.image, &results, now);
        for result in &results {
            self.observer.emit(Event::depth_region(
                self.frame_number(),
                &result.rule,
                result.region,
                &format!("{:?}", result.metric).to_lowercase(),
                result.value,
                result.threshold_m,
                result.triggered,
                result.valid_pixels,
                result.valid_ratio,
                result.calibration,
            ));
        }
    }
}

/// Installs evaluated depth policy results into the control process image.
pub(super) fn wire_depth_evidence(
    image: &mut mana_control::ProcessImage,
    results: &[mana_control::DepthRuleResult],
    now: Instant,
) {
    image.set_depth(mana_control::DepthRuleSnapshot::from_results(results), now);
}

fn depth_summary(
    depth: Option<&crate::depth_map::DepthFrame>,
    fallback_width: u32,
    fallback_height: u32,
) -> (u32, u32, u64, Option<f32>, Option<f32>) {
    let Some(depth) = depth else {
        return (fallback_width, fallback_height, 0, None, None);
    };
    let (width, height) = depth.dims();
    let mut valid_pixels = 0;
    let mut min_depth = f32::INFINITY;
    let mut max_depth: f32 = 0.0;
    for value in depth.iter_values() {
        if value.is_finite() && value > 0.0 {
            valid_pixels += 1;
            min_depth = min_depth.min(value);
            max_depth = max_depth.max(value);
        }
    }
    if valid_pixels == 0 {
        (width, height, 0, None, None)
    } else {
        (
            width,
            height,
            valid_pixels,
            Some(min_depth),
            Some(max_depth),
        )
    }
}
