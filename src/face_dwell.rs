use crate::fsm::FsmSnapshot;
use crate::logger::{Event, FaceDwellTimerRecord};
use crate::scan::ControlStamp;
use mana_control::domain::SignalTag;
use mana_control::signals::{SceneSignalsSnapshot, SignalValue};

/// Application strategy that publishes the facial state machine as diagnostic
/// evidence without owning or duplicating its transition logic.
#[derive(Debug, Clone, Copy, Default)]
pub struct FaceDwellLogStrategy;

impl FaceDwellLogStrategy {
    pub fn keyframe_event(
        &self,
        stamp: ControlStamp,
        signals: &SceneSignalsSnapshot,
        snapshot: &FsmSnapshot,
    ) -> Event {
        self.event(stamp, "keyframe", signals, snapshot)
    }

    pub fn wildcard_event(
        &self,
        stamp: ControlStamp,
        signals: &SceneSignalsSnapshot,
        snapshot: &FsmSnapshot,
    ) -> Event {
        self.event(stamp, "wildcard", signals, snapshot)
    }

    fn event(
        &self,
        stamp: ControlStamp,
        source: &str,
        signals: &SceneSignalsSnapshot,
        snapshot: &FsmSnapshot,
    ) -> Event {
        let active_timers = snapshot
            .active_timers
            .iter()
            .map(|timer| FaceDwellTimerRecord {
                trigger: timer.trigger.clone(),
                elapsed_ms: timer.elapsed_ms,
                required_ms: timer.required_ms,
            })
            .collect();

        Event::face_dwell(
            stamp,
            source,
            &snapshot.state,
            snapshot.state_label.as_deref(),
            snapshot.state_dwell_ms,
            snapshot.state_dwell_required_ms,
            signal_label(signals, "ocupacion.cardinalidad"),
            signal_bool(signals, "persona.presente"),
            signal_bool(signals, "cara.presente"),
            signal_ratio(signals, "cara.confianza"),
            signal_optional_bool(signals, "cara.en_dwell"),
            signal_bool(signals, "cara.en_borde"),
            signal_bool(signals, "cara.estuvo_dentro"),
            signal_bool(signals, "cara.modelo_corrio"),
            active_timers,
        )
    }
}

fn value<'a>(signals: &'a SceneSignalsSnapshot, tag: &str) -> Option<&'a SignalValue> {
    signals.get(&SignalTag::new(tag))
}

fn signal_bool(signals: &SceneSignalsSnapshot, tag: &str) -> bool {
    matches!(value(signals, tag), Some(SignalValue::Bool(value)) if *value)
}

fn signal_optional_bool(signals: &SceneSignalsSnapshot, tag: &str) -> Option<bool> {
    match value(signals, tag) {
        Some(SignalValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn signal_ratio(signals: &SceneSignalsSnapshot, tag: &str) -> Option<f32> {
    match value(signals, tag) {
        Some(SignalValue::Ratio(value)) => Some(value.get()),
        _ => None,
    }
}

fn signal_label<'a>(signals: &'a SceneSignalsSnapshot, tag: &str) -> Option<&'a str> {
    match value(signals, tag) {
        Some(SignalValue::Label(value)) => Some(value.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mana_control::signals::{SignalTable, scene_signal_catalog};

    #[test]
    fn strategy_reads_scene_evidence_from_snapshot() {
        let catalog = scene_signal_catalog();
        let mut table = SignalTable::new();
        for (tag, value) in [
            ("persona.presente", SignalValue::Bool(true)),
            ("cara.presente", SignalValue::Bool(true)),
            ("cara.en_dwell", SignalValue::Bool(false)),
            ("cara.en_borde", SignalValue::Bool(true)),
            ("cara.modelo_corrio", SignalValue::Bool(true)),
            ("cara.estuvo_dentro", SignalValue::Bool(true)),
            (
                "cara.confianza",
                SignalValue::Ratio(mana_control::signals::Ratio::new(0.87).unwrap()),
            ),
            (
                "ocupacion.cardinalidad",
                SignalValue::Label("single".into()),
            ),
        ] {
            table.insert(catalog, SignalTag::new(tag), value).unwrap();
        }
        let signals = table.snapshot(catalog);
        let fsm = FsmSnapshot {
            state: "watching".into(),
            state_label: None,
            state_dwell_ms: 200,
            state_dwell_required_ms: None,
            face_was_inside: false,
            active_timers: Vec::new(),
        };

        let event = FaceDwellLogStrategy.keyframe_event(
            ControlStamp {
                scan_seq: 4,
                evidence_frame_id: 9,
                observations_age_ms: 2,
                depth_age_ms: None,
            },
            &signals,
            &fsm,
        );
        let Event::FaceDwell {
            cardinality,
            person_present,
            face_present,
            face_confidence,
            face_in_dwell,
            at_edge,
            face_was_inside,
            face_model_ran,
            ..
        } = event
        else {
            panic!("expected FaceDwell event")
        };
        assert_eq!(cardinality.as_deref(), Some("single"));
        assert!(person_present);
        assert!(face_present);
        assert_eq!(face_confidence, Some(0.87));
        assert_eq!(face_in_dwell, Some(false));
        assert!(at_edge);
        assert!(face_was_inside);
        assert!(face_model_ran);
    }
}
