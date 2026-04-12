//! Layout-stage positioned output consumed by the renderer bridge.

use crate::draw_list::{PositionedGlyph, RectCmd};
use crate::scene::BlockId;

use super::super::document::BlockRect;

/// One block's positioned lines and block-level drawing metadata.
#[derive(Clone, Debug)]
pub(crate) struct LayoutBlock {
    pub(crate) block_id: BlockId,
    pub(crate) z_order: u32,
    pub(crate) lines: Vec<LayoutLine>,
    pub(crate) background_rect: Option<RectCmd>,
    pub(crate) clip_rect: BlockRect,
}

/// One laid out line sharing the same baseline.
#[derive(Clone, Debug)]
pub(crate) struct LayoutLine {
    pub(crate) runs: Vec<LayoutRun>,
    // Kept for layout assertions and future debug overlays.
    #[allow(dead_code)]
    pub(crate) y: f32,
    // Kept for layout assertions and future debug overlays.
    #[allow(dead_code)]
    pub(crate) line_height: f32,
    // Kept for layout assertions and future debug overlays.
    #[allow(dead_code)]
    pub(crate) baseline: f32,
}

/// One contiguous run from a single span on a single line.
#[derive(Clone, Debug)]
pub(crate) struct LayoutRun {
    pub(crate) glyphs: Vec<PositionedGlyph>,
    pub(crate) decoration_rects: Vec<RectCmd>,
}
