//! mana-video/src/raw.rs — Raw Frame Reader (for replay/testing)
//! ==============================================================
//! Reads pre-decoded RGB frames from disk. Each frame is a .raw file
//! containing width * height * 3 bytes of RGB24 data.
//!
//! Scenario layout:
//!   test_data/scenarios/<name>/
//!   ├── scenario.toml      ← width, height, fps
//!   ├── frame_00000.raw    ← raw RGB bytes
//!   └── ...

use anyhow::{Context, Result};
use mana_types::frame::{PixelFormat, RawFrameV1};
use std::fs;
use std::path::PathBuf;

use super::{DecodedFrame, FrameDecoder, FrameDecoderError};

pub struct RawFrameReader {
    #[allow(dead_code)] // Read by serde deserialization; available for runtime introspection.
    name: String,
    width: u32,
    height: u32,
    fps: u32,
    dir: PathBuf,
    frame_count: u32,
    bytes_per_frame: usize,
    current_frame: u32,
    should_loop: bool,
}

impl RawFrameReader {
    pub fn new(scenario_dir: &str, should_loop: bool) -> Result<Self> {
        let dir = PathBuf::from(scenario_dir);
        let toml_path = dir.join("scenario.toml");

        let toml_str = fs::read_to_string(&toml_path)
            .with_context(|| format!("missing scenario.toml in {}", dir.display()))?;

        #[derive(serde::Deserialize)]
        struct Meta {
            name: String,
            width: u32,
            height: u32,
            fps: u32,
        }
        let meta: Meta = toml::from_str(&toml_str)?;

        let bytes_per_frame = meta.width as usize * meta.height as usize * 3;

        let frame_count = (0u32..)
            .take_while(|i| dir.join(format!("frame_{i:05}.raw")).exists())
            .count() as u32;

        if frame_count == 0 {
            anyhow::bail!("no .raw frames found in {}", dir.display());
        }

        Ok(Self {
            name: meta.name,
            width: meta.width,
            height: meta.height,
            fps: meta.fps,
            dir,
            frame_count,
            bytes_per_frame,
            current_frame: 0,
            should_loop,
        })
    }

    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }
}

impl FrameDecoder for RawFrameReader {
    async fn next_frame(&mut self) -> Result<DecodedFrame, FrameDecoderError> {
        if self.current_frame >= self.frame_count {
            if self.should_loop {
                self.current_frame = 0;
            } else {
                return Err(FrameDecoderError::EndOfStream);
            }
        }

        let filename = format!("frame_{:05}.raw", self.current_frame);
        let path = self.dir.join(&filename);
        let bytes = tokio::fs::read(&path).await?;

        if bytes.len() != self.bytes_per_frame {
            return Err(FrameDecoderError::Config(format!(
                "frame {f} size mismatch: {got} vs expected {exp} bytes",
                f = self.current_frame,
                got = bytes.len(),
                exp = self.bytes_per_frame
            )));
        }

        let header = RawFrameV1 {
            frame_id: self.current_frame as u64,
            timestamp_ns: (self.current_frame as i64 * 1_000_000_000 / self.fps as i64),
            width: self.width,
            height: self.height,
            pixel_format: PixelFormat::Rgb8 as u32,
            // Replay → stamp emit time so e2e measures the replay pipeline.
            capture_mono_ns: mana_types::now_mono_boot_ns(),
            ..Default::default()
        };

        self.current_frame += 1;
        Ok(DecodedFrame {
            header,
            pixels: bytes,
        })
    }

    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn pixel_format(&self) -> PixelFormat {
        PixelFormat::Rgb8
    }
    fn fps(&self) -> u32 {
        self.fps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_frame_reader_rejects_missing_dir() {
        let result = RawFrameReader::new("nonexistent_dir", false);
        assert!(result.is_err());
    }

    /// Locate the workspace root by walking up from this crate until we find the
    /// directory that holds `Cargo.lock` (only the workspace root does).
    ///
    /// This deliberately avoids a hardcoded `ancestors().nth(N)`: the crate has
    /// already moved once (`crates/pipeline/mana-video` → its current home), which
    /// silently broke a fixed depth. A marker search survives future moves.
    fn workspace_root() -> Option<PathBuf> {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .find(|p| p.join("Cargo.lock").exists())
            .map(|p| p.to_path_buf())
    }

    #[test]
    fn raw_frame_reader_finds_scenario_toml_in_test_data() {
        // Optional integration fixture: test_data/scenarios/bed_entry is produced
        // by extract_scenario.sh and is NOT committed. When present, validate that
        // the reader parses it; when absent (fresh checkout / CI without the
        // script) skip cleanly instead of asserting on a specific error string.
        let Some(root) = workspace_root() else {
            return; // can't locate the workspace root → nothing to validate
        };
        let scenario_path = root.join("test_data/scenarios/bed_entry");
        if !scenario_path.join("scenario.toml").exists() {
            return; // fixture not extracted — nothing to validate
        }

        let reader = RawFrameReader::new(&scenario_path.to_string_lossy(), false)
            .expect("scenario.toml is present but the reader failed to load it");
        assert_eq!(reader.width, 1920);
        assert_eq!(reader.height, 1080);
        assert_eq!(reader.fps, 6);
        assert!(
            reader.frame_count > 0,
            "frames should be present after extract_scenario.sh"
        );
    }
}
