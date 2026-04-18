//! Application-supplied hooks that feed the generic store with model state.

use crate::font::FreeTypeRasterizer;
use crate::io::InputEvent;

use super::model::{Model, StoreBootstrap};
use super::types::{InputFilter, InteractionConfig, InteractionState};

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

    /// Returns the scroll configuration snapshot used by the store.
    fn interaction_config(&self) -> InteractionConfig {
        InteractionConfig::default()
    }

    /// Optionally vetoes the default store-side input handling for one event.
    fn filter_input(&self, _state: &InteractionState, _event: &InputEvent) -> InputFilter {
        InputFilter::RunDefault
    }
}
