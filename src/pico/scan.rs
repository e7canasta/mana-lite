use std::time::{Duration, Instant};

/// Reloj virtual de control. Cuenta ticks y traduce a tiempo de pared.
///
/// El tiempo de control es una cuenta de ticks por el periodo, no una lectura
/// del reloj. Esto garantiza que la política clínica (confirmación de presencia,
/// dwell del FSM) corre sobre tiempo real, no sobre la frecuencia de keyframes.
pub struct ScanTimeline {
    tick: u64,
    period_ms: u64,
    origin: Instant,
}

impl ScanTimeline {
    pub fn new(origin: Instant, period_ms: u64) -> Self {
        Self {
            tick: 0,
            period_ms,
            origin,
        }
    }

    pub fn advance(&mut self) -> Instant {
        self.tick += 1;
        self.now()
    }

    pub fn now(&self) -> Instant {
        self.origin + Duration::from_millis(self.tick * self.period_ms)
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }
}

/// Reloj de vencimientos de cadencia fija.
///
/// Reemplaza a `tokio::time::interval`: conserva la recuperación en ráfaga pero
/// devuelve el atraso en vez de descartarlo. El invariante: el próximo
/// vencimiento se calcula desde el anterior, nunca desde `now`.
pub struct ScanDeadline {
    period: Duration,
    next: Instant,
}

impl ScanDeadline {
    /// Ancla la grilla en el mismo origen que `ScanTimeline`.
    /// El primer vencimiento es `origin`, no `origin + period`.
    pub fn anchored_at(origin: Instant, period: Duration) -> Self {
        Self { period, next: origin }
    }

    pub fn next(&self) -> Instant {
        self.next
    }

    /// Registra que el ciclo arrancó en `now`. Devuelve el atraso.
    pub fn arrive(&mut self, now: Instant) -> Duration {
        let late = now.saturating_duration_since(self.next);
        self.next += self.period;
        late
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_millis(200);

    #[test]
    fn deadlines_do_not_drift_under_lateness() {
        let origin = Instant::now();
        let mut deadline = ScanDeadline::anchored_at(origin, PERIOD);

        let lateness_ms = [0u64, 37, 5, 216, 1, 90];
        for (tick, late_ms) in lateness_ms.iter().enumerate() {
            let due = origin + PERIOD * tick as u32;
            assert_eq!(deadline.next(), due);
            let observed = deadline.arrive(due + Duration::from_millis(*late_ms));
            assert_eq!(observed, Duration::from_millis(*late_ms));
        }

        assert_eq!(
            deadline.next(),
            origin + PERIOD * lateness_ms.len() as u32,
        );
    }

    #[test]
    fn control_time_tracks_wall_time_through_lateness() {
        let origin = Instant::now();
        let mut deadline = ScanDeadline::anchored_at(origin, PERIOD);
        let mut timeline = ScanTimeline::new(origin, 200);

        for (tick, late_ms) in [0u64, 216, 3, 500, 12].iter().enumerate() {
            let due = deadline.next();
            if tick != 0 {
                timeline.advance();
            }
            deadline.arrive(due + Duration::from_millis(*late_ms));
            assert_eq!(timeline.now(), due);
        }
    }

    #[test]
    fn an_on_time_arrival_reports_no_lateness() {
        let origin = Instant::now();
        let mut deadline = ScanDeadline::anchored_at(origin, PERIOD);
        for tick in 0..10u32 {
            let late = deadline.arrive(origin + PERIOD * tick);
            assert_eq!(late, Duration::ZERO);
        }
    }
}
