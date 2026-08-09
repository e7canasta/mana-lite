use crate::fsm::{FsmSceneContext, FsmSnapshot};
use crate::logger::{Event, FaceDwellTimerRecord};
use crate::scan::ControlStamp;

/// Application strategy that publishes the facial state machine as diagnostic
/// evidence without owning or duplicating its transition logic.
#[derive(Debug, Clone, Copy, Default)]
pub struct FaceDwellLogStrategy;

impl FaceDwellLogStrategy {
    pub fn keyframe_event(
        &self,
        stamp: ControlStamp,
        context: &FsmSceneContext,
        snapshot: &FsmSnapshot,
    ) -> Event {
        self.event(stamp, "keyframe", context, snapshot)
    }

    pub fn wildcard_event(
        &self,
        stamp: ControlStamp,
        context: &FsmSceneContext,
        snapshot: &FsmSnapshot,
    ) -> Event {
        self.event(stamp, "wildcard", context, snapshot)
    }

    fn event(
        &self,
        stamp: ControlStamp,
        source: &str,
        context: &FsmSceneContext,
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
            context.cardinality.as_deref(),
            context.person_present,
            context.face_present,
            context.face_confidence,
            context.face_in_dwell,
            context.at_edge,
            snapshot.face_was_inside,
            context.face_model_ran,
            active_timers,
        )
    }
}
