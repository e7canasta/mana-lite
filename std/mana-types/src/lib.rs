//! Media frame wire types shared by the binary and `mana-viz`.
//!
//! Iceoryx scene/detection V1 types were removed in Sprint 4; they live in
//! Full Mana OS. Only frame transport types remain here until `mana-media`
//! absorbs them.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum PixelFormat {
    #[default]
    Rgb8 = 0,
    Bgr8 = 1,
    Nv12 = 2,
    Yuyv = 3,
    Gray8 = 4,
    Rgba8 = 5,
    Bgra8 = 6,
}

impl PixelFormat {
    pub const fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Rgb8,
            1 => Self::Bgr8,
            2 => Self::Nv12,
            3 => Self::Yuyv,
            4 => Self::Gray8,
            5 => Self::Rgba8,
            6 => Self::Bgra8,
            _ => Self::Rgb8,
        }
    }

    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb8 | Self::Bgr8 => 3,
            Self::Rgba8 | Self::Bgra8 => 4,
            Self::Yuyv => 2,
            Self::Gray8 => 1,
            Self::Nv12 => 1,
        }
    }

    pub const fn frame_byte_size(self, width: u32, height: u32) -> usize {
        let w = width as usize;
        let h = height as usize;
        match self {
            Self::Nv12 => w * h + 2 * w.div_ceil(2) * h.div_ceil(2),
            _ => w * h * self.bytes_per_pixel(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct RawFrameV1 {
    pub frame_id: u64,
    pub timestamp_ns: i64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub schema_version: u32,
    pub capture_mono_ns: i64,
    pub stride_bytes: u32,
    pub source_id: u64,
}

impl Default for RawFrameV1 {
    fn default() -> Self {
        Self {
            frame_id: 0,
            timestamp_ns: 0,
            width: 0,
            height: 0,
            pixel_format: PixelFormat::Rgb8,
            schema_version: 4,
            capture_mono_ns: 0,
            stride_bytes: 0,
            source_id: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_format_discriminant() {
        assert_eq!(PixelFormat::Rgb8 as u32, 0);
        assert_eq!(PixelFormat::Bgr8 as u32, 1);
        assert_eq!(PixelFormat::Nv12 as u32, 2);
    }

    #[test]
    fn bytes_per_pixel_rgb8() {
        assert_eq!(PixelFormat::Rgb8.bytes_per_pixel(), 3);
    }

    #[test]
    fn byte_size_4k() {
        assert_eq!(
            PixelFormat::Rgb8.frame_byte_size(3840, 2160),
            3840 * 2160 * 3
        );
    }

    #[test]
    fn byte_size_nv12() {
        assert_eq!(
            PixelFormat::Nv12.frame_byte_size(1920, 1080),
            1920 * 1080 * 3 / 2
        );
    }

    #[test]
    fn unknown_pixel_format_falls_back() {
        assert_eq!(PixelFormat::from_u32(99), PixelFormat::Rgb8);
        assert_eq!(PixelFormat::from_u32(u32::MAX), PixelFormat::Rgb8);
    }

    #[test]
    fn roundtrip() {
        for pf in [PixelFormat::Rgb8, PixelFormat::Bgr8, PixelFormat::Nv12] {
            assert_eq!(PixelFormat::from_u32(pf.as_u32()), pf);
        }
    }
}
