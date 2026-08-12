//! Frame, crop, and depth image logging for Rerun viz.

use mana_media::RawFrameV1;
use ultralytics_inference::visualizer::color::{Colormap, DepthViz};

use crate::depth_map::DepthFrame;
use crate::infer::CropFrameInfo;

use super::{DEPTH_OVERLAY_ALPHA, Inner, VizBridge, VizImageFormat, sanitize_entity_name};

pub(super) fn log_frame_rgb24(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    header: &RawFrameV1,
    rgb: &[u8],
) -> Result<(), String> {
    let w = header.width;
    let h = header.height;
    let expected = (w * h * 3) as usize;
    let data = if rgb.len() >= expected {
        rgb[..expected].to_vec()
    } else {
        log::warn!(
            "frame pixel data too short; padding with zeros (frame_id={}, got={}, expected={})",
            header.frame_id,
            rgb.len(),
            expected
        );
        let mut padded = vec![0u8; expected];
        padded[..rgb.len()].copy_from_slice(rgb);
        padded
    };
    log_frame_rgb24_owned(rec, entity_path, header, data)
}

pub(super) fn log_frame_rgb24_owned(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    header: &RawFrameV1,
    data: Vec<u8>,
) -> Result<(), String> {
    log_at(
        rec,
        entity_path,
        header.timestamp_ns,
        &rerun::Image::from_rgb24(data, [header.width, header.height]),
        || format!("rrd frame log failed (frame_id={})", header.frame_id),
    )
}

fn log_archetype<A: rerun::AsComponents>(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    archetype: &A,
    ctx: impl FnOnce() -> String,
) -> Result<(), String> {
    rec.log(entity_path, archetype)
        .map_err(|e| format!("{}: {e}", ctx()))
}

fn log_at<A: rerun::AsComponents>(
    rec: &rerun::RecordingStream,
    entity_path: &str,
    timestamp_ns: i64,
    archetype: &A,
    ctx: impl FnOnce() -> String,
) -> Result<(), String> {
    rec.set_timestamp_nanos_since_epoch("frame_time", timestamp_ns);
    log_archetype(rec, entity_path, archetype, ctx)
}

fn finite_depth_min(depth: &DepthFrame) -> Option<f32> {
    depth
        .iter_values()
        .filter(|value| value.is_finite() && *value > 0.0)
        .reduce(f32::min)
}

fn finite_depth_max(depth: &DepthFrame) -> Option<f32> {
    depth
        .iter_values()
        .filter(|value| value.is_finite() && *value > 0.0)
        .reduce(f32::max)
}

