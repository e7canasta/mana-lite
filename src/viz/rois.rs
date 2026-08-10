//! Static ROI entities for Rerun viz.

use crate::detection::CropRect;

use super::{Inner, VizBridge, boxes::boxes2d_from_xyxy, sanitize_entity_name};

impl VizBridge {
    pub(crate) fn send_fixed_rois(rec: &rerun::RecordingStream, fixed_rois: &[super::FixedRoi]) {
        for fixed in fixed_rois {
            let model = sanitize_entity_name(&fixed.model);
            let path = format!("/world/camera/rois/fixed/{model}/roi");
            let [x1, y1, x2, y2] = fixed.rect.to_array();
            let x1 = x1 as f32;
            let y1 = y1 as f32;
            let x2 = x2 as f32;
            let y2 = y2 as f32;
            let label = format!("fixed {model} [{:.0},{:.0} {:.0},{:.0}]", x1, y1, x2, y2);
            let color = rerun::Color::from_unmultiplied_rgba(255, 200, 0, 255);
            let bbox = boxes2d_from_xyxy([x1, y1, x2, y2], color, Some(&label), 2.0);
            if let Err(e) = rec.log_static(path.as_str(), &bbox) {
                log::warn!("viz fixed ROI {model} failed: {e}");
            }
        }
    }

    pub fn log_roi_boxes(&self, model: &str, rect: CropRect) {
        if !self.toggles.roi_rects {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = format!("/world/camera/rois/{model}");
        rec.log(path.as_str(), &rerun::Clear::recursive()).ok();

        let x1 = rect.x1 as f32;
        let y1 = rect.y1 as f32;
        let x2 = rect.x2 as f32;
        let y2 = rect.y2 as f32;
        let label = format!("ROI {:.0}x{:.0}", (x2 - x1).abs(), (y2 - y1).abs());

        let bbox = boxes2d_from_xyxy(
            [x1, y1, x2, y2],
            rerun::Color::from_unmultiplied_rgba(0, 255, 0, 255),
            Some(&label),
            2.0,
        );

        let entity = format!("{path}/roi/0");
        if let Err(e) = rec.log(entity.as_str(), &bbox) {
            log::warn!("viz roi boxes {model} failed: {e}");
        }
    }
}
