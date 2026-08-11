use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::config::{RerunRoot, VizSendToggles};
use crate::detection::CropRect;
use crate::domain::{ModelRegistry, ModelRole};

mod boxes;
mod connection;
mod frame;
mod masks;
mod rois;
mod state;

// Bring helpers into this module so `tests` / `mask_debug_tests` keep `use super::*`.
#[cfg(test)]
use crate::detection::Detection;
#[cfg(test)]
use crate::occupancy::SignalValidity;
#[cfg(test)]
use boxes::{bbox_in_crop, bbox_in_roi, detection_label, pose_keypoint_visible};
#[cfg(test)]
use image::Rgb;
#[cfg(test)]
use masks::{build_mask_overlay, frame_strip, render_mask_debug_images};

enum Inner {
    /// A sink exists. This is *not* proof that a viewer is listening: the gRPC
    /// sink connects lazily, so `Connected` only means "we have somewhere to
    /// write". Liveness is established by the first flush that returns `Ok`.
    Connected {
        rec: rerun::RecordingStream,
        last_flush_warn: Instant,
        /// Run of consecutive [`rerun::SinkFlushError::Timeout`] results.
        /// A timeout is backpressure, not a disconnect, so it takes a
        /// sustained run of them to justify dropping the sink.
        flush_timeouts: u32,
    },
    Disconnected {
        next_retry: Instant,
        last_warn: Instant,
    },
    Disabled,
}

pub struct VizBridge {
    inner: Inner,
    addr: String,
    /// Retry delay, owned by the bridge rather than by [`Inner::Disconnected`].
    /// Creating a sink always succeeds, so a backoff scoped to the disconnected
    /// state would be reset on every retry and never grow. It is reset only by
    /// a flush that actually reaches a viewer.
    retry_backoff_ms: u64,
    /// Whether any flush has succeeded on the current sink.
    stream_proven: bool,
    toggles: VizSendToggles,
    fixed_rois: Vec<FixedRoi>,
    roles: HashMap<String, ModelRole>,
    face_models: HashSet<String>,
    last_infer_at: HashMap<String, Instant>,
    last_occupancy_state: Option<crate::occupancy::RoomCardinality>,
    last_second_person_state: Option<crate::occupancy::SecondPersonState>,
    last_signal_state: Option<crate::occupancy::SignalValidity>,
    last_face_state: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FixedRoi {
    pub model: String,
    pub rect: CropRect,
}

const DEPTH_OVERLAY_ALPHA: u8 = 150;
const POSE_KEYPOINT_CONFIDENCE: f32 = 0.25;
const FRAME_NUMBER_TIMELINE: &str = "frame_nr";
const FRAME_TIME_TIMELINE: &str = "frame_time";
const INITIAL_BACKOFF_MS: u64 = 1_000;
const MAX_BACKOFF_MS: u64 = 30_000;
/// How long a liveness probe waits for the batcher to drain.
const FLUSH_TIMEOUT_MS: u64 = 100;
/// Sustained backpressure tolerated before the sink is dropped. At scan
/// cadence this is a couple of seconds, long enough to ride out a large frame
/// on a slow link but short enough to bound the batcher backlog.
const MAX_FLUSH_TIMEOUTS: u32 = 10;

/// Instance-mask palette, keyed by (class-id − 1) mod len (ADR-022).
pub(super) const PALETTE: [[u8; 3]; 8] = [
    [230, 25, 75],
    [60, 180, 75],
    [255, 225, 25],
    [0, 130, 200],
    [245, 130, 48],
    [145, 30, 180],
    [70, 240, 240],
    [240, 50, 230],
];

fn sanitize_entity_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl VizBridge {
    pub fn new(
        rerun_addr: &str,
        toggles: &VizSendToggles,
        _blueprint: &RerunRoot,
        fixed_rois: Vec<FixedRoi>,
        models: &ModelRegistry,
    ) -> Self {
        Self {
            inner: Inner::Disconnected {
                next_retry: Instant::now(),
                last_warn: Instant::now(),
            },
            addr: rerun_addr.to_string(),
            retry_backoff_ms: INITIAL_BACKOFF_MS,
            stream_proven: false,
            toggles: toggles.clone(),
            fixed_rois,
            roles: models
                .iter()
                .map(|(id, entry)| (id.as_str().to_owned(), entry.semantics.role))
                .collect(),
            face_models: models
                .iter()
                .filter(|(id, _)| models.is_face_model(id.as_str()))
                .map(|(id, _)| id.as_str().to_owned())
                .collect(),
            last_infer_at: HashMap::new(),
            last_occupancy_state: None,
            last_second_person_state: None,
            last_signal_state: None,
            last_face_state: None,
        }
    }

    pub fn disabled() -> Self {
        Self {
            inner: Inner::Disabled,
            addr: String::new(),
            toggles: VizSendToggles::default(),
            fixed_rois: Vec::new(),
            roles: HashMap::new(),
            face_models: HashSet::new(),
            last_infer_at: HashMap::new(),
            last_occupancy_state: None,
            last_second_person_state: None,
            last_signal_state: None,
            last_face_state: None,
        }
    }

    pub(super) fn role_of(&self, model: &str) -> ModelRole {
        self.roles.get(model).copied().unwrap_or(ModelRole::Boxes)
    }

    pub(super) fn is_face_model(&self, model: &str) -> bool {
        self.face_models.contains(model)
    }
}

#[cfg(test)]
mod mask_debug_tests;
#[cfg(test)]
mod tests;
