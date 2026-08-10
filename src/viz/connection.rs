//! Connection lifecycle and Rerun blueprint setup.

use std::time::{Duration, Instant};

use super::{INITIAL_BACKOFF_MS, Inner, MAX_BACKOFF_MS, VizBridge};

impl VizBridge {
    pub(super) fn try_connect(&mut self) {
        let url = format!("rerun+http://{}/proxy", self.addr);
        let rec = rerun::RecordingStreamBuilder::new("mana-lite")
            .batcher_config(rerun::log::ChunkBatcherConfig {
                max_bytes_in_flight: 32 * 1024 * 1024,
                ..Default::default()
            })
            .connect_grpc_opts(url);
        match rec {
            Ok(rec) => {
                // Keep the SDK wall-clock timeline available alongside the frame timelines.
                rec.set_log_time_enabled(true);
                Self::send_default_blueprint(&rec);
                Self::send_presence_state_configuration(&rec);
                Self::send_fixed_rois(&rec, &self.fixed_rois);
                self.last_occupancy_state = None;
                self.last_second_person_state = None;
                self.last_signal_state = None;
                self.last_face_state = None;
                log::info!("viz: connected to {}", self.addr);
                self.inner = Inner::Connected {
                    rec,
                    last_flush_warn: Instant::now(),
                };
            }
            Err(e) => {
                if let Inner::Disconnected {
                    ref mut backoff_ms,
                    ref mut last_warn,
                    ..
                } = self.inner
                {
                    if last_warn.elapsed().as_secs() >= 30 {
                        log::warn!(
                            "viz: connect failed (retry in {}s): {e}",
                            *backoff_ms / 1000
                        );
                        *last_warn = Instant::now();
                    }
                    *backoff_ms = (*backoff_ms * 2).min(MAX_BACKOFF_MS);
                }
            }
        }
    }

    fn camera_blueprint_tab() -> rerun::blueprint::Horizontal {
        use rerun::blueprint::{ContainerLike, Horizontal, Spatial2DView, Vertical};

        let main_camera = Spatial2DView::new("Main frame")
            .with_origin("/world/camera")
            .with_contents([
                "+ /world/camera/bgr",
                "+ /world/camera/entities/**",
                "+ /world/camera/observations/**",
                "+ /world/camera/detections/**",
                "+ /world/camera/rois/**",
            ]);
        let face_crop = Spatial2DView::new("Face crop")
            .with_origin("/world/camera/crops/face-yolo")
            .with_contents(["+ $origin/**"]);
        let segmentation_crop = Spatial2DView::new("Mask + border")
            .with_origin("/world/camera/crops/seg-standard")
            .with_contents(["+ $origin/**"]);
        let depth_crop = Spatial2DView::new("Depth disparity")
            .with_origin("/world/camera/crops/depth-standard/depth")
            .with_contents([
                "+ $origin/disparity",
                "+ $origin/context/seg-standard/polygon/**",
                "+ $origin/context/detect-fast/**",
                "+ $origin/context/face-yolo/**",
            ]);
        let camera_details = Vertical::new(vec![
            face_crop.into(),
            segmentation_crop.into(),
            depth_crop.into(),
        ])
        .with_row_shares([1.0, 1.0, 1.0]);
        Horizontal::new(vec![
            ContainerLike::from(main_camera),
            ContainerLike::from(camera_details),
        ])
        .with_column_shares([3.0, 2.0])
    }

    fn metrics_blueprint_tab() -> rerun::blueprint::Vertical {
        use rerun::blueprint::{StateTimelineView, TimeSeriesView, Vertical};

        let stream_view = TimeSeriesView::new("Stream")
            .with_origin("/ingest")
            .with_contents(["+ $origin/**"]);
        let inference_view = TimeSeriesView::new("Inference")
            .with_origin("/pipeline")
            .with_contents(["+ $origin/**"]);
        let depth_stats = TimeSeriesView::new("Depth stats")
            .with_origin("/world/camera/depth")
            .with_contents(["+ $origin/**"]);
        let class_stats = TimeSeriesView::new("Class stats")
            .with_origin("/infer")
            .with_contents(["+ $origin/**"]);
        let room_state = StateTimelineView::new("Room state")
            .with_origin("/pipeline/state/room")
            .with_contents(["+ $origin/**"]);
        let face_state = StateTimelineView::new("Face state")
            .with_origin("/pipeline/state/face")
            .with_contents(["+ $origin"]);
        Vertical::new(vec![
            stream_view.into(),
            inference_view.into(),
            depth_stats.into(),
            class_stats.into(),
            room_state.into(),
            face_state.into(),
        ])
        .with_row_shares([1.0, 1.0, 1.0, 1.0, 1.5, 1.5])
    }

