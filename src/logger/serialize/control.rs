use super::Event;
use super::writers::{write_control_stamp, write_f32, write_json_string, write_u64};
use crate::logger::event::FaceDwellTimerRecord;
use mana_control::signals::{SignalKind, SignalValue, scene_signal_catalog};

pub(super) fn write_entity_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Entity {
        track_id,
        class,
        bbox,
        sources,
        scan_seq,
        evidence_frame_id,
        observations_age_ms,
        depth_age_ms,
    } = event
    else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"entity\",\"track_id\":");
    write_u64(*track_id, buf);
    buf.extend_from_slice(b",\"class\":\"");
    write_json_string(class, buf);
    buf.extend_from_slice(b"\",\"bbox\":[");
    for (i, value) in bbox.iter().enumerate() {
        if i > 0 {
            buf.push(b',');
        }
        write_f32(*value, buf);
    }
    buf.extend_from_slice(b"],\"sources\":[");
    for (i, source) in sources.iter().enumerate() {
        if i > 0 {
            buf.push(b',');
        }
        buf.push(b'\"');
        write_json_string(source, buf);
        buf.push(b'\"');
    }
    buf.extend_from_slice(b"]");
    write_control_stamp(
        *scan_seq,
        *evidence_frame_id,
        *observations_age_ms,
        *depth_age_ms,
        buf,
    );
}

pub(super) fn write_zone_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Zone {
        zone,
        event,
        class,
        label,
        confidence,
        scan_seq,
        evidence_frame_id,
        observations_age_ms,
        depth_age_ms,
    } = event
    else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"zone\",\"zone\":\"");
    write_json_string(zone, buf);
    buf.extend_from_slice(b"\",\"event\":\"");
    write_json_string(event, buf);
    buf.extend_from_slice(b"\",\"class\":\"");
    write_json_string(class, buf);
    buf.extend_from_slice(b"\"");
    if let Some(l) = label {
        buf.extend_from_slice(b",\"label\":\"");
        write_json_string(l, buf);
        buf.extend_from_slice(b"\"");
    }
    if let Some(c) = confidence {
        buf.extend_from_slice(b",\"confidence\":");
        write_f32(*c, buf);
    }
    write_control_stamp(
        *scan_seq,
        *evidence_frame_id,
        *observations_age_ms,
        *depth_age_ms,
        buf,
    );
}

pub(super) fn write_fsm_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Fsm {
        from,
        from_label,
        to,
        to_label,
        trigger,
        dwell_ms,
    } = event
    else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"fsm\",\"from\":\"");
    write_json_string(from, buf);
    buf.extend_from_slice(b"\"");
    if let Some(l) = from_label {
        buf.extend_from_slice(b",\"from_label\":\"");
        write_json_string(l, buf);
        buf.extend_from_slice(b"\"");
    }
    buf.extend_from_slice(b",\"to\":\"");
    write_json_string(to, buf);
    buf.extend_from_slice(b"\"");
    if let Some(l) = to_label {
        buf.extend_from_slice(b",\"to_label\":\"");
        write_json_string(l, buf);
        buf.extend_from_slice(b"\"");
    }
    buf.extend_from_slice(b",\"trigger\":\"");
    write_json_string(trigger, buf);
    buf.extend_from_slice(b"\",\"dwell_ms\":");
    write_u64(*dwell_ms, buf);
}

pub(super) fn write_presence_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::Presence {
        scan_seq,
        evidence_frame_id,
        observations_age_ms,
        depth_age_ms,
        state,
        poi_state,
        second_person,
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
    } = event
    else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"presence\"");
    write_control_stamp(
        *scan_seq,
        *evidence_frame_id,
        *observations_age_ms,
        *depth_age_ms,
        buf,
    );
    buf.extend_from_slice(b",\"state\":\"");
    write_json_string(state, buf);
    buf.extend_from_slice(b"\",\"poi_state\":\"");
    write_json_string(poi_state, buf);
    buf.extend_from_slice(b"\",\"second_person\":\"");
    write_json_string(second_person, buf);
    buf.extend_from_slice(b"\",\"raw_count\":");
    write_u64(*raw_count as u64, buf);
    buf.extend_from_slice(b",\"confirmed_count\":");
    write_u64(*confirmed_count as u64, buf);
    buf.extend_from_slice(b",\"signal_valid\":");
    buf.extend_from_slice(if *signal_valid { b"true" } else { b"false" });
    buf.extend_from_slice(b",\"held\":");
    buf.extend_from_slice(if *held { b"true" } else { b"false" });
    buf.extend_from_slice(b",\"poi_positive_ms\":");
    write_u64(*poi_positive_ms, buf);
    buf.extend_from_slice(b",\"poi_empty_ms\":");
    write_u64(*poi_empty_ms, buf);
    buf.extend_from_slice(b",\"single_timer_ms\":");
    write_u64(*single_timer_ms, buf);
    buf.extend_from_slice(b",\"empty_timer_ms\":");
    write_u64(*empty_timer_ms, buf);
    buf.extend_from_slice(b",\"multiple_candidate_timer_ms\":");
    write_u64(*multiple_candidate_timer_ms, buf);
    buf.extend_from_slice(b",\"multiple_exit_timer_ms\":");
    write_u64(*multiple_exit_timer_ms, buf);
}

