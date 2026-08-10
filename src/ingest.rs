use crate::config::IngestConfig;
use crate::error::*;
use crate::window::ErrorWindow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Instant;

pub trait FrameReader {
    async fn next_frame(&mut self) -> Option<Frame>;

    /// Optional Retina-specific counters; default readers return `None`.
    fn take_retina_counters(&mut self) -> Option<RetinaCounters> {
        None
    }
}

/// Raw frame as delivered by the RTSP demuxer (Annex-B H.264).
/// Not yet decoded to pixels — see `FrameDecoder` in `snapshot.rs`.
pub struct Frame {
    pub h264: Vec<u8>,
    pub is_keyframe: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RetinaCounters {
    pub ssrc_changes: u64,
    pub rtp_errors: u64,
    pub stream_ends: u64,
    pub reconnect_attempts: u64,
    pub timeouts: u64,
}

/// A deduplicated IDR keyframe ready for downstream decode.
pub struct RawKeyframe {
    pub h264: Vec<u8>,
    pub keyframes_seen: u64,
    pub keyframes_dropped: u64,
    pub source_window_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct IngestCounters {
    pub pframes_dropped: u64,
    pub keyframes_dup: u64,
    pub keyframes_seen: u64,
    pub keyframes_dropped: u64,
    pub retina: Option<RetinaCounters>,
}

pub struct IngestEngine<R: FrameReader> {
    reader: R,
    last_digest: Option<u64>,
    pframes_dropped: u64,
    keyframes_dup: u64,
    keyframes_seen: u64,
    keyframes_dropped: u64,
    pending_keyframes_seen: u64,
    pending_keyframes_dropped: u64,
    last_keyframe_at: Instant,
}

impl<R: FrameReader> IngestEngine<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            last_digest: None,
            pframes_dropped: 0,
            keyframes_dup: 0,
            keyframes_seen: 0,
            keyframes_dropped: 0,
            pending_keyframes_seen: 0,
            pending_keyframes_dropped: 0,
            last_keyframe_at: Instant::now(),
        }
    }

    /// Drain all buffered frames and return the freshest IDR keyframe, or
    /// `None` if no new keyframe arrived before the reader timed out.
    /// Duplicate consecutive keyframes (same 64-bit digest) are suppressed.
    pub async fn poll_freshest_keyframe(&mut self) -> Option<RawKeyframe> {
        let mut latest: Option<Frame> = None;
        let mut pframes: u64 = 0;

        loop {
            match self.reader.next_frame().await {
                Some(frame) => {
                    if frame.is_keyframe {
                        self.keyframes_seen += 1;
                        self.pending_keyframes_seen += 1;
                        if latest.is_some() {
                            self.keyframes_dropped += 1;
                            self.pending_keyframes_dropped += 1;
                        }
                        latest = Some(frame);
                    } else {
                        pframes += 1;
                    }
                }
                None => break,
            }
        }

        self.pframes_dropped += pframes;

        let kf = latest?;

        let digest = h264_digest(&kf.h264);
        if self.last_digest == Some(digest) {
            self.keyframes_dup += 1;
            return None;
        }

        let h264 = kf.h264;
        self.last_digest = Some(digest);
        let now = Instant::now();
        let source_window_ms = now.duration_since(self.last_keyframe_at).as_millis() as u64;
        let raw = RawKeyframe {
            h264,
            keyframes_seen: self.pending_keyframes_seen,
            keyframes_dropped: self.pending_keyframes_dropped,
            source_window_ms,
        };
        self.pending_keyframes_seen = 0;
        self.pending_keyframes_dropped = 0;
        self.last_keyframe_at = now;
        Some(raw)
    }

    pub fn drain_ingest_counters(&mut self) -> IngestCounters {
        let retina = self.reader.take_retina_counters();
        let c = IngestCounters {
            pframes_dropped: self.pframes_dropped,
            keyframes_dup: self.keyframes_dup,
            keyframes_seen: self.keyframes_seen,
            keyframes_dropped: self.keyframes_dropped,
            retina,
        };
        self.pframes_dropped = 0;
        self.keyframes_dup = 0;
        self.keyframes_seen = 0;
        self.keyframes_dropped = 0;
        c
    }
}

fn h264_digest(h264: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    h264.hash(&mut hasher);
    hasher.finish()
}

