use std::fs;
use std::io::Write;
use std::path::PathBuf;

use ffmpeg_next::util::frame::Video;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::ExtendedColorType;
use image::ImageEncoder;
use mana_video::decoder::SoftwareDecoder;

pub struct FrameBuffer {
    pub w: u32,
    pub h: u32,
    pub rgb: Vec<u8>,
}

pub struct SnapshotSaver {
    dir: PathBuf,
    decoder: SoftwareDecoder,
    verbose: bool,
}

impl SnapshotSaver {
    pub fn new(dir: PathBuf, verbose: bool) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        ffmpeg_next::init().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let decoder = SoftwareDecoder::new(ffmpeg_next::codec::Id::H264)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(Self { dir, decoder, verbose })
    }

    pub fn capture(&mut self, h264_data: &[u8]) -> Option<FrameBuffer> {
        self.write_h264(h264_data);
        let fb = self.decode_rgb(h264_data)?;
        self.write_png(&fb.rgb, fb.w, fb.h);
        Some(fb)
    }

    fn write_h264(&self, data: &[u8]) {
        let tmp = self.dir.join(".latest_frame.h264.tmp");
        let dst = self.dir.join("latest_frame.h264");
        let _ = (|| -> std::io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(data)?;
            f.flush()?;
            fs::rename(&tmp, &dst)?;
            Ok(())
        })();
    }

    fn decode_rgb(&mut self, h264_data: &[u8]) -> Option<FrameBuffer> {
        let mut result = None;
        let ok = self.decoder.decode(h264_data, |frame| {
            let w = frame.width();
            let h = frame.height();
            let fmt = frame.format();

            let mut scaler = ffmpeg_next::software::scaling::Context::get(
                fmt, w, h,
                ffmpeg_next::format::Pixel::RGB24,
                w, h,
                ffmpeg_next::software::scaling::Flags::BILINEAR,
            ).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            let mut rgb = Video::empty();
            scaler.run(frame, &mut rgb)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            let data = rgb.data(0);
            let stride = rgb.stride(0);
            let tight = w as usize * 3;
            let mut packed = Vec::with_capacity(tight * h as usize);
            for row in 0..h as usize {
                packed.extend_from_slice(&data[row * stride..row * stride + tight]);
            }
            result = Some(FrameBuffer { w, h, rgb: packed });
            Ok(())
        });
        if let Err(e) = ok {
            if self.verbose {
                log::warn!("snapshot decode failed: {e}");
            }
        }
        result
    }

    fn write_png(&self, rgb_data: &[u8], w: u32, h: u32) {
        let tmp = self.dir.join(".latest_frame.png.tmp");
        let dst = self.dir.join("latest_frame.png");
        let _ = (|| -> std::io::Result<()> {
            let f = fs::File::create(&tmp)?;
            let encoder = PngEncoder::new_with_quality(f, CompressionType::Fast, FilterType::NoFilter);
            encoder.write_image(rgb_data, w, h, ExtendedColorType::Rgb8)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            fs::rename(&tmp, &dst)?;
            Ok(())
        })();
    }
}

impl Drop for SnapshotSaver {
    fn drop(&mut self) {
        if let Ok(exists) = fs::exists(&self.dir) {
            if exists {
                let _ = fs::remove_file(self.dir.join("latest_frame.h264"));
                let _ = fs::remove_file(self.dir.join("latest_frame.png"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_writes_and_overwrites() {
        let dir = std::env::temp_dir().join("mana_snapshot_test");
        let _ = fs::remove_dir_all(&dir);

        let saver = SnapshotSaver::new(dir.clone(), false).unwrap();

        saver.write_h264(&[1, 2, 3]);
        let data = fs::read(dir.join("latest_frame.h264")).unwrap();
        assert_eq!(data, vec![1, 2, 3]);

        saver.write_h264(&[4, 5, 6, 7]);
        let data = fs::read(dir.join("latest_frame.h264")).unwrap();
        assert_eq!(data, vec![4, 5, 6, 7]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_no_tmp_left_behind() {
        let dir = std::env::temp_dir().join("mana_snapshot_tmp_test");
        let _ = fs::remove_dir_all(&dir);

        let saver = SnapshotSaver::new(dir.clone(), false).unwrap();
        saver.write_h264(b"hello");

        assert!(!dir.join(".latest_frame.h264.tmp").exists());
        assert!(dir.join("latest_frame.h264").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_capture_returns_none_for_non_h264() {
        let dir = std::env::temp_dir().join("mana_snapshot_capture_test");
        let _ = fs::remove_dir_all(&dir);

        let mut saver = SnapshotSaver::new(dir.clone(), false).unwrap();
        let result = saver.capture(b"not valid h264");
        assert!(result.is_none(), "non-H264 data should not decode");
        assert!(dir.join("latest_frame.h264").exists(), "raw h264 is always saved");

        let _ = fs::remove_dir_all(&dir);
    }
}
