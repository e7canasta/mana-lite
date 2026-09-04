use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use crate::pico::config::IngestConfig;

/// Frame decoded to RGB, ready for visualization.
pub struct DecodedFrame {
    pub rgb: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub frame_id: u64,
    pub timestamp_ns: i64,
}

/// Raw keyframe from RTSP, before decode.
pub struct RawKeyframe {
    pub h264: Vec<u8>,
    pub frame_id: u64,
}

/// Simple error window for RTP error tracking.
struct ErrorWindow {
    size: usize,
    threshold: u32,
    errors: Vec<bool>,
    pos: usize,
}

impl ErrorWindow {
    fn new(size: usize, threshold: u32) -> Self {
        Self {
            size,
            threshold,
            errors: vec![false; size],
            pos: 0,
        }
    }

    fn record(&mut self, is_error: bool) -> bool {
        self.errors[self.pos] = is_error;
        self.pos = (self.pos + 1) % self.size;
        self.errors.iter().filter(|&&e| e).count() as u32 >= self.threshold
    }
}

/// Ingest engine: connects to RTSP, extracts keyframes, deduplicates.
/// Produces raw H.264 keyframes. Decode happens elsewhere.
pub struct IngestEngine {
    demuxed: retina::client::Demuxed,
    url: url::Url,
    username: Option<String>,
    password: Option<String>,
    transport: String,
    retry_count: u32,
    rtp_window: ErrorWindow,
    counters: Counters,
    poll_timeout_ms: u64,
    backoff_initial_ms: u64,
    backoff_max_ms: u64,
    last_digest: Option<u64>,
    staged: Option<Vec<u8>>,
    keyframes_seen: u64,
    keyframes_dropped: u64,
    pending_seen: u64,
    pending_dropped: u64,
    last_keyframe_at: Instant,
    dedup_max_suppress_ms: u64,
    frame_id: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Counters {
    pub keyframes_seen: u64,
    pub keyframes_dropped: u64,
    pub pframes_dropped: u64,
    pub reconnects: u64,
    pub rtp_errors: u64,
    pub timeouts: u64,
}

impl IngestEngine {
    pub async fn connect(url: &str, cfg: &IngestConfig) -> Result<Self, String> {
        let parsed = url::Url::parse(url).map_err(|e| format!("invalid url: {e}"))?;
        let demuxed = open_rtsp(&parsed, None, None, "tcp").await?;
        log::info!("rtsp connected: {url}");
        Ok(Self {
            demuxed,
            url: parsed,
            username: None,
            password: None,
            transport: "tcp".into(),
            retry_count: 0,
            rtp_window: ErrorWindow::new(cfg.error_window_size, cfg.error_window_threshold),
            counters: Counters::default(),
            poll_timeout_ms: cfg.poll_timeout_ms,
            backoff_initial_ms: cfg.reconnect_backoff_initial_ms,
            backoff_max_ms: cfg.reconnect_backoff_max_ms,
            last_digest: None,
            staged: None,
            keyframes_seen: 0,
            keyframes_dropped: 0,
            pending_seen: 0,
            pending_dropped: 0,
            last_keyframe_at: Instant::now(),
            dedup_max_suppress_ms: cfg.dedup_max_suppress_ms,
            frame_id: 0,
        })
    }

    /// Poll for the freshest keyframe. Returns raw H.264 if available.
    pub async fn poll(&mut self) -> Option<RawKeyframe> {
        // Drain all available frames, keeping only the freshest keyframe.
        loop {
            let poll = tokio::time::timeout(
                Duration::from_millis(self.poll_timeout_ms),
                futures::StreamExt::next(&mut self.demuxed),
            )
            .await;

            match poll {
                Ok(Some(Ok(retina::codec::CodecItem::VideoFrame(vf)))) => {
                    self.rtp_window.record(false);
                    let h264 = vf.into_data();
                    let is_keyframe = mana_media::h264::contains_idr(&h264);
                    if is_keyframe {
                        self.keyframes_seen += 1;
                        self.pending_seen += 1;
                        if self.staged.is_some() {
                            self.keyframes_dropped += 1;
                            self.pending_dropped += 1;
                        }
                        self.staged = Some(h264);
                    } else {
                        self.counters.pframes_dropped += 1;
                    }
                }
                Ok(Some(Err(e))) => {
                    let msg = e.to_string();
                    if msg.contains("wrong ssrc") {
                        self.counters.rtp_errors += 1;
                        log::error!("rtp ssrc changed: {msg}");
                        self.reconnect().await;
                        continue;
                    }
                    self.counters.rtp_errors += 1;
                    if self.rtp_window.record(true) {
                        log::error!("rtp errors exceeded threshold: {msg}");
                        self.reconnect().await;
                        continue;
                    }
                    continue;
                }
                Ok(Some(Ok(_))) => continue,
                Ok(None) => {
                    log::warn!("rtsp stream ended, reconnecting...");
                    self.reconnect().await;
                    continue;
                }
                Err(_) => {
                    self.counters.timeouts += 1;
                    return None;
                }
            }

            // If we have a staged keyframe and no more frames pending, return it.
            // The poll_timeout will trigger on the next iteration if there are
            // more frames, so we can return now with the freshest keyframe.
            if let Some(h264) = self.staged.take() {
                // Deduplicate: suppress identical consecutive keyframes within window.
                let digest = h264_digest(&h264);
                let since_emit_ms = self.last_keyframe_at.elapsed().as_millis() as u64;
                if self.last_digest == Some(digest) && since_emit_ms < self.dedup_max_suppress_ms {
                    return None;
                }

                let now = Instant::now();
                self.last_digest = Some(digest);
                self.last_keyframe_at = now;
                self.frame_id += 1;

                return Some(RawKeyframe {
                    h264,
                    frame_id: self.frame_id,
                });
            }
        }
    }