pub struct RetinaReader {
    demuxed: retina::client::Demuxed,
    url: url::Url,
    username: Option<String>,
    password: Option<String>,
    transport: String,
    retry_count: u32,
    rtp_window: ErrorWindow,
    pub counters: RetinaCounters,
    poll_timeout_ms: u64,
    backoff_initial_ms: u64,
    backoff_max_ms: u64,
}

fn fast_jitter(half: u64, seed: u64) -> u64 {
    let m = half.max(1);
    ((seed
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(0xBF58476D1CE4E5B9)
        >> 32) as u64)
        % m
}

impl RetinaReader {
    pub async fn connect(
        url: &str,
        username: Option<&str>,
        password: Option<&str>,
        transport: &str,
        cfg: &IngestConfig,
    ) -> Result<Self> {
        let parsed =
            url::Url::parse(url).map_err(|e| ManaError::Ingest(format!("invalid url: {e}")))?;
        let demuxed = open_rtsp(&parsed, username, password, transport).await?;
        log::info!("rtsp connected: {url}");
        Ok(Self {
            demuxed,
            url: parsed,
            username: username.map(String::from),
            password: password.map(String::from),
            transport: transport.to_string(),
            retry_count: 0,
            rtp_window: ErrorWindow::new(cfg.error_window_size, cfg.error_window_threshold),
            counters: RetinaCounters::default(),
            poll_timeout_ms: cfg.poll_timeout_ms,
            backoff_initial_ms: cfg.reconnect_backoff_initial_ms,
            backoff_max_ms: cfg.reconnect_backoff_max_ms,
        })
    }

