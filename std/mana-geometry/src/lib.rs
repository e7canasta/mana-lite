//! mana-geometry — Spatial Geometry Commons
//! =======================================================
//! CompactMask (crop-RLE mask storage), polygon geometry, geometric
//! primitives and transforms.
//!
//! Imported from mana-os std (`crates/std/mana-geometry` + polygonization
//! from `crates/std/mana-annotate/src/geom`). See
//! `docs/adrs/019-import-mana-os-std.md`.
#![forbid(unsafe_code)]

extern crate alloc;

pub mod bbox;
pub mod compact_mask;
pub mod iou;
pub mod polygon;
#[cfg(feature = "polygonize")]
pub mod polygonize;
pub mod transform;
