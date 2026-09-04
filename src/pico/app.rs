use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::pico::config::Config;
use crate::pico::infer::InferenceResult;
use crate::pico::ingest::{RawKeyframe, IngestEngine, Decoder};
use crate::pico::perception;
use crate::pico::scan::{ScanDeadline, ScanTimeline};
use crate::pico::viz::{self, VizBatch};
use crate::slot::Slot;

pub struct App {
    scan_timeline: ScanTimeline,
    scan_deadline: ScanDeadline,
    // Slots: the fundamental primitive for inter-stage communication.
    raw_slot: Arc<Slot<RawKeyframe>>,
    decode_slot: Arc<Slot<crate::pico::ingest::DecodedFrame>>,
    viz_slot: Arc<Slot<VizBatch>>,
    infer_slot: Arc<Slot<InferenceResult>>,
    // Threads and tasks.
    viz_thread: Option<std::thread::JoinHandle<()>>,
    perception_thread: Option<std::thread::JoinHandle<()>>,
    ingestion: tokio::task::JoinHandle<()>,
    // Decoder runs in the main thread (not Send).
    decoder: Decoder,
    // State.
    state: State,
    boot_instant: Instant,
}

struct State {
    scans: u64,
    frames_decoded: u64,
    frames_sent_viz: u64,
    frames_sent_perception: u64,
    infer_results: u64,
    last_report: Instant,
    report_interval: Duration,
}

impl App {
    pub async fn bootstrap(config: &Config) -> Result<Self, String> {
        let boot_instant = Instant::now();
        let period_ms = config.scan.period_ms;

        // Slots: the fundamental primitive for inter-stage communication.
        // ADR-034: if losing the old sample is correct, use Slot.
        let raw_slot = Arc::new(Slot::new());
        let decode_slot = Arc::new(Slot::new());
        let viz_slot = Arc::new(Slot::new());
        let infer_slot = Arc::new(Slot::new());

        // Viz bridge thread: owns the Rerun connection.
        let viz_thread = if config.viz.enabled {
            Some(viz::spawn(
                config.viz.rerun_addr.clone(),
                Arc::clone(&viz_slot),
            ))
        } else {
            None
        };

        // Perception thread: runs ONNX inference.
        let perception_thread = if !config.inference.model_path.is_empty() {
            Some(perception::spawn(
                config.inference.model_path.clone(),
                config.inference.model_name.clone(),
                config.inference.confidence,
                Arc::clone(&decode_slot),
                Arc::clone(&infer_slot),
            ))
        } else {
            None
        };

        // Ingest task: RTSP → raw keyframes → slot.
        let ingest_cfg = config.ingest.clone();
        let slot_clone = Arc::clone(&raw_slot);
        let url = config.source.url.clone();
        let ingestion = tokio::task::spawn(async move {
            if let Err(e) = ingest_loop(url, ingest_cfg, slot_clone).await {
                log::error!("ingest task failed: {e}");
            }
        });

        // Decoder runs in the main thread (not Send, so can't cross await).
        let decoder = Decoder::new()?;

        Ok(Self {
            scan_timeline: ScanTimeline::new(boot_instant, period_ms),
            scan_deadline: ScanDeadline::anchored_at(boot_instant, Duration::from_millis(period_ms)),
            raw_slot,
            decode_slot,
            viz_slot,
            infer_slot,
            viz_thread,
            perception_thread,
            ingestion,
            decoder,
            state: State {
                scans: 0,
                frames_decoded: 0,
                frames_sent_viz: 0,
                frames_sent_perception: 0,
                infer_results: 0,
                last_report: Instant::now(),
                report_interval: Duration::from_secs(5),
            },
            boot_instant,
        })
    }

