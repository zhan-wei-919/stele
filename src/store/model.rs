//! Store-owned logical model, block draw commands, and prepare-stage cache.

use crate::font::FreeTypeRasterizer;
use crate::layout::prepare_tree::{prepare_tree_with_text_inputs, PreparedTree, TextInputResolver};
use crate::layout::tree::DocumentTree;

use super::text_input::TextInputStates;

/// Bootstrap payload supplied by the application boundary.
pub(crate) struct StoreBootstrap {
    model: Model,
    layout_cache: LayoutCache,
}

impl StoreBootstrap {
    /// Creates a store bootstrap payload from a semantic document tree.
    pub(crate) fn new(document: DocumentTree, rasterizer: &FreeTypeRasterizer) -> Self {
        let model = Model::new(document);
        let layout_cache = LayoutCache::from_model(&model, rasterizer);
        Self {
            model,
            layout_cache,
        }
    }

    /// Splits the bootstrap payload into model and layout cache ownership.
    pub(crate) fn into_parts(self) -> (Model, LayoutCache) {
        (self.model, self.layout_cache)
    }
}

/// Store-owned logical document model.
pub(crate) struct Model {
    document: DocumentTree,
    text_inputs: TextInputStates,
}

impl Model {
    /// Creates the model from a rich-text tree.
    pub(crate) fn new(document: DocumentTree) -> Self {
        let text_inputs = TextInputStates::from_document(&document);
        Self {
            document,
            text_inputs,
        }
    }

    /// Returns the current semantic document.
    pub(crate) fn document(&self) -> &DocumentTree {
        &self.document
    }

    /// Returns the editable text input registry.
    pub(crate) fn text_inputs(&self) -> &TextInputStates {
        &self.text_inputs
    }

    /// Returns mutable access to the editable text input registry.
    pub(crate) fn text_inputs_mut(&mut self) -> &mut TextInputStates {
        &mut self.text_inputs
    }
}

impl TextInputResolver for Model {
    fn resolve_text_input(
        &self,
        text_input: crate::layout::tree::TextInputId,
    ) -> Option<crate::layout::prepare_tree::TextInputValue<'_>> {
        self.text_inputs.prepare_value(text_input)
    }
}

/// Prepared layout cache reused across reflow-only updates.
pub(crate) struct LayoutCache {
    prepared: PreparedTree,
}

impl LayoutCache {
    /// Creates the prepared layout cache once for later layout-only updates.
    pub(crate) fn new(prepared_tree: PreparedTree) -> Self {
        Self {
            prepared: prepared_tree,
        }
    }

    /// Returns the owned prepare cache representation.
    pub(crate) fn prepared(&self) -> &PreparedTree {
        &self.prepared
    }

    /// Prepares a fresh cache from the current model text input state.
    pub(crate) fn from_model(model: &Model, rasterizer: &FreeTypeRasterizer) -> Self {
        Self::new(prepare_tree_with_text_inputs(
            model.document(),
            rasterizer,
            model,
        ))
    }

    /// Rebuilds cold-path layout data after model text changes.
    pub(crate) fn rebuild_from_model(&mut self, model: &Model, rasterizer: &FreeTypeRasterizer) {
        self.prepared = prepare_tree_with_text_inputs(model.document(), rasterizer, model);
    }
}
