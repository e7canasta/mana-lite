#![allow(dead_code)] // API surface for downstream phases
use mana_types::{DetectionBatchV1, RawFrameV1, RoiCommandV1, SceneMsgV1};
use mana_viz::logging::{self, util::FrameSize};

pub struct VizBridge {
    rec: rerun::RecordingStream,
    frame_w: u32,
    frame_h: u32,
}

impl VizBridge {
    pub fn new(rerun_addr: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("rerun+http://{rerun_addr}/proxy");
        let rec = rerun::RecordingStreamBuilder::new("mana-lite")
            .batcher_config(rerun::log::ChunkBatcherConfig {
                max_bytes_in_flight: 32 * 1024 * 1024,
                ..Default::default()
            })
            .connect_grpc_opts(url)?;
        Ok(Self { rec, frame_w: 0, frame_h: 0 })
    }

    pub fn tick(&mut self) {
        let _ = self.rec.flush_blocking();
    }

    pub fn log_frame(&mut self, header: &RawFrameV1, rgb: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.frame_w = header.width;
        self.frame_h = header.height;
        Ok(logging::frame::log_frame_rgb24(&self.rec, "/world/camera/bgr", header, rgb)?)
    }

    pub fn log_detections(&self, batch: &DetectionBatchV1) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(logging::boxes::log_detections_2d(
            &self.rec,
            "/world/camera/detections",
            batch,
            self.frame_size(),
        )?)
    }

    pub fn log_zones(&self, msg: &SceneMsgV1) {
        logging::boxes::log_zones_2d(&self.rec, "/world/zones", msg);
    }

    pub fn log_roi(&self, cmd: &RoiCommandV1) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(logging::boxes::log_roi_2d(&self.rec, "/world/camera/roi", cmd, self.frame_size())?)
    }

    pub fn log_event_text(&self, timestamp_ns: i64, frame_id: u64, event_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(logging::event::log_scene_event_text(&self.rec, "/world/events", timestamp_ns, frame_id, event_name)?)
    }

    pub fn log_text(&self, entity_path: &str, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(logging::text::log_static_text(&self.rec, entity_path, text)?)
    }

    fn frame_size(&self) -> FrameSize {
        FrameSize::new(self.frame_w, self.frame_h)
    }
}
