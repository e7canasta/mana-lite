use crate::detection::{Detection, DetectionMask};
use crate::health::HealthTransition;
use crate::metrics::{MetricsReport, PerClassFrameStats};
use crate::occupancy::SignalValidity;
use crate::scan::{ControlStamp, SceneEvent};
use crate::track::TrackEvent;
use crate::zones::ZoneEvent;

/// Version del esquema del evento `depth`. v2 agrega `roi`, `map_width`,
/// `map_height` y `valid_ratio` (contrato `DepthRoiMap`, spec §8/§10).
pub const DEPTH_EVENT_VERSION: u8 = 2;
/// Version of the top-level JSONL event stream schema.
/// v2: control events carry scan_seq / evidence_frame_id / observations_age_ms /
/// depth_age_ms instead of perception coordinates (frame_id, keyframe_gap_ms, …).
pub const JSONL_SCHEMA_VERSION: u8 = 2;

#[derive(Debug, Clone)]
pub enum Event {
    Meta {
        event: String,
        detail: String,
        attrs: Vec<(String, String)>,
    },
    Health {
        event: String,
        frame_id: Option<u64>,
        cycle_us: Option<u64>,
        message: Option<String>,
    },
    Frame {
        frame_id: u64,
        is_keyframe: bool,
        decode_ms: u64,
        gap_ms: u64,
    },
    Detection {
        frame_id: u64,
        model: String,
        infer_ms: u64,
        pipeline_ms: u64,
        detections: Vec<DetRecord>,
        postprocess_rejected: usize,
        post_nms_suppressed: usize,
        per_class: Option<PerClassFrameStats>,
        crop: Option<[u32; 4]>,
    },
    Depth {
        version: u8,
        frame_id: u64,
        model: String,
        infer_ms: u64,
        pipeline_ms: u64,
        roi: Option<[u32; 4]>,
        map_width: u32,
        map_height: u32,
        valid_pixels: u64,
        valid_ratio: Option<f32>,
        min_depth_m: Option<f32>,
        max_depth_m: Option<f32>,
    },
    DepthRegion {
        version: u8,
        frame_id: u64,
        rule: String,
        region: [u32; 4],
        metric: String,
        value: Option<f32>,
        threshold_m: f32,
        triggered: bool,
        valid_pixels: u64,
        valid_ratio: Option<f32>,
        calibration: Option<mana_control::DepthCalibration>,
    },
    ConsolidatedDetection {
        frame_id: u64,
        class: String,
        confidence: f32,
        bbox: [f32; 4],
        primary_model: String,
        sources: Vec<String>,
    },
    Entity {
        track_id: u64,
        class: String,
        bbox: [f32; 4],
        sources: Vec<String>,
        scan_seq: u64,
        evidence_frame_id: u64,
        observations_age_ms: u64,
        depth_age_ms: Option<u64>,
    },
    Zone {
        zone: String,
        event: String,
        class: String,
        label: Option<String>,
        confidence: Option<f32>,
        scan_seq: u64,
        evidence_frame_id: u64,
        observations_age_ms: u64,
        depth_age_ms: Option<u64>,
    },
    Fsm {
        from: String,
        from_label: Option<String>,
        to: String,
        to_label: Option<String>,
        trigger: String,
        dwell_ms: u64,
    },
    Presence {
        scan_seq: u64,
        evidence_frame_id: u64,
        observations_age_ms: u64,
        depth_age_ms: Option<u64>,
        state: String,
        poi_state: String,
        second_person: String,
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
    },
    FaceDwell {
        scan_seq: u64,
        evidence_frame_id: u64,
        observations_age_ms: u64,
        depth_age_ms: Option<u64>,
        source: String,
        state: String,
        state_label: Option<String>,
        state_dwell_ms: u64,
        state_dwell_required_ms: Option<u64>,
        cardinality: Option<String>,
        person_present: bool,
        face_present: bool,
        face_confidence: Option<f32>,
        face_in_dwell: Option<bool>,
        at_edge: bool,
        face_was_inside: bool,
        face_model_ran: bool,
        active_timers: Vec<FaceDwellTimerRecord>,
    },
    Metrics(MetricsReport),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JsonlLevel {
    Debug = 0,
    Info = 1,
    Quiet = 2,
}

impl JsonlLevel {
    pub fn from_str(s: &str) -> Self {
        match s {
            "debug" => JsonlLevel::Debug,
            "info" => JsonlLevel::Info,
            "quiet" => JsonlLevel::Quiet,
            _ => JsonlLevel::Info,
        }
    }

