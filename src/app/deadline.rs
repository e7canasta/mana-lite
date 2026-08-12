//! El reloj de vencimientos del scan: cuándo *debía* correr cada ciclo.
//!
//! Separado de `App::run` a propósito. Es la única aritmética de esta fase que
//! puede causar daño clínico silencioso, y dentro de un `tokio::select!` no hay
//! forma de testearla sin un runtime: acá es una función de instantes y se
//! prueba con un reloj falso.

use std::time::{Duration, Instant};

/// Reloj de vencimientos de cadencia fija.
///
/// Reemplaza a `tokio::time::interval`, cuyo `MissedTickBehavior::Burst`
/// **absorbe el atraso**: los ticks perdidos se recuperan en ráfaga y el
/// periodo promedio se mantiene, así que la medición de periodo se ve sana
/// aunque un scan haya arrancado tarde. Este tipo conserva la misma
/// recuperación en ráfaga, pero antes de descartar el atraso lo devuelve.
///
/// El ancla es la misma que la de [`ScanTimeline`](crate::scan::ScanTimeline):
/// `boot_instant`. Los dos relojes recorren entonces la misma grilla —el
/// vencimiento k y el tick k caen en `boot_instant + k * periodo`— y el atraso
/// que este tipo mide es exactamente cuánto se separó el tiempo de control del
/// tiempo de pared.
pub(crate) struct ScanDeadline {
    period: Duration,
    next: Instant,
}

impl ScanDeadline {
    /// Ancla la grilla de vencimientos en el mismo origen que la
    /// `ScanTimeline`, con el primer vencimiento **en** el origen.
    ///
    /// Que el primer vencimiento sea `origin` y no `origin + period` no es un
    /// detalle de arranque. La `ScanTimeline` corre el primer scan en su tick
    /// 0, es decir en `origin`; si el primer vencimiento cayera un periodo más
    /// tarde, el tick 0 se ejecutaría en `origin + period` y **el tiempo de
    /// control quedaría un periodo atrás del de pared para siempre**. Las
    /// edades clínicas se calculan como `tiempo de control − sello de la
    /// evidencia`, donde el sello viene del reloj de pared: un desfasaje
    /// constante las subestima en un periodo entero, en todos los ciclos, sin
    /// error ni síntoma. `tokio::time::interval` no tenía este problema porque
    /// su primer tick se completa de inmediato.
    pub(crate) fn anchored_at(origin: Instant, period: Duration) -> Self {
        Self {
            period,
            next: origin,
        }
    }

    /// Instante en el que vence el ciclo pendiente.
    pub(crate) fn next(&self) -> Instant {
        self.next
    }

