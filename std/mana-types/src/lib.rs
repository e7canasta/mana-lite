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

// ── RawFrameV1 ──

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

// ── DetectionV1 + DetectionBatchV1 ──

pub const MAX_DETECTIONS: usize = 128;

#[derive(Copy, Clone, Debug, Default)]
pub struct DetectionV1 {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub confidence: f32,
    pub class_id: u16,
}

#[derive(Clone, Debug)]
pub struct DetectionBatchV1 {
    pub frame_id: u64,
    pub timestamp_ns: i64,
    pub schema_version: u32,
    pub model_fingerprint: [u8; 32],
    pub count: u32,
    pub detections: [DetectionV1; MAX_DETECTIONS],
    pub source_id: u64,
}

impl Default for DetectionBatchV1 {
    fn default() -> Self {
        Self {
            frame_id: 0,
            timestamp_ns: 0,
            schema_version: 3,
            model_fingerprint: [0u8; 32],
            count: 0,
            detections: [DetectionV1::default(); MAX_DETECTIONS],
            source_id: 0,
        }
    }
}

impl DetectionBatchV1 {
    pub fn valid(&self) -> &[DetectionV1] {
        let count = self.count.min(MAX_DETECTIONS as u32) as usize;
        &self.detections[..count]
    }
}

// ── EntityRole ──

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum EntityRole {
    #[default]
    Furniture = 0,
    PersonActive = 1,
    PersonPoi = 2,
    PersonSuppressed = 3,
}

impl EntityRole {
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Furniture,
            1 => Self::PersonActive,
            2 => Self::PersonPoi,
            _ => Self::PersonSuppressed,
        }
    }
}

// ── ZoneV1 ──

pub const ZONE_NAME_LEN: usize = 32;
pub const MAX_ZONES: usize = 8;

#[derive(Copy, Clone, Debug)]
pub struct ZoneV1 {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub name: [u8; ZONE_NAME_LEN],
    pub zone_type: u8,
}

impl Default for ZoneV1 {
    fn default() -> Self {
        Self {
            cx: 0.0,
            cy: 0.0,
            w: 0.0,
            h: 0.0,
            name: [0; ZONE_NAME_LEN],
            zone_type: 0,
        }
    }
}

impl ZoneV1 {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name)
            .unwrap_or("?")
            .trim_end_matches('\0')
    }
}

// ── SceneEntityV1 ──

pub const MAX_SCENE_ENTITIES: usize = 16;

#[derive(Copy, Clone, Debug)]
pub struct SceneEntityV1 {
    pub track_id: u64,
    pub time_in_zone_secs: f32,
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub confidence: f32,
    pub vx: f32,
    pub vy: f32,
    pub age_frames: u32,
    pub missed_frames: u32,
    pub class_id: u16,
    pub role: u8,
    pub status: u8,
    pub zone_count: u8,
    pub zone_indices: [u8; 4],
    pub line_side: u8,
}

impl Default for SceneEntityV1 {
    fn default() -> Self {
        Self {
            track_id: 0,
            time_in_zone_secs: 0.0,
            cx: 0.0,
            cy: 0.0,
            w: 0.0,
            h: 0.0,
            confidence: 0.0,
            vx: 0.0,
            vy: 0.0,
            age_frames: 0,
            missed_frames: 0,
            class_id: 0,
            role: 0,
            status: 0,
            zone_count: 0,
            zone_indices: [0; 4],
            line_side: 0,
        }
    }
}

// ── SceneMsgV1 ──

#[derive(Clone, Debug)]
pub struct SceneMsgV1 {
    pub frame_id: u64,
    pub timestamp_ns: i64,
    pub source_id: [u8; 48],
    pub schema_version: u32,
    pub zone_count: u32,
    pub entity_count: u32,
    pub diag_flags: u16,
    pub dropped: u16,
    pub vision_state: u8,
    pub scene_mode: u8,
    pub person_count: u8,
    pub zones: [ZoneV1; MAX_ZONES],
    pub entities: [SceneEntityV1; MAX_SCENE_ENTITIES],
}

impl Default for SceneMsgV1 {
    fn default() -> Self {
        Self {
            frame_id: 0,
            timestamp_ns: 0,
            source_id: [0; 48],
            schema_version: 1,
            zone_count: 0,
            entity_count: 0,
            diag_flags: 0,
            dropped: 0,
            vision_state: 0,
            scene_mode: 0,
            person_count: 0,
            zones: [ZoneV1::default(); MAX_ZONES],
            entities: [SceneEntityV1::default(); MAX_SCENE_ENTITIES],
        }
    }
}

