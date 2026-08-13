//! Connection lifecycle and Rerun blueprint setup.

use std::time::{Duration, Instant};

use super::{
    FLUSH_TIMEOUT_MS, INITIAL_BACKOFF_MS, Inner, MAX_BACKOFF_MS, MAX_FLUSH_TIMEOUTS, VizBridge,
};

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
                // Deliberately not logged as "connected": `connect_grpc_opts`
                // is lazy and returns `Ok` even with no viewer listening, so
                // announcing a connection here would be a claim we have not
                // verified. The info-level line is emitted by `tick` once a
                // flush proves a viewer is on the other end.
                log::debug!("viz: sink created for {}", self.addr);
                self.stream_proven = false;
                self.inner = Inner::Connected {
                    rec,
                    last_flush_warn: Instant::now(),
                    flush_timeouts: 0,
                };
            }
            Err(e) => {
                if let Inner::Disconnected {
                    ref mut last_warn, ..
                } = self.inner
                {
                    if last_warn.elapsed().as_secs() >= 30 {
                        log::warn!(
                            "viz: sink creation failed (retry in {}s): {e}",
                            self.retry_backoff_ms / 1000
                        );
                        *last_warn = Instant::now();
                    }
                }
            }
        }
    }

    /// Drop the current sink and schedule a retry with an escalating delay.
    fn disconnect(&mut self, reason: &str) {
        let delay = self.retry_backoff_ms;
        self.retry_backoff_ms = self.retry_backoff_ms.saturating_mul(2).min(MAX_BACKOFF_MS);
        if self.stream_proven {
            log::warn!("viz: {reason} — retrying in {}ms", delay);
        } else {
            log::debug!("viz: {reason} — retrying in {}ms", delay);
        }
        self.stream_proven = false;
        self.inner = Inner::Disconnected {
            next_retry: Instant::now() + Duration::from_millis(delay),
            last_warn: Instant::now(),
        };
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
                "+ /world/camera/body_parts/**",
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
        let mut drop_reason: Option<String> = None;
        let mut reconnect = false;

        match &mut self.inner {
            Inner::Connected {
                rec,
                last_flush_warn,
                flush_timeouts,
            } => match rec.flush_with_timeout(Duration::from_millis(FLUSH_TIMEOUT_MS)) {
                Ok(()) => {
                    *flush_timeouts = 0;
                    // A completed flush is the only evidence that a viewer is
                    // actually consuming the stream.
                    if !self.stream_proven {
                        self.stream_proven = true;
                        log::info!("viz: connected to {}", self.addr);
                    }
                    self.retry_backoff_ms = INITIAL_BACKOFF_MS;
                }
                // Backpressure, not a disconnect. A 1080p frame can take longer
                // than the probe window to drain over the network; treating that
                // as a drop would resend the viewer blueprint and reset the
                // state-dedup caches on every slow frame.
                Err(rerun::sink::SinkFlushError::Timeout) => {
                    *flush_timeouts += 1;
                    if *flush_timeouts >= MAX_FLUSH_TIMEOUTS {
                        drop_reason = Some(format!(
                            "sink backlogged ({MAX_FLUSH_TIMEOUTS} consecutive flush timeouts)"
                        ));
                    } else if last_flush_warn.elapsed().as_secs() >= 30 {
                        log::debug!("viz: flush timed out ({} in a row)", *flush_timeouts);
                        *last_flush_warn = Instant::now();
                    }
                }
                Err(e) => drop_reason = Some(format!("viewer unreachable: {e}")),
            },
            Inner::Disconnected { next_retry, .. } => {
                if Instant::now() >= *next_retry {
                    reconnect = true;
                }
            }
            Inner::Disabled => {}
        }

        if let Some(reason) = drop_reason {
            self.disconnect(&reason);
        } else if reconnect {
            self.try_connect();
        }
    }
}
