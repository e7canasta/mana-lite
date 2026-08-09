//! Per-keyframe cycle inputs shared across pipeline stages.

use crate::snapshot::FrameBuffer;
use std::time::Instant;

/// Bundle of per-cycle facts previously passed as loose parameters through
/// `run_inference` and related stage methods.
#[derive(Clone, Copy)]
pub struct CycleContext<'a> {
    pub frame: &'a FrameBuffer,
    pub now: Instant,
    pub keyframe_gap_ms: u64,
    pub source_window_ms: u64,
    pub keyframes_seen: u64,
    pub keyframes_dropped: u64,
    pub frame_number: u64,
    pub timestamp_ns: i64,
}

impl<'a> CycleContext<'a> {
    #[must_use]
    pub const fn new(
        frame: &'a FrameBuffer,
        now: Instant,
        keyframe_gap_ms: u64,
        source_window_ms: u64,
        keyframes_seen: u64,
        keyframes_dropped: u64,
        frame_number: u64,
        timestamp_ns: i64,
    ) -> Self {
        Self {
            frame,
            now,
            keyframe_gap_ms,
            source_window_ms,
            keyframes_seen,
            keyframes_dropped,
            frame_number,
            timestamp_ns,
        }
    }

    #[must_use]
    pub const fn frame_size(&self) -> mana_viz::logging::util::FrameSize {
        mana_viz::logging::util::FrameSize::new(self.frame.w, self.frame.h)
    }
}
