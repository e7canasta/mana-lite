use anyhow::Result;

use super::util::log_at;

pub fn log_scene_event_text(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    timestamp_ns: i64,
    frame_id: u64,
    event_name: &str,
) -> Result<()> {
    log_at(
        rec,
        entity_path,
        timestamp_ns,
        &rerun::TextLog::new(format!("[{event_name}] frame_id={frame_id}")),
        || format!("rrd event log failed ({event_name})"),
    )
}
