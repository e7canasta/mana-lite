//! Fixed-cadence scan clock and aged evidence for scan-driven clinical logic.

use std::time::{Duration, Instant};

/// Configuration for the clinical scan period (independent of I-frame GOP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanConfig {
    /// Wall-clock period between scan ticks.
    pub period_ms: u64,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self { period_ms: 200 }
    }
}

impl ScanConfig {
    #[must_use]
    pub fn period(self) -> Duration {
        Duration::from_millis(self.period_ms.max(1))
    }
}

/// Evidence retained between measurements, carrying observation age.
#[derive(Debug, Clone)]
pub struct AgedEvidence<T> {
    pub value: T,
    pub observed_at: Instant,
}

impl<T> AgedEvidence<T> {
    #[must_use]
    pub fn new(value: T, observed_at: Instant) -> Self {
        Self { value, observed_at }
    }

    #[must_use]
    pub fn age_ms(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.observed_at)
            .as_millis() as u64
    }
}

/// Helper that advances a monotonic scan timeline in tests without sleeping.
#[derive(Debug, Clone, Copy)]
pub struct ScanTimeline {
    start: Instant,
    period_ms: u64,
    tick: u64,
}

impl ScanTimeline {
    #[must_use]
    pub fn new(start: Instant, period_ms: u64) -> Self {
        Self {
            start,
            period_ms: period_ms.max(1),
            tick: 0,
        }
    }

    #[must_use]
    pub fn now(self) -> Instant {
        self.start + Duration::from_millis(self.tick.saturating_mul(self.period_ms))
    }

    pub fn advance(&mut self) -> Instant {
        self.tick = self.tick.saturating_add(1);
        self.now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aged_evidence_reports_wall_age() {
        let t0 = Instant::now();
        let aged = AgedEvidence::new(42_u32, t0);
        assert_eq!(aged.age_ms(t0), 0);
        assert_eq!(aged.age_ms(t0 + Duration::from_millis(250)), 250);
    }

    #[test]
    fn timeline_matches_period() {
        let t0 = Instant::now();
        let mut tl = ScanTimeline::new(t0, 200);
        assert_eq!(tl.now(), t0);
        let t1 = tl.advance();
        assert_eq!(t1, t0 + Duration::from_millis(200));
        let t2 = tl.advance();
        assert_eq!(t2, t0 + Duration::from_millis(400));
    }
}
