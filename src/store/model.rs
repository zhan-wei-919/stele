//! Store-owned logical model, block draw commands, and prepare-stage cache.

use std::collections::HashMap;

use crate::draw_list::{ImageCmd, PathCmd};
use crate::layout::prepare_tree::PreparedTree;
use crate::layout::tree::DocumentTree;
use crate::layout::{Document, PreparedBlock};
use crate::scene::BlockId;

/// Bootstrap payload supplied by the application boundary.
pub(crate) struct StoreBootstrap {
    model: Model,
    layout_cache: LayoutCache,
}

impl StoreBootstrap {
    /// Creates a store bootstrap payload from a document, cached prepare output, and block draws.
    pub(crate) fn new(
        document: Document,
        prepared_blocks: Vec<PreparedBlock>,
        block_draw_commands: BlockDrawCommands,
    ) -> Self {
        Self {
            model: Model::new(document, block_draw_commands),
            layout_cache: LayoutCache::new(prepared_blocks),
        }
    }

    /// Creates a tree-path bootstrap payload from a semantic document tree and prepared cache.
    pub(crate) fn new_tree(document: DocumentTree, prepared_tree: PreparedTree) -> Self {
        Self {
            model: Model::new_tree(document),
            layout_cache: LayoutCache::new_tree(prepared_tree),
        }
    }

    /// Splits the bootstrap payload into model and layout cache ownership.
    pub(crate) fn into_parts(self) -> (Model, LayoutCache) {
        (self.model, self.layout_cache)
    }
}

/// Non-text draw commands keyed by their owning block.
#[derive(Clone, Debug, Default)]
pub(crate) struct BlockDrawCommands {
    images: HashMap<BlockId, Vec<ImageCmd>>,
    paths: HashMap<BlockId, Vec<PathCmd>>,
}

impl BlockDrawCommands {
    /// Creates the block draw command tables consumed by scene composition.
    pub(crate) fn new(
        images: HashMap<BlockId, Vec<ImageCmd>>,
        paths: HashMap<BlockId, Vec<PathCmd>>,
    ) -> Self {
        Self { images, paths }
    }

    /// Returns image commands grouped by block id.
    pub(crate) fn images(&self) -> &HashMap<BlockId, Vec<ImageCmd>> {
        &self.images
    }

    /// Returns path commands grouped by block id.
    pub(crate) fn paths(&self) -> &HashMap<BlockId, Vec<PathCmd>> {
        &self.paths
    }
}

/// Store-owned logical document model.
pub(crate) struct Model {
    document: DocumentSource,
    block_draw_commands: BlockDrawCommands,
}

impl Model {
    /// Creates the model from a document plus any block-level draw commands.
    pub(crate) fn new(document: Document, block_draw_commands: BlockDrawCommands) -> Self {
        Self {
            document: DocumentSource::Legacy(document),
            block_draw_commands,
        }
    }

    /// Creates the model from a rich-text tree. Tree-owned payloads are carried by layout nodes.
    pub(crate) fn new_tree(document: DocumentTree) -> Self {
        Self {
            document: DocumentSource::Tree(document),
            block_draw_commands: BlockDrawCommands::default(),
        }
    }

    /// Returns the current logical document.
    pub(crate) fn document(&self) -> &Document {
        match &self.document {
            DocumentSource::Legacy(document) => document,
            DocumentSource::Tree(_) => {
                panic!("tree-backed model does not expose a legacy document")
            }
        }
    }

    /// Returns mutable access to the logical document.
    pub(crate) fn document_mut(&mut self) -> &mut Document {
        match &mut self.document {
            DocumentSource::Legacy(document) => document,
            DocumentSource::Tree(_) => {
                panic!("tree-backed model does not expose a legacy document")
            }
        }
    }

    /// Returns the current document source.
    pub(crate) fn document_source(&self) -> &DocumentSource {
        &self.document
    }

    /// Returns block-level non-text draw commands attached to the current document.
    pub(crate) fn block_draw_commands(&self) -> &BlockDrawCommands {
        &self.block_draw_commands
    }

    /// Replaces the block-level draw commands after a model update.
    pub(crate) fn set_block_draw_commands(&mut self, block_draw_commands: BlockDrawCommands) {
        self.block_draw_commands = block_draw_commands;
    }
}

/// Store-owned logical document source for legacy and tree layout pipelines.
pub(crate) enum DocumentSource {
    Legacy(Document),
    Tree(DocumentTree),
}

/// Prepared layout cache reused across reflow-only updates.
pub(crate) struct LayoutCache {
    prepared: PreparedLayoutCache,
}

impl LayoutCache {
    /// Creates the prepared layout cache once for later reflow-only updates.
    pub(crate) fn new(prepared_blocks: Vec<PreparedBlock>) -> Self {
        Self {
            prepared: PreparedLayoutCache::Legacy(prepared_blocks),
        }
    }

    /// Creates the tree-path prepared cache once for later layout-only updates.
    pub(crate) fn new_tree(prepared_tree: PreparedTree) -> Self {
        Self {
            prepared: PreparedLayoutCache::Tree(prepared_tree),
        }
    }

    /// Returns the prepared blocks consumed by the layout stage.
    pub(crate) fn prepared_blocks(&self) -> &[PreparedBlock] {
        match &self.prepared {
            PreparedLayoutCache::Legacy(prepared_blocks) => prepared_blocks,
            PreparedLayoutCache::Tree(_) => {
                panic!("tree-backed cache does not expose legacy prepared blocks")
            }
        }
    }

    /// Returns the owned prepare cache representation.
    pub(crate) fn prepared(&self) -> &PreparedLayoutCache {
        &self.prepared
    }
}

/// Prepared layout cache for either the legacy or tree layout pipeline.
pub(crate) enum PreparedLayoutCache {
    Legacy(Vec<PreparedBlock>),
    Tree(PreparedTree),
}