pub(super) fn write_face_dwell_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::FaceDwell {
        scan_seq,
        evidence_frame_id,
        observations_age_ms,
        depth_age_ms,
        source,
        state,
        state_label,
        state_dwell_ms,
        state_dwell_required_ms,
        cardinality,
        person_present,
        face_present,
        face_confidence,
        face_in_dwell,
        at_edge,
        face_was_inside,
        face_model_ran,
        active_timers,
    } = event
    else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"face_dwell\"");
    write_control_stamp(
        *scan_seq,
        *evidence_frame_id,
        *observations_age_ms,
        *depth_age_ms,
        buf,
    );
    buf.extend_from_slice(b",\"source\":\"");
    write_json_string(source, buf);
    write_face_dwell_state(
        state,
        state_label.as_deref(),
        *state_dwell_ms,
        *state_dwell_required_ms,
        cardinality.as_deref(),
        *person_present,
        *face_present,
        *face_confidence,
        *face_in_dwell,
        *at_edge,
        *face_was_inside,
        *face_model_ran,
        buf,
    );
    write_face_dwell_timers(active_timers, buf);
}

pub(super) fn write_scene_signals_event(event: &Event, buf: &mut Vec<u8>) {
    let Event::SceneSignals { stamp, snapshot } = event else {
        unreachable!()
    };
    buf.extend_from_slice(b"\"type\":\"scene_signals\"");
    write_control_stamp(
        stamp.scan_seq,
        stamp.evidence_frame_id,
        stamp.observations_age_ms,
        stamp.depth_age_ms,
        buf,
    );
    buf.extend_from_slice(b",\"catalog_version\":");
    write_u64(snapshot.catalog_version() as u64, buf);
    buf.extend_from_slice(b",\"signals\":[");
    for (index, (tag, value)) in snapshot.iter().enumerate() {
        if index > 0 {
            buf.push(b',');
        }
        buf.extend_from_slice(b"{\"tag\":\"");
        write_json_string(tag.as_str(), buf);
        buf.extend_from_slice(b"\",\"kind\":\"");
        write_signal_kind(
            scene_signal_catalog()
                .get(tag)
                .expect("snapshot tag must be declared in catalog")
                .kind(),
            buf,
        );
        buf.push(b'\"');
        match value {
            Some(SignalValue::Bool(value)) => {
                buf.extend_from_slice(b",\"value\":");
                buf.extend_from_slice(if *value { b"true" } else { b"false" });
            }
            Some(SignalValue::Count(value)) => {
                buf.extend_from_slice(b",\"value\":");
                write_u64(*value, buf);
            }
            Some(SignalValue::Ratio(value)) => {
                buf.extend_from_slice(b",\"value\":");
                write_f32(value.get(), buf);
            }
            Some(SignalValue::Label(value)) => {
                buf.extend_from_slice(b",\"value\":\"");
                write_json_string(value, buf);
                buf.push(b'\"');
            }
            None => buf.extend_from_slice(b",\"absent\":true"),
        }
        buf.push(b'}');
    }
    buf.push(b']');
}

fn write_signal_kind(kind: SignalKind, buf: &mut Vec<u8>) {
    buf.extend_from_slice(match kind {
        SignalKind::Bool => b"bool",
        SignalKind::Count => b"count",
        SignalKind::Ratio => b"ratio",
        SignalKind::Label => b"label",
    });
}

pub(super) fn write_face_dwell_state(
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
    buf: &mut Vec<u8>,
) {
    buf.extend_from_slice(b"\",\"state\":\"");
    write_json_string(state, buf);
    buf.extend_from_slice(b"\"");
    if let Some(label) = state_label {
        buf.extend_from_slice(b",\"state_label\":\"");
        write_json_string(label, buf);
        buf.extend_from_slice(b"\"");
    }
    buf.extend_from_slice(b",\"state_dwell_ms\":");
    write_u64(state_dwell_ms, buf);
    if let Some(required) = state_dwell_required_ms {
        buf.extend_from_slice(b",\"state_dwell_required_ms\":");
        write_u64(required, buf);
    }
    if let Some(card) = cardinality {
        buf.extend_from_slice(b",\"cardinality\":\"");
        write_json_string(card, buf);
        buf.extend_from_slice(b"\"");
    }
    buf.extend_from_slice(b",\"person_present\":");
    buf.extend_from_slice(if person_present { b"true" } else { b"false" });
    buf.extend_from_slice(b",\"face_present\":");
    buf.extend_from_slice(if face_present { b"true" } else { b"false" });
    if let Some(conf) = face_confidence {
        buf.extend_from_slice(b",\"face_confidence\":");
        write_f32(conf, buf);
    }
    if let Some(in_dwell) = face_in_dwell {
        buf.extend_from_slice(b",\"face_in_dwell\":");
        buf.extend_from_slice(if in_dwell { b"true" } else { b"false" });
    }
    buf.extend_from_slice(b",\"at_edge\":");
    buf.extend_from_slice(if at_edge { b"true" } else { b"false" });
    buf.extend_from_slice(b",\"face_was_inside\":");
    buf.extend_from_slice(if face_was_inside { b"true" } else { b"false" });
    buf.extend_from_slice(b",\"face_model_ran\":");
    buf.extend_from_slice(if face_model_ran { b"true" } else { b"false" });
}

pub(super) fn write_face_dwell_timers(active_timers: &[FaceDwellTimerRecord], buf: &mut Vec<u8>) {
    buf.extend_from_slice(b",\"active_timers\":[");
    for (i, timer) in active_timers.iter().enumerate() {
        if i > 0 {
            buf.push(b',');
        }
        buf.extend_from_slice(b"{\"trigger\":\"");
        write_json_string(&timer.trigger, buf);
        buf.extend_from_slice(b"\",\"elapsed_ms\":");
        write_u64(timer.elapsed_ms, buf);
        buf.extend_from_slice(b",\"required_ms\":");
        write_u64(timer.required_ms, buf);
        buf.push(b'}');
    }
    buf.push(b']');
}
