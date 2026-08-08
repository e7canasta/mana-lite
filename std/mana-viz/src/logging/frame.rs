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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_and_crop_images_have_frame_time_and_log_time() {
        let (rec, storage) = rerun::RecordingStreamBuilder::new("mana-frame-test")
            .batcher_config(rerun::log::ChunkBatcherConfig::NEVER)
            .memory()
            .expect("memory recording");
        rec.set_log_time_enabled(true);
        let header = RawFrameV1 {
            frame_id: 7,
            timestamp_ns: 1_000,
            width: 1,
            height: 1,
            ..Default::default()
        };

        log_frame_rgb24_owned(&rec, "/world/camera/bgr", &header, vec![0, 0, 0])
            .expect("BGR image");
        log_frame_rgb24_owned(
            &rec,
            "/world/camera/crops/detect-fast/bgr",
            &header,
            vec![0, 0, 0],
        )
        .expect("crop image");

        let image_chunks = storage
            .take()
            .into_iter()
            .filter_map(|msg| match msg {
                rerun::log::LogMsg::ArrowMsg(_, msg) => {
                    Some(rerun::log::Chunk::from_arrow_msg(&msg).expect("valid chunk"))
                }
                _ => None,
            })
            .filter(|chunk| {
                matches!(
                    chunk.entity_path().to_string().as_str(),
                    "/world/camera/bgr" | "/world/camera/crops/detect-fast/bgr"
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(image_chunks.len(), 2);
        for chunk in image_chunks {
            let timelines = chunk.timelines();
            assert_eq!(
                timelines
                    .get(&rerun::TimelineName::from("frame_time"))
                    .expect("frame timestamp timeline")
                    .timeline()
                    .typ(),
                rerun::external::re_log_types::TimeType::TimestampNs
            );
            assert!(timelines.contains_key(&rerun::TimelineName::log_time()));
        }
    }
}
