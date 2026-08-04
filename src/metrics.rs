use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct Metrics {
    pub cycles: u64,
    pub frames_total: u64,
    pub keyframes: u64,
    pub pframes_dropped: u64,
    pub inferences: u64,
    pub infer_total_us: u64,
    pub decode_total_us: u64,
    pub blind_cycles: u64,
    pub timeouts: u64,
    pub ssrc_changes: u64,
    pub rtp_errors: u64,
    pub stream_ends: u64,
    pub reconnect_attempts: u64,
}

#[derive(Debug, Clone)]
pub struct MetricsReport {
    pub window_s: u64,
    pub cycles: u64,
    pub frames_total: u64,
    pub keyframes: u64,
    pub pframes_dropped: u64,
    pub inferences: u64,
    pub infer_total_ms: u64,
    pub decode_total_ms: u64,
    pub blind_cycles: u64,
    pub timeouts: u64,
    pub ssrc_changes: u64,
    pub rtp_errors: u64,
    pub stream_ends: u64,
    pub reconnect_attempts: u64,
}

pub struct MetricsEngine {
    current: Metrics,
    window_start: Instant,
    report_interval_s: u64,
}

impl MetricsEngine {
    pub fn new(report_interval_s: u64) -> Self {
        Self {
            current: Metrics::default(),
            window_start: Instant::now(),
            report_interval_s,
        }
    }

    pub fn tick_cycle(&mut self) {
        self.current.cycles += 1;
    }

    pub fn tick_keyframe(&mut self) {
        self.current.keyframes += 1;
        self.current.frames_total += 1;
    }

    #[allow(dead_code)]
    pub fn tick_pframe_dropped(&mut self) {
        self.current.pframes_dropped += 1;
        self.current.frames_total += 1;
    }

    #[allow(dead_code)]
    pub fn tick_inference(&mut self, elapsed_us: u64) {
        self.current.inferences += 1;
        self.current.infer_total_us += elapsed_us;
    }

    pub fn tick_decode(&mut self, elapsed_us: u64) {
        self.current.decode_total_us += elapsed_us;
    }

    pub fn tick_blind(&mut self) {
        self.current.blind_cycles += 1;
    }

    pub fn tick_retina_counters(&mut self, c: &crate::ingest::RetinaCounters) {
        self.current.timeouts = (self.current.timeouts).saturating_add(c.timeouts);
        self.current.ssrc_changes = (self.current.ssrc_changes).saturating_add(c.ssrc_changes);
        self.current.rtp_errors = (self.current.rtp_errors).saturating_add(c.rtp_errors);
        self.current.stream_ends = (self.current.stream_ends).saturating_add(c.stream_ends);
        self.current.reconnect_attempts = (self.current.reconnect_attempts).saturating_add(c.reconnect_attempts);
    }

    pub fn take_report(&mut self) -> Option<MetricsReport> {
        let elapsed = self.window_start.elapsed().as_secs();
        if elapsed < self.report_interval_s {
            return None;
        }
        let report = MetricsReport {
            window_s: elapsed,
            cycles: self.current.cycles,
            frames_total: self.current.frames_total,
            keyframes: self.current.keyframes,
            pframes_dropped: self.current.pframes_dropped,
            inferences: self.current.inferences,
            infer_total_ms: self.current.infer_total_us / 1000,
            decode_total_ms: self.current.decode_total_us / 1000,
            blind_cycles: self.current.blind_cycles,
            timeouts: self.current.timeouts,
            ssrc_changes: self.current.ssrc_changes,
            rtp_errors: self.current.rtp_errors,
            stream_ends: self.current.stream_ends,
            reconnect_attempts: self.current.reconnect_attempts,
        };
        self.current = Metrics::default();
        self.window_start = Instant::now();
        Some(report)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HealthTransition {
    Stale { component: &'static str, ms_since_frame: u64 },
    Blind { ms_since_frame: u64 },
    Recovered,
    None,
}

pub struct Health {
    last_frame_at: Instant,
    data_stale_ms: u64,
    blind: bool,
    stale: bool,
}

impl Health {
    pub fn new(data_stale_ms: u64) -> Self {
        Self {
            last_frame_at: Instant::now(),
            data_stale_ms,
            blind: false,
            stale: false,
        }
    }

    pub fn touch(&mut self) {
        self.last_frame_at = Instant::now();
        self.blind = false;
        self.stale = false;
    }

    pub fn evaluate(&mut self) -> HealthTransition {
        let stale_ms = self.last_frame_at.elapsed().as_millis() as u64;

        if stale_ms > self.data_stale_ms {
            if !self.blind {
                self.blind = true;
                return HealthTransition::Blind { ms_since_frame: stale_ms };
            }
        } else if stale_ms > self.data_stale_ms / 2 {
            if !self.blind && !self.stale {
                self.stale = true;
                return HealthTransition::Stale { component: "ingest", ms_since_frame: stale_ms };
            }
        }

        if (self.blind || self.stale) && stale_ms <= self.data_stale_ms / 2 {
            let was_blind = self.blind;
            self.blind = false;
            self.stale = false;
            if was_blind {
                return HealthTransition::Recovered;
            }
        }

        HealthTransition::None
    }

    pub fn is_blind(&self) -> bool {
        self.blind
    }
}
