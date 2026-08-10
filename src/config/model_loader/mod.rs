mod load;
mod overlay;
mod patch;

pub use load::load_model_catalog;
pub use overlay::apply_model_overlay;

#[cfg(test)]
use patch::{ModelPatch, PostprocessPatch};

#[cfg(test)]
mod tests;
