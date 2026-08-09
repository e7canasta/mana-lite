//! Owned depth frame wrapper that hides `ultralytics_inference::DepthMap`
//! from most of the pipeline.

use ultralytics_inference::DepthMap;
use ultralytics_inference::visualizer::color::{Colormap, DepthViz};

/// Depth map owned by the pipeline, with a narrow public surface.
#[derive(Clone)]
pub struct DepthFrame {
    inner: DepthMap,
}

impl DepthFrame {
    #[must_use]
    pub fn from_ultralytics(map: DepthMap) -> Self {
        Self { inner: map }
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.dims().0
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.dims().1
    }

    /// Map dimensions `(width, height)` in local ROI coordinates.
    #[must_use]
    pub fn dims(&self) -> (u32, u32) {
        let shape = self.inner.data.shape();
        #[allow(clippy::cast_possible_truncation)]
        (shape[1] as u32, shape[0] as u32)
    }

    #[must_use]
    pub fn value_at(&self, y: usize, x: usize) -> f32 {
        self.inner.data[[y, x]]
    }

    pub fn iter_values(&self) -> impl Iterator<Item = f32> + '_ {
        self.inner.data.iter().copied()
    }

    #[must_use]
    pub fn colorize(&self, colormap: Colormap, viz: DepthViz) -> Vec<[u8; 3]> {
        self.inner.colorize(colormap, viz)
    }

    /// Temporary escape hatch while stages migrate off the raw type.
    #[must_use]
    pub fn as_ultralytics(&self) -> &DepthMap {
        &self.inner
    }

    #[must_use]
    pub fn into_ultralytics(self) -> DepthMap {
        self.inner
    }
}

impl std::fmt::Debug for DepthFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (w, h) = self.dims();
        f.debug_struct("DepthFrame")
            .field("width", &w)
            .field("height", &h)
            .field("len", &self.inner.data.len())
            .finish()
    }
}
