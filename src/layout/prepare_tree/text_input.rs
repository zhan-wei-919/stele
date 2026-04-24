//! Prepared text input measurements for the tree layout path.

use crate::layout::prepare::PreparedGlyph;
use crate::layout::tree::{NodeId, TextInputId, TextInputStyle};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextInputValue<'a> {
    pub(crate) text: &'a str,
    pub(crate) cursor_index: usize,
}

pub(crate) trait TextInputResolver {
    /// Returns the model-owned text snapshot for a semantic text input id.
    fn resolve_text_input(&self, text_input: TextInputId) -> Option<TextInputValue<'_>>;
}

#[cfg(test)]
pub(crate) struct EmptyTextInputResolver;

#[cfg(test)]
impl TextInputResolver for EmptyTextInputResolver {
    fn resolve_text_input(&self, _text_input: TextInputId) -> Option<TextInputValue<'_>> {
        None
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedTextInput {
    pub(crate) node_id: NodeId,
    pub(crate) text_input_id: TextInputId,
    pub(crate) glyphs: Vec<PreparedGlyph>,
    pub(crate) content_width: f32,
    pub(crate) caret_advance: f32,
    pub(crate) default_ascent: f32,
    pub(crate) default_line_height: f32,
    pub(crate) style: TextInputStyle,
}
