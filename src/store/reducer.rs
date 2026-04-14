//! Reducer that updates store-owned model state from incoming actions.

use crate::demo::resize_demo_document;
use crate::io::Action;

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
                resize_demo_document(model.document_mut(), logical_viewport(*viewport));
                ReduceOutcome::Changed
            }
        }
    }
}

/// Converts the current physical viewport into logical layout dimensions.
pub(crate) fn logical_viewport(viewport: ViewportState) -> [f32; 2] {
    debug_assert!(
        viewport.scale_factor > 0.0,
        "viewport scale factor must stay positive"
    );
    [
        viewport.width as f32 / viewport.scale_factor,
        viewport.height as f32 / viewport.scale_factor,
    ]
}
