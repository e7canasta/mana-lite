use crate::config::MetricsTextConfig;
use crate::logger::{Event, LogSink};
use crate::metrics::{Health, HealthTransition, MetricsEngine, MetricsReport, PerModelMetrics};
use std::time::Instant;

pub struct PipelineState {
    frame_count: u64,
    panic_count: u32,
    last_keyframe_at: Instant,
    metrics_text: MetricsTextConfig,
}

impl PipelineState {
    pub fn new(metrics_text: MetricsTextConfig) -> Self {
        Self {
            frame_count: 0,
            panic_count: 0,
            last_keyframe_at: Instant::now(),
            metrics_text,
        }
    }

    pub fn frame_number(&self) -> u64 {
        self.frame_count
    }

    pub fn on_ok(&mut self) {
        self.panic_count = 0;
    }

    pub fn on_panic(&mut self, max_consecutive: u32) -> bool {
        self.panic_count += 1;
        self.panic_count >= max_consecutive
    }

    pub fn on_keyframe(
        &mut self,
        decode_us: u64,
        metrics: &mut MetricsEngine,
        health: &mut Health,
        log: &mut dyn LogSink,
    ) -> u64 {
        self.frame_count += 1;
        let now = Instant::now();
        let dt_ms = now.duration_since(self.last_keyframe_at).as_millis() as u64;
        self.last_keyframe_at = now;
        if health.touch_at(now) {
            log.emit(Event::health_heartbeat(0, "ingest", 0));
        }
        metrics.tick_keyframe();
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

    pub fn evaluate_health(
        &self,
        now: Instant,
        health: &mut Health,
        log: &mut dyn LogSink,
        metrics: &mut MetricsEngine,
    ) {
        match health.evaluate_at(now) {
            HealthTransition::Blind { ms_since_frame } => {
                log.emit(Event::health_blind(ms_since_frame))
            }
            HealthTransition::Stale {
                component,
                ms_since_frame,
            } => log.emit(Event::health_stale(component, ms_since_frame)),
            HealthTransition::Recovered => log.emit(Event::health_heartbeat(0, "ingest", 0)),
            HealthTransition::None => {}
        }
        if health.is_blind() {
            metrics.tick_blind();
        }
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

fn log_ingest_line(report: &MetricsReport, config: &MetricsTextConfig) {
    let decode_avg = avg_ms(report.decode_total_ms, report.keyframes);
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
        "ingest: {:.1} Hz — {} keyframes processed ({} seen) in {}s | decode {}ms avg | cycles {}{}",
        hz(report.keyframes, report.window_s),
        report.keyframes,
        report.keyframes_seen.max(report.keyframes),
        report.window_s,
        decode_avg,
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
    if config.ingest_line {
        log_ingest_line(report, config);
    }
    if config.infer_summary {
        log_infer_line(report, model_order, config);
    }
}
