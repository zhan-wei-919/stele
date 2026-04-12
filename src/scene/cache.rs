//! View-side scene cache shared between the bridge and renderer rebuilds.

use std::collections::HashMap;

use crate::draw_list::{ClipRect, PathCmd};
use crate::io::SceneFrame;
use crate::renderer::instance::{GlyphInstance, ImageInstance, RectInstance};

/// Stable block identifier used across snapshots and scene frames.
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

/// View-owned block scene cache updated only through applied scene frames.
#[derive(Clone, Debug, Default)]
pub(crate) struct ViewState {
    block_order: Vec<BlockId>,
    blocks: HashMap<BlockId, BlockSceneBatch>,
    requested_viewport_revision: u64,
    applied_viewport_revision: u64,
    ready_atlas_generation: Option<u64>,
    pending_scene_frame: Option<SceneFrame>,
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
        self.drop_stale_pending_scene_frame();
    }

    /// Returns the latest viewport revision whose scene diff was actually applied.
    pub(crate) fn applied_viewport_revision(&self) -> u64 {
        self.applied_viewport_revision
    }

    /// Records the latest viewport revision whose scene diff was actually applied.
    pub(crate) fn set_applied_viewport_revision(&mut self, viewport_revision: u64) {
        self.applied_viewport_revision = self.applied_viewport_revision.max(viewport_revision);
        self.requested_viewport_revision = self.requested_viewport_revision.max(viewport_revision);
        self.drop_stale_pending_scene_frame();
    }

    /// Returns the latest logical atlas generation known to the view thread.
    pub(crate) fn ready_atlas_generation(&self) -> Option<u64> {
        self.ready_atlas_generation
    }

    /// Records the latest logical atlas generation applied on the renderer side.
    pub(crate) fn set_ready_atlas_generation(&mut self, generation: u64) {
        self.ready_atlas_generation = Some(
            self.ready_atlas_generation
                .map(|ready| ready.max(generation))
                .unwrap_or(generation),
        );
    }

    /// Returns the newest scene frame waiting for its required atlas generation.
    pub(crate) fn pending_scene_frame(&self) -> Option<&SceneFrame> {
        self.pending_scene_frame.as_ref()
    }

    /// Stores the newest pending scene frame that is still relevant to the requested viewport.
    ///
    /// A newer frame subsumes any older frame that is still waiting on atlas uploads, because
    /// the view only ever wants to apply the latest requested viewport revision once its glyphs
    /// are ready. Older pending frames would only waste work and can never become more correct.
    pub(crate) fn set_pending_scene_frame(&mut self, scene_frame: SceneFrame) {
        if scene_frame.viewport_revision < self.requested_viewport_revision {
            return;
        }

        let replace_existing = self
            .pending_scene_frame
            .as_ref()
            .map(|pending| pending.viewport_revision <= scene_frame.viewport_revision)
            .unwrap_or(true);
        if replace_existing {
            self.pending_scene_frame = Some(scene_frame);
        }
    }

    /// Takes the pending scene frame, if any.
    pub(crate) fn take_pending_scene_frame(&mut self) -> Option<SceneFrame> {
        self.pending_scene_frame.take()
    }

    /// Clears any pending scene frame that can no longer be applied.
    pub(crate) fn clear_pending_scene_frame(&mut self) {
        self.pending_scene_frame = None;
    }

    /// Drops any queued frame that has already fallen behind the latest requested viewport.
    fn drop_stale_pending_scene_frame(&mut self) {
        let should_drop = self
            .pending_scene_frame
            .as_ref()
            .map(|scene_frame| scene_frame.viewport_revision < self.requested_viewport_revision)
            .unwrap_or(false);
        if should_drop {
            self.pending_scene_frame = None;
        }
    }
}
