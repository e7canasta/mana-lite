use super::*;
use ndarray::array;
use ultralytics_inference::DepthMap;

#[test]
fn depth_metrics_count_only_finite_positive_pixels() {
    let mut engine = MetricsEngine::new(0, 50);
    let depth = DepthFrame::from_ultralytics(DepthMap::new(
        array![[0.0, 1.0, 2.0], [f32::NAN, 3.0, f32::INFINITY]],
        (2, 3),
    ));

    engine.tick_inference_depth("depth-standard", 190_000, Some(&depth), None);
    let (report, order) = engine.take_report().expect("zero-second report");
    let metrics = &report.model_metrics["depth-standard"];

    assert_eq!(order, vec!["depth-standard"]);
    assert_eq!(metrics.depth_frames, 1);
    assert_eq!(metrics.depth_valid_pixels, 3);
    assert_eq!(metrics.depth_empty, 0);
    assert_eq!(metrics.depth_min_m, 1.0);
    assert_eq!(metrics.depth_max_m, 3.0);
    assert_eq!(report.infer_empty, 0);
}

#[test]
fn empty_depth_does_not_count_as_detection_empty() {
    let mut engine = MetricsEngine::new(0, 50);
    engine.tick_inference_depth("depth-standard", 10, None, None);
    let (report, _) = engine.take_report().expect("zero-second report");
    let metrics = &report.model_metrics["depth-standard"];

    assert_eq!(metrics.depth_empty, 1);
    assert_eq!(report.infer_empty, 0);
}

/// Aceptación del item "presupuesto de ciclo": un ciclo sintético por
/// encima del presupuesto incrementa cycle_overruns y sale en el
/// reporte con p95 y max.
#[test]
fn cycle_overruns_trip_above_budget_and_show_in_report() {
    let start = Instant::now();
    let at = |ms: u64| start + std::time::Duration::from_millis(ms);
    let mut engine = MetricsEngine::new_at(0, 50, start);

    engine.tick_cycle_at(at(40)); // 40ms: dentro de presupuesto
    engine.tick_cycle_at(at(91)); // 51ms > 50: overrun
    engine.tick_cycle_at(at(111)); // 20ms: dentro de presupuesto
    engine.tick_cycle_at(at(171)); // 60ms: overrun
    engine.tick_cycle_at(at(221)); // 50ms: justo en el presupuesto, no lo pasa

    let (report, _) = engine.take_report().expect("zero-second report");
    assert_eq!(report.cycles, 5);
    assert_eq!(report.cycle_overruns, 2);
    assert_eq!(report.cycle_min_ms, 20);
    assert_eq!(report.cycle_max_ms, 60);
    assert_eq!(report.cycle_p95_ms, 60, "p95 de [40,51,20,60,50]");
    assert_eq!(report.cycle_budget_ms, 50);
}

#[test]
fn keyframe_gap_distribution_is_reported() {
    let start = Instant::now();
    let mut engine = MetricsEngine::new_at(0, 50, start);

    for gap_ms in [40, 80, 120, 200] {
        engine.tick_keyframe(gap_ms);
    }

    let (report, _) = engine.take_report().expect("zero-second report");
    assert_eq!(report.keyframe_gap_min_ms, 40);
    assert_eq!(report.keyframe_gap_p50_ms, 80);
    assert_eq!(report.keyframe_gap_p95_ms, 200);
    assert_eq!(report.keyframe_gap_max_ms, 200);
}

/// Un ciclo que no procesó keyframes pero tardó de más **sí** declara overrun.
///
/// Es el caso que el guard `processed` ocultaba: el hilo del scan bloqueado
/// drenando un sink de visualización saturado no procesa nada, y era
/// justamente esa parada la que quedaba exenta del presupuesto.
#[test]
fn a_stalled_cycle_without_work_still_trips_the_budget() {
    let start = Instant::now();
    let at = |ms: u64| start + std::time::Duration::from_millis(ms);
    let mut engine = MetricsEngine::new_at(0, 50, start);

    engine.tick_cycle_at(at(50));
    engine.tick_cycle_at(at(102)); // 52ms sin trabajo: el periodo se pasó
    let (report, _) = engine.take_report().expect("zero-second report");
    assert_eq!(
        report.cycle_overruns, 1,
        "el presupuesto mide el periodo, no el trabajo"
    );
}