impl SceneMsgV1 {
    pub fn valid_entities(&self) -> &[SceneEntityV1] {
        let count = (self.entity_count as usize).min(MAX_SCENE_ENTITIES);
        &self.entities[..count]
    }

    pub fn valid_zones(&self) -> &[ZoneV1] {
        let count = (self.zone_count as usize).min(MAX_ZONES);
        &self.zones[..count]
    }
}

// ── RoiCommandV1 ──

#[derive(Debug, Copy, Clone, Default)]
pub struct RoiCommandV1 {
    pub sequence: u64,
    pub schema_version: u8,
    pub mode: u8,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub target_name: [u8; 32],
    pub base_width: f32,
}

impl RoiCommandV1 {
    pub const FULL: u8 = 0;
    pub const CENTER_SQUARE: u8 = 1;
    pub const RECT: u8 = 2;
    pub const BED: u8 = 3;

    pub fn target_name_str(&self) -> &str {
        let end = self.target_name.iter().position(|&b| b == 0).unwrap_or(self.target_name.len());
        core::str::from_utf8(&self.target_name[..end]).unwrap_or("<invalid>")
    }
}

// ── bbox helpers ──

pub mod bbox {
    #[inline]
    pub fn to_pixels(norm_x: f32, norm_y: f32, frame_w: u32, frame_h: u32) -> (f32, f32) {
        (norm_x * frame_w as f32, norm_y * frame_h as f32)
    }

    #[inline]
    pub fn box_halfsize_to_pixels(cx: f32, cy: f32, w: f32, h: f32, frame_w: u32, frame_h: u32) -> (f32, f32, f32, f32) {
        let fw = frame_w as f32;
        let fh = frame_h as f32;
        (cx * fw, cy * fh, w * fw / 2.0, h * fh / 2.0)
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
        assert_eq!(PixelFormat::Rgb8.frame_byte_size(3840, 2160), 3840 * 2160 * 3);
    }

    #[test]
    fn byte_size_nv12() {
        assert_eq!(PixelFormat::Nv12.frame_byte_size(1920, 1080), 1920 * 1080 * 3 / 2);
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

    #[test]
    fn detection_batch_valid() {
        let mut batch = DetectionBatchV1 { count: 3, ..Default::default() };
        assert_eq!(batch.valid().len(), 3);
    }

    #[test]
    fn detection_batch_valid_clamped() {
        let batch = DetectionBatchV1 { count: 999, ..Default::default() };
        assert_eq!(batch.valid().len(), MAX_DETECTIONS);
    }

    #[test]
    fn entity_role_from_u8() {
        assert_eq!(EntityRole::from_u8(0), EntityRole::Furniture);
        assert_eq!(EntityRole::from_u8(2), EntityRole::PersonPoi);
        assert_eq!(EntityRole::from_u8(255), EntityRole::PersonSuppressed);
    }

    #[test]
    fn zone_name_str() {
        let mut name = [0u8; 32];
        name[..7].copy_from_slice(b"cama_01");
        let z = ZoneV1 { name, ..Default::default() };
        assert_eq!(z.name_str(), "cama_01");
    }

    #[test]
    fn scene_msg_valid_entities_clamped() {
        let m = SceneMsgV1 { entity_count: 999, ..Default::default() };
        assert_eq!(m.valid_entities().len(), MAX_SCENE_ENTITIES);
    }

    #[test]
    fn scene_msg_valid_zones_clamped() {
        let m = SceneMsgV1 { zone_count: 999, ..Default::default() };
        assert_eq!(m.valid_zones().len(), MAX_ZONES);
    }

    #[test]
    fn bbox_halfsize_1080p() {
        let (cx, cy, hw, hh) = bbox::box_halfsize_to_pixels(0.5, 0.5, 0.2, 0.4, 1920, 1080);
        assert!((cx - 960.0).abs() < 1.0);
        assert!((cy - 540.0).abs() < 1.0);
        assert!((hw - 192.0).abs() < 1.0);
        assert!((hh - 216.0).abs() < 1.0);
    }

    #[test]
    fn roi_target_name() {
        let mut cmd = RoiCommandV1::default();
        cmd.target_name[..7].copy_from_slice(b"cama_01");
        assert_eq!(cmd.target_name_str(), "cama_01");
    }
}
