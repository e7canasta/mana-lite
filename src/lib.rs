//! Librería compartida por el binario `mana-lite`.
//!
//! La frontera crate es de compilación (tests de integración, reuso entre
//! bins), no de despliegue: el artefacto desplegado sigue siendo un único
//! binario (ADR-001).

pub mod app;
pub use mana_control::assignment;
// FIXME(ADR-027): cascade.rs lives under mana-perception but is compiled as a
// binary module via #[path], so its `crate::track` / `crate::kalman` imports
// resolve against mana-control re-exports. The T1→T2 violation survived the
// migration as a path hack rather than a Cargo.toml edge.
#[path = "../core/mana-perception/src/cascade.rs"]
pub mod cascade;
pub mod config;
pub mod depth;
pub use mana_perception::depth_map;
pub use mana_perception::detection;
pub mod domain;
pub mod error;
pub mod face_dwell;
pub use mana_control::fsm;
pub use mana_control::health;
pub mod infer;
pub mod ingest;
pub use mana_control::kalman;
pub mod logger;
pub mod metrics;
pub mod model_runner;
pub use mana_control::occupancy;
pub mod pipeline;
pub use mana_control::presence;
pub use mana_control::scan;
pub mod snapshot;
pub use mana_control::timing;
pub use mana_control::track;
pub mod viz;
pub use mana_control::window;
pub use mana_control::zones;

pub use app::App;
pub use error::{ManaError, Result};
