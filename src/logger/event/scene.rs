use super::Event;
use crate::health::HealthTransition;
use crate::occupancy::SignalValidity;
use crate::scan::{ControlStamp, SceneEvent};
use crate::track::TrackEvent;
use crate::zones::ZoneEvent;

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
            SceneEvent::Track { .. } => append_track_scene_event(event, &mut out, &mut last_stamp),
            SceneEvent::Presence { .. } => {
                append_presence_scene_event(event, &mut out, &mut last_stamp)
            }
            SceneEvent::Zone { .. } => append_zone_scene_event(event, &mut out, &mut last_stamp),
            SceneEvent::EntityBoxes(_) => {
                append_entity_boxes_scene_event(event, &mut out, last_stamp)
            }
            SceneEvent::FsmTransition(_) => append_fsm_transition_scene_event(event, &mut out),
            SceneEvent::Health(_) => append_health_scene_event(event, &mut out),
            // Viz-only: occupancy cardinality travels inside presence.state.
            SceneEvent::Occupancy { .. } | SceneEvent::FsmState(_) => {}
        }
    }
    out
}

fn append_track_scene_event(
    event: &SceneEvent,
    out: &mut Vec<Event>,
    last_stamp: &mut Option<ControlStamp>,
) {
    let SceneEvent::Track { event, stamp } = event else {
        unreachable!()
    };
    *last_stamp = Some(*stamp);
    out.push(track_event_to_log(event, *stamp));
}

fn append_presence_scene_event(
    event: &SceneEvent,
    out: &mut Vec<Event>,
    last_stamp: &mut Option<ControlStamp>,
) {
    let SceneEvent::Presence {
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
    } = event
    else {
        unreachable!()
    };
    *last_stamp = Some(*stamp);
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

fn append_zone_scene_event(
    event: &SceneEvent,
    out: &mut Vec<Event>,
    last_stamp: &mut Option<ControlStamp>,
) {
    let SceneEvent::Zone { event, stamp } = event else {
        unreachable!()
    };
    *last_stamp = Some(*stamp);
    out.push(zone_event_to_log(event, *stamp));
}

fn append_entity_boxes_scene_event(
    event: &SceneEvent,
    out: &mut Vec<Event>,
    last_stamp: Option<ControlStamp>,
) {
    let SceneEvent::EntityBoxes(tracks) = event else {
        unreachable!()
    };
    let Some(stamp) = last_stamp else {
        return;
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

fn append_fsm_transition_scene_event(event: &SceneEvent, out: &mut Vec<Event>) {
    let SceneEvent::FsmTransition(tr) = event else {
        unreachable!()
    };
    out.push(Event::fsm_transition(
        &tr.from,
        tr.from_label.as_deref(),
        &tr.to,
        tr.to_label.as_deref(),
        &tr.trigger,
        tr.dwell_ms,
    ));
}

fn append_health_scene_event(event: &SceneEvent, out: &mut Vec<Event>) {
    let SceneEvent::Health(transition) = event else {
        unreachable!()
    };
    match transition {
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
    }
}
