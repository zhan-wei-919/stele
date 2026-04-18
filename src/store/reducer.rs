//! Reducer that updates store-owned model state from incoming actions.

use log::warn;

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
                event_time,
            } => {
                if *scale_factor <= 0.0 {
                    debug_assert!(
                        *scale_factor > 0.0,
                        "viewport scale factor must stay positive"
                    );
                    warn!(
                        "store.invalid_scale_factor scale_factor={} viewport_revision={}",
                        scale_factor, viewport_revision
                    );
                    return ReduceOutcome::NoChange;
                }
                *viewport = ViewportState::new(
                    *width,
                    *height,
                    *scale_factor,
                    *viewport_revision,
                    Some(*event_time),
                );
                delegate.resize(model, viewport.logical_size());
                ReduceOutcome::Changed
            }
        }
    }
}
