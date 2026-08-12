use crate::config::IngestConfig;
use crate::error::*;
use crate::window::ErrorWindow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Instant;

pub trait FrameReader: Send + 'static {
    /// Desazucarado a propósito en vez de `async fn`.
    ///
    /// Un `async fn` en trait no deja declarar cotas auto sobre el future que
    /// devuelve, y desde la Fase 4 la ingesta corre en su propia task: sin
    /// `Send` explícito, `tokio::spawn` no la acepta. Es el arreglo que la
    /// propia advertencia del compilador venía pidiendo.
    fn next_frame(&mut self) -> impl std::future::Future<Output = Option<Frame>> + Send;

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
    /// El keyframe más fresco ya drenado pero todavía no emitido.
    ///
    /// Vive en el engine y no en la pila de
    /// [`Self::poll_freshest_keyframe`] porque ese future se puede cancelar a
    /// mitad de un drenaje. Un keyframe guardado en un local se contaría en
    /// `keyframes_seen` y se descartaría en silencio al cancelarse.
    ///
    /// Hasta la Fase 4 lo cancelaba el `tokio::select!` del lazo, en cada tick
    /// de scan que ganaba la carrera — era el bug más caro de esta base de
    /// código. Ahora la ingesta tiene su propia task y lo único que la cancela
    /// es el aborto del apagado, así que esto ya no protege contra un bug de
    /// diseño sino contra perder un keyframe al terminar.
    staged: Option<Frame>,
    /// Cuánto puede suprimirse un keyframe idéntico antes de emitirlo igual.
    /// Ver [`should_suppress`].
    dedup_max_suppress_ms: u64,
}

/// Decide si un keyframe duplicado debe suprimirse.
///
/// La supresión **vence**. Ante una escena completamente inmóvil un encoder
/// puede emitir IDR byte-idénticos indefinidamente, y sin vencimiento la propia
/// supresión terminaría disparando `data_stale`: aguas arriba no habría forma de
/// distinguir "la escena no cambió" de "el stream murió". En un sistema clínico
/// esas dos cosas no pueden confundirse — `data_stale` significa "perdí la
/// señal" y tiene que seguir significando eso.
///
/// Función pura a propósito: el vencimiento depende del reloj, y aislar la
/// decisión permite probarla exhaustivamente sin inyectar tiempo en un future
/// que vive dentro de un `tokio::select!`.
fn should_suppress(
    last_digest: Option<u64>,
    digest: u64,
    since_emit_ms: u64,
    max_suppress_ms: u64,
) -> bool {
    last_digest == Some(digest) && since_emit_ms < max_suppress_ms
}

impl<R: FrameReader> IngestEngine<R> {
    pub fn new(reader: R, dedup_max_suppress_ms: u64) -> Self {
        Self {
            reader,
            dedup_max_suppress_ms,
            last_digest: None,
            pframes_dropped: 0,
            keyframes_dup: 0,
            keyframes_seen: 0,
            keyframes_dropped: 0,
            pending_keyframes_seen: 0,
            pending_keyframes_dropped: 0,
            last_keyframe_at: Instant::now(),
            staged: None,
        }
    }

