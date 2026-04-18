//! Arena-backed block payloads stored inside one self-contained scene buffer.

use bumpalo::collections::Vec as BumpVec;

use crate::draw_list::{ClipRect, ImageCmd, PathCmd};
use crate::renderer::instance::{GlyphInstance, RectInstance};

/// Stable block identifier carried across scene-buffer rebuilds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct BlockId(u64);

impl BlockId {
    /// Creates a stable block identifier.
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// One render-ready block payload allocated inside one arena-backed scene buffer.
#[derive(Debug)]
pub(crate) struct BlockDataArena<'a> {
    block_id: BlockId,
    clip_rect: ClipRect,
    z_order: u32,
    glyphs: BumpVec<'a, GlyphInstance>,
    rects: BumpVec<'a, RectInstance>,
    paths: BumpVec<'a, PathCmd>,
    images: BumpVec<'a, ImageCmd>,
    // Retained so diagnostics can correlate one rendered block with the composer fingerprint
    // without recomputing hashes after the scene buffer has already been assembled.
    #[allow(dead_code)]
    fingerprint: u64,
}

impl<'a> BlockDataArena<'a> {
    /// Creates an empty block payload inside the provided bump arena.
    pub(crate) fn new_in(
        owner: &'a bumpalo::Bump,
        block_id: BlockId,
        clip_rect: ClipRect,
        z_order: u32,
        fingerprint: u64,
    ) -> Self {
        Self {
            block_id,
            clip_rect,
            z_order,
            glyphs: BumpVec::new_in(owner),
            rects: BumpVec::new_in(owner),
            paths: BumpVec::new_in(owner),
            images: BumpVec::new_in(owner),
            fingerprint,
        }
    }

    /// Returns the owning block id.
    pub(crate) fn block_id(&self) -> BlockId {
        self.block_id
    }

    /// Returns the block clip rectangle.
    pub(crate) fn clip_rect(&self) -> ClipRect {
        self.clip_rect
    }

    /// Returns the block z-order.
    pub(crate) fn z_order(&self) -> u32 {
        self.z_order
    }

    /// Returns the glyph instances ready for GPU upload.
    pub(crate) fn glyphs(&self) -> &[GlyphInstance] {
        self.glyphs.as_slice()
    }

    /// Returns the rectangle instances ready for GPU upload.
    pub(crate) fn rects(&self) -> &[RectInstance] {
        self.rects.as_slice()
    }

    /// Returns the view-side path commands still requiring tessellation.
    pub(crate) fn paths(&self) -> &[PathCmd] {
        self.paths.as_slice()
    }

    /// Returns the image commands still requiring renderer-side batching.
    pub(crate) fn images(&self) -> &[ImageCmd] {
        self.images.as_slice()
    }

    /// Returns the mutable glyph arena while the composer is assembling this block.
    pub(crate) fn glyphs_mut(&mut self) -> &mut BumpVec<'a, GlyphInstance> {
        &mut self.glyphs
    }

    /// Returns the mutable rectangle arena while the composer is assembling this block.
    pub(crate) fn rects_mut(&mut self) -> &mut BumpVec<'a, RectInstance> {
        &mut self.rects
    }

    /// Returns the mutable path arena while the composer is assembling this block.
    pub(crate) fn paths_mut(&mut self) -> &mut BumpVec<'a, PathCmd> {
        &mut self.paths
    }

    /// Returns the mutable image arena while the composer is assembling this block.
    pub(crate) fn images_mut(&mut self) -> &mut BumpVec<'a, ImageCmd> {
        &mut self.images
    }
}
