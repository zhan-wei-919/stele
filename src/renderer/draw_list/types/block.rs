//! Block-grouped draw-list primitives.

use super::{BlockSubLayer, ClipRect, ImageCmd, PathCmd, PositionedGlyph, RectCmd, RenderLayer};

/// Primitives that belong to one block-local layer.
#[derive(Clone, Debug, Default)]
pub(crate) struct BlockLayer {
    glyphs: Vec<PositionedGlyph>,
    rects: Vec<RectCmd>,
    paths: Vec<PathCmd>,
    images: Vec<ImageCmd>,
}

impl BlockLayer {
    /// Returns the glyphs assigned to this block-local layer.
    pub(crate) fn glyphs(&self) -> &[PositionedGlyph] {
        &self.glyphs
    }

    /// Returns the rectangles assigned to this block-local layer.
    pub(crate) fn rects(&self) -> &[RectCmd] {
        &self.rects
    }

    /// Returns the paths assigned to this block-local layer.
    pub(crate) fn paths(&self) -> &[PathCmd] {
        &self.paths
    }

    /// Returns the images assigned to this block-local layer.
    pub(crate) fn images(&self) -> &[ImageCmd] {
        &self.images
    }
}

/// Renderer-facing draw primitives grouped by stacking block and clip lifetime.
#[derive(Clone, Debug)]
pub(crate) struct BlockDrawGroup {
    block_index: usize,
    z_order: u32,
    clip_rect: Option<ClipRect>,
    sub_layers: [BlockLayer; RenderLayer::ALL.len()],
}

impl BlockDrawGroup {
    /// Creates an empty draw group for one block submission unit.
    pub(crate) fn new(block_index: usize, z_order: u32, clip_rect: Option<ClipRect>) -> Self {
        Self {
            block_index,
            z_order,
            clip_rect,
            sub_layers: std::array::from_fn(|_| BlockLayer::default()),
        }
    }

    /// Returns the document-order block index used to break z-order ties.
    pub(crate) fn block_index(&self) -> usize {
        self.block_index
    }

    /// Returns the stacking order where larger values are visually on top.
    pub(crate) fn z_order(&self) -> u32 {
        self.z_order
    }

    /// Returns the clip rectangle applied while drawing this group.
    pub(crate) fn clip_rect(&self) -> Option<ClipRect> {
        self.clip_rect
    }

    /// Returns the primitive collections assigned to the requested block-local layer.
    pub(crate) fn layer(&self, layer: BlockSubLayer) -> &BlockLayer {
        &self.sub_layers[layer.index()]
    }

    /// Appends glyphs to the requested block-local layer.
    pub(crate) fn extend_glyphs(&mut self, layer: BlockSubLayer, glyphs: Vec<PositionedGlyph>) {
        self.sub_layers[layer.index()].glyphs.extend(glyphs);
    }

    /// Appends one rectangle to the layer encoded on the command itself.
    pub(crate) fn push_rect(&mut self, rect: RectCmd) {
        self.sub_layers[rect.layer().index()].rects.push(rect);
    }

    /// Appends one path to the layer encoded on the command itself.
    pub(crate) fn push_path(&mut self, path: PathCmd) {
        self.sub_layers[path.layer().index()].paths.push(path);
    }

    /// Appends one image to the layer encoded on the command itself.
    pub(crate) fn push_image(&mut self, image: ImageCmd) {
        self.sub_layers[image.layer().index()].images.push(image);
    }
}