    pub fn allows(&self, event_level: JsonlLevel) -> bool {
        *self <= event_level
    }
}

impl Event {
    pub fn min_level(&self) -> JsonlLevel {
        match self {
            Event::Meta { .. } => JsonlLevel::Info,
            Event::Health { .. } => JsonlLevel::Info,
            Event::Fsm { .. } => JsonlLevel::Info,
            Event::Presence { .. } => JsonlLevel::Debug,
            Event::FaceDwell { .. } => JsonlLevel::Debug,
            Event::Metrics { .. } => JsonlLevel::Info,
            Event::Frame { .. } => JsonlLevel::Debug,
            Event::Detection { .. } => JsonlLevel::Debug,
            Event::Depth { .. } => JsonlLevel::Debug,
            Event::DepthRegion { .. } => JsonlLevel::Debug,
            Event::ConsolidatedDetection { .. } => JsonlLevel::Debug,
            Event::Entity { .. } => JsonlLevel::Debug,
            Event::Zone { .. } => JsonlLevel::Debug,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetRecord {
    pub class: String,
    pub confidence: f32,
    pub bbox: [f32; 4],
    /// Bounding-box area in source-frame pixels and as a fraction of the full frame.
    pub area_px: f32,
    pub area_ratio: f32,
    pub mask: Option<MaskRecord>,
}

#[derive(Debug, Clone)]
pub struct FaceDwellTimerRecord {
    pub trigger: String,
    pub elapsed_ms: u64,
    pub required_ms: u64,
}

/// JSONL wire record for an instance mask (Spec-003).
///
/// `rle` are column-major run-length counts of the mask crop; `bbox` is the
/// detection box inside mask space (`bbox[2]-bbox[0]` = RLE width,
/// `bbox[3]-bbox[1]` = RLE height). `origin` positions mask space in the
/// original frame; `mask_dims` is its size. `polygons` are contours
/// normalized to the full frame.
///
/// Built by [`MaskRecord::from_mask`]: el formato de cable lo posee este
/// modulo, no el tipo de dominio.
#[derive(Debug, Clone)]
pub struct MaskRecord {
    pub rle: Vec<u32>,
    pub bbox: [f32; 4],
    pub origin: [u32; 2],
    pub mask_dims: [u32; 2],
    pub polygons: Vec<Vec<[f32; 2]>>,
}

impl MaskRecord {
    /// JSONL wire record (Spec-003). `rle` are column-major run-length
    /// counts of the mask crop; `bbox` is the detection box inside mask
    /// space; `origin`/`mask_dims` place mask space in the frame and
    /// `polygons` are contours normalized to the full frame.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn from_mask(mask: &DetectionMask) -> Self {
        let (bbox_h, bbox_w) = if let Some(rle) = mask.compact.rles.first() {
            (rle.h, rle.w)
        } else {
            (0, 0)
        };
        let (off_x, off_y) = mask.compact.offsets.first().copied().unwrap_or((0, 0));
        MaskRecord {
            rle: mask
                .compact
                .rles
                .first()
                .map(|rle| rle.counts.to_vec())
                .unwrap_or_default(),
            bbox: [
                off_x as f32,
                off_y as f32,
                (off_x + bbox_w) as f32,
                (off_y + bbox_h) as f32,
            ],
            origin: mask.origin,
            mask_dims: mask.mask_dims,
            polygons: mask.polygons.as_ref().clone(),
        }
    }
}

impl DetRecord {
    pub fn from_detection(detection: &Detection, frame_w: u32, frame_h: u32) -> Self {
        DetRecord {
            class: detection.class.clone(),
            confidence: detection.confidence,
            bbox: detection.bbox,
            area_px: detection.area_px(),
            area_ratio: detection.area_ratio(frame_w, frame_h),
            mask: detection.mask.as_ref().map(MaskRecord::from_mask),
        }
    }
}

/// Convert tracker events to logger events.
pub fn track_event_to_log(event: &TrackEvent, stamp: ControlStamp) -> Event {
    match event {
        TrackEvent::Created { id, class, bbox } => Event::Meta {
            event: "track_created".into(),
            detail: format!("track{id}"),
            attrs: control_stamp_attrs(
                stamp,
                vec![
                    ("class".into(), class.to_string()),
                    ("bbox".into(), format_bbox(bbox)),
                ],
            ),
        },
        TrackEvent::Updated { id, bbox } => Event::Meta {
            event: "track_updated".into(),
            detail: format!("track{id}"),
            attrs: control_stamp_attrs(stamp, vec![("bbox".into(), format_bbox(bbox))]),
        },
        TrackEvent::Lost { id, class, misses } => Event::Meta {
            event: "track_lost".into(),
            detail: format!("track{id}"),
            attrs: control_stamp_attrs(
                stamp,
                vec![
                    ("class".into(), class.to_string()),
                    ("misses".into(), misses.to_string()),
                ],
            ),
        },
        TrackEvent::Deleted { id, class, reason } => Event::Meta {
            event: "track_deleted".into(),
            detail: format!("track{id}"),
            attrs: control_stamp_attrs(
                stamp,
                vec![
                    ("class".into(), class.to_string()),
                    ("reason".into(), reason.clone()),
                ],
            ),
        },
    }
}

fn control_stamp_attrs(
    stamp: ControlStamp,
    mut extra: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let mut attrs = vec![
        ("scan_seq".into(), stamp.scan_seq.to_string()),
        (
            "evidence_frame_id".into(),
            stamp.evidence_frame_id.to_string(),
        ),
        (
            "observations_age_ms".into(),
            stamp.observations_age_ms.to_string(),
        ),
    ];
    if let Some(age) = stamp.depth_age_ms {
        attrs.push(("depth_age_ms".into(), age.to_string()));
    }
    attrs.append(&mut extra);
    attrs
}

fn format_bbox(bbox: &[f32; 4]) -> String {
    format!(
        "[{:.0},{:.0},{:.0},{:.0}]",
        bbox[0], bbox[1], bbox[2], bbox[3]
    )
}

pub fn zone_event_to_log(ev: &ZoneEvent, stamp: ControlStamp) -> Event {
    match ev {
        ZoneEvent::Occupied {
            zone,
            label,
            track_id: _,
            class,
            confidence,
        } => Event::zone_occupied(
            zone,
            label.as_deref().unwrap_or(zone),
            class,
            *confidence,
            stamp,
        ),
        ZoneEvent::Vacated {
            zone,
            label,
            track_id: _,
            class,
        } => Event::zone_vacated(zone, label.as_deref().unwrap_or(zone), class, stamp),
    }
}

/// Maps a full scan batch to JSONL events.
///
/// Takes the whole batch because `SceneEvent::EntityBoxes` carries no stamp:
/// `scan()` always emits `Presence` before `EntityBoxes`, so the mapper keeps
/// the last stamp seen. `Occupancy` and `FsmState` are deliberately omitted —
/// they go only to viz; occupancy cardinality travels inside `presence.state`.
#[must_use]
pub fn scene_events_to_log(events: &[SceneEvent]) -> Vec<Event> {
    let mut out = Vec::new();
    let mut last_stamp: Option<ControlStamp> = None;
    for event in events {
        match event {
            SceneEvent::Track { event, stamp } => {
                last_stamp = Some(*stamp);
                out.push(track_event_to_log(event, *stamp));
            }
            SceneEvent::Presence {
                stamp,
                state,
                presence,
                second_person,
                signal,
                raw_person_count,
                confirmed_person_count,
                held,
                positive_ms,
                empty_ms,
                single_timer_ms,
                empty_timer_ms,
                multiple_candidate_timer_ms,
                multiple_exit_timer_ms,
            } => {
                last_stamp = Some(*stamp);
                out.push(Event::presence(
                    *stamp,
                    state.as_str(),
                    presence.as_str(),
                    second_person.as_str(),
                    *raw_person_count,
                    *confirmed_person_count,
                    *signal == SignalValidity::Valid,
                    *held,
                    *positive_ms,
                    *empty_ms,
                    *single_timer_ms,
                    *empty_timer_ms,
                    *multiple_candidate_timer_ms,
                    *multiple_exit_timer_ms,
                ));
            }
            SceneEvent::Zone { event, stamp } => {
                last_stamp = Some(*stamp);
                out.push(zone_event_to_log(event, *stamp));
            }
            SceneEvent::EntityBoxes(tracks) => {
                let Some(stamp) = last_stamp else {
                    continue;
                };
                for track in tracks {
                    let mut sources: Vec<String> = track
                        .evidence
                        .iter()
                        .map(|model| model.as_str().to_string())
                        .collect();
                    sources.sort();
                    sources.dedup();
                    out.push(Event::entity(
                        track.id,
                        track.class.as_str(),
                        track.bbox,
                        sources,
                        stamp,
                    ));
                }
            }
            SceneEvent::FsmTransition(tr) => {
                out.push(Event::fsm_transition(
                    &tr.from,
                    tr.from_label.as_deref(),
                    &tr.to,
                    tr.to_label.as_deref(),
                    &tr.trigger,
                    tr.dwell_ms,
                ));
            }
            SceneEvent::Health(transition) => match transition {
                HealthTransition::Blind { ms_since_frame } => {
                    out.push(Event::health_blind(*ms_since_frame));
                }
                HealthTransition::Stale {
                    component,
                    ms_since_frame,
                } => {
                    out.push(Event::health_stale(component, *ms_since_frame));
                }
                HealthTransition::Recovered => {
                    out.push(Event::health_heartbeat(0, "ingest", 0));
                }
                HealthTransition::None => {}
            },
            // Viz-only: occupancy cardinality travels inside presence.state.
            SceneEvent::Occupancy { .. } | SceneEvent::FsmState(_) => {}
        }
    }
    out
}

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
        detections: Vec<DetRecord>,
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
}
