//! Fan-out of per-cycle pipeline observations to sinks (viz, JSONL, null).

use crate::logger::{Event, LogSink};
use crate::occupancy::{RoomCardinality, SecondPersonState, SignalValidity};
#[cfg(feature = "rerun")]
use crate::viz::VizBridge;

/// Decouples stage code from concrete observability backends so stages can
/// be tested with [`NullObserver`].
pub trait PipelineObserver {
    fn on_occupancy(
        &mut self,
        state: RoomCardinality,
        second_person: SecondPersonState,
        signal: SignalValidity,
    );

    fn emit(&mut self, event: Event);

    fn flush(&mut self);

    #[cfg(feature = "rerun")]
    fn viz_mut(&mut self) -> Option<&mut VizBridge> {
        None
    }

    fn log_mut(&mut self) -> Option<&mut dyn LogSink> {
        None
    }
}

/// No-op observer for tests that exercise stages without Rerun or disk I/O.
#[derive(Debug, Default)]
pub struct NullObserver;

impl PipelineObserver for NullObserver {
    fn on_occupancy(
        &mut self,
        _state: RoomCardinality,
        _second_person: SecondPersonState,
        _signal: SignalValidity,
    ) {
    }

    fn emit(&mut self, _event: Event) {}

    fn flush(&mut self) {}
}

/// Combined Rerun + JSONL observer used by the production binary.
pub struct FanoutObserver {
    #[cfg(feature = "rerun")]
    pub viz: VizBridge,
    pub log: Box<dyn LogSink>,
}

impl FanoutObserver {
    #[cfg(feature = "rerun")]
    pub fn new(viz: VizBridge, log: Box<dyn LogSink>) -> Self {
        Self { viz, log }
    }

    #[cfg(not(feature = "rerun"))]
    pub fn new(log: Box<dyn LogSink>) -> Self {
        Self { log }
    }
}

impl PipelineObserver for FanoutObserver {
    fn on_occupancy(
        &mut self,
        state: RoomCardinality,
        second_person: SecondPersonState,
        signal: SignalValidity,
    ) {
        #[cfg(feature = "rerun")]
        self.viz.log_occupancy_state(state, second_person, signal);
        #[cfg(not(feature = "rerun"))]
        let _ = (state, second_person, signal);
    }

    fn emit(&mut self, event: Event) {
        self.log.emit(event);
    }

    fn flush(&mut self) {
        self.log.flush();
        #[cfg(feature = "rerun")]
        self.viz.tick();
    }

    #[cfg(feature = "rerun")]
    fn viz_mut(&mut self) -> Option<&mut VizBridge> {
        Some(&mut self.viz)
    }

    fn log_mut(&mut self) -> Option<&mut dyn LogSink> {
        Some(self.log.as_mut())
    }
}
