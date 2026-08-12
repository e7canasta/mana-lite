use crate::config::MetricsTextConfig;
use crate::health::Health;
use crate::logger::{Event, LogSink};
use crate::metrics::{MetricsEngine, MetricsReport, PerModelMetrics};
use crate::window::ErrorWindow;
use std::time::Instant;

/// Estado del **lazo de control**: densidad de pánicos y política de reporte.
///
/// El conteo de frames y el gap entre keyframes se fueron con la etapa de
/// percepción (Fase 3): eran coordenadas de percepción viviendo del lado del
/// programa.
pub struct PipelineState {
    panic_window: ErrorWindow,
    metrics_text: MetricsTextConfig,
}

impl PipelineState {
    pub fn new(
        metrics_text: MetricsTextConfig,
        panic_window_cycles: usize,
        max_panics_in_window: u32,
    ) -> Self {
        Self {
            panic_window: ErrorWindow::new(panic_window_cycles, max_panics_in_window),
            metrics_text,
        }
    }

    /// Un ciclo con keyframe procesado sin panic: la ventana envejece.
    /// Devuelve true si la alerta sigue activa (la densidad no ha vuelto
    /// a estar bajo el umbral).
    pub fn on_ok(&mut self) -> bool {
        self.panic_window.record(false)
    }

    /// Un ciclo con keyframe roto por panic. Devuelve true cuando la
    /// densidad de panics en la ventana supera el máximo configurado:
    /// la máquina avisa que no está procesando una fracción tolerable del
    /// trabajo — no cuenta rachas, un panic aislado entre trabajo sano
    /// no es señal.
    pub fn on_panic(&mut self) -> bool {
        self.panic_window.record(true)
    }

    /// La senal es fresca solo si el keyframe produjo un frame decodificable:
    /// un decode fallido no es informacion, y no debe sacar a Health de blind.
    /// Devuelve el heartbeat si esto marca el retorno de una ceguera — es la
    /// recuperacion real del superloop (evaluate_at ya encuentra las
    /// banderas limpias y nunca emite Recovered).
    ///
    /// Desde la Fase 3 lo llama el lazo de control al instalar evidencia nueva
    /// del slot: percepción sólo publica imagen cuando el decode funcionó, así
    /// que "hay imagen nueva" *es* la señal de frescura.
    pub fn mark_health_fresh(&mut self, health: &mut Health, log: &mut dyn LogSink, now: Instant) {
        if health.touch_at(now) {
            log.emit(Event::health_heartbeat(0, "ingest", 0));
        }
    }

    /// Publish periodic telemetry after the control loop has evaluated health.
    /// Kept outside `scan()` because reports are application instrumentation,
    /// not scene-control decisions.
    pub fn emit_metrics(&self, log: &mut dyn LogSink, metrics: &mut MetricsEngine) {
        if let Some((report, model_order)) = metrics.take_report() {
            log_report(&report, &model_order, &self.metrics_text);
            log.emit(Event::scan_deadline(&report));
            if let Some(event) = Event::evidence_age(&report) {
                log.emit(event);
            }
            log.emit(Event::metrics(report));
        }
    }
}

fn hz(count: u64, window_s: u64) -> f64 {
    if window_s > 0 {
        count as f64 / window_s as f64
    } else {
        0.0
    }
}

fn avg_ms(total_ms: u64, count: u64) -> u64 {
    if count > 0 { total_ms / count } else { 0 }
}

/// El periodo real del scan contra su presupuesto declarado: p95 es la
/// cola que rompe el scan, max el peor caso, min el suelo. `overruns`
/// cuenta los ciclos con trabajo real que superaron el presupuesto.
fn log_cycle_line(report: &MetricsReport) {
    log::info!(
        "cycle:  {:.1} Hz — {} scans in {}s | p95 {}ms max {}ms min {}ms | {} overruns (budget {}ms)",
        hz(report.cycles, report.window_s),
        report.cycles,
        report.window_s,
        report.cycle_p95_ms,
        report.cycle_max_ms,
        report.cycle_min_ms,
        report.cycle_overruns,
        report.cycle_budget_ms,
    );
}

/// Microsegundos como milisegundos con una decimal: el atraso de un lazo sano
/// vive por debajo del milisegundo y la división entera lo borraría.
fn us_as_ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