    /// Drena todos los frames en buffer y devuelve el keyframe IDR más fresco,
    /// o `None` si no llegó ninguno nuevo antes de que el reader diera timeout.
    /// Los keyframes duplicados consecutivos (mismo digest de 64 bits) se
    /// suprimen.
    ///
    /// **Cancelación-segura.** Cada observación se compromete a `self` en el
    /// momento en que se hace, así que descartar este future no pierde trabajo:
    /// un keyframe ya drenado queda en `staged` y lo emite la llamada
    /// siguiente.
    pub async fn poll_freshest_keyframe(&mut self) -> Option<RawKeyframe> {
        loop {
            match self.reader.next_frame().await {
                Some(frame) => {
                    if frame.is_keyframe {
                        self.keyframes_seen += 1;
                        self.pending_keyframes_seen += 1;
                        if self.staged.is_some() {
                            self.keyframes_dropped += 1;
                            self.pending_keyframes_dropped += 1;
                        }
                        self.staged = Some(frame);
                    } else {
                        self.pframes_dropped += 1;
                    }
                }
                None => break,
            }
        }

        let kf = self.staged.take()?;

        let digest = h264_digest(&kf.h264);
        let since_emit_ms = self.last_keyframe_at.elapsed().as_millis() as u64;
        if should_suppress(
            self.last_digest,
            digest,
            since_emit_ms,
            self.dedup_max_suppress_ms,
        ) {
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

    /// Ventana holgada: los tests de dedupe fijan la supresión, no su
    /// vencimiento. Ese se prueba aparte sobre `should_suppress`.
    const TEST_SUPPRESS_MS: u64 = 60_000;

    #[test]
    fn suppression_expires_so_a_still_scene_never_looks_dead() {
        const D: u64 = 0xABCD;
        const WINDOW: u64 = 5_000;

        // Digest distinto: nunca se suprime, sin importar el reloj.
        assert!(!should_suppress(Some(0x1111), D, 0, WINDOW));
        assert!(!should_suppress(None, D, 0, WINDOW));

        // Mismo digest dentro de la ventana: se suprime.
        assert!(should_suppress(Some(D), D, 0, WINDOW));
        assert!(should_suppress(Some(D), D, WINDOW - 1, WINDOW));

        // Alcanzada la ventana, se emite igual: es lo que evita que una escena
        // inmóvil termine indistinguible de un stream muerto.
        assert!(!should_suppress(Some(D), D, WINDOW, WINDOW));
        assert!(!should_suppress(Some(D), D, WINDOW * 10, WINDOW));

        // Ventana cero desactiva la deduplicación por completo.
        assert!(!should_suppress(Some(D), D, 0, 0));
    }

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
        // Ventana holgada: estos tests fijan el dedupe, no su vencimiento.
        IngestEngine::new(SyntheticReader::new(frames), TEST_SUPPRESS_MS)
    }

    /// Entrega su cola y después queda pendiente para siempre, en vez de
    /// reportar agotamiento.
    ///
    /// Modela la condición real de un stream vivo: los frames llegan más rápido
    /// que `poll_timeout_ms`, así que el bucle de drenaje nunca termina solo y
    /// siempre lo corta una cancelación desde afuera.
    struct PendingReader {
        frames: std::collections::VecDeque<Frame>,
        pending: bool,
    }

    impl FrameReader for PendingReader {
        async fn next_frame(&mut self) -> Option<Frame> {
            if let Some(frame) = self.frames.pop_front() {
                return Some(frame);
            }
            if self.pending {
                std::future::pending().await
            } else {
                None
            }
        }
    }

    #[tokio::test]
    async fn staged_keyframe_survives_cancellation() {
        let mut engine = IngestEngine::new(
            PendingReader {
                frames: vec![make_keyframe(9)].into(),
                pending: true,
            },
            TEST_SUPPRESS_MS,
        );

        // Cancela el drenaje igual que lo hace el aborto del apagado.
        let cancelled = tokio::time::timeout(
            std::time::Duration::from_millis(20),
            engine.poll_freshest_keyframe(),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "el drenaje tiene que seguir corriendo cuando se lo cancela"
        );
        assert_eq!(engine.keyframes_seen, 1, "el keyframe se observó");

        // Dejar que el reader reporte agotamiento tiene que sacar ese mismo
        // keyframe: una cancelación no puede consumirlo.
        engine.reader.pending = false;
        let kf = engine
            .poll_freshest_keyframe()
            .await
            .expect("el keyframe staged antes de la cancelación debe sobrevivir");
        assert_eq!(kf.h264, vec![9u8; 64]);
        assert_eq!(
            engine.keyframes_seen, 1,
            "the surviving keyframe must not be counted twice"
        );
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
