//! Layout-stage positioned output consumed by the renderer bridge.

use crate::renderer::{PositionedGlyph, RectCmd};

use super::super::document::BlockRect;

/// One block's positioned lines and block-level drawing metadata.
#[derive(Clone, Debug)]
pub(crate) struct LayoutBlock {
    pub(crate) block_index: usize,
    pub(crate) z_order: u32,
    pub(crate) lines: Vec<LayoutLine>,
    pub(crate) background_rect: Option<RectCmd>,
    pub(crate) clip_rect: BlockRect,
}

/// One laid out line sharing the same baseline.
#[derive(Clone, Debug)]
pub(crate) struct LayoutLine {
    pub(crate) runs: Vec<LayoutRun>,
    pub(crate) y: f32,
    pub(crate) line_height: f32,
    pub(crate) baseline: f32,
}

/// One contiguous run from a single span on a single line.
#[derive(Clone, Debug)]
pub(crate) struct LayoutRun {
    pub(crate) glyphs: Vec<PositionedGlyph>,
    pub(crate) decoration_rects: Vec<RectCmd>,
}
