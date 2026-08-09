//! Librería compartida por el binario `mana-lite`.
//!
//! La frontera crate es de compilación (tests de integración, reuso entre
//! bins), no de despliegue: el artefacto desplegado sigue siendo un único
//! binario (ADR-001).

pub mod app;
pub mod assignment;
pub mod cascade;
pub mod config;
pub mod depth;
pub mod depth_map;
pub mod detection;
pub mod domain;
pub mod error;
pub mod face_dwell;
pub mod fsm;
pub mod infer;
pub mod ingest;
pub mod kalman;
pub mod logger;
pub mod metrics;
pub mod model_runner;
pub mod occupancy;
pub mod pipeline;
pub mod presence;
pub mod scan;
pub mod snapshot;
pub mod timing;
pub mod track;
pub mod viz;
pub mod window;
pub mod zones;

pub use app::App;
pub use error::{ManaError, Result};
