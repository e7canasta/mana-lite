//! Control-owned vocabulary built on `mana-id` (ADR-030).
//!
//! Shared mechanism lives in `mana-id`. This crate owns `StateId`, `ZoneId`,
//! `ClassName`, `ModelId` (process-image port), and `LoopId`.

pub use mana_id::DomStr;
pub use mana_id::domain_id;

domain_id!(StateId, "FSM state identifier from the catalog.");
domain_id!(ZoneId, "Spatial zone identifier from the catalog.");
domain_id!(ClassName, "Detection class label on the control port.");
domain_id!(ModelId, "Model catalog key on the control port.");
domain_id!(
    LoopId,
    "Control-loop identity for multi-stream portability."
);

impl StateId {
    /// Structural safe state used after panics / data loss.
    pub const BLIND: &'static str = "blind";
}

impl LoopId {
    /// Single-loop identity used by mana-lite (N=1).
    pub const DEFAULT: &'static str = "default";

    #[must_use]
    pub fn default_loop() -> Self {
        Self::new(Self::DEFAULT)
    }
}
