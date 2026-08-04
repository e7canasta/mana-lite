use anyhow::{Context, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameSize {
    pub w: u32,
    pub h: u32,
}

impl FrameSize {
    #[inline]
    pub const fn new(w: u32, h: u32) -> Self {
        Self { w, h }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }

    #[inline]
    pub const fn or(self, fallback: FrameSize) -> FrameSize {
        FrameSize {
            w: if self.w > 0 { self.w } else { fallback.w },
            h: if self.h > 0 { self.h } else { fallback.h },
        }
    }
}

pub(super) fn log_archetype<A: rerun::AsComponents>(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    archetype: &A,
    ctx: impl FnOnce() -> String,
) -> Result<()> {
    rec.log(entity_path, archetype).with_context(ctx)
}

pub(super) fn log_at<A: rerun::AsComponents>(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    timestamp_ns: i64,
    archetype: &A,
    ctx: impl FnOnce() -> String,
) -> Result<()> {
    rec.set_time_sequence("frame_ns", timestamp_ns);
    log_archetype(rec, entity_path, archetype, ctx)
}

pub(super) fn log_many<A: rerun::AsComponents>(
    rec: &rerun::RecordingStream,
    timestamp_ns: i64,
    items: impl IntoIterator<Item = (String, A)>,
    what: &str,
) {
    rec.set_time_sequence("frame_ns", timestamp_ns);
    for (path, archetype) in items {
        if let Err(err) = rec.log(path.as_str(), &archetype) {
            tracing::warn!(%err, target_path = %path, "{what} log failed");
        }
    }
}
