use super::{DEPTH_EVENT_VERSION, Event, FaceDwellTimerRecord, JSONL_SCHEMA_VERSION};
use crate::metrics::{MetricsReport, PerClassFrameStats};
use crate::scan::ControlStamp;
use mana_control::signals::SceneSignalsSnapshot;

impl Event {
    pub fn meta_startup(version: &str, config: &str) -> Self {
        Event::Meta {
            event: "startup".into(),
            detail: "mana-lite".into(),
            attrs: vec![
                ("schema".into(), JSONL_SCHEMA_VERSION.to_string()),
                ("version".into(), version.into()),
                ("config".into(), config.into()),
            ],
        }
    }

    pub fn meta_model_loaded(model: &str, path: &str, task: &str, warmup_ms: u64) -> Self {
        Event::Meta {
            event: "model_loaded".into(),
            detail: model.into(),
            attrs: vec![
                ("path".into(), path.into()),
                ("task".into(), task.into()),
                ("warmup_ms".into(), warmup_ms.to_string()),
            ],
        }
    }

    pub fn health_heartbeat(frame: u64, phase: &str, cycle_us: u64) -> Self {
        Event::Health {
            event: "heartbeat".into(),
            frame_id: Some(frame),
            cycle_us: Some(cycle_us),
            message: Some(format!("phase={phase}")),
        }
    }

    pub fn health_stale(component: &str, ms_since_frame: u64) -> Self {
        Event::Health {
            event: "stale".into(),
            frame_id: None,
            cycle_us: None,
            message: Some(format!("{component}: {ms_since_frame}ms since last frame")),
        }
    }

    /// Cumplimiento de cadencia de la ventana, tomado del mismo reporte que
    /// alimenta la consola para que las dos vistas no puedan discrepar.
    pub fn scan_deadline(report: &MetricsReport) -> Self {
        Event::ScanDeadline {
            window_s: report.window_s,
            deadlines: report.scan_deadlines,
            missed: report.scan_deadlines_missed,
            late_min_us: report.scan_late_min_us,
            late_p50_us: report.scan_late_p50_us,
            late_p95_us: report.scan_late_p95_us,
            late_max_us: report.scan_late_max_us,
            tolerance_us: report.scan_late_tolerance_us,
        }
    }

    /// Edad de la evidencia de la ventana, del mismo reporte que alimenta la
    /// consola para que las dos vistas no puedan discrepar. `None` cuando
    /// ningún scan tuvo evidencia: un evento con ceros diría que la evidencia
    /// era fresca, que es lo contrario de lo que pasó.
    pub fn evidence_age(report: &MetricsReport) -> Option<Self> {
        (report.evidence_scans > 0).then(|| Event::EvidenceAge {
            window_s: report.window_s,
            scans: report.evidence_scans,
            min_ms: report.evidence_age_min_ms,
            p50_ms: report.evidence_age_p50_ms,
            p95_ms: report.evidence_age_p95_ms,
            max_ms: report.evidence_age_max_ms,
        })
    }

    /// Pánico de la etapa de percepción. Se registra como salud y no como
    /// error suelto porque su consecuencia es clínica: si percepción deja de
    /// producir, la evidencia envejece y el lazo se va a `blind`.
    pub fn health_perception_panic(message: &str) -> Self {
        Event::Health {
            event: "perception_panic".into(),
            frame_id: None,
            cycle_us: None,
            message: Some(message.to_string()),
        }
    }

    /// Una etapa del pipeline terminó sola. Es salud y no error suelto porque
    /// su consecuencia es clínica: sin esa etapa, la evidencia envejece y el
    /// lazo se va a `blind` — y `blind` sin causa no se puede diagnosticar.
    pub fn health_stage_died(stage: &str) -> Self {
        Event::Health {
            event: "stage_died".into(),
            frame_id: None,
            cycle_us: None,
            message: Some(format!("{stage}: la etapa terminó sola")),
        }
    }

    pub fn health_blind(ms_since_frame: u64) -> Self {
        Event::Health {
            event: "blind".into(),
            frame_id: None,
            cycle_us: None,
            message: Some(format!(
                "No frame for {ms_since_frame}ms, data plane silent"
            )),
        }
    }

    pub fn frame_ingest(frame_id: u64, is_keyframe: bool, decode_ms: u64, gap_ms: u64) -> Self {
        Event::Frame {
            frame_id,
            is_keyframe,
            decode_ms,
            gap_ms,
        }
    }

    pub fn detection(
        frame_id: u64,
        model: &str,
        infer_ms: u64,
        pipeline_ms: u64,
        detections: Vec<super::DetRecord>,
        postprocess_rejected: usize,
        post_nms_suppressed: usize,
        per_class: Option<PerClassFrameStats>,
        crop: Option<[u32; 4]>,
    ) -> Self {
        Event::Detection {
            frame_id,
            model: model.into(),
            infer_ms,
            pipeline_ms,
            detections,
            postprocess_rejected,
            post_nms_suppressed,
            per_class,
            crop,
        }
    }

    pub fn consolidated_detection(
        frame_id: u64,
        class: &str,
        confidence: f32,
        bbox: [f32; 4],
        primary_model: &str,
        sources: Vec<String>,
    ) -> Self {
        Event::ConsolidatedDetection {
            frame_id,
            class: class.into(),
            confidence,
            bbox,
            primary_model: primary_model.into(),
            sources,
        }
    }

