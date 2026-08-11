use crate::config::MetricsTextConfig;
use crate::health::Health;
use crate::logger::{Event, LogSink};
use crate::metrics::{MetricsEngine, MetricsReport, PerModelMetrics};
use crate::window::ErrorWindow;
use std::time::Instant;

pub struct PipelineState {
    frame_count: u64,
    panic_window: ErrorWindow,
    last_keyframe_at: Instant,
    metrics_text: MetricsTextConfig,
}

impl PipelineState {
    pub fn new(
        metrics_text: MetricsTextConfig,
        panic_window_cycles: usize,
        max_panics_in_window: u32,
    ) -> Self {
        Self {
            frame_count: 0,
            panic_window: ErrorWindow::new(panic_window_cycles, max_panics_in_window),
            last_keyframe_at: Instant::now(),
            metrics_text,
        }
    }

    pub fn frame_number(&self) -> u64 {
        self.frame_count
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

    pub fn on_keyframe(
        &mut self,
        decode_us: u64,
        metrics: &mut MetricsEngine,
        log: &mut dyn LogSink,
        now: Instant,
    ) -> u64 {
        self.frame_count += 1;
        let dt_ms = now.duration_since(self.last_keyframe_at).as_millis() as u64;
        self.last_keyframe_at = now;
        metrics.tick_keyframe(dt_ms);
        metrics.tick_decode(decode_us);
        if self.frame_count % 5 == 1 {
            log::info!(
                "frame #{} ingested (decode {}us)",
                self.frame_count,
                decode_us
            );
        }
        log.emit(Event::frame_ingest(
            self.frame_count,
            true,
            decode_us,
            dt_ms,
        ));
        dt_ms
    }

    /// La senal es fresca solo si el keyframe produjo un frame decodificable:
    /// un decode fallido no es informacion, y no debe sacar a Health de blind.
    /// Devuelve el heartbeat si esto marca el retorno de una ceguera — es la
    /// recuperacion real del superloop (evaluate_at ya encuentra las
    /// banderas limpias y nunca emite Recovered).
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
                log_per_model(name, m, report.window_s, report.keyframes);
            }
        }
    }
}

fn log_per_model(name: &str, m: &PerModelMetrics, window_s: u64, keyframes: u64) {
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
            Some("skip".to_string())
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

    log::info!(
        "  {:>16}: {:.1} Hz | {} calls | {} ({}) | {}{}",
        name,
        m_hz,
        m.inferences,
        m_avg,
        m_range,
        m_ratio,
        flag_str,
    );
}

fn log_report(report: &MetricsReport, model_order: &[String], config: &MetricsTextConfig) {
    if config.cycle_line {
        log_cycle_line(report);
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
