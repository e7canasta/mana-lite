//! Validez temporal de la señal: decide si la evidencia sirve para decidir.
//!
//! No es telemetría. `Health` no cuenta lo que pasó para un reporte: lo
//! consumen los guards del FSM (`data_stale`) y por eso su resultado cambia
//! transiciones. Vive fuera de `metrics` para que esa diferencia sea
//! visible en el árbol de módulos.

use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum HealthTransition {
    Stale {
        component: &'static str,
        ms_since_frame: u64,
    },
    Blind {
        ms_since_frame: u64,
    },
    Recovered,
    None,
}

pub struct Health {
    last_frame_at: Instant,
    data_stale_ms: u64,
    stale_warn_ms: u64,
    blind: bool,
    stale: bool,
}

impl Health {
    // FIXME(ADR-029): wall-clock escape hatch; prefer `new_at` with an injected clock.
    pub fn new(data_stale_ms: u64, stale_warn_ms: u64) -> Self {
        Self::new_at(data_stale_ms, stale_warn_ms, Instant::now())
    }

    pub fn new_at(data_stale_ms: u64, stale_warn_ms: u64, now: Instant) -> Self {
        Self {
            last_frame_at: now,
            data_stale_ms,
            stale_warn_ms,
            blind: false,
            stale: false,
        }
    }

    // FIXME(ADR-029): wall-clock escape hatch; prefer `touch_at`.
    #[allow(dead_code)]
    pub fn touch(&mut self) {
        self.touch_at(Instant::now());
    }

    /// Marca senal fresca. Devuelve si se venia de blind: en el superloop la
    /// recuperacion real ocurre aca (el keyframe llega), no en `evaluate_at`,
    /// que ya encuentra las banderas limpias. El evento de heartbeat debe
    /// emitirse cuando esto devuelve true.
    pub fn touch_at(&mut self, now: Instant) -> bool {
        let was_blind = self.blind;
        self.last_frame_at = now;
        self.blind = false;
        self.stale = false;
        was_blind
    }

    // FIXME(ADR-029): wall-clock escape hatch; prefer `evaluate_at`.
    #[allow(dead_code)]
    pub fn evaluate(&mut self) -> HealthTransition {
        self.evaluate_at(Instant::now())
    }

    pub fn evaluate_at(&mut self, now: Instant) -> HealthTransition {
        let stale_ms = now
            .saturating_duration_since(self.last_frame_at)
            .as_millis() as u64;

        if stale_ms > self.data_stale_ms {
            if !self.blind {
                self.blind = true;
                return HealthTransition::Blind {
                    ms_since_frame: stale_ms,
                };
            }
        } else if stale_ms > self.stale_warn_ms {
            if !self.blind && !self.stale {
                self.stale = true;
                return HealthTransition::Stale {
                    component: "ingest",
                    ms_since_frame: stale_ms,
                };
            }
        }

        if (self.blind || self.stale) && stale_ms <= self.stale_warn_ms {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_enters_stale_at_half_threshold() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        assert_eq!(health.evaluate_at(at(5_000)), HealthTransition::None);
        assert_eq!(
            health.evaluate_at(at(5_001)),
            HealthTransition::Stale {
                component: "ingest",
                ms_since_frame: 5_001,
            }
        );
    }

    #[test]
    fn health_uses_configured_stale_warning_threshold() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 2_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        assert_eq!(health.evaluate_at(at(2_000)), HealthTransition::None);
        assert!(matches!(
            health.evaluate_at(at(2_001)),
            HealthTransition::Stale {
                ms_since_frame: 2_001,
                ..
            }
        ));
    }

    #[test]
    fn health_enters_blind_at_full_threshold() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        assert_eq!(
            health.evaluate_at(at(10_001)),
            HealthTransition::Blind {
                ms_since_frame: 10_001,
            }
        );
        assert!(health.is_blind());
    }

    #[test]
    fn blind_fires_once_while_condition_holds() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        assert_eq!(
            health.evaluate_at(at(10_001)),
            HealthTransition::Blind {
                ms_since_frame: 10_001,
            }
        );
        assert_eq!(health.evaluate_at(at(12_000)), HealthTransition::None);
        assert_eq!(health.evaluate_at(at(60_000)), HealthTransition::None);
        assert!(health.is_blind());
    }

    #[test]
    fn blind_persists_across_intermediate_hysteresis_band() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        let _ = health.evaluate_at(at(10_001));
        assert_eq!(health.evaluate_at(at(7_500)), HealthTransition::None);
        assert!(health.is_blind(), "still blind inside the band");
        assert_eq!(health.evaluate_at(at(5_000)), HealthTransition::Recovered);
        assert!(!health.is_blind());
    }

    #[test]
    fn recovered_fires_once_then_stays_silent() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        let _ = health.evaluate_at(at(10_001));
        assert_eq!(health.evaluate_at(at(4_000)), HealthTransition::Recovered);
        assert_eq!(health.evaluate_at(at(3_000)), HealthTransition::None);
        assert!(!health.is_blind());
    }

    #[test]
    fn superloop_recovery_is_reported_by_touch_not_evaluate() {
        // En el ciclo real el keyframe que vuelve hace touch_at (limpia
        // blind/stale) antes de que evaluate_at corra: la rama Recovered de
        // evaluate jamas se cumple en vivo. La recuperacion la reporta
        // touch_at, y el heartbeat se emite en el touch del frame valido.
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        let _ = health.evaluate_at(at(10_001));
        assert!(health.touch_at(at(11_000)), "frame marks the recovery");
        assert!(!health.is_blind());
        assert_eq!(health.evaluate_at(at(11_000)), HealthTransition::None);
    }

    #[test]
    fn evaluate_at_saturates_when_clock_goes_backwards() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        assert_eq!(
            health.evaluate_at(start - std::time::Duration::from_millis(100)),
            HealthTransition::None
        );
    }
}