    pub fn take_counters(&mut self) -> Counters {
        let mut c = std::mem::take(&mut self.counters);
        c.keyframes_seen = self.keyframes_seen;
        c.keyframes_dropped = self.keyframes_dropped;
        self.keyframes_seen = 0;
        self.keyframes_dropped = 0;
        c
    }

    async fn reconnect(&mut self) {
        let mut base_ms = self.backoff_initial_ms;
        loop {
            self.retry_count += 1;
            self.counters.reconnects += 1;
            let delay = base_ms / 2;
            log::warn!("rtsp reconnect attempt {} (delay {}ms)", self.retry_count, delay);
            tokio::time::sleep(Duration::from_millis(delay)).await;
            match open_rtsp(&self.url, self.username.as_deref(), self.password.as_deref(), &self.transport).await {
                Ok(demuxed) => {
                    self.demuxed = demuxed;
                    self.rtp_window = ErrorWindow::new(128, 25);
                    log::info!("rtsp reconnected after {} attempts", self.retry_count);
                    self.retry_count = 0;
                    return;
                }
                Err(e) => {
                    log::error!("rtsp reconnect failed: {e}");
                    base_ms = (base_ms * 2).min(self.backoff_max_ms);
                }
            }
        }
    }
}

async fn open_rtsp(
    url: &url::Url,
    username: Option<&str>,
    password: Option<&str>,
    transport: &str,
) -> Result<retina::client::Demuxed, String> {
    let mut opts = retina::client::SessionOptions::default();
    if let (Some(u), Some(p)) = (username, password) {
        opts = opts.creds(Some(retina::client::Credentials {
            username: u.into(),
            password: p.into(),
        }));
    }

    let session = retina::client::Session::describe(url.clone(), opts)
        .await
        .map_err(|e| format!("rtsp describe: {e}"))?;

    let rtsp_transport = match transport {
        "tcp" => retina::client::Transport::Tcp(retina::client::TcpTransportOptions::default()),
        _ => retina::client::Transport::Udp(retina::client::UdpTransportOptions::default()),
    };

    let stream_count = session.streams().len();
    let mut session = session;
    for i in 0..stream_count {
        if session.streams()[i].media() == "video" {
            session
                .setup(
                    i,
                    retina::client::SetupOptions::default()
                        .transport(rtsp_transport.clone())
                        .frame_format(retina::codec::FrameFormat::SIMPLE),
                )
                .await
                .map_err(|e| format!("rtsp setup stream {i}: {e}"))?;
        }
    }

    let session = session
        .play(retina::client::PlayOptions::default())
        .await
        .map_err(|e| format!("rtsp play: {e}"))?;

    session
        .demuxed()
        .map_err(|e| format!("rtsp demuxed: {e}"))
}

fn h264_digest(h264: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    h264.hash(&mut hasher);
    hasher.finish()
}

/// Minimal H.264 → RGB decoder using ffmpeg.
pub struct Decoder {
    decoder: mana_media::decoder::SoftwareDecoder,
    scaler: Option<(
        ffmpeg_next::software::scaling::Context,
        u32,
        u32,
        ffmpeg_next::format::Pixel,
    )>,
}

impl Decoder {
    pub fn new() -> Result<Self, String> {
        ffmpeg_next::init().map_err(|e| format!("ffmpeg init: {e}"))?;
        let decoder = mana_media::decoder::SoftwareDecoder::new(ffmpeg_next::codec::Id::H264)
            .map_err(|e| format!("decoder create: {e}"))?;
        Ok(Self {
            decoder,
            scaler: None,
        })
    }

    pub fn decode(&mut self, h264: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
        let mut result = None;
        let mut scaler = self.scaler.take();
        self.decoder
            .decode(h264, |frame| {
                let w = frame.width();
                let h = frame.height();
                let fmt = frame.format();

                let needs_new = match &scaler {
                    Some((_, sw, sh, sf)) => *sw != w || *sh != h || *sf != fmt,
                    None => true,
                };
                if needs_new {
                    if let Ok(s) = ffmpeg_next::software::scaling::Context::get(
                        fmt,
                        w,
                        h,
                        ffmpeg_next::format::Pixel::RGB24,
                        w,
                        h,
                        ffmpeg_next::software::scaling::Flags::BILINEAR,
                    ) {
                        scaler = Some((s, w, h, fmt));
                    }
                }

                if let Some((s, _, _, _)) = &mut scaler {
                    let mut rgb_frame = ffmpeg_next::util::frame::Video::empty();
                    if s.run(frame, &mut rgb_frame).is_ok() {
                        let data = rgb_frame.data(0).to_vec();
                        result = Some((data, w, h));
                    }
                }
                Ok(())
            })
            .ok()?;
        self.scaler = scaler;
        result
    }
}
