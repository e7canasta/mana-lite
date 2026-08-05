use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ffmpeg_next::util::frame::Video;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::ExtendedColorType;
use image::ImageEncoder;
use mana_video::decoder::SoftwareDecoder;

// ── Frame buffer ─────────────────────────────────────────────────

pub struct FrameBuffer {
    pub w: u32,
    pub h: u32,
    pub rgb: Vec<u8>,
}

// ── Decoder ──────────────────────────────────────────────────────

pub struct FrameDecoder {
    decoder: SoftwareDecoder,
    scaler: Option<(ffmpeg_next::software::scaling::Context, u32, u32, ffmpeg_next::format::Pixel)>,
}

impl FrameDecoder {
    pub fn new() -> std::io::Result<Self> {
        ffmpeg_next::init()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let decoder = SoftwareDecoder::new(ffmpeg_next::codec::Id::H264)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(Self { decoder, scaler: None })
    }

    pub fn decode(&mut self, h264: &[u8]) -> Option<FrameBuffer> {
        let mut result = None;
        let mut scaler = self.scaler.take();
        if let Err(e) = self.decoder.decode(h264, |frame| {
            result = Some(yuv_to_rgb24(frame, &mut scaler)?);
            Ok(())
        }) {
            log::warn!("h264 decode error: {e}");
        }
        self.scaler = scaler;
        if result.is_none() {
            log::info!("h264 decode: {} bytes → pending (EAGAIN)", h264.len());
        }
        result
    }

    pub fn decode_timed(&mut self, h264: &[u8]) -> (Option<FrameBuffer>, u64) {
        let t0 = std::time::Instant::now();
        let fb = self.decode(h264);
        (fb, t0.elapsed().as_micros() as u64)
    }
}

type CachedScaler = Option<(ffmpeg_next::software::scaling::Context, u32, u32, ffmpeg_next::format::Pixel)>;

fn yuv_to_rgb24(
    frame: &Video,
    cached: &mut CachedScaler,
) -> std::io::Result<FrameBuffer> {
    let w = frame.width();
    let h = frame.height();
    let fmt = frame.format();

    let needs_new = match cached {
        Some((_, sw, sh, sf)) => *sw != w || *sh != h || *sf != fmt,
        None => true,
    };

    if needs_new {
        let s = ffmpeg_next::software::scaling::Context::get(
            fmt, w, h,
            ffmpeg_next::format::Pixel::RGB24, w, h,
            ffmpeg_next::software::scaling::Flags::BILINEAR,
        )?;
        *cached = Some((s, w, h, fmt));
    }

    let entry = cached.as_mut().unwrap();
    let scaler = &mut entry.0;
    let mut rgb_frame = Video::empty();
    scaler
        .run(frame, &mut rgb_frame)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let data = rgb_frame.data(0);
    let stride = rgb_frame.stride(0);
    let row_bytes = w as usize * 3;
    let mut packed = Vec::with_capacity(row_bytes * h as usize);
    for y in 0..h as usize {
        packed.extend_from_slice(&data[y * stride..y * stride + row_bytes]);
    }
    Ok(FrameBuffer { w, h, rgb: packed })
}

// ── Snapshot saver ───────────────────────────────────────────────

pub struct SnapshotSaver {
    dir: Option<PathBuf>,
    verbose: bool,
}

impl SnapshotSaver {
    pub fn new(dir: Option<PathBuf>, verbose: bool) -> std::io::Result<Self> {
        if let Some(ref d) = dir {
            fs::create_dir_all(d)?;
        }
        Ok(Self { dir, verbose })
    }

    /// Persist the raw H.264 NAL and an RGB PNG for the current frame.
    /// When `fb` is `None` (decode failed) only the raw NAL is saved — useful
    /// for post-mortem inspection of frames that couldn't be decoded.
    /// No-op when no snapshot directory is configured.
    pub fn save(&self, h264: &[u8], fb: Option<&FrameBuffer>) {
        let Some(ref dir) = self.dir else { return };
        self.write_h264_to(dir, h264);
        if let Some(fb) = fb {
            Self::write_png_to(dir, &fb.rgb, fb.w, fb.h);
        }
        if self.verbose {
            log::info!(
                "snapshot: {}/latest_frame.h264{}",
                dir.display(),
                if fb.is_some() { " + .png" } else { " (no png — decode pending)" }
            );
        }
    }

