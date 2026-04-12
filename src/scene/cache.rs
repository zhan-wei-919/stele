//! View-side scene cache shared between the bridge and renderer rebuilds.

use std::collections::HashMap;

use crate::draw_list::{ClipRect, PathCmd};
use crate::renderer::instance::{GlyphInstance, ImageInstance, RectInstance};

/// Stable block identifier used across snapshots and SceneDiffs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct BlockId(u64);

impl BlockId {
    /// Creates a stable block identifier.
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Final render-ready payload for one block in the current scene cache.
#[derive(Clone, Debug)]
pub(crate) struct BlockSceneBatch {
    clip_rect: ClipRect,
    z_order: u32,
    glyphs: Vec<GlyphInstance>,
    rects: Vec<RectInstance>,
    paths: Vec<PathCmd>,
    images: Vec<ImageInstance>,
    fingerprint: u64,
}

impl BlockSceneBatch {
    /// Creates one block batch whose payload is already validated at the source.
    pub(crate) fn new(
        clip_rect: ClipRect,
        z_order: u32,
        glyphs: Vec<GlyphInstance>,
        rects: Vec<RectInstance>,
        paths: Vec<PathCmd>,
        images: Vec<ImageInstance>,
        fingerprint: u64,
    ) -> Self {
        Self {
            clip_rect,
            z_order,
            glyphs,
            rects,
            paths,
            images,
            fingerprint,
        }
    }

    /// Returns the block clip rectangle.
    pub(crate) fn clip_rect(&self) -> ClipRect {
        self.clip_rect
    }

    /// Returns the block z-order.
    pub(crate) fn z_order(&self) -> u32 {
        self.z_order
    }

    /// Returns the glyph instances ready for upload.
    pub(crate) fn glyphs(&self) -> &[GlyphInstance] {
        &self.glyphs
    }

    /// Returns the rectangle instances ready for upload.
    pub(crate) fn rects(&self) -> &[RectInstance] {
        &self.rects
    }

    /// Returns the path commands that still need view-side tessellation.
    pub(crate) fn paths(&self) -> &[PathCmd] {
        &self.paths
    }

    /// Returns the image instances ready for upload.
    pub(crate) fn images(&self) -> &[ImageInstance] {
        &self.images
    }

    /// Returns the stable fingerprint used by snapshot diffing.
    pub(crate) fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// View-owned block scene cache updated only through SceneDiff apply.
#[derive(Clone, Debug, Default)]
pub(crate) struct ViewState {
    block_order: Vec<BlockId>,
    blocks: HashMap<BlockId, BlockSceneBatch>,
    requested_viewport_revision: u64,
    applied_viewport_revision: u64,
}

impl ViewState {
    /// Creates an empty view state.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns the current block draw order.
    pub(crate) fn block_order(&self) -> &[BlockId] {
        &self.block_order
    }

    /// Returns the current block scene cache.
    pub(crate) fn blocks(&self) -> &HashMap<BlockId, BlockSceneBatch> {
        &self.blocks
    }

    /// Clears the cached scene before a self-contained viewport revision apply.
    pub(crate) fn clear_scene(&mut self) {
        self.block_order.clear();
        self.blocks.clear();
    }

    /// Replaces the full block order.
    pub(crate) fn set_block_order(&mut self, block_order: Vec<BlockId>) {
        self.block_order = block_order;
    }

    /// Replaces or inserts one block batch.
    pub(crate) fn replace_block(&mut self, block_id: BlockId, batch: BlockSceneBatch) {
        self.blocks.insert(block_id, batch);
    }

    /// Removes one block batch.
    pub(crate) fn remove_block(&mut self, block_id: BlockId) {
        self.blocks.remove(&block_id);
    }

    /// Returns the latest viewport revision requested by window events.
    pub(crate) fn requested_viewport_revision(&self) -> u64 {
        self.requested_viewport_revision
    }

    /// Records the latest viewport revision requested by window events.
    pub(crate) fn set_requested_viewport_revision(&mut self, viewport_revision: u64) {
        self.requested_viewport_revision = self.requested_viewport_revision.max(viewport_revision);
    }

    /// Returns the latest viewport revision whose scene diff was actually applied.
    pub(crate) fn applied_viewport_revision(&self) -> u64 {
        self.applied_viewport_revision
    }

    /// Records the latest viewport revision whose scene diff was actually applied.
    pub(crate) fn set_applied_viewport_revision(&mut self, viewport_revision: u64) {
        self.applied_viewport_revision = self.applied_viewport_revision.max(viewport_revision);
        self.requested_viewport_revision = self.requested_viewport_revision.max(viewport_revision);
    }
}
