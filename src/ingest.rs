use crate::config::IngestConfig;
use crate::error::*;
use std::collections::VecDeque;

pub trait FrameReader {
    async fn next_frame(&mut self) -> Option<Frame>;
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
}

#[derive(Debug, Clone, Default)]
pub struct IngestCounters {
    pub pframes_dropped: u64,
    pub keyframes_dup: u64,
    pub retina: Option<RetinaCounters>,
}

pub struct IngestEngine<R: FrameReader> {
    reader: R,
    last_h264: Option<Vec<u8>>,
    pframes_dropped: u64,
    keyframes_dup: u64,
}

impl<R: FrameReader> IngestEngine<R> {
    pub fn new(reader: R) -> Self {
        Self { reader, last_h264: None, pframes_dropped: 0, keyframes_dup: 0 }
    }

    /// Drain all buffered frames and return the freshest IDR keyframe, or
    /// `None` if no new keyframe arrived before the reader timed out.
    /// Duplicate consecutive keyframes (same bytes) are suppressed.
    pub async fn poll_freshest_keyframe(&mut self) -> Option<RawKeyframe> {
        let mut latest: Option<Frame> = None;
        let mut pframes: u64 = 0;

        loop {
            match self.reader.next_frame().await {
                Some(frame) => {
                    if frame.is_keyframe {
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

        if self.last_h264.as_ref().is_some_and(|last| last == &kf.h264) {
            self.keyframes_dup += 1;
            return None;
        }

        let h264 = kf.h264;
        self.last_h264 = Some(h264.clone());
        Some(RawKeyframe { h264 })
    }
}

impl IngestEngine<AnyReader> {
    pub fn drain_retina_counters(&mut self) -> Option<RetinaCounters> {
        match &mut self.reader {
            AnyReader::Retina(r) => {
                let c = r.counters.clone();
                r.counters = RetinaCounters::default();
                Some(c)
            }
            _ => None,
        }
    }

    pub fn drain_ingest_counters(&mut self) -> IngestCounters {
        let retina = self.drain_retina_counters();
        let c = IngestCounters {
            pframes_dropped: self.pframes_dropped,
            keyframes_dup: self.keyframes_dup,
            retina,
        };
        self.pframes_dropped = 0;
        self.keyframes_dup = 0;
        c
    }
}

pub struct QueuedReader {
    frames: VecDeque<Frame>,
}

impl QueuedReader {
    pub fn new(frames: Vec<Frame>) -> Self {
        Self { frames: frames.into() }
    }
}

impl FrameReader for QueuedReader {
    async fn next_frame(&mut self) -> Option<Frame> {
        self.frames.pop_front()
    }
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
    ((seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(0xBF58476D1CE4E5B9) >> 32) as u64) % m
}

struct ErrorWindow {
    ring: VecDeque<bool>,
    count: u32,
    cap: usize,
    threshold: u32,
}

impl ErrorWindow {
    fn new(cap: usize, threshold: u32) -> Self {
        Self { ring: VecDeque::with_capacity(cap), count: 0, cap, threshold }
    }

    fn record(&mut self, is_error: bool) -> bool {
        self.ring.push_back(is_error);
        if is_error { self.count += 1; }
        if self.ring.len() > self.cap {
            if self.ring.pop_front().unwrap() { self.count -= 1; }
        }
        self.count > self.threshold
    }

    fn reset(&mut self) {
        self.ring.clear();
        self.count = 0;
    }
}

impl RetinaReader {
    pub async fn connect(
        url: &str,
        username: Option<&str>,
        password: Option<&str>,
        transport: &str,
        cfg: &IngestConfig,
    ) -> Result<Self> {
        let parsed = url::Url::parse(url)
            .map_err(|e| ManaError::Ingest(format!("invalid url: {e}")))?;
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
                self.retry_count, delay
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
                    let is_keyframe = mana_rtsp::h264::contains_idr(&h264);
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
}

pub enum AnyReader {
    Queued(QueuedReader),
    Retina(RetinaReader),
}

impl FrameReader for AnyReader {
    async fn next_frame(&mut self) -> Option<Frame> {
        match self {
            Self::Queued(r) => r.next_frame().await,
            Self::Retina(r) => r.next_frame().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_keyframe(id: u8) -> Frame {
        Frame { h264: vec![id; 64], is_keyframe: true }
    }

    fn make_pframe(id: u8) -> Frame {
        Frame { h264: vec![id; 32], is_keyframe: false }
    }

    fn make_reader(frames: Vec<Frame>) -> IngestEngine<QueuedReader> {
        IngestEngine::new(QueuedReader::new(frames))
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
    }

    #[tokio::test]
    async fn pframes_between_keyframes_are_dropped() {
        let mut engine = make_reader(vec![
            make_pframe(1), make_pframe(2),
            make_keyframe(10),
            make_pframe(3), make_pframe(4),
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
    async fn partial_drain_respects_exhaustion() {
        let mut engine = make_reader(vec![make_keyframe(1)]);
        let _ = engine.poll_freshest_keyframe().await;

        engine.reader.frames.push_back(make_keyframe(2));
        engine.reader.frames.push_back(make_keyframe(3));
        let kf = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(kf.h264, vec![3u8; 64]);
    }
}
