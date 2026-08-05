use crate::logger::{Event, Logger};
use crate::metrics::{Health, HealthTransition, MetricsEngine, MetricsReport, PerModelMetrics};
use std::time::Instant;

pub struct PipelineState {
    frame_count: u64,
    max_frames: Option<u64>,
    panic_count: u32,
    last_keyframe_at: Instant,
}

impl PipelineState {
    pub fn new(demo_mode: bool) -> Self {
        Self {
            frame_count: 0,
            max_frames: if demo_mode { Some(5) } else { None },
            panic_count: 0,
            last_keyframe_at: Instant::now(),
        }
    }

    pub fn frame_number(&self) -> u64 { self.frame_count }

    pub fn on_ok(&mut self) { self.panic_count = 0; }

    pub fn on_panic(&mut self, max_consecutive: u32) -> bool {
        self.panic_count += 1;
        self.panic_count >= max_consecutive
    }

    pub fn on_keyframe(&mut self, decode_us: u64, metrics: &mut MetricsEngine, health: &mut Health, log: &mut Logger) -> u64 {
        self.frame_count += 1;
        let now = Instant::now();
        let dt_ms = now.duration_since(self.last_keyframe_at).as_millis() as u64;
        self.last_keyframe_at = now;
        health.touch();
        metrics.tick_keyframe();
        metrics.tick_decode(decode_us);
        if self.frame_count % 5 == 1 {
            log::info!("frame #{} ingested (decode {}us)", self.frame_count, decode_us);
        }
        log.emit(Event::frame_ingest(self.frame_count, true, decode_us, dt_ms));
        dt_ms
    }

    pub fn evaluate_health(&self, health: &mut Health, log: &mut Logger, metrics: &mut MetricsEngine) {
        match health.evaluate() {
            HealthTransition::Blind { ms_since_frame } => log.emit(Event::health_blind(ms_since_frame)),
            HealthTransition::Stale { component, ms_since_frame } => log.emit(Event::health_stale(component, ms_since_frame)),
            HealthTransition::Recovered => log.emit(Event::health_heartbeat(0, "ingest", 0)),
            HealthTransition::None => {}
        }
        if health.is_blind() { metrics.tick_blind(); }
        if let Some((report, model_order)) = metrics.take_report() {
            log_report(&report, &model_order);
            log.emit(Event::metrics(report));
        }
    }

    pub fn is_demo(&self) -> bool { self.max_frames.is_some() }

    pub fn should_exit(&self) -> bool {
        self.max_frames.map_or(false, |max| self.frame_count >= max)
    }
}

fn hz(count: u64, window_s: u64) -> f64 {
    if window_s > 0 { count as f64 / window_s as f64 } else { 0.0 }
}

fn avg_ms(total_ms: u64, count: u64) -> u64 {
    if count > 0 { total_ms / count } else { 0 }
}

fn log_ingest_line(report: &MetricsReport) {
    let decode_avg = avg_ms(report.decode_total_ms, report.keyframes);
    let mut flags = Vec::new();
    if report.ingest_pframes > 0 { flags.push(format!("pframes:{}", report.ingest_pframes)); }
    if report.ingest_dup_keyframes > 0 { flags.push(format!("dup:{}", report.ingest_dup_keyframes)); }
    if report.timeouts > 0 { flags.push(format!("timeouts:{}", report.timeouts)); }
    if report.reconnect_attempts > 0 { flags.push(format!("reconnect:{}", report.reconnect_attempts)); }
    if report.ssrc_changes > 0 { flags.push(format!("ssrc:{}", report.ssrc_changes)); }
    if report.rtp_errors > 0 { flags.push(format!("rtp:{}", report.rtp_errors)); }
    let flag_str = if flags.is_empty() { String::new() } else { format!(" | {}", flags.join(", ")) };

    log::info!(
        "ingest: {:.1} Hz — {} keyframes in {}s | decode {}ms avg | cycles {}{}",
        hz(report.keyframes, report.window_s),
        report.keyframes, report.window_s,
        decode_avg,
        report.cycles,
        flag_str,
    );
}

fn log_infer_line(report: &MetricsReport, model_order: &[String]) {
    let infer_avg = avg_ms(report.infer_total_ms, report.inferences);
    let mut flags = Vec::new();
    if report.infer_skips > 0 { flags.push(format!("skips:{}", report.infer_skips)); }
    if report.infer_empty > 0 { flags.push(format!("empty:{}", report.infer_empty)); }
    let flag_str = if flags.is_empty() { String::new() } else { format!(" | {}", flags.join(", ")) };
    let range_str = if report.inferences > 0 {
        format!("{}-{}ms", report.infer_min_ms, report.infer_max_ms)
    } else {
        "---".into()
    };

    log::info!(
        "infer:  {:.1} Hz — {} calls in {}s | {} ({}) | {} dets{}",
        hz(report.inferences, report.window_s),
        report.inferences, report.window_s,
        infer_avg,
        range_str,
        report.infer_total_dets,
        flag_str,
    );

    for name in model_order {
        if let Some(m) = report.model_metrics.get(name) {
            log_per_model(name, m, report.window_s);
        }
    }
}

fn log_per_model(name: &str, m: &PerModelMetrics, window_s: u64) {
    let m_hz = hz(m.inferences, window_s);
    let m_avg = avg_ms(m.infer_total_us / 1000, m.inferences);
    let m_range = if m.inferences > 0 {
        format!("{}-{}ms", m.infer_min_us / 1000, m.infer_max_us / 1000)
    } else {
        "---".into()
    };
    let m_ratio = if window_s > 0 {
        format!("{}/{}fr", m.total_dets, window_s)
    } else {
        "?/0".into()
    };
    let flags: Vec<&str> = vec![
        if m.skips > 0 { Some("skip") } else { None },
        if m.empty > 0 { Some("empty") } else { None },
    ].into_iter().flatten().collect();
    let flag_str = if flags.is_empty() { String::new() } else { format!(" | {}", flags.join(",")) };

    log::info!(
        "  {:>16}: {:.1} Hz | {} calls | {} ({}) | {}{}",
        name, m_hz, m.inferences, m_avg, m_range, m_ratio, flag_str,
    );
}

fn log_report(report: &MetricsReport, model_order: &[String]) {
    log_ingest_line(report);
    log_infer_line(report, model_order);
}
