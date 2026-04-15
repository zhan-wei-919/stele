//! Application-supplied hooks that feed the generic store with model state.

use crate::font::FreeTypeRasterizer;

use super::model::{Model, StoreBootstrap};

/// Boundary interface implemented by application code that wants to drive the store.
pub(crate) trait StoreDelegate: Send + Sync {
    /// Builds the initial store state for the first viewport.
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        logical_viewport: [f32; 2],
    ) -> StoreBootstrap;

    /// Updates the model for a new logical viewport.
    fn resize(&self, model: &mut Model, logical_viewport: [f32; 2]);
}
