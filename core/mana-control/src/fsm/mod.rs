//! Finite state machine engine for clinical scene logic.

mod engine;
mod guard;
mod program;

pub use engine::{
    FsmDwellTimerSnapshot, FsmEngine, FsmSceneContext, FsmSnapshot, FsmTransitionResult,
};
pub use guard::{FsmGuard, GuardCtx};
pub use program::FsmProgram;

#[cfg(test)]
mod tests;