    pub async fn run(&mut self, config: &Config) {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM");
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        let infer_info = if config.inference.model_path.is_empty() {
            "disabled".to_string()
        } else {
            format!("{} ({})", config.inference.model_name, config.inference.model_path)
        };

        log::info!(
            "mana-pico v{} starting (scan {}ms, viz {}, infer {})",
            env!("CARGO_PKG_VERSION"),
            config.scan.period_ms,
            if config.viz.enabled { &config.viz.rerun_addr } else { "disabled" },
            infer_info,
        );

        loop {
            tokio::select! {
                () = tokio::time::sleep_until(self.scan_deadline.next().into()) => {
                    let now = Instant::now();
                    let late = self.scan_deadline.arrive(now);
                    self.scan_tick(late);
                }
                _ = term.recv() => break,
                _ = &mut ctrl_c => break,
            }
        }

        self.shutdown();
    }

    fn scan_tick(&mut self, late: Duration) {
        self.state.scans += 1;

        // Take the freshest raw keyframe from the ingest slot.
        if let Some(raw) = self.raw_slot.take() {
            // Decode H.264 → RGB (CPU-bound, runs in scan tick).
            if let Some((rgb, w, h)) = self.decoder.decode(&raw.h264) {
                self.state.frames_decoded += 1;

                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as i64;

                let frame = crate::pico::ingest::DecodedFrame {
                    rgb: rgb.clone(),
                    width: w,
                    height: h,
                    frame_id: raw.frame_id,
                    timestamp_ns: ts,
                };

                // Forward to viz.
                self.viz_slot.put(VizBatch { frame });
                self.state.frames_sent_viz += 1;

                // Forward to perception (clone the RGB data).
                let frame_for_perception = crate::pico::ingest::DecodedFrame {
                    rgb,
                    width: w,
                    height: h,
                    frame_id: raw.frame_id,
                    timestamp_ns: ts,
                };
                self.decode_slot.put(frame_for_perception);
                self.state.frames_sent_perception += 1;
            }
        }

        // Check for inference results.
        while let Some(result) = self.infer_slot.take() {
            self.state.infer_results += 1;

            // Log detections.
            if !result.detections.is_empty() {
                let classes: Vec<String> = result
                    .detections
                    .iter()
                    .map(|d| format!("{}:{:.2}", d.class_name, d.confidence))
                    .collect();
                log::info!(
                    "infer: frame {} → {} detections ({:?}) [{}]",
                    result.frame_id,
                    result.detections.len(),
                    Duration::from_millis(result.infer_ms),
                    classes.join(", ")
                );
            }
        }

        // Advance the virtual clock.
        if self.state.scans > 1 {
            self.scan_timeline.advance();
        }

        // Report every N seconds.
        if self.state.last_report.elapsed() >= self.state.report_interval {
            self.report(late);
            self.state.last_report = Instant::now();
        }
    }

    fn report(&self, late: Duration) {
        let uptime = self.boot_instant.elapsed().as_secs();
        let raw_overwritten = self.raw_slot.drain_overwritten();
        let viz_overwritten = self.viz_slot.drain_overwritten();
        let infer_overwritten = self.infer_slot.drain_overwritten();

        log::info!(
            "cycle: {} scans in {}s | dline late {:?} | decoded={} viz={} infer={} | raw_pisados={} viz_pisados={} infer_pisados={}",
            self.state.scans,
            uptime,
            late,
            self.state.frames_decoded,
            self.state.frames_sent_viz,
            self.state.infer_results,
            raw_overwritten,
            viz_overwritten,
            infer_overwritten,
        );
    }

    fn shutdown(&mut self) {
        log::info!("mana-pico shutting down");

        // Close slots first: this wakes up threads waiting on take_blocking.
        self.raw_slot.close();
        self.decode_slot.close();
        self.viz_slot.close();
        self.infer_slot.close();

        // Abort the ingest task.
        self.ingestion.abort();

        // Wait for threads to finish.
        if let Some(handle) = self.perception_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.viz_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Ingest loop: connects to RTSP, extracts keyframes, publishes to slot.
async fn ingest_loop(
    url: String,
    cfg: crate::pico::config::IngestConfig,
    slot: Arc<Slot<RawKeyframe>>,
) -> Result<(), String> {
    let mut engine = IngestEngine::connect(&url, &cfg).await?;

    log::info!("ingest: connected, entering poll loop");

    loop {
        match engine.poll().await {
            Some(raw) => {
                slot.put(raw);
            }
            None => {
                // Timeout or deduplication, continue polling.
            }
        }
    }
}
