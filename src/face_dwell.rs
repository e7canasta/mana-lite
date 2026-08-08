use crate::fsm::{FsmSceneContext, FsmSnapshot};
use crate::logger::{Event, FaceDwellTimerRecord, LogSink};

/// Application strategy that publishes the facial state machine as diagnostic
/// evidence without owning or duplicating its transition logic.
#[derive(Debug, Clone, Copy, Default)]
pub struct FaceDwellLogStrategy;

impl FaceDwellLogStrategy {
    pub fn log_keyframe(
        &self,
        log: &mut dyn LogSink,
        frame_id: u64,
        context: &FsmSceneContext,
        snapshot: &FsmSnapshot,
    ) {
        self.log(log, frame_id, "keyframe", context, snapshot);
    }

    pub fn log_wildcard(
        &self,
        log: &mut dyn LogSink,
        frame_id: u64,
        context: &FsmSceneContext,
        snapshot: &FsmSnapshot,
    ) {
        self.log(log, frame_id, "wildcard", context, snapshot);
    }

    fn log(
        &self,
        log: &mut dyn LogSink,
        frame_id: u64,
        source: &str,
        context: &FsmSceneContext,
        snapshot: &FsmSnapshot,
    ) {
        let active_timers = snapshot
            .active_timers
            .iter()
            .map(|timer| FaceDwellTimerRecord {
                trigger: timer.trigger.clone(),
                elapsed_ms: timer.elapsed_ms,
                required_ms: timer.required_ms,
            })
            .collect();

        log.emit(Event::face_dwell(
            frame_id,
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
        ));
    }
}
