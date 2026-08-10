//! Scene signal vocabulary: catalog, typed values, and per-cycle table.
//!
//! Stage A of scene-signals: the contract exists without producers or
//! consumers outside this module and its tests. See
//! `docs/scene-signals/design.md`.

mod catalog;
mod table;
mod value;

pub use catalog::{SignalCatalog, SignalDescriptor, SignalPresence, scene_signal_catalog};
pub use table::{SceneSignalsSnapshot, SignalTable, SignalTableError};
pub use value::{
    CompareError, OpCompatibilityError, Ratio, RatioError, SignalKind, SignalOp, SignalValue,
};