    pub fn depth(
        frame_id: u64,
        model: &str,
        infer_ms: u64,
        pipeline_ms: u64,
        roi: Option<[u32; 4]>,
        map_width: u32,
        map_height: u32,
        valid_pixels: u64,
        valid_ratio: Option<f32>,
        min_depth_m: Option<f32>,
        max_depth_m: Option<f32>,
    ) -> Self {
        Event::Depth {
            version: DEPTH_EVENT_VERSION,
            frame_id,
            model: model.into(),
            infer_ms,
            pipeline_ms,
            roi,
            map_width,
            map_height,
            valid_pixels,
            valid_ratio,
            min_depth_m,
            max_depth_m,
        }
    }

    pub fn depth_region(
        frame_id: u64,
        rule: &str,
        region: [u32; 4],
        metric: &str,
        value: Option<f32>,
        threshold_m: f32,
        triggered: bool,
        valid_pixels: u64,
        valid_ratio: Option<f32>,
        calibration: Option<mana_control::DepthCalibration>,
    ) -> Self {
        Event::DepthRegion {
            version: crate::depth::DEPTH_REGION_EVENT_VERSION,
            frame_id,
            rule: rule.into(),
            region,
            metric: metric.into(),
            value,
            threshold_m,
            triggered,
            valid_pixels,
            valid_ratio,
            calibration,
        }
    }

    pub fn entity(
        track_id: u64,
        class: &str,
        bbox: [f32; 4],
        sources: Vec<String>,
        stamp: ControlStamp,
    ) -> Self {
        Event::Entity {
            track_id,
            class: class.into(),
            bbox,
            sources,
            scan_seq: stamp.scan_seq,
            evidence_frame_id: stamp.evidence_frame_id,
            observations_age_ms: stamp.observations_age_ms,
            depth_age_ms: stamp.depth_age_ms,
        }
    }

    pub fn zone_occupied(
        zone: &str,
        label: &str,
        by_class: &str,
        confidence: f32,
        stamp: ControlStamp,
    ) -> Self {
        Event::Zone {
            zone: zone.into(),
            event: "occupied".into(),
            class: by_class.into(),
            label: if label == zone {
                None
            } else {
                Some(label.into())
            },
            confidence: Some(confidence),
            scan_seq: stamp.scan_seq,
            evidence_frame_id: stamp.evidence_frame_id,
            observations_age_ms: stamp.observations_age_ms,
            depth_age_ms: stamp.depth_age_ms,
        }
    }

    pub fn zone_vacated(zone: &str, label: &str, by_class: &str, stamp: ControlStamp) -> Self {
        Event::Zone {
            zone: zone.into(),
            event: "vacated".into(),
            class: by_class.into(),
            label: if label == zone {
                None
            } else {
                Some(label.into())
            },
            confidence: None,
            scan_seq: stamp.scan_seq,
            evidence_frame_id: stamp.evidence_frame_id,
            observations_age_ms: stamp.observations_age_ms,
            depth_age_ms: stamp.depth_age_ms,
        }
    }

    pub fn fsm_transition(
        from: &str,
        from_label: Option<&str>,
        to: &str,
        to_label: Option<&str>,
        trigger: &str,
        dwell_ms: u64,
    ) -> Self {
        Event::Fsm {
            from: from.into(),
            from_label: from_label.map(str::to_string),
            to: to.into(),
            to_label: to_label.map(str::to_string),
            trigger: trigger.into(),
            dwell_ms,
        }
    }

    pub fn presence(
        stamp: ControlStamp,
        state: &str,
        poi_state: &str,
        second_person: &str,
        raw_count: usize,
        confirmed_count: usize,
        signal_valid: bool,
        held: bool,
        poi_positive_ms: u64,
        poi_empty_ms: u64,
        single_timer_ms: u64,
        empty_timer_ms: u64,
        multiple_candidate_timer_ms: u64,
        multiple_exit_timer_ms: u64,
    ) -> Self {
        Event::Presence {
            scan_seq: stamp.scan_seq,
            evidence_frame_id: stamp.evidence_frame_id,
            observations_age_ms: stamp.observations_age_ms,
            depth_age_ms: stamp.depth_age_ms,
            state: state.into(),
            poi_state: poi_state.into(),
            second_person: second_person.into(),
            raw_count,
            confirmed_count,
            signal_valid,
            held,
            poi_positive_ms,
            poi_empty_ms,
            single_timer_ms,
            empty_timer_ms,
            multiple_candidate_timer_ms,
            multiple_exit_timer_ms,
        }
    }

    pub fn face_dwell(
        stamp: ControlStamp,
        source: &str,
        state: &str,
        state_label: Option<&str>,
        state_dwell_ms: u64,
        state_dwell_required_ms: Option<u64>,
        cardinality: Option<&str>,
        person_present: bool,
        face_present: bool,
        face_confidence: Option<f32>,
        face_in_dwell: Option<bool>,
        at_edge: bool,
        face_was_inside: bool,
        face_model_ran: bool,
        active_timers: Vec<FaceDwellTimerRecord>,
    ) -> Self {
        Event::FaceDwell {
            scan_seq: stamp.scan_seq,
            evidence_frame_id: stamp.evidence_frame_id,
            observations_age_ms: stamp.observations_age_ms,
            depth_age_ms: stamp.depth_age_ms,
            source: source.into(),
            state: state.into(),
            state_label: state_label.map(str::to_string),
            state_dwell_ms,
            state_dwell_required_ms,
            cardinality: cardinality.map(str::to_string),
            person_present,
            face_present,
            face_confidence,
            face_in_dwell,
            at_edge,
            face_was_inside,
            face_model_ran,
            active_timers,
        }
    }

    pub fn metrics(report: MetricsReport) -> Self {
        Event::Metrics(report)
    }

    pub fn scene_signals(stamp: ControlStamp, snapshot: SceneSignalsSnapshot) -> Self {
        Event::SceneSignals { stamp, snapshot }
    }
}