    pub(super) fn send_default_blueprint(rec: &rerun::RecordingStream) {
        use rerun::blueprint::components::PanelState;
        use rerun::blueprint::{ContainerLike, Tabs};

        let viewport = Tabs::new(vec![
            ContainerLike::from(Self::camera_blueprint_tab()),
            ContainerLike::from(Self::metrics_blueprint_tab()),
        ]);

        let blueprint = rerun::blueprint::Blueprint::new(viewport)
            .with_blueprint_panel(
                rerun::blueprint::BlueprintPanel::new().with_state(PanelState::Expanded),
            )
            .with_selection_panel(
                rerun::blueprint::SelectionPanel::new().with_state(PanelState::Expanded),
            )
            .with_time_panel(rerun::blueprint::TimePanel::new().with_state(PanelState::Expanded))
            .with_auto_views(true);

        if let Err(e) = blueprint.send(rec, Default::default()) {
            log::warn!("viz blueprint send failed: {e}");
        }
    }

    pub(super) fn send_presence_state_configuration(rec: &rerun::RecordingStream) {
        let configs = [
            (
                "/pipeline/state/room/cardinality",
                rerun::StateConfiguration::new()
                    .with_values(["empty", "single", "multiple"])
                    .with_labels(["Empty", "Single person", "Multiple people"])
                    .with_colors([0x607D8BFF, 0x4CAF50FF, 0xFF9800FF]),
            ),
            (
                "/pipeline/state/room/second_person",
                rerun::StateConfiguration::new()
                    .with_values(["none", "candidate", "confirmed"])
                    .with_labels(["No second person", "Second candidate", "Second confirmed"])
                    .with_colors([0x607D8BFF, 0xFFEB3BFF, 0xF44336FF]),
            ),
            (
                "/pipeline/state/room/signal",
                rerun::StateConfiguration::new()
                    .with_values(["valid", "invalid"])
                    .with_labels(["Valid inference", "Invalid inference"])
                    .with_colors([0x4CAF50FF, 0xF44336FF]),
            ),
            (
                "/pipeline/state/face",
                rerun::StateConfiguration::new()
                    .with_values([
                        "idle",
                        "searching",
                        "detected",
                        "other",
                        "in_bed",
                        "edge",
                        "exiting",
                    ])
                    .with_labels([
                        "Idle",
                        "Buscando cara",
                        "Cara detectada",
                        "Otra posición",
                        "En cama",
                        "En borde",
                        "Saliendo",
                    ])
                    .with_colors([
                        0x607D8BFF, 0x2196F3FF, 0x4CAF50FF, 0x9E9E9EFF, 0x9C27B0FF, 0xFF9800FF,
                        0xF44336FF,
                    ]),
            ),
        ];
        for (path, config) in configs {
            if let Err(e) = rec.log_static(path, &config) {
                log::warn!("viz presence state configuration {path} failed: {e}");
            }
        }
    }

    pub fn tick(&mut self) {
        match &mut self.inner {
            Inner::Connected {
                rec,
                last_flush_warn,
            } => {
                if let Err(e) = rec.flush_with_timeout(Duration::from_millis(100)) {
                    if last_flush_warn.elapsed().as_secs() >= 30 {
                        log::warn!("viz disconnected — viewer may be offline: {e}");
                        *last_flush_warn = Instant::now();
                    }
                    self.inner = Inner::Disconnected {
                        next_retry: Instant::now() + Duration::from_millis(INITIAL_BACKOFF_MS),
                        backoff_ms: INITIAL_BACKOFF_MS * 2,
                        last_warn: Instant::now(),
                    };
                }
            }
            Inner::Disconnected { next_retry, .. } => {
                if Instant::now() >= *next_retry {
                    self.try_connect();
                }
            }
            Inner::Disabled => {}
        }
    }
}
