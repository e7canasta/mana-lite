use mana_types::{DetectionBatchV1, RawFrameV1, RoiCommandV1, SceneMsgV1};
use mana_viz::logging::{self, util::FrameSize};

pub struct VizBridge {
    rec: rerun::RecordingStream,
    frame_w: u32,
    frame_h: u32,
}

impl VizBridge {
    pub fn new(rerun_addr: &str) -> crate::error::Result<Self> {
        let url = format!("rerun+http://{rerun_addr}/proxy");
        let rec = rerun::RecordingStreamBuilder::new("mana-lite")
            .batcher_config(rerun::log::ChunkBatcherConfig {
                max_bytes_in_flight: 32 * 1024 * 1024,
                ..Default::default()
            })
            .connect_grpc_opts(url)
            .map_err(|e| crate::error::ManaError::Viz(e.to_string()))?;
        Ok(Self { rec, frame_w: 0, frame_h: 0 })
    }

    pub fn tick(&mut self) {
        let _ = self.rec.flush_blocking();
    }

    pub fn log_frame(&mut self, header: &RawFrameV1, rgb: &[u8]) {
        self.frame_w = header.width;
        self.frame_h = header.height;
        if let Err(e) = logging::frame::log_frame_rgb24(&self.rec, "/world/camera/bgr", header, rgb) {
            log::warn!("viz frame log failed: {e}");
        }
    }

    pub fn log_detections(&self, batch: &DetectionBatchV1) {
        if let Err(e) = logging::boxes::log_detections_2d(
            &self.rec, "/world/camera/detections", batch, self.frame_size(),
        ) {
            log::warn!("viz detection log failed: {e}");
        }
    }

    pub fn log_zones(&self, msg: &SceneMsgV1) {
        logging::boxes::log_zones_2d(&self.rec, "/world/zones", msg);
    }

    pub fn log_roi(&self, cmd: &RoiCommandV1) {
        if let Err(e) = logging::boxes::log_roi_2d(&self.rec, "/world/camera/roi", cmd, self.frame_size()) {
            log::warn!("viz roi log failed: {e}");
        }
    }

    pub fn log_event_text(&self, timestamp_ns: i64, frame_id: u64, event_name: &str) {
        if let Err(e) = logging::event::log_scene_event_text(&self.rec, "/world/events", timestamp_ns, frame_id, event_name) {
            log::warn!("viz event log failed: {e}");
        }
    }

    fn frame_size(&self) -> FrameSize {
        FrameSize::new(self.frame_w, self.frame_h)
    }
}
