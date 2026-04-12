//! Store-owned logical model and prepare-stage cache.

use crate::demo::DemoState;
use crate::layout::{Document, PreparedBlock};

/// Store-owned logical document model.
pub(crate) struct Model {
    document: Document,
}

impl Model {
    /// Builds the model and its prepare cache from the demo bootstrap state.
    pub(crate) fn from_demo_state(state: DemoState) -> (Self, LayoutCache) {
        let (document, prepared_blocks) = state.into_parts();
        (Self { document }, LayoutCache { prepared_blocks })
    }

    /// Returns the current logical document.
    pub(crate) fn document(&self) -> &Document {
        &self.document
    }

    /// Returns mutable access to the logical document.
    pub(crate) fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }
}

/// Prepared layout cache reused across reflow-only updates.
pub(crate) struct LayoutCache {
    prepared_blocks: Vec<PreparedBlock>,
}

impl LayoutCache {
    /// Returns the prepared blocks consumed by the layout stage.
    pub(crate) fn prepared_blocks(&self) -> &[PreparedBlock] {
        &self.prepared_blocks
    }
}
