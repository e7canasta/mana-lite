use anyhow::{Context, Result};

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
    rec.set_timestamp_nanos_since_epoch("frame_time", timestamp_ns);
    log_archetype(rec, entity_path, archetype, ctx)
}