/// El cumplimiento de cadencia: cuánto después de su vencimiento arrancó cada
/// scan. Es un eje distinto del periodo de la línea `cycle:` — el periodo se
/// autocorrige con la recuperación en ráfaga y se ve sano aunque el lazo llegue
/// tarde; el atraso no se autocorrige.
///
/// `missed` cuenta los vencimientos por encima de la tolerancia, que se imprime
/// junto al contador: es el piso del temporizador, no un umbral de política, y
/// la distribución que está a su izquierda va sin recortar.
fn log_deadline_line(report: &MetricsReport) {
    log::info!(
        "dline: {} deadlines in {}s | late min {:.1}ms p50 {:.1}ms p95 {:.1}ms max {:.1}ms | {} missed (>{:.1}ms)",
        report.scan_deadlines,
        report.window_s,
        us_as_ms(report.scan_late_min_us),
        us_as_ms(report.scan_late_p50_us),
        us_as_ms(report.scan_late_p95_us),
        us_as_ms(report.scan_late_max_us),
        report.scan_deadlines_missed,
        us_as_ms(report.scan_late_tolerance_us),
    );
}

/// **El número clínico.** Cuán vieja era la evidencia sobre la que cada scan
/// decidió, contra el reloj de control.
///
/// Las demás líneas dicen si la máquina está sana. Ésta dice si la decisión se
/// tomó sobre algo actual, que es una pregunta distinta y es la que le importa
/// a una revisión de incidente.
///
/// El piso no es cero y no debería serlo: con keyframes a 1 Hz y un lazo a
/// 5 Hz, cuatro de cada cinco scans deciden sobre evidencia que ya tenían. Un
/// p50 cercano a medio intervalo de keyframe es lo sano; lo que hay que mirar
/// es el `max` contra `health.data_stale_ms`.
fn log_evidence_line(report: &MetricsReport) {
    // Sin evidencia la distribución no existe, y cuatro ceros al lado de
    // `0 scans` se leen como "la evidencia tiene 0 ms de edad" —- exactamente lo
    // contrario de lo que pasa. Un estado real informado de forma ambigua deja
    // de ser un instrumento.
    if report.evidence_scans == 0 {
        log::info!(
            "evid:  sin evidencia en {}s — ningún scan tuvo observaciones sobre las que decidir",
            report.window_s,
        );
        return;
    }
    log::info!(
        "evid:  {} scans con evidencia in {}s | edad min {}ms p50 {}ms p95 {}ms max {}ms",
        report.evidence_scans,
        report.window_s,
        report.evidence_age_min_ms,
        report.evidence_age_p50_ms,
        report.evidence_age_p95_ms,
        report.evidence_age_max_ms,
    );
}

fn log_ingest_line(report: &MetricsReport, config: &MetricsTextConfig) {
    let decode_avg = avg_ms(report.decode_total_ms, report.keyframes);
    let gap_str = if config.keyframe_gap_line && report.keyframes > 0 {
        format!(
            " | gap min {}ms p50 {}ms p95 {}ms max {}ms",
            report.keyframe_gap_min_ms,
            report.keyframe_gap_p50_ms,
            report.keyframe_gap_p95_ms,
            report.keyframe_gap_max_ms,
        )
    } else {
        String::new()
    };
    let mut flags = Vec::new();
    if config.flags.ingest_pframes && report.ingest_pframes > 0 {
        flags.push(format!("pframes:{}", report.ingest_pframes));
    }
    if config.flags.ingest_dup && report.ingest_dup_keyframes > 0 {
        flags.push(format!("dup:{}", report.ingest_dup_keyframes));
    }
    if config.flags.ingest_keyframe_drops && report.keyframes_dropped > 0 {
        flags.push(format!("kf_dropped:{}", report.keyframes_dropped));
    }
    // Bordes entre etapas (ADR-034). `kf_pisados` significa que percepción no
    // dio abasto con la cámara; `img_pisadas`, que produjo dos evidencias entre
    // dos scans. Se publican junto al resto de los descartes porque es donde un
    // lector busca "qué se tiró": un borde sin instrumentar es un borde sobre
    // el que no se puede razonar cuando algo va mal.
    if report.slot_keyframes_dropped > 0 {
        flags.push(format!("kf_pisados:{}", report.slot_keyframes_dropped));
    }
    if report.slot_images_dropped > 0 {
        flags.push(format!("img_pisadas:{}", report.slot_images_dropped));
    }
    if report.slot_viz_dropped > 0 {
        flags.push(format!("viz_pisados:{}", report.slot_viz_dropped));
    }
    if config.flags.ingest_timeouts && report.timeouts > 0 {
        flags.push(format!("timeouts:{}", report.timeouts));
    }
    if config.flags.ingest_reconnect && report.reconnect_attempts > 0 {
        flags.push(format!("reconnect:{}", report.reconnect_attempts));
    }
    if config.flags.ingest_ssrc && report.ssrc_changes > 0 {
        flags.push(format!("ssrc:{}", report.ssrc_changes));
    }
    if config.flags.ingest_rtp && report.rtp_errors > 0 {
        flags.push(format!("rtp:{}", report.rtp_errors));
    }
    let flag_str = if flags.is_empty() {
        String::new()
    } else {
        format!(" | {}", flags.join(", "))
    };

    log::info!(
        "ingest: {:.1} Hz — {} keyframes processed ({} seen) in {}s | decode {}ms avg{} | cycles {}{}",
        hz(report.keyframes, report.window_s),
        report.keyframes,
        // Sin `.max(keyframes)`: `processed > seen` en una ventana es un estado
        // real y no una anomalía de conteo — significa que un keyframe drenado
        // al final de la ventana anterior quedó staged y se emitió en esta.
        // Enmascararlo hacía que el par dejara de conservarse a lo largo de la
        // corrida, y con eso `seen` vs `processed` no servía para detectar
        // pérdidas: cada straddle sumaba deriva permanente sin que se hubiera
        // perdido nada.
        report.keyframes_seen,
        report.window_s,
        decode_avg,
        gap_str,
        report.cycles,
        flag_str,
    );
}

