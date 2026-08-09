use std::collections::VecDeque;

/// Ventana de densidad de errores. Mide qué fracción del trabajo reciente
/// está fallando, no una racha: cuenta los errores dentro de los últimos
/// `cap` eventos y suena cuando la cuenta supera `threshold`.
///
/// Se expresa en eventos (ciclos de trabajo, keyframes, paquetes RTP) y no
/// en milisegundos a propósito: la señal que importa es qué proporción del
/// trabajo está fallando. Un umbral por tiempo confundiría un plazo con una
/// fracción — el mismo razonamiento que `min_hits`.
///
/// Compartido entre el reconectador RTP (ingest) y el watchdog de panics
/// (pipeline): dos consumidores son la señal de que es un primitivo.
pub struct ErrorWindow {
    ring: VecDeque<bool>,
    count: u32,
    cap: usize,
    threshold: u32,
}

impl ErrorWindow {
    pub fn new(cap: usize, threshold: u32) -> Self {
        Self {
            ring: VecDeque::with_capacity(cap),
            count: 0,
            cap,
            threshold,
        }
    }

    /// Registra un evento. Devuelve true si la densidad supera el umbral
    /// (más errores en la ventana de los que `threshold` tolera).
    pub fn record(&mut self, is_error: bool) -> bool {
        self.ring.push_back(is_error);
        if is_error {
            self.count += 1;
        }
        if self.ring.len() > self.cap && self.ring.pop_front().unwrap() {
            self.count -= 1;
        }
        self.count > self.threshold
    }

    pub fn reset(&mut self) {
        self.ring.clear();
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_below_threshold() {
        let mut w = ErrorWindow::new(20, 3);
        for _ in 0..3 {
            assert!(!w.record(false));
        }
        for _ in 0..3 {
            assert!(!w.record(true));
        }
    }

    #[test]
    fn surpasses_threshold_within_window() {
        let mut w = ErrorWindow::new(20, 3);
        for _ in 0..3 {
            assert!(!w.record(true), "hasta el maximo tolerado no suena");
        }
        assert!(w.record(true), "superar el maximo suena");
    }

    #[test]
    fn alternating_failures_trip_by_density_not_streak() {
        let mut w = ErrorWindow::new(20, 3);
        let mut tripped = false;
        for i in 0..20 {
            if w.record(i % 2 == 0) {
                tripped = true;
                break;
            }
        }
        assert!(
            tripped,
            "1 fallo de cada 2 eventos al final es densidad alta"
        );
    }

    #[test]
    fn window_slides_and_old_failures_decay() {
        let mut w = ErrorWindow::new(20, 3);
        for _ in 0..4 {
            w.record(true);
        }
        for _ in 0..21 {
            let _ = w.record(false);
        }
        for _ in 0..3 {
            assert!(!w.record(true), "los fallos viejos salieron de la ventana");
        }
        assert!(w.record(true), "y el umbral vuelve a operar sobre la nueva");
    }

    #[test]
    fn reset_clears_history() {
        let mut w = ErrorWindow::new(20, 3);
        for _ in 0..10 {
            w.record(true);
        }
        w.reset();
        for _ in 0..3 {
            assert!(!w.record(true), "el reset borro el historial");
        }
        assert!(w.record(true), "y el umbral vuelve a operar desde cero");
    }
}