    /// Registra que el ciclo pendiente arrancó en `now` y programa el
    /// siguiente. Devuelve cuánto después de su vencimiento arrancó éste.
    ///
    /// **El próximo vencimiento se calcula desde el vencimiento anterior,
    /// nunca desde `now`.** Es el invariante crítico de este archivo:
    /// `ScanTimeline` cuenta ticks y traduce cada tick a tiempo de pared
    /// multiplicando por el periodo (`ARCHITECTURE.md` §2.2-2.3). Si el
    /// vencimiento se recalculara desde `now`, cada atraso correría el reloj
    /// hacia adelante de forma permanente, la cuenta de ticks quedaría por
    /// debajo del tiempo transcurrido, y **todos los tiempos clínicos
    /// configurados durarían más de lo que declaran** — sin error, sin log y
    /// sin síntoma observable.
    ///
    /// Como consecuencia, si un scan se pasa de varios periodos los
    /// vencimientos atrasados quedan en el pasado y se recuperan de a uno, en
    /// ráfaga: idéntico a `Burst`, y es lo que mantiene la cuenta de ticks
    /// alineada con el reloj de pared.
    pub(crate) fn arrive(&mut self, now: Instant) -> Duration {
        let late = now.saturating_duration_since(self.next);
        self.next += self.period;
        late
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::ScanTimeline;
    use mana_control::domain::LoopId;

    const PERIOD: Duration = Duration::from_millis(200);

    /// El test que exige el plan de la Fase 1: con atrasos arbitrarios, el
    /// n-ésimo vencimiento sigue siendo `origen + n * periodo`. Si alguien
    /// cambia `next += period` por `next = now + period`, esto falla.
    #[test]
    fn deadlines_do_not_drift_under_lateness() {
        let origin = Instant::now();
        let mut deadline = ScanDeadline::anchored_at(origin, PERIOD);

        // Cada ciclo arranca tarde por una cantidad distinta; ninguno de esos
        // atrasos puede moverle el vencimiento al siguiente.
        let lateness_ms = [0u64, 37, 5, 216, 1, 90];
        for (tick, late_ms) in lateness_ms.iter().enumerate() {
            let due = origin + PERIOD * tick as u32;
            assert_eq!(
                deadline.next(),
                due,
                "el vencimiento del tick {tick} debe caer en origen + {tick} periodos"
            );
            let observed = deadline.arrive(due + Duration::from_millis(*late_ms));
            assert_eq!(observed, Duration::from_millis(*late_ms));
        }

        assert_eq!(
            deadline.next(),
            origin + PERIOD * lateness_ms.len() as u32,
            "seis ciclos con atraso acumulan seis periodos exactos, no más"
        );
    }

    /// El invariante que el atraso existe para proteger: la grilla de
    /// vencimientos y la de `ScanTimeline` son la misma, tick a tick, pase lo
    /// que pase con el atraso. Si se separan, el tiempo de control se corre
    /// respecto del de pared y todas las edades clínicas mienten.
    #[test]
    fn control_time_tracks_wall_time_through_lateness() {
        let origin = Instant::now();
        let mut deadline = ScanDeadline::anchored_at(origin, PERIOD);
        let mut timeline = ScanTimeline::new(LoopId::default_loop(), origin, 200);

        for (tick, late_ms) in [0u64, 216, 3, 500, 12].iter().enumerate() {
            // El lazo real avanza la timeline una vez por vencimiento
            // atendido, salvo en el primero (App::scan_tick).
            let due = deadline.next();
            if tick != 0 {
                timeline.advance();
            }
            deadline.arrive(due + Duration::from_millis(*late_ms));

            assert_eq!(
                timeline.now().as_instant(),
                due,
                "el tick {tick} de control debe caer en su vencimiento de pared"
            );
        }
    }

    /// Un scan que se pasa de varios periodos deja vencimientos en el pasado;
    /// recuperarlos de a uno es lo que mantiene la cuenta de ticks alineada con
    /// el reloj de pared. El atraso reportado decrece de a un periodo hasta
    /// alcanzarlo.
    #[test]
    fn a_long_stall_recovers_one_deadline_at_a_time() {
        let origin = Instant::now();
        let mut deadline = ScanDeadline::anchored_at(origin, PERIOD);

        // El scan del vencimiento t=0 bloquea hasta t = 1000ms: cinco
        // vencimientos (0, 200, 400, 600, 800) quedan atrás.
        let stall_end = origin + Duration::from_millis(1000);
        assert_eq!(
            deadline.arrive(stall_end),
            Duration::from_millis(1000),
            "el primer vencimiento arrancó y terminó 1000ms después de vencer"
        );

        for expected_late_ms in [800u64, 600, 400, 200, 0] {
            assert_eq!(
                deadline.arrive(stall_end),
                Duration::from_millis(expected_late_ms),
                "los vencimientos atrasados se recuperan de a uno"
            );
        }

        assert_eq!(
            deadline.next(),
            origin + Duration::from_millis(1200),
            "tras recuperar, el reloj vuelve a la grilla del periodo"
        );
    }

    /// Sin atraso, el atraso medido es cero: el instrumento no puede inventar
    /// incumplimiento donde no lo hay (compuerta del escenario 01).
    #[test]
    fn an_on_time_arrival_reports_no_lateness() {
        let origin = Instant::now();
        let mut deadline = ScanDeadline::anchored_at(origin, PERIOD);

        for tick in 0..10u32 {
            let late = deadline.arrive(origin + PERIOD * tick);
            assert_eq!(late, Duration::ZERO);
        }
    }

    /// Un arranque *anterior* al vencimiento no puede producir atraso negativo
    /// ni envolver el u64. No debería pasar con `sleep_until`, pero el tipo no
    /// depende de eso.
    #[test]
    fn an_early_arrival_saturates_at_zero() {
        let origin = Instant::now();
        let mut deadline = ScanDeadline::anchored_at(origin, PERIOD);
        deadline.arrive(origin);
        assert_eq!(
            deadline.arrive(origin + Duration::from_millis(150)),
            Duration::ZERO
        );
    }
}
