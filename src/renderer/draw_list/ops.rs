//! Incremental mutations applied to the renderer-owned draw list.

use super::types::PositionedGlyph;

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
}

/// Renderer input state assembled from line-based glyph lists and solid rectangles.
#[derive(Clone, Debug, Default)]
pub struct DrawList {
    pub lines: Vec<Vec<PositionedGlyph>>,
    pub rects: Vec<super::types::RectCmd>,
    pub cursor: Option<super::types::RectCmd>,
}

impl DrawList {
    /// Creates an empty draw list with no lines, rectangles, or cursor.
    ///
    /// This convenience constructor is currently only used by the unit test.
    /// Production code builds the renderer-owned draw list through Default plus
    /// incremental ops, so cargo run reports this helper as unused for now.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies incremental line operations while preserving the existing rect and cursor state.
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
    use super::{DrawList, DrawListOp};
    use crate::font::SubpixelBin;
    use crate::renderer::draw_list::PositionedGlyph;

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
}