    #[cfg(test)]
    pub fn write_h264(&self, data: &[u8]) {
        let Some(ref dir) = self.dir else { return };
        self.write_h264_to(dir, data);
    }

    fn write_h264_to(&self, dir: &Path, data: &[u8]) {
        atomic_write(dir, ".latest_frame.h264.tmp", "latest_frame.h264", data);
    }

    fn write_png_to(dir: &Path, rgb: &[u8], w: u32, h: u32) {
        let tmp = dir.join(".latest_frame.png.tmp");
        let dst = dir.join("latest_frame.png");
        if let Err(e) = (|| -> std::io::Result<()> {
            let f = fs::File::create(&tmp)?;
            PngEncoder::new_with_quality(f, CompressionType::Fast, FilterType::NoFilter)
                .write_image(rgb, w, h, ExtendedColorType::Rgb8)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            fs::rename(&tmp, &dst)?;
            Ok(())
        })() {
            log::warn!("snapshot PNG write failed: {e}");
        }
    }
}

fn atomic_write(dir: &Path, tmp_name: &str, dst_name: &str, data: &[u8]) {
    let tmp = dir.join(tmp_name);
    let dst = dir.join(dst_name);
    if let Err(e) = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.flush()?;
        fs::rename(&tmp, &dst)?;
        Ok(())
    })() {
        log::warn!("snapshot atomic_write failed: {e}");
    }
}

impl Drop for SnapshotSaver {
    fn drop(&mut self) {
        let Some(ref dir) = self.dir else { return };
        if fs::exists(dir).unwrap_or(false) {
            let _ = fs::remove_file(dir.join("latest_frame.h264"));
            let _ = fs::remove_file(dir.join("latest_frame.png"));
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_writes_and_overwrites() {
        let dir = std::env::temp_dir().join("mana_snapshot_write_test");
        let _ = fs::remove_dir_all(&dir);

        let saver = SnapshotSaver::new(Some(dir.clone()), false).unwrap();

        saver.write_h264(&[1, 2, 3]);
        assert_eq!(fs::read(dir.join("latest_frame.h264")).unwrap(), vec![1, 2, 3]);

        saver.write_h264(&[4, 5, 6, 7]);
        assert_eq!(fs::read(dir.join("latest_frame.h264")).unwrap(), vec![4, 5, 6, 7]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_no_tmp_left_behind() {
        let dir = std::env::temp_dir().join("mana_snapshot_tmp_test");
        let _ = fs::remove_dir_all(&dir);

        let saver = SnapshotSaver::new(Some(dir.clone()), false).unwrap();
        saver.write_h264(b"hello");

        assert!(!dir.join(".latest_frame.h264.tmp").exists());
        assert!(dir.join("latest_frame.h264").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_save_always_writes_h264() {
        let dir = std::env::temp_dir().join("mana_snapshot_h264_test");
        let _ = fs::remove_dir_all(&dir);

        let saver = SnapshotSaver::new(Some(dir.clone()), false).unwrap();
        // save with no decoded frame (decode pending scenario)
        saver.save(b"raw_h264_bytes", None);

        assert!(dir.join("latest_frame.h264").exists(), "h264 always saved");
        assert!(!dir.join("latest_frame.png").exists(), "png skipped without fb");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn frame_decoder_returns_none_for_invalid_h264() {
        let mut dec = FrameDecoder::new().unwrap();
        assert!(dec.decode(b"not valid h264").is_none());
    }

    #[test]
    fn snapshot_saver_without_dir_is_noop() {
        let saver = SnapshotSaver::new(None, false).unwrap();
        saver.write_h264(b"anything");
    }
}
