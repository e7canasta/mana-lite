//! mana-media — Media commons
//! ==========================
//! Frame transport types, video decoding contract, buffer pooling, and
//! H.264 Annex-B helpers. Absorbs former `mana-video`, `mana-rtsp`, and
//! the live frame types from `mana-types`.
//!
//! Tier T1/T0: no dependency on `mana-control`, `mana-perception`, or
//! `mana-viz`.

#![forbid(unsafe_code)]

pub mod buffer_pool;
pub mod frame;
pub mod h264;

#[cfg(feature = "ffmpeg")]
pub mod decoder;
#[cfg(feature = "ffmpeg")]
pub mod format;

pub use frame::{PixelFormat, RawFrameV1};

use std::fmt;
use std::io;

// ── Concrete Error Type ─────────────────────────────────────

/// Errors that can occur during frame decoding.
///
/// Library traits use typed errors, not anyhow.
/// Implementors convert their internal errors to these variants.
#[derive(Debug)]
pub enum FrameDecoderError {
    /// Filesystem error (missing file, permission denied, etc.)
    Io(io::Error),
    /// Configuration or state error (invalid TOML, frame size mismatch, etc.)
    Config(String),
    /// RTSP network, protocol, or decode error (connection refused, DESCRIBE timeout, etc.)
    Rtsp(String),
    /// Source has been exhausted (end of file, stream ended).
    /// Non-looping sources return this instead of Ok(None).
    EndOfStream,
}

impl fmt::Display for FrameDecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Config(msg) => write!(f, "Config error: {msg}"),
            Self::Rtsp(msg) => write!(f, "RTSP error: {msg}"),
            Self::EndOfStream => write!(f, "end of stream"),
        }
    }
}

impl std::error::Error for FrameDecoderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Config(_) | Self::Rtsp(_) | Self::EndOfStream => None,
        }
    }
}

impl From<io::Error> for FrameDecoderError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl FrameDecoderError {
    /// Convenience: returns `true` for `EndOfStream`.
    /// Equivalent to `matches!(err, FrameDecoderError::EndOfStream)`.
    pub fn is_end_of_stream(&self) -> bool {
        matches!(self, Self::EndOfStream)
    }
}

impl Clone for FrameDecoderError {
    fn clone(&self) -> Self {
        match self {
            Self::Io(e) => Self::Io(io::Error::new(e.kind(), e.to_string())),
            Self::Config(msg) => Self::Config(msg.clone()),
            Self::Rtsp(msg) => Self::Rtsp(msg.clone()),
            Self::EndOfStream => Self::EndOfStream,
        }
    }
}

// ── Decoded Frame ───────────────────────────────────────────

/// A decoded frame: header metadata + raw pixel bytes.
///
/// The header carries width, height, pixel_format so consumers
/// know how to interpret the pixel bytes without side channels.
/// Timestamp and frame_id are source-specific.
///
/// Named struct (not tuple) prevents ordering bugs:
/// `let DecodedFrame { header, pixels } = frame;`
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub header: RawFrameV1,
    pub pixels: Vec<u8>,
}

// ── FrameDecoder Trait ──────────────────────────────────────

/// Common trait for all video frame sources.
///
/// Each call to next_frame() returns the next decoded frame
/// as a DecodedFrame (header metadata + raw RGB pixel bytes).
///
/// Returns `Err(FrameDecoderError::EndOfStream)` when source is exhausted
/// (end of file) or permanently disconnected (RTSP stream ended).
///
/// Returns `Future + Send` so trait objects can be used across
/// tokio tasks without additional bounds.
pub trait FrameDecoder: Send {
    /// Get the next decoded frame.
    fn next_frame(
        &mut self,
    ) -> impl std::future::Future<Output = Result<DecodedFrame, FrameDecoderError>> + Send;

    /// Source width in pixels.
    fn width(&self) -> u32;
    /// Source height in pixels.
    fn height(&self) -> u32;
    /// Pixel format of the output bytes.
    fn pixel_format(&self) -> PixelFormat;
    /// Expected frames per second (0 if unknown/variable).
    fn fps(&self) -> u32;
}
