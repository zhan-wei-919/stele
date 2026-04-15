//! Reducer that updates store-owned model state from incoming actions.

use crate::io::Action;

use super::delegate::StoreDelegate;
use super::model::Model;
use super::types::ViewportState;

/// Result of applying one action to the store.
pub(crate) enum ReduceOutcome {
    NoChange,
    Changed,
    Shutdown,
}

/// Applies actions to the logical model and viewport state.
pub(crate) struct Reducer;

impl Reducer {
    /// Applies one action to the current store state.
    pub(crate) fn apply(
        &self,
        model: &mut Model,
        viewport: &mut ViewportState,
        action: &Action,
        delegate: &dyn StoreDelegate,
    ) -> ReduceOutcome {
        match action {
            Action::Shutdown => ReduceOutcome::Shutdown,
            Action::Input { .. } => ReduceOutcome::NoChange,
            Action::Resize {
                width,
                height,
                scale_factor,
                viewport_revision,
            } => {
                *viewport = ViewportState::new(*width, *height, *scale_factor, *viewport_revision);
                delegate.resize(model, viewport.logical_size());
                ReduceOutcome::Changed
            }
        }
    }
}