fn log_infer_line(report: &MetricsReport, model_order: &[String], config: &MetricsTextConfig) {
    let infer_avg = avg_ms(report.infer_total_ms, report.inferences);
    let mut flags = Vec::new();
    if config.flags.infer_skips && report.infer_skips > 0 {
        flags.push(format!("skips:{}", report.infer_skips));
    }
    if config.flags.infer_not_due && report.infer_not_due > 0 {
        flags.push(format!("not_due:{}", report.infer_not_due));
    }
    if config.flags.infer_due_but_gated && report.infer_due_but_gated > 0 {
        flags.push(format!("due_but_gated:{}", report.infer_due_but_gated));
    }
    if config.flags.infer_due_but_no_target && report.infer_due_but_no_target > 0 {
        flags.push(format!(
            "due_but_no_target:{}",
            report.infer_due_but_no_target
        ));
    }
    // Razón distinta de `skips`, y por eso contador distinto: el modelo no fue
    // salteado por su regla, el estado del FSM no lo pidió.
    if config.flags.infer_gated && report.infer_gated > 0 {
        flags.push(format!("apagados:{}", report.infer_gated));
    }
    if config.flags.infer_urgent && report.infer_urgent > 0 {
        flags.push(format!("urgent:{}", report.infer_urgent));
    }
    if config.flags.infer_urgent_expired && report.infer_urgent_expired > 0 {
        flags.push(format!("urgent_expired:{}", report.infer_urgent_expired));
    }
    if config.flags.infer_urgent_requests && report.urgent_requests > 0 {
        flags.push(format!("urgent_requests:{}", report.urgent_requests));
    }
    if config.flags.infer_urgent_wait && report.urgent_wait_samples > 0 {
        flags.push(format!(
            "urgent_wait p50:{}ms p95:{}ms max:{}ms",
            report.urgent_wait_p50_ms, report.urgent_wait_p95_ms, report.urgent_wait_max_ms
        ));
    }
    if config.flags.infer_urgent_starvation && report.urgent_starvation > 0 {
        flags.push(format!("urgent_starvation:{}", report.urgent_starvation));
    }
    if config.flags.infer_empty && report.infer_empty > 0 {
        flags.push(format!("empty:{}", report.infer_empty));
    }
    let flag_str = if flags.is_empty() {
        String::new()
    } else {
        format!(" | {}", flags.join(", "))
    };
    let range_str = if report.inferences > 0 {
        format!("{}-{}ms", report.infer_min_ms, report.infer_max_ms)
    } else {
        "---".into()
    };

    log::info!(
        "infer:  {:.1} Hz — {} calls in {}s | {} ({}) | {} dets{}",
        hz(report.inferences, report.window_s),
        report.inferences,
        report.window_s,
        infer_avg,
        range_str,
        report.infer_total_dets,
        flag_str,
    );

    if config.per_model_lines {
        for name in model_order {
            if let Some(m) = report.model_metrics.get(name) {
                log_per_model(name, m, report.window_s, report.keyframes, config);
            }
        }
    }
}

