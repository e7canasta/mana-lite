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

/// El atraso de vencimiento se publica como distribución completa, en
/// microsegundos: es el eje que el periodo no puede mostrar.
#[test]
fn scan_deadline_lateness_is_reported_as_a_distribution() {
    let mut engine = MetricsEngine::new(0, 50);

    for late_us in [0u64, 120, 300, 216_000] {
        engine.tick_scan_deadline(Duration::from_micros(late_us));
    }

    let (report, _) = engine.take_report().expect("zero-second report");
    assert_eq!(report.scan_deadlines, 4);
    assert_eq!(report.scan_late_min_us, 0);
    assert_eq!(report.scan_late_p50_us, 120);
    assert_eq!(report.scan_late_p95_us, 216_000);
    assert_eq!(report.scan_late_max_us, 216_000);
    assert_eq!(
        report.scan_deadlines_missed, 1,
        "solo el atraso por encima de la tolerancia es incumplimiento"
    );
    assert_eq!(report.scan_late_tolerance_us, SCAN_DEADLINE_TOLERANCE_US);
}

/// El piso del temporizador no puede leerse como incumplimiento: un lazo
/// ocioso despierta ~2 ms tarde y eso no es una falla.
///
/// Las muestras son las medidas en el escenario 01 —180 s, 487 vencimientos,
/// nada que pueda bloquear el lazo— y fijan la compuerta: si `missed` deja de
/// dar 0 acá, la tolerancia quedó por debajo del piso real y el contador vuelve
/// a valer ~100% en reposo, que es como nació y estaba mal.
#[test]
fn the_measured_timer_floor_is_not_a_missed_deadline() {
    let mut engine = MetricsEngine::new(0, 50);

    for late_us in [800u64, 1_145, 1_879, 2_083, 2_117] {
        engine.tick_scan_deadline(Duration::from_micros(late_us));
    }

    let (report, _) = engine.take_report().expect("zero-second report");
    assert_eq!(report.scan_deadlines, 5);
    assert_eq!(
        report.scan_deadlines_missed, 0,
        "el peor vencimiento del escenario de control no es un incumplimiento"
    );
    assert_eq!(
        report.scan_late_max_us, 2_117,
        "el atraso crudo se publica igual: la tolerancia gobierna el contador, \
         no la distribución"
    );
}

/// La prueba de que el contador **discrimina**, que es lo único que se le pide.
///
/// Reproduce una ventana del escenario 03: veinticinco vencimientos, cinco de
/// ellos alcanzados por el bloqueo de un keyframe (~115 ms) y el resto en el
/// piso del temporizador. La mediana tiene que quedarse en el piso y la p95
/// saltar al bloqueo: sin la mediana, la línea parece un lazo degradado en vez
/// de uno que cumple cuatro de cada cinco veces.
#[test]
fn one_missed_deadline_per_blocked_keyframe() {
    let mut engine = MetricsEngine::new(0, 500);

    for tick in 0..25u64 {
        let late_us = if tick % 5 == 0 { 115_000 } else { 2_000 };
        engine.tick_scan_deadline(Duration::from_micros(late_us));
    }

    let (report, _) = engine.take_report().expect("zero-second report");
    assert_eq!(
        report.scan_deadlines_missed, 5,
        "un incumplimiento por keyframe bloqueante, no veinticinco"
    );
    assert_eq!(report.scan_late_p50_us, 2_000, "la mediana queda en el piso");
    assert_eq!(report.scan_late_p95_us, 115_000, "la cola delata el bloqueo");
}

/// El periodo y el atraso son ejes distintos y no se pueden deducir uno del
/// otro. Este es el caso que motiva la fase: cinco ciclos con periodo perfecto
/// —lo que muestra la recuperación en ráfaga— sobre un lazo que llegó tarde a
/// todos sus vencimientos.
#[test]
fn a_healthy_period_can_hide_a_late_loop() {
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(ms);
    let mut engine = MetricsEngine::new_at(0, 500, start);

    for tick in 1..=5u64 {
        engine.tick_cycle_at(at(tick * 200));
        engine.tick_scan_deadline(Duration::from_millis(180));
    }

    let (report, _) = engine.take_report().expect("zero-second report");
    assert_eq!(report.cycle_p95_ms, 200, "el periodo se ve perfecto");
    assert_eq!(report.cycle_overruns, 0, "y no rompe ningún presupuesto");
    assert_eq!(
        report.scan_deadlines_missed, 5,
        "y sin embargo ningún scan arrancó a tiempo"
    );
    assert_eq!(report.scan_late_max_us, 180_000);
}

/// La edad de la evidencia es el número clínico, y su distribución es bimodal
/// por la misma razón que el atraso: con keyframes a 1 Hz y un lazo a 5 Hz,
/// cuatro de cada cinco scans deciden sobre evidencia que ya tenían.
#[test]
fn evidence_age_is_reported_as_a_distribution() {
    let mut engine = MetricsEngine::new(0, 50);

    // Un intervalo de keyframe: evidencia nueva y cuatro scans envejeciéndola.
    for age_ms in [5u64, 205, 405, 605, 805] {
        engine.tick_evidence_age(age_ms);
    }

    let (report, _) = engine.take_report().expect("zero-second report");
    assert_eq!(report.evidence_scans, 5);
    assert_eq!(report.evidence_age_min_ms, 5);
    assert_eq!(report.evidence_age_p50_ms, 405);
    assert_eq!(report.evidence_age_max_ms, 805);
}

/// Sin evidencia la edad vale `u64::MAX`, y ese centinela **no puede** entrar en
/// la distribución: un solo scan ciego dejaría el `max` en 18 trillones de
/// milisegundos y la línea diría cualquier cosa para siempre.
///
/// La ausencia de evidencia ya la cuenta `blind_cycles`; esta métrica mide la
/// edad *de lo que hubo*, y son preguntas distintas.
#[test]
fn a_window_without_evidence_reports_zero_not_a_sentinel() {
    let mut engine = MetricsEngine::new(0, 50);
    let (report, _) = engine.take_report().expect("zero-second report");

    assert_eq!(report.evidence_scans, 0);
    assert_eq!(
        report.evidence_age_min_ms, 0,
        "sin muestras el mínimo se informa como 0, no como el centinela"
    );
    assert_eq!(report.evidence_age_max_ms, 0);
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
