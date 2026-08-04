use crate::error::*;
use std::collections::VecDeque;

pub trait FrameReader {
    async fn next_frame(&mut self) -> Option<Frame>;
}

pub struct Frame {
    pub data: Vec<u8>,
    #[allow(dead_code)]
    pub is_keyframe: bool,
    #[allow(dead_code)]
    pub timestamp: i64,
}

pub struct DecodedFrame {
    #[allow(dead_code)]
    pub data: Vec<u8>,
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
    pub decode_us: u64,
}

pub struct IngestEngine<R: FrameReader> {
    reader: R,
    last_keyframe_data: Option<Vec<u8>>,
    pub target_width: u32,
    pub target_height: u32,
}

impl<R: FrameReader> IngestEngine<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            last_keyframe_data: None,
            target_width: 640,
            target_height: 480,
        }
    }

    pub async fn poll_freshest_keyframe(&mut self) -> Option<DecodedFrame> {
        let mut latest: Option<Frame> = None;

        loop {
            match self.reader.next_frame().await {
                Some(frame) => {
                    latest = Some(frame);
                }
                None => break,
            }
        }

        let kf = latest?;

        let is_new = self.last_keyframe_data.as_ref() != Some(&kf.data);
        if !is_new {
            return None;
        }

        let decode_start = std::time::Instant::now();
        let decoded = DecodedFrame {
            data: kf.data.clone(),
            width: self.target_width,
            height: self.target_height,
            decode_us: decode_start.elapsed().as_micros() as u64,
        };
        self.last_keyframe_data = Some(kf.data);
        Some(decoded)
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
    retry_count: u32,
}

impl RetinaReader {
    pub async fn connect(
        url: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<Self> {
        let parsed = url::Url::parse(url)
            .map_err(|e| ManaError::Ingest(format!("invalid url: {e}")))?;
        let demuxed = open_rtsp(&parsed, username, password).await?;
        log::info!("rtsp connected: {url}");
        Ok(Self {
            demuxed,
            url: parsed,
            username: username.map(String::from),
            password: password.map(String::from),
            retry_count: 0,
        })
    }

    async fn reconnect(&mut self) {
        let mut backoff_ms: u64 = 1000;
        let max_backoff_ms: u64 = 30_000;
        loop {
            self.retry_count += 1;
            log::warn!(
                "rtsp reconnect attempt {} (backoff {}ms)",
                self.retry_count, backoff_ms
            );
            tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            match open_rtsp(
                &self.url,
                self.username.as_deref(),
                self.password.as_deref(),
            )
            .await
            {
                Ok(demuxed) => {
                    self.demuxed = demuxed;
                    log::info!("rtsp reconnected after {} attempts", self.retry_count);
                    self.retry_count = 0;
                    return;
                }
                Err(e) => {
                    log::error!("rtsp reconnect failed: {e}");
                    backoff_ms = (backoff_ms * 2).min(max_backoff_ms);
                }
            }
        }
    }
}

async fn open_rtsp(
    url: &url::Url,
    username: Option<&str>,
    password: Option<&str>,
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

    let mut session = session;
    session
        .setup(0, retina::client::SetupOptions::default())
        .await
        .map_err(|e| ManaError::Ingest(format!("rtsp setup: {e}")))?;

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
                std::time::Duration::ZERO,
                futures::StreamExt::next(&mut self.demuxed),
            )
            .await;

            match poll {
                Ok(Some(Ok(retina::codec::CodecItem::VideoFrame(vf)))) => {
                    let is_keyframe = vf.is_random_access_point();
                    let timestamp = vf.timestamp().timestamp();
                    let data = vf.into_data();
                    return Some(Frame { data, is_keyframe, timestamp });
                }
                Ok(Some(Err(e))) => {
                    log::error!("retina stream: {e}");
                    continue;
                }
                Ok(Some(Ok(_))) => continue,
                Ok(None) => {
                    log::warn!("rtsp stream ended, reconnecting...");
                    self.reconnect().await;
                }
                Err(_elapsed) => return None,
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
        Frame {
            data: vec![id; 64],
            is_keyframe: true,
            timestamp: id as i64 * 1000,
        }
    }

    fn make_pframe(id: u8) -> Frame {
        Frame {
            data: vec![id; 32],
            is_keyframe: false,
            timestamp: id as i64 * 500,
        }
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
    async fn processes_all_frames() {
        let mut engine = make_reader(vec![
            make_pframe(1),
            make_pframe(2),
            make_pframe(3),
        ]);
        let decoded = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(decoded.data, vec![3u8; 32]);
    }

    #[tokio::test]
    async fn returns_keyframe_when_present() {
        let mut engine = make_reader(vec![make_keyframe(42)]);
        let decoded = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(decoded.data, vec![42u8; 64]);
    }

    #[tokio::test]
    async fn returns_freshest_keyframe_drops_stale() {
        let mut engine = make_reader(vec![
            make_keyframe(1),
            make_keyframe(2),
            make_keyframe(3),
        ]);
        let decoded = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(decoded.data, vec![3u8; 64]);
    }

    #[tokio::test]
    async fn pframees_between_keyframes_are_dropped() {
        let mut engine = make_reader(vec![
            make_pframe(1),
            make_pframe(2),
            make_keyframe(10),
            make_pframe(3),
            make_pframe(4),
            make_keyframe(20),
        ]);
        let decoded = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(decoded.data, vec![20u8; 64]);
    }

    #[tokio::test]
    async fn same_frame_returns_none_on_second_poll() {
        let mut engine = make_reader(vec![make_keyframe(7)]);
        let first = engine.poll_freshest_keyframe().await;
        assert!(first.is_some());

        engine.reader.frames.push_back(make_keyframe(7));
        let second = engine.poll_freshest_keyframe().await;
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn new_keyframe_after_same_returns_some() {
        let mut engine = make_reader(vec![make_keyframe(7)]);
        assert!(engine.poll_freshest_keyframe().await.is_some());

        engine.reader.frames.push_back(make_keyframe(8));
        let second = engine.poll_freshest_keyframe().await;
        assert!(second.is_some());
        assert_eq!(second.unwrap().data, vec![8u8; 64]);
    }

    #[tokio::test]
    async fn decoded_frame_has_configured_dimensions() {
        let mut engine = make_reader(vec![make_keyframe(1)]);
        engine.target_width = 1920;
        engine.target_height = 1080;
        let decoded = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(decoded.width, 1920);
        assert_eq!(decoded.height, 1080);
    }

    #[tokio::test]
    async fn partial_drain_respects_exhaustion() {
        let mut engine = make_reader(vec![make_keyframe(1)]);
        let _ = engine.poll_freshest_keyframe().await;

        engine.reader.frames.push_back(make_keyframe(2));
        engine.reader.frames.push_back(make_keyframe(3));
        let decoded = engine.poll_freshest_keyframe().await.unwrap();
        assert_eq!(decoded.data, vec![3u8; 64]);
    }
}
