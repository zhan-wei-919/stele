//! Store-local text input buffers for focused text editing commands.

use std::collections::HashMap;

use crate::layout::tree::{
    is_insertable_text_input_char, single_line_text, BlockNode, DocumentTree, TextInputId,
};

/// Editable text and cursor state for a focused text input target.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextInputState {
    text: String,
    cursor_index: usize,
    revision: u64,
}

impl TextInputState {
    /// Creates editable state with the cursor at the end of the initial text.
    pub(crate) fn new(text: impl Into<String>) -> Self {
        let text = single_line_text(&text.into());
        Self {
            cursor_index: text.len(),
            text,
            revision: 0,
        }
    }

    /// Returns the current text contents.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Returns the cursor as a UTF-8 byte index into the current text.
    pub(crate) fn cursor_index(&self) -> usize {
        self.cursor_index
    }

    /// Returns the monotonic state revision used for cheap change detection.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Inserts one character at the cursor and advances past it.
    pub(crate) fn insert_char(&mut self, ch: char) -> bool {
        self.debug_assert_cursor_boundary();
        if !is_insertable_text_input_char(ch) {
            return false;
        }

        self.text.insert(self.cursor_index, ch);
        self.cursor_index += ch.len_utf8();
        self.bump_revision();
        true
    }

    /// Inserts text at the cursor and advances past the inserted bytes.
    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        self.debug_assert_cursor_boundary();
        if text.is_empty() {
            return false;
        }

        let inserted = single_line_text(text);
        if inserted.is_empty() {
            return false;
        }

        self.text.insert_str(self.cursor_index, &inserted);
        self.cursor_index += inserted.len();
        self.bump_revision();
        true
    }

    /// Deletes the character before the cursor when one exists.
    pub(crate) fn delete_backward(&mut self) -> bool {
        self.debug_assert_cursor_boundary();
        let Some(previous_index) = self.previous_cursor_index() else {
            return false;
        };

        self.text.drain(previous_index..self.cursor_index);
        self.cursor_index = previous_index;
        self.bump_revision();
        true
    }

    /// Moves the cursor left by one character when possible.
    pub(crate) fn move_cursor_left(&mut self) -> bool {
        self.debug_assert_cursor_boundary();
        let Some(previous_index) = self.previous_cursor_index() else {
            return false;
        };

        self.cursor_index = previous_index;
        self.bump_revision();
        true
    }

    /// Moves the cursor right by one character when possible.
    pub(crate) fn move_cursor_right(&mut self) -> bool {
        self.debug_assert_cursor_boundary();
        let Some(next_index) = self.next_cursor_index() else {
            return false;
        };

        self.cursor_index = next_index;
        self.bump_revision();
        true
    }

    fn bump_revision(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("text input revision exhausted");
    }

    fn previous_cursor_index(&self) -> Option<usize> {
        self.text[..self.cursor_index]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
    }

    fn next_cursor_index(&self) -> Option<usize> {
        let ch = self.text[self.cursor_index..].chars().next()?;
        Some(self.cursor_index + ch.len_utf8())
    }

    fn debug_assert_cursor_boundary(&self) {
        debug_assert!(
            self.text.is_char_boundary(self.cursor_index),
            "text input cursor must stay on a UTF-8 character boundary"
        );
    }
}

/// Editable text states keyed by semantic text input identity.
#[derive(Clone, Debug, Default)]
pub(crate) struct TextInputStates {
    states: HashMap<TextInputId, TextInputState>,
}

impl TextInputStates {
    /// Creates one empty state for each text input declared by the document.
    pub(crate) fn from_document(document: &DocumentTree) -> Self {
        let mut states = Self::default();
        collect_text_inputs(document.root(), &mut states.states);
        debug_assert_eq!(
            states.states.len(),
            document.text_input_ids().len(),
            "document validation must ensure one state per text input id"
        );
        states
    }

    /// Returns whether the registry has a state for the id.
    pub(crate) fn contains(&self, text_input: TextInputId) -> bool {
        self.states.contains_key(&text_input)
    }

    /// Returns editable state for an id when it is still present in the model.
    pub(crate) fn get(&self, text_input: TextInputId) -> Option<&TextInputState> {
        self.states.get(&text_input)
    }

    /// Returns mutable editable state for an id when it is still present in the model.
    pub(crate) fn get_mut(&mut self, text_input: TextInputId) -> Option<&mut TextInputState> {
        self.states.get_mut(&text_input)
    }

    /// Returns the number of registered text inputs.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.states.len()
    }

    /// Returns a monotonic aggregate revision for cheap batch-level change detection.
    pub(crate) fn revision(&self) -> u64 {
        self.states.values().fold(0u64, |revision, state| {
            revision.wrapping_add(state.revision())
        })
    }

    /// Returns the current text and cursor snapshot used by the prepare stage.
    pub(crate) fn prepare_value(
        &self,
        text_input: TextInputId,
    ) -> Option<crate::layout::prepare_tree::TextInputValue<'_>> {
        self.get(text_input)
            .map(|state| crate::layout::prepare_tree::TextInputValue {
                text: state.text(),
                cursor_index: state.cursor_index(),
            })
    }
}

