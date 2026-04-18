//! Store-owned logical model, block draw commands, and prepare-stage cache.

use crate::layout::prepare_tree::PreparedTree;
use crate::layout::tree::DocumentTree;

/// Bootstrap payload supplied by the application boundary.
pub(crate) struct StoreBootstrap {
    model: Model,
    layout_cache: LayoutCache,
}

impl StoreBootstrap {
    /// Creates a store bootstrap payload from a semantic document tree and prepared cache.
    pub(crate) fn new(document: DocumentTree, prepared_tree: PreparedTree) -> Self {
        Self {
            model: Model::new(document),
            layout_cache: LayoutCache::new(prepared_tree),
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
}

impl Model {
    /// Creates the model from a rich-text tree.
    pub(crate) fn new(document: DocumentTree) -> Self {
        Self { document }
    }

    /// Returns the current semantic document.
    pub(crate) fn document(&self) -> &DocumentTree {
        &self.document
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
}
