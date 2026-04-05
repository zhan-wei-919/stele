//! Incremental mutations applied to the renderer-owned draw list.

use super::types::{ImageCmd, PathCmd, PositionedGlyph, RectCmd};

/// Incremental update applied to a renderer-owned line list.
#[derive(Clone, Debug)]
pub enum DrawListOp {
    Insert {
        line_index: usize,
        glyphs: Vec<PositionedGlyph>,
    },
    // M0 only emits Insert operations from the hard-coded layout path.
    // Remove stays in the API because incremental scene updates will need it,
    // and the current unit test is the only non-future caller during cargo run.
    Remove {
        line_index: usize,
    },
    // M0 does not replace lines in normal execution yet.
    // This variant exists to keep the incremental update surface complete ahead
    // of upstream dirty-line updates, so cargo run currently sees it as dead code.
    Replace {
        line_index: usize,
        glyphs: Vec<PositionedGlyph>,
    },
    SetRects(Vec<RectCmd>),
    SetPaths(Vec<PathCmd>),
    SetImages(Vec<ImageCmd>),
}

/// Renderer input state assembled from glyphs plus layer-aware CPU primitives.
#[derive(Clone, Debug, Default)]
pub struct DrawList {
    pub lines: Vec<Vec<PositionedGlyph>>,
    pub rects: Vec<RectCmd>,
    pub paths: Vec<PathCmd>,
    pub images: Vec<ImageCmd>,
}

impl DrawList {
    /// Creates an empty draw list with no glyphs or primitive commands.
    ///
    /// This convenience constructor is currently only used by the unit test.
    /// Production code builds the renderer-owned draw list through Default plus
    /// incremental ops, so cargo run reports this helper as unused for now.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies incremental line and primitive operations in-order.
    pub fn apply_ops<I>(&mut self, ops: I)
    where
        I: IntoIterator<Item = DrawListOp>,
    {
        for op in ops {
            match op {
                DrawListOp::Insert { line_index, glyphs } => {
                    if !self.valid_insert_index(line_index) {
                        continue;
                    }
                    self.lines.insert(line_index, glyphs);
                }
                DrawListOp::Remove { line_index } => {
                    if !self.valid_existing_index(line_index) {
                        continue;
                    }
                    self.lines.remove(line_index);
                }
                DrawListOp::Replace { line_index, glyphs } => {
                    if !self.valid_existing_index(line_index) {
                        continue;
                    }
                    self.lines[line_index] = glyphs;
                }
                DrawListOp::SetRects(rects) => {
                    self.rects = rects;
                }
                DrawListOp::SetPaths(paths) => {
                    self.paths = paths;
                }
                DrawListOp::SetImages(images) => {
                    self.images = images;
                }
            }
        }
    }

    fn valid_insert_index(&self, line_index: usize) -> bool {
        if line_index <= self.lines.len() {
            true
        } else {
            debug_assert!(
                false,
                "DrawListOp::Insert line_index out of bounds: {line_index} > {}",
                self.lines.len()
            );
            false
        }
    }

    fn valid_existing_index(&self, line_index: usize) -> bool {
        if line_index < self.lines.len() {
            true
        } else {
            debug_assert!(
                false,
                "DrawListOp line_index out of bounds: {line_index} >= {}",
                self.lines.len()
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{DrawList, DrawListOp};
    use crate::font::SubpixelBin;
    use crate::renderer::draw_list::{
        ImageCmd, ImageData, PathCmd, PathVerb, PositionedGlyph, RectCmd, RenderLayer,
    };

    fn glyph(id: u16) -> PositionedGlyph {
        PositionedGlyph {
            font_id: 0,
            glyph_id: id,
            font_size: 14.0,
            pos: [0.0, 0.0],
            color: [1.0, 1.0, 1.0, 1.0],
            subpixel_offset: SubpixelBin::new(0, 0),
        }
    }

    #[test]
    fn apply_ops_handles_insert_replace_and_remove() {
        let mut draw_list = DrawList::new();
        draw_list.apply_ops([DrawListOp::Insert {
            line_index: 0,
            glyphs: vec![glyph(1)],
        }]);
        assert_eq!(draw_list.lines[0][0].glyph_id, 1);

        draw_list.apply_ops([DrawListOp::Replace {
            line_index: 0,
            glyphs: vec![glyph(2)],
        }]);
        assert_eq!(draw_list.lines[0][0].glyph_id, 2);

        draw_list.apply_ops([DrawListOp::Remove { line_index: 0 }]);
        assert!(draw_list.lines.is_empty());
    }

    #[test]
    fn apply_ops_replaces_rects_paths_and_images() {
        let mut draw_list = DrawList::new();
        let image = Arc::new(ImageData::new(vec![255, 0, 0, 255], 1, 1));

        draw_list.apply_ops([
            DrawListOp::SetRects(vec![RectCmd {
                pos: [1.0, 2.0],
                size: [3.0, 4.0],
                color: [0.1, 0.2, 0.3, 1.0],
                layer: RenderLayer::Background,
            }]),
            DrawListOp::SetPaths(vec![PathCmd {
                verbs: vec![
                    PathVerb::MoveTo { to: [0.0, 0.0] },
                    PathVerb::LineTo { to: [10.0, 10.0] },
                ],
                fill: None,
                stroke: None,
                layer: RenderLayer::Content,
            }]),
            DrawListOp::SetImages(vec![ImageCmd {
                pos: [5.0, 6.0],
                size: [7.0, 8.0],
                data: image,
                layer: RenderLayer::Overlay,
            }]),
        ]);

        assert_eq!(draw_list.rects.len(), 1);
        assert_eq!(draw_list.paths.len(), 1);
        assert_eq!(draw_list.images.len(), 1);
    }
}
