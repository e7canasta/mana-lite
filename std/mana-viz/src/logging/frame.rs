use anyhow::Result;
use mana_types::RawFrameV1;

use super::util::log_at;

pub fn log_frame_rgb24(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    header: &RawFrameV1,
    rgb: &[u8],
) -> Result<()> {
    let w = header.width;
    let h = header.height;
    let expected = (w * h * 3) as usize;
    let data = if rgb.len() >= expected {
        rgb[..expected].to_vec()
    } else {
        tracing::warn!(
            frame_id = header.frame_id,
            got = rgb.len(),
            expected,
            "frame pixel data too short; padding with zeros"
        );
        let mut padded = vec![0u8; expected];
        padded[..rgb.len()].copy_from_slice(rgb);
        padded
    };
    log_frame_rgb24_owned(rec, entity_path, header, data)
}

pub fn log_frame_rgb24_owned(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    header: &RawFrameV1,
    data: Vec<u8>,
) -> Result<()> {
    log_at(
        rec,
        entity_path,
        header.timestamp_ns,
        &rerun::Image::from_rgb24(data, [header.width, header.height]),
        || format!("rrd frame log failed (frame_id={})", header.frame_id),
    )
}
