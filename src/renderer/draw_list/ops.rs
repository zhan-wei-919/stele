use super::types::PositionedGlyph;

#[derive(Clone, Debug)]
pub enum DrawListOp {
    Insert {
        line_index: usize,
        glyphs: Vec<PositionedGlyph>,
    },
    Remove {
        line_index: usize,
    },
    Replace {
        line_index: usize,
        glyphs: Vec<PositionedGlyph>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct DrawList {
    pub lines: Vec<Vec<PositionedGlyph>>,
    pub rects: Vec<super::types::RectCmd>,
    pub cursor: Option<super::types::RectCmd>,
}

impl DrawList {
    pub fn new() -> Self {
        Self::default()
    }

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