/// Encode JPEG de un buffer RGB24.
///
/// Es la palanca que baja el presupuesto del enlace **sin tocar la
/// resolución**: la imagen conserva sus dimensiones en píxeles, así que todo
/// overlay logueado en coordenadas nativas del frame sigue alineado sin
/// compensación.
pub(super) fn encode_jpeg(rgb: &[u8], w: u32, h: u32, quality: u8) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode(rgb, w, h, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

impl VizBridge {
    pub fn log_frame(&self, header: &RawFrameV1, rgb: &[u8]) {
        if !self.toggles.frames {
            return;
        }
        let path = "/world/camera/bgr";
        if let Inner::Connected { ref rec, .. } = self.inner {
            // A truncated buffer falls through to the raw path, which pads and
            // warns; encoding one would read past its end.
            let complete = rgb.len() >= (header.width * header.height * 3) as usize;

            let result = match self.image_format {
                VizImageFormat::Jpeg { quality } if complete => {
                    match encode_jpeg(rgb, header.width, header.height, quality) {
                        Ok(bytes) => log_at(
                            rec,
                            path,
                            header.timestamp_ns,
                            &rerun::EncodedImage::from_file_contents(bytes),
                            || format!("rrd jpeg frame log failed (frame_id={})", header.frame_id),
                        ),
                        Err(e) => {
                            log::warn!("viz: jpeg encode failed, sending raw: {e}");
                            log_frame_rgb24(rec, path, header, rgb)
                        }
                    }
                }
                _ => log_frame_rgb24(rec, path, header, rgb),
            };
            if let Err(e) = result {
                log::warn!("viz frame log failed: {e}");
            }
        }
    }

    pub fn log_crop_frame(&self, model: &str, header: &RawFrameV1, crop: CropFrameInfo) {
        if !self.toggles.crop_frames {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let path = format!("/world/camera/crops/{model}/bgr");
        if let Err(e) = log_frame_rgb24_owned(
            rec,
            &path,
            &RawFrameV1 {
                width: crop.w,
                height: crop.h,
                ..header.clone()
            },
            crop.rgb,
        ) {
            log::warn!("viz crop frame {model} failed: {e}");
        }
    }

    pub fn log_model_depth(&self, model: &str, depth: Option<&DepthFrame>) {
        if !self.toggles.depth && !self.toggles.depth_stats {
            return;
        }
        let rec = match &self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };
        let model = sanitize_entity_name(model);
        let base = format!("/world/camera/depth/{model}");
        let visual_path = format!("/world/camera/crops/{model}/depth");
        if self.toggles.depth {
            rec.log(base.as_str(), &rerun::Clear::recursive()).ok();
            rec.log(visual_path.as_str(), &rerun::Clear::recursive())
                .ok();
        }
        if let Some(depth) = depth {
            let (width, height) = depth.dims();
            if self.toggles.depth && width > 0 && height > 0 {
                let (viz, suffix) = match self.toggles.depth_viz.to_ascii_lowercase().as_str() {
                    "metric" => (DepthViz::Metric, "metric"),
                    "disparity" | "depthanything" => (DepthViz::Disparity, "disparity"),
                    invalid => {
                        log::warn!("viz: invalid depth_viz '{invalid}', using disparity");
                        (DepthViz::Disparity, "disparity")
                    }
                };
                let path = format!("{visual_path}/{suffix}");
                let colors = depth.colorize(Colormap::Inferno, viz);
                let rgba: Vec<u8> = depth
                    .iter_values()
                    .zip(colors.iter())
                    .flat_map(|(value, color)| {
                        let alpha = if value.is_finite() && value > 0.0 {
                            DEPTH_OVERLAY_ALPHA
                        } else {
                            0
                        };
                        [color[0], color[1], color[2], alpha]
                    })
                    .collect();
                let image = rerun::Image::from_rgba32(rgba, [width, height]);
                if let Err(e) = rec.log(path.as_str(), &image) {
                    log::warn!("viz depth {model} failed: {e}");
                }
            }

            if self.toggles.depth_stats {
                let valid_pixels = depth
                    .iter_values()
                    .filter(|&value| value.is_finite() && value > 0.0)
                    .count();
                self.log_scalar_inner(
                    rec,
                    &format!("{base}/stats/valid_pixels"),
                    valid_pixels as f64,
                );
                if let Some(min) = finite_depth_min(depth) {
                    self.log_scalar_inner(rec, &format!("{base}/stats/min_depth_m"), min as f64);
                }
                if let Some(max) = finite_depth_max(depth) {
                    self.log_scalar_inner(rec, &format!("{base}/stats/max_depth_m"), max as f64);
                }
            }
        } else if self.toggles.depth_stats {
            self.log_scalar_inner(rec, &format!("{base}/stats/valid_pixels"), 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gradient plus a high-frequency component, so JPEG cannot cheat the way
    /// it would on a flat field.
    fn synthetic_1080p() -> Vec<u8> {
        let (w, h) = (1920usize, 1080usize);
        let mut buf = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 3;
                buf[i] = (x % 256) as u8;
                buf[i + 1] = (y % 256) as u8;
                buf[i + 2] = ((x ^ y) % 256) as u8;
            }
        }
        buf
    }

    #[test]
    fn jpeg_preserves_dimensions_and_shrinks_payload() {
        let src = synthetic_1080p();
        let t = std::time::Instant::now();
        let bytes = encode_jpeg(&src, 1920, 1080, 75).expect("encode");
        let elapsed = t.elapsed();

        // The point of this path: the wire payload drops while the image keeps
        // native resolution, so overlays need no coordinate compensation.
        let decoded = image::load_from_memory(&bytes).expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (1920, 1080));
        assert!(
            bytes.len() * 4 < src.len(),
            "jpeg must be well under a quarter of raw: {} vs {}",
            bytes.len(),
            src.len()
        );
        eprintln!(
            "jpeg q75: {} -> {} bytes ({:.1}x) in {:?}",
            src.len(),
            bytes.len(),
            src.len() as f64 / bytes.len() as f64,
            elapsed
        );
    }

    #[test]
    fn unknown_image_format_falls_back_to_raw() {
        assert_eq!(VizImageFormat::parse("raw", 75), VizImageFormat::Raw);
        assert_eq!(
            VizImageFormat::parse("JPEG", 75),
            VizImageFormat::Jpeg { quality: 75 }
        );
        assert_eq!(VizImageFormat::parse("webp", 75), VizImageFormat::Raw);
        // Quality is clamped rather than trusted from the catalog.
        assert_eq!(
            VizImageFormat::parse("jpeg", 200),
            VizImageFormat::Jpeg { quality: 100 }
        );
    }

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
