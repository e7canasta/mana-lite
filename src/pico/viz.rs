use std::time::{Duration, Instant};

use crate::slot::Slot;
use crate::pico::ingest::DecodedFrame;

const INITIAL_BACKOFF_MS: u64 = 1_000;
const MAX_BACKOFF_MS: u64 = 30_000;
const FLUSH_TIMEOUT_MS: u64 = 100;

/// Batch of frames to send to Rerun. One keyframe worth of data.
pub struct VizBatch {
    pub frame: DecodedFrame,
}

enum Inner {
    Connected {
        rec: rerun::RecordingStream,
        last_flush_warn: Instant,
        flush_timeouts: u32,
    },
    Disconnected {
        next_retry: Instant,
        last_warn: Instant,
    },
}

pub struct VizBridge {
    inner: Inner,
    addr: String,
    retry_backoff_ms: u64,
    stream_proven: bool,
}

impl VizBridge {
    pub fn new(addr: &str) -> Self {
        Self {
            inner: Inner::Disconnected {
                next_retry: Instant::now(),
                last_warn: Instant::now(),
            },
            addr: addr.to_string(),
            retry_backoff_ms: INITIAL_BACKOFF_MS,
            stream_proven: false,
        }
    }

    fn try_connect(&mut self) {
        let url = format!("rerun+http://{}/proxy", self.addr);
        match rerun::RecordingStreamBuilder::new("mana-pico")
            .batcher_config(rerun::log::ChunkBatcherConfig {
                max_bytes_in_flight: 32 * 1024 * 1024,
                ..Default::default()
            })
            .connect_grpc_opts(url)
        {
            Ok(rec) => {
                rec.set_log_time_enabled(true);
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
                        log::warn!("viz: sink creation failed: {e}");
                        *last_warn = Instant::now();
                    }
                }
            }
        }
    }

    fn disconnect(&mut self, reason: &str) {
        log::debug!("viz: disconnected ({reason})");
        self.inner = Inner::Disconnected {
            next_retry: Instant::now() + Duration::from_millis(self.retry_backoff_ms),
            last_warn: Instant::now(),
        };
        self.retry_backoff_ms = (self.retry_backoff_ms * 2).min(MAX_BACKOFF_MS);
        self.stream_proven = false;
    }

    /// Send a frame to Rerun. Non-blocking: drops the frame if viewer is slow.
    pub fn send_frame(&mut self, frame: &DecodedFrame) {
        // Lazy connect.
        if let Inner::Disconnected { next_retry, .. } = &self.inner {
            if Instant::now() >= *next_retry {
                self.try_connect();
            }
            return;
        }

        let rec = match &mut self.inner {
            Inner::Connected { rec, .. } => rec,
            _ => return,
        };

        let w = frame.width;
        let h = frame.height;
        let data = frame.rgb.clone();

        let img = rerun::Image::from_rgb24(data, [w, h]);
        if let Err(e) = rec.log("frame", &img) {
            self.disconnect(&format!("log failed: {e}"));
            return;
        }

        // Liveness probe: first successful flush proves viewer is listening.
        if !self.stream_proven {
            match rec.flush_with_timeout(Duration::from_millis(FLUSH_TIMEOUT_MS)) {
                Ok(()) => {
                    self.stream_proven = true;
                    self.retry_backoff_ms = INITIAL_BACKOFF_MS;
                    log::info!("viz: connected to {}", self.addr);
                }
                Err(rerun::sink::SinkFlushError::Timeout) => {
                    // Backpressure, not disconnect. Keep trying.
                }
                Err(e) => {
                    self.disconnect(&format!("flush failed: {e}"));
                }
            }
        }
    }
}

/// Spawn the viz bridge thread. Reads from the slot and sends to Rerun.
pub fn spawn(
    addr: String,
    slot: std::sync::Arc<Slot<VizBatch>>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("viz".into())
        .spawn(move || {
            let mut bridge = VizBridge::new(&addr);
            loop {
                let batch = match slot.take_blocking() {
                    Some(b) => b,
                    None => break, // slot closed, shutting down
                };
                bridge.send_frame(&batch.frame);
            }
            log::debug!("viz: thread finished");
        })
        .expect("failed to spawn viz thread")
}