fn collect_text_inputs(node: &BlockNode, states: &mut HashMap<TextInputId, TextInputState>) {
    match node {
        BlockNode::Stack(stack) => {
            for child in &stack.children {
                collect_text_inputs(child, states);
            }
        }
        BlockNode::TextInput(text_input) => {
            states.insert(text_input.text_input_id, TextInputState::new(""));
        }
        BlockNode::Overlay(overlay) => collect_text_inputs(overlay.child.as_ref(), states),
        BlockNode::Paragraph(_) | BlockNode::Embed(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::tree::{
        BlockStyle, DocumentTree, FlowDirection, StackNode, TextInputNode, TextInputStyle,
        TextStyle,
    };

    #[test]
    fn revision_increments_only_when_state_changes() {
        let mut text_input = TextInputState::default();

        assert_eq!(text_input.revision(), 0);
        assert!(!text_input.delete_backward());
        assert!(!text_input.move_cursor_left());
        assert_eq!(text_input.revision(), 0);

        assert!(text_input.insert_char('a'));
        assert_eq!(text_input.revision(), 1);

        assert!(text_input.move_cursor_left());
        assert_eq!(text_input.revision(), 2);
        assert!(!text_input.move_cursor_left());
        assert_eq!(text_input.revision(), 2);

        assert!(text_input.move_cursor_right());
        assert_eq!(text_input.revision(), 3);
        assert!(!text_input.move_cursor_right());
        assert_eq!(text_input.revision(), 3);

        assert!(text_input.delete_backward());
        assert_eq!(text_input.revision(), 4);
        assert_eq!(text_input.text(), "");
    }

    #[test]
    fn bulk_insert_updates_at_cursor_and_preserves_utf8_boundaries() {
        let mut text_input = TextInputState::default();

        assert!(text_input.insert_char('a'));
        assert!(text_input.insert_char('中'));
        assert!(text_input.insert_char('d'));
        assert!(text_input.move_cursor_left());

        assert!(text_input.insert_text("βc"));

        assert_eq!(text_input.text(), "a中βcd");
        assert_eq!(text_input.cursor_index(), "a中βc".len());
        assert!(text_input
            .text()
            .is_char_boundary(text_input.cursor_index()));
    }

    #[test]
    fn empty_bulk_insert_does_not_change_revision() {
        let mut text_input = TextInputState::default();
        assert!(text_input.insert_char('a'));
        let revision = text_input.revision();

        assert!(!text_input.insert_text(""));

        assert_eq!(text_input.text(), "a");
        assert_eq!(text_input.cursor_index(), 1);
        assert_eq!(text_input.revision(), revision);
    }

    #[test]
    fn new_state_places_cursor_at_utf8_boundary_end() {
        let text_input = TextInputState::new("a中");

        assert_eq!(text_input.text(), "a中");
        assert_eq!(text_input.cursor_index(), "a中".len());
        assert!(text_input
            .text()
            .is_char_boundary(text_input.cursor_index()));
    }

    #[test]
    fn rejects_control_characters_at_source() {
        let mut text_input = TextInputState::new("a\nb");

        assert_eq!(text_input.text(), "ab");
        assert_eq!(text_input.cursor_index(), 2);
        assert!(!text_input.insert_char('\n'));
        assert_eq!(text_input.revision(), 0);

        assert!(text_input.insert_text("\rc\t中"));

        assert_eq!(text_input.text(), "abc中");
        assert_eq!(text_input.cursor_index(), "abc中".len());
        assert_eq!(text_input.revision(), 1);
    }

    #[test]
    fn states_initialize_one_entry_per_document_text_input() {
        let first = TextInputId::new(1);
        let second = TextInputId::new(2);
        let document = document_with_text_inputs([first, second]);
        let states = TextInputStates::from_document(&document);

        assert_eq!(states.len(), 2);
        assert!(states.contains(first));
        assert!(states.contains(second));
        assert_eq!(states.get(first).expect("first state").text(), "");
    }

    fn document_with_text_inputs(ids: [TextInputId; 2]) -> DocumentTree {
        let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let children = ids
            .into_iter()
            .map(|id| {
                BlockNode::TextInput(
                    TextInputNode::new(id, "placeholder", style, TextInputStyle::default())
                        .expect("text input must be valid"),
                )
            })
            .collect();
        DocumentTree::new(BlockNode::Stack(
            StackNode::new(FlowDirection::Vertical, children, BlockStyle::default())
                .expect("stack must be valid"),
        ))
        .expect("document must be valid")
    }
}
