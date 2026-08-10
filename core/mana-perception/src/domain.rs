//! Perception-owned vocabulary built on `mana-id` (ADR-030).

pub use mana_id::DomStr;
pub use mana_id::domain_id;

domain_id!(ModelId, "Catalog key for an inference model.");
domain_id!(ClassName, "Detection class label (person, face, …).");
