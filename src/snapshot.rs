use std::fs;
use std::io::Write;
use std::path::PathBuf;

use ffmpeg_next::util::frame::Video;
use mana_video::decoder::SoftwareDecoder;

pub struct SnapshotSaver {
    dir: PathBuf,
    decoder: SoftwareDecoder,
    scaler: Option<ffmpeg_next::software::scaling::Context>,
    dec_width: u32,
    dec_height: u32,
}

impl SnapshotSaver {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        ffmpeg_next::init().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let decoder = SoftwareDecoder::new(ffmpeg_next::codec::Id::H264)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(Self {
            dir,
            decoder,
            scaler: None,
            dec_width: 0,
            dec_height: 0,
        })
    }

    pub fn save(&mut self, h264_data: &[u8]) -> std::io::Result<()> {
        let final_path = self.dir.join("latest_frame.h264");
        let tmp_path = self.dir.join(".latest_frame.h264.tmp");
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(h264_data)?;
        f.flush()?;
        fs::rename(&tmp_path, &final_path)?;

        let _ = self.decoder.decode(h264_data, |frame| {
            let w = frame.width();
            let h = frame.height();
            let fmt = frame.format();

            if self.scaler.is_none() || self.dec_width != w || self.dec_height != h {
                self.dec_width = w;
                self.dec_height = h;
                self.scaler = Some(
                    ffmpeg_next::software::scaling::Context::get(
                        fmt, w, h,
                        ffmpeg_next::format::Pixel::RGB24,
                        w, h,
                        ffmpeg_next::software::scaling::Flags::BILINEAR,
                    )
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?,
                );
            }

            let mut rgb = Video::empty();
            self.scaler.as_mut().unwrap().run(frame, &mut rgb)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            let data = rgb.data(0);
            let stride = rgb.stride(0);
            let ppm = self.dir.join(".latest_frame.ppm.tmp");
            let mut f = fs::File::create(&ppm)?;
            write!(f, "P6\n{w} {h}\n255\n")?;
            for row in 0..h as usize {
                f.write_all(&data[row * stride..row * stride + w as usize * 3])?;
            }
            f.flush()?;
            drop(f);
            fs::rename(&ppm, self.dir.join("latest_frame.ppm"))?;

            Ok(())
        });

        Ok(())
    }
}

impl Drop for SnapshotSaver {
    fn drop(&mut self) {
        if let Ok(exists) = fs::exists(&self.dir) {
            if exists {
                let _ = fs::remove_file(self.dir.join("latest_frame.h264"));
                let _ = fs::remove_file(self.dir.join("latest_frame.ppm"));
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

        let mut saver = SnapshotSaver::new(dir.clone()).unwrap();

        saver.save(&[1, 2, 3]).unwrap();
        let data = fs::read(dir.join("latest_frame.h264")).unwrap();
        assert_eq!(data, vec![1, 2, 3]);

        saver.save(&[4, 5, 6, 7]).unwrap();
        let data = fs::read(dir.join("latest_frame.h264")).unwrap();
        assert_eq!(data, vec![4, 5, 6, 7]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_no_tmp_left_behind() {
        let dir = std::env::temp_dir().join("mana_snapshot_tmp_test");
        let _ = fs::remove_dir_all(&dir);

        let mut saver = SnapshotSaver::new(dir.clone()).unwrap();
        saver.save(b"hello").unwrap();

        assert!(!dir.join(".latest_frame.h264.tmp").exists());
        assert!(dir.join("latest_frame.h264").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
