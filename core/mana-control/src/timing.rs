//! Shared temporal primitives for hysteresis and dwell timers.

use std::time::Instant;

/// Milisegundos entre dos instantes, saturados.
///
/// `Duration::as_millis` devuelve `u128` y todo el tier lo bajaba a `u64` con
/// un `as` que trunca en silencio. El desbordamiento real necesita ~584
/// millones de años, así que nunca fue un bug — pero en un lazo de control el
/// recorte se declara, no se asume. Saturar deja el peor caso en "muchísimo
/// tiempo" en vez de en un número chico y arbitrario.
#[must_use]
pub fn elapsed_ms(now: Instant, since: Instant) -> u64 {
    u64::try_from(now.saturating_duration_since(since).as_millis()).unwrap_or(u64::MAX)
}

/// Rising/falling edge debounce over wall-clock time.
#[derive(Debug, Clone, Default)]
pub struct Debouncer {
    high_since: Option<Instant>,
    low_since: Option<Instant>,
    pub engaged: bool,
}

impl Debouncer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            high_since: None,
            low_since: None,
            engaged: false,
        }
    }

    /// Update with the current condition and return whether the debounced
    /// output is engaged. `on_ms` / `off_ms` are confirmation delays.
    pub fn update_at(&mut self, condition: bool, on_ms: u64, off_ms: u64, now: Instant) -> bool {
        if condition {
            self.low_since = None;
            if !self.engaged {
                let started = *self.high_since.get_or_insert(now);
                if elapsed_ms(now, started) >= on_ms {
                    self.engaged = true;
                    self.high_since = None;
                }
            } else {
                self.high_since = None;
            }
        } else {
            self.high_since = None;
            if self.engaged {
                let started = *self.low_since.get_or_insert(now);
                if elapsed_ms(now, started) >= off_ms {
                    self.engaged = false;
                    self.low_since = None;
                }
            } else {
                self.low_since = None;
            }
        }
        self.engaged
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    #[must_use]
    pub fn elapsed_high_ms(&self, now: Instant) -> u64 {
        self.high_since.map(|t| elapsed_ms(now, t)).unwrap_or(0)
    }

    #[must_use]
    pub fn elapsed_low_ms(&self, now: Instant) -> u64 {
        self.low_since.map(|t| elapsed_ms(now, t)).unwrap_or(0)
    }
}

/// Time spent continuously in a candidate state.
#[derive(Debug, Clone, Default)]
pub struct Dwell {
    since: Option<Instant>,
}

impl Dwell {
    #[must_use]
    pub const fn new() -> Self {
        Self { since: None }
    }

    pub fn start_or_keep(&mut self, now: Instant) {
        self.since.get_or_insert(now);
    }

    pub fn clear(&mut self) {
        self.since = None;
    }

    #[must_use]
    pub fn elapsed_ms(&self, now: Instant) -> u64 {
        self.since.map(|t| elapsed_ms(now, t)).unwrap_or(0)
    }

    #[must_use]
    pub fn ready(&self, now: Instant, required_ms: u64) -> bool {
        self.elapsed_ms(now) >= required_ms
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.since.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn debouncer_requires_on_ms_before_engage() {
        let start = Instant::now(); // cfg(test)
        let mut d = Debouncer::new();
        assert!(!d.update_at(true, 100, 50, start));
        assert!(!d.update_at(true, 100, 50, start + Duration::from_millis(50)));
        assert!(d.update_at(true, 100, 50, start + Duration::from_millis(100)));
    }

    #[test]
    fn dwell_tracks_elapsed() {
        let start = Instant::now(); // cfg(test)
        let mut dwell = Dwell::new();
        dwell.start_or_keep(start);
        assert!(!dwell.ready(start + Duration::from_millis(50), 100));
        assert!(dwell.ready(start + Duration::from_millis(100), 100));
        dwell.clear();
        assert!(!dwell.is_active());
    }
}