fn log_per_model(
    name: &str,
    m: &PerModelMetrics,
    window_s: u64,
    keyframes: u64,
    config: &MetricsTextConfig,
) {
    let m_hz = hz(m.inferences, window_s);
    let m_avg = avg_ms(m.infer_total_us / 1000, m.inferences);
    let m_range = if m.inferences > 0 {
        format!("{}-{}ms", m.infer_min_us / 1000, m.infer_max_us / 1000)
    } else {
        "---".into()
    };
    let m_ratio = if keyframes > 0 {
        format!("{}/{}fr", m.total_dets, keyframes)
    } else {
        "?/0".into()
    };
    let flags: Vec<String> = vec![
        if m.skips > 0 {
            Some(format!("skip:{}", m.skips))
        } else {
            None
        },
        if m.not_due > 0 {
            Some(format!("not_due:{}", m.not_due))
        } else {
            None
        },
        if config.flags.infer_due_but_gated && m.due_but_gated > 0 {
            Some(format!("due_but_gated:{}", m.due_but_gated))
        } else {
            None
        },
        if config.flags.infer_due_but_no_target && m.due_but_no_target > 0 {
            Some(format!("due_but_no_target:{}", m.due_but_no_target))
        } else {
            None
        },
        if m.gated > 0 {
            Some(format!("apagado:{}", m.gated))
        } else {
            None
        },
        if config.flags.infer_urgent && m.urgent > 0 {
            Some(format!("urgent:{}", m.urgent))
        } else {
            None
        },
        if config.flags.infer_urgent_expired && m.urgent_expired > 0 {
            Some(format!("urgent_expired:{}", m.urgent_expired))
        } else {
            None
        },
        if config.flags.infer_urgent_requests && m.urgent_requests > 0 {
            Some(format!("urgent_requests:{}", m.urgent_requests))
        } else {
            None
        },
        if config.flags.infer_urgent_wait && m.urgent_wait_samples > 0 {
            Some(format!(
                "urgent_wait p50:{}ms p95:{}ms max:{}ms",
                m.urgent_wait_p50_ms, m.urgent_wait_p95_ms, m.urgent_wait_max_ms
            ))
        } else {
            None
        },
        if config.flags.infer_urgent_starvation && m.urgent_starvation > 0 {
            Some(format!("urgent_starvation:{}", m.urgent_starvation))
        } else {
            None
        },
        if m.empty > 0 {
            Some("empty".to_string())
        } else {
            None
        },
        m.roi
            .map(|[x1, y1, x2, y2]| format!("roi:[{x1},{y1} {x2},{y2}]")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let flag_str = if flags.is_empty() {
        String::new()
    } else {
        format!(" | {}", flags.join(","))
    };
    let timing_str = if m.gap_samples > 0 {
        format!(
            " | interval {}ms | gap n:{} p50:{}ms p95:{}ms max:{}ms | due_late n:{} p50:{}ms p95:{}ms max:{}ms",
            m.interval_min_ms,
            m.gap_samples,
            m.gap_p50_ms,
            m.gap_p95_ms,
            m.gap_max_ms,
            m.due_late_samples,
            m.due_late_p50_ms,
            m.due_late_p95_ms,
            m.due_late_max_ms,
        )
    } else {
        format!(
            " | interval {}ms | gap n:0 | due_late n:0",
            m.interval_min_ms
        )
    };

    log::info!(
        "  {:>16}: {:.1} Hz | {} calls | {} ({}) | {}{}{}",
        name,
        m_hz,
        m.inferences,
        m_avg,
        m_range,
        m_ratio,
        flag_str,
        timing_str,
    );
}

fn log_report(report: &MetricsReport, model_order: &[String], config: &MetricsTextConfig) {
    if config.cycle_line {
        log_cycle_line(report);
    }
    if config.deadline_line {
        log_deadline_line(report);
    }
    if config.evidence_line {
        log_evidence_line(report);
    }
    if config.ingest_line {
        log_ingest_line(report, config);
    }
    if config.infer_summary {
        log_infer_line(report, model_order, config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Default)]
    struct RecordingSink {
        events: Vec<Event>,
    }

    impl LogSink for RecordingSink {
        fn emit(&mut self, event: Event) {
            self.events.push(event);
        }
        fn flush(&mut self) {}
        fn shutdown(&mut self, _reason: &str) {}
    }

    #[test]
    fn mark_health_fresh_emits_heartbeat_only_when_leaving_blind() {
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let mut state = PipelineState::new(MetricsTextConfig::default(), 20, 3);
        let mut log = RecordingSink::default();

        state.mark_health_fresh(&mut health, &mut log, start);
        assert!(log.events.is_empty(), "llegada sana no es evento");

        let _ = health.evaluate_at(start + Duration::from_secs(20));
        assert!(health.is_blind());
        state.mark_health_fresh(&mut health, &mut log, start + Duration::from_secs(30));
        assert_eq!(log.events.len(), 1, "return de blind emite heartbeat");
        assert!(
            matches!(&log.events[0], Event::Health { event, .. } if event == "heartbeat"),
            "esperaba evento heartbeat"
        );
        assert!(!health.is_blind());

        state.mark_health_fresh(&mut health, &mut log, start + Duration::from_secs(31));
        assert_eq!(log.events.len(), 1, "la recuperacion dispara una sola vez");
    }

    #[test]
    fn decode_failure_keeps_health_blind() {
        // En process_keyframe el touch queda detras de `frame_buf.is_some()`:
        // la secuencia de un decode roto es (a) no tocar Health y (b) dejar
        // que el evaluador del ciclo siga su curso hasta blind.
        let start = Instant::now();
        let mut health = Health::new_at(10_000, 5_000, start);
        let mut state = PipelineState::new(MetricsTextConfig::default(), 20, 3);
        let mut log = RecordingSink::default();

        state.mark_health_fresh(&mut health, &mut log, start);
        let _ = health.evaluate_at(start + Duration::from_secs(20));
        assert!(health.is_blind(), "sin frames validos se llega a blind");

        // El keyframe roto no toca Health; la ceguera persiste.
        let _ = health.evaluate_at(start + Duration::from_secs(30));
        assert!(health.is_blind());
        assert!(log.events.is_empty(), "ningun heartbeat falso");
    }

    /// El caso clínico de la mini-spec: un panic cada dos frames hacía oscilar
    /// la cuenta de 0 a 1 y nunca disparaba. Con densidad, 10 panics en una
    /// ventana de 20 ciclos es señal inequívoca, y dispara.
    #[test]
    fn watchdog_fires_on_alternating_panics_within_window() {
        let mut state = PipelineState::new(MetricsTextConfig::default(), 20, 3);
        // 20 ciclos alternados: panic, ok, panic, ok...
        // 4º panic → 4 panics en ventana > 3 → dispara, dentro de la ventana.
        let mut tripped_at = u32::MAX;
        for i in 0..20u32 {
            let tripped = if i % 2 == 0 {
                state.on_panic()
            } else {
                state.on_ok()
            };
            if tripped {
                tripped_at = i;
                break;
            }
        }
        assert!(
            tripped_at < 20,
            "el watchdog debe disparar dentro de la ventana (disparo en {tripped_at})"
        );
    }

    /// Densidad y no racha: 3 panics consecutivos y después trabajo sano no
    /// pueden tumbar el proceso, aunque la racha sea igual al umbral.
    #[test]
    fn watchdog_ignores_streaks_when_work_recovers() {
        let mut state = PipelineState::new(MetricsTextConfig::default(), 20, 3);
        for _ in 0..3 {
            assert!(!state.on_panic(), "racha de 3 no es señal");
        }
        for _ in 0..20 {
            assert!(!state.on_ok(), "trabajo sano envejece la ventana");
        }
        assert!(
            !state.on_panic(),
            "un panic aislado post-recuperacion no suena"
        );
    }

    #[test]
    fn watchdog_sparse_never_trips() {
        // Un panic cada 8 ciclos: 3 piezas máximas simultáneas en la ventana
        // de 20 — el umbral tolera 3, no se supera.
        let mut state = PipelineState::new(MetricsTextConfig::default(), 20, 3);
        for i in 0..160u32 {
            let tripped = if i % 8 == 0 {
                state.on_panic()
            } else {
                state.on_ok()
            };
            assert!(!tripped, "densidad de 1/8 no puede disparar");
        }
    }
}