    async fn reconnect(&mut self) {
        let mut base_ms: u64 = self.backoff_initial_ms;
        let max_ms: u64 = self.backoff_max_ms;
        loop {
            self.retry_count += 1;
            self.counters.reconnect_attempts += 1;
            let half = base_ms / 2;
            let jitter = fast_jitter(half, self.retry_count as u64);
            let delay = half + jitter;
            log::warn!(
                "rtsp reconnect attempt {} (delay {}ms)",
                self.retry_count,
                delay
            );
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            match open_rtsp(
                &self.url,
                self.username.as_deref(),
                self.password.as_deref(),
                &self.transport,
            )
            .await
            {
                Ok(demuxed) => {
                    self.demuxed = demuxed;
                    self.rtp_window.reset();
                    log::info!("rtsp reconnected after {} attempts", self.retry_count);
                    self.retry_count = 0;
                    return;
                }
                Err(e) => {
                    log::error!("rtsp reconnect failed: {e}");
                    base_ms = (base_ms * 2).min(max_ms);
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
) -> Result<retina::client::Demuxed> {
    let mut opts = retina::client::SessionOptions::default();
    if let (Some(u), Some(p)) = (username, password) {
        opts = opts.creds(Some(retina::client::Credentials {
            username: u.into(),
            password: p.into(),
        }));
    }

    let session = retina::client::Session::describe(url.clone(), opts)
        .await
        .map_err(|e| ManaError::Ingest(format!("rtsp describe: {e}")))?;

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
                .map_err(|e| ManaError::Ingest(format!("rtsp setup stream {i}: {e}")))?;
        }
    }

    let session = session
        .play(retina::client::PlayOptions::default())
        .await
        .map_err(|e| ManaError::Ingest(format!("rtsp play: {e}")))?;

    session
        .demuxed()
        .map_err(|e| ManaError::Ingest(format!("rtsp demuxed: {e}")).into())
}

impl FrameReader for RetinaReader {
    async fn next_frame(&mut self) -> Option<Frame> {
        loop {
            let poll = tokio::time::timeout(
                std::time::Duration::from_millis(self.poll_timeout_ms),
                futures::StreamExt::next(&mut self.demuxed),
            )
            .await;

            match poll {
                Ok(Some(Ok(retina::codec::CodecItem::VideoFrame(vf)))) => {
                    self.rtp_window.record(false);
                    let h264 = vf.into_data();
                    let is_keyframe = mana_media::h264::contains_idr(&h264);
                    return Some(Frame { h264, is_keyframe });
                }
                Ok(Some(Err(e))) => {
                    let msg = e.to_string();
                    if msg.contains("wrong ssrc") {
                        self.counters.ssrc_changes += 1;
                        log::error!("rtp ssrc changed — reconnecting: {msg}");
                        self.reconnect().await;
                        continue;
                    }
                    self.counters.rtp_errors += 1;
                    if self.rtp_window.record(true) {
                        log::error!("rtp errors exceeded threshold — reconnecting: {msg}");
                        self.reconnect().await;
                        continue;
                    }
                    log::debug!("retina rtp: {msg}");
                    continue;
                }
                Ok(Some(Ok(_))) => continue,
                Ok(None) => {
                    self.counters.stream_ends += 1;
                    log::warn!("rtsp stream ended, reconnecting...");
                    self.reconnect().await;
                }
                Err(_elapsed) => {
                    self.counters.timeouts += 1;
                    return None;
                }
            }
        }
    }

    fn take_retina_counters(&mut self) -> Option<RetinaCounters> {
        let counters = self.counters.clone();
        self.counters = RetinaCounters::default();
        Some(counters)
    }
}

/// In-memory frame source for integration tests (no RTSP).
pub struct SyntheticReader {
    frames: std::collections::VecDeque<Frame>,
}

impl SyntheticReader {
    #[must_use]
    pub fn new(frames: Vec<Frame>) -> Self {
        Self {
            frames: frames.into(),
        }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }
}

impl FrameReader for SyntheticReader {
    async fn next_frame(&mut self) -> Option<Frame> {
        self.frames.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_keyframe(id: u8) -> Frame {
        Frame {
            h264: vec![id; 64],
            is_keyframe: true,
        }
    }

    fn make_pframe(id: u8) -> Frame {
        Frame {
            h264: vec![id; 32],
            is_keyframe: false,
        }
    }

    fn make_reader(frames: Vec<Frame>) -> IngestEngine<SyntheticReader> {
        IngestEngine::new(SyntheticReader::new(frames))
    }

    #[tokio::test]
    async fn returns_none_on_empty_source() {
        let mut engine = make_reader(vec![]);
        assert!(engine.poll_freshest_keyframe().await.is_none());
    }

    #[tokio::test]
    async fn drops_all_pframes() {
        let mut engine = make_reader(vec![make_pframe(1), make_pframe(2), make_pframe(3)]);
        assert!(engine.poll_freshest_keyframe().await.is_none());
    }

    #[tokio::test]
    async fn returns_keyframe_when_present() {
        let mut engine = make_reader(vec![make_keyframe(42)]);
        let kf = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(kf.h264, vec![42u8; 64]);
    }

    #[tokio::test]
    async fn returns_freshest_keyframe_drops_stale() {
        let mut engine = make_reader(vec![make_keyframe(1), make_keyframe(2), make_keyframe(3)]);
        let kf = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(kf.h264, vec![3u8; 64]);
        assert_eq!(kf.keyframes_seen, 3);
        assert_eq!(kf.keyframes_dropped, 2);
    }

    #[tokio::test]
    async fn pframes_between_keyframes_are_dropped() {
        let mut engine = make_reader(vec![
            make_pframe(1),
            make_pframe(2),
            make_keyframe(10),
            make_pframe(3),
            make_pframe(4),
            make_keyframe(20),
        ]);
        let kf = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(kf.h264, vec![20u8; 64]);
    }

    #[tokio::test]
    async fn same_frame_returns_none_on_second_poll() {
        let mut engine = make_reader(vec![make_keyframe(7)]);
        assert!(engine.poll_freshest_keyframe().await.is_some());

        engine.reader.frames.push_back(make_keyframe(7));
        assert!(engine.poll_freshest_keyframe().await.is_none());
    }

    #[tokio::test]
    async fn new_keyframe_after_same_returns_some() {
        let mut engine = make_reader(vec![make_keyframe(7)]);
        assert!(engine.poll_freshest_keyframe().await.is_some());

        engine.reader.frames.push_back(make_keyframe(8));
        let kf = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(kf.h264, vec![8u8; 64]);
    }

    #[tokio::test]
    async fn distinct_keyframes_are_not_deduplicated() {
        let mut engine = make_reader(vec![make_keyframe(7)]);
        assert!(engine.poll_freshest_keyframe().await.is_some());

        engine.reader.frames.push_back(Frame {
            h264: vec![7; 63],
            is_keyframe: true,
        });
        assert!(engine.poll_freshest_keyframe().await.is_some());
    }

    #[tokio::test]
    async fn partial_drain_respects_exhaustion() {
        let mut engine = make_reader(vec![make_keyframe(1)]);
        let _ = engine.poll_freshest_keyframe().await;

        engine.reader.frames.push_back(make_keyframe(2));
        engine.reader.frames.push_back(make_keyframe(3));
        let kf = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(kf.h264, vec![3u8; 64]);
    }
}
