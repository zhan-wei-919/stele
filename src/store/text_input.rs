//! Store-local text input buffers for focused text editing commands.

use std::collections::HashMap;

use crate::layout::tree::{
    is_insertable_text_input_char, single_line_text, BlockNode, DocumentTree, TextInputId,
};

/// Caret or range selection expressed as UTF-8 byte indices.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextSelection {
    pub(crate) anchor: usize,
    pub(crate) focus: usize,
}

impl TextSelection {
    /// Creates a collapsed selection at one caret index.
    pub(crate) fn collapsed(index: usize) -> Self {
        Self {
            anchor: index,
            focus: index,
        }
    }

    /// Returns whether the selection is a caret without selected text.
    pub(crate) fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }

    /// Returns the selected byte range in document order.
    pub(crate) fn range(self) -> std::ops::Range<usize> {
        self.anchor.min(self.focus)..self.anchor.max(self.focus)
    }
}

/// Editable text and cursor state for a focused text input target.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextInputState {
    text: String,
    selection: TextSelection,
    content_revision: u64,
    visual_revision: u64,
}

impl TextInputState {
    /// Creates editable state with the cursor at the end of the initial text.
    pub(crate) fn new(text: impl Into<String>) -> Self {
        let text = single_line_text(&text.into());
        Self {
            selection: TextSelection::collapsed(text.len()),
            text,
            content_revision: 0,
            visual_revision: 0,
        }
    }

    /// Returns the current text contents.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Returns the cursor as a UTF-8 byte index into the current text.
    pub(crate) fn cursor_index(&self) -> usize {
        self.selection.focus
    }

    /// Returns the current caret/selection state.
    pub(crate) fn selection(&self) -> TextSelection {
        self.selection
    }

    /// Returns the aggregate monotonic revision used for cheap change detection.
    pub(crate) fn revision(&self) -> u64 {
        self.content_revision.wrapping_add(self.visual_revision)
    }

    /// Returns the monotonic revision for text content changes.
    pub(crate) fn content_revision(&self) -> u64 {
        self.content_revision
    }

    /// Returns the monotonic revision for caret and selection changes.
    #[cfg(test)]
    pub(crate) fn visual_revision(&self) -> u64 {
        self.visual_revision
    }

    /// Returns the current selected text when the selection is non-empty.
    pub(crate) fn selected_text(&self) -> Option<&str> {
        self.debug_assert_selection_boundaries(self.selection);
        (!self.selection.is_collapsed()).then(|| {
            self.text
                .get(self.selection.range())
                .expect("text input selection must stay on UTF-8 boundaries")
        })
    }

    /// Inserts one character at the cursor and advances past it.
    pub(crate) fn insert_char(&mut self, ch: char) -> bool {
        if !is_insertable_text_input_char(ch) {
            return false;
        }

        let mut inserted = [0u8; 4];
        let inserted = ch.encode_utf8(&mut inserted);
        self.replace_selection(inserted);
        true
    }

    /// Inserts text at the cursor and advances past the inserted bytes.
    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }

        let inserted = single_line_text(text);
        if inserted.is_empty() {
            return false;
        }

        self.replace_selection(&inserted);
        true
    }

    /// Deletes the selected text or the character before the cursor.
    pub(crate) fn delete_backward(&mut self) -> bool {
        self.debug_assert_selection_boundaries(self.selection);
        if self.delete_selection() {
            return true;
        }

        let Some(previous_index) = self.previous_cursor_index() else {
            return false;
        };

        self.text.drain(previous_index..self.cursor_index());
        self.bump_content_revision();
        self.set_selection(TextSelection::collapsed(previous_index));
        true
    }

    /// Deletes the selected text or the character after the cursor.
    pub(crate) fn delete_forward(&mut self) -> bool {
        self.debug_assert_selection_boundaries(self.selection);
        if self.delete_selection() {
            return true;
        }

        let Some(next_index) = self.next_cursor_index() else {
            return false;
        };

        self.text.drain(self.cursor_index()..next_index);
        self.bump_content_revision();
        true
    }

    /// Deletes the selected text without falling back to adjacent characters.
    pub(crate) fn delete_selected_text(&mut self) -> bool {
        self.debug_assert_selection_boundaries(self.selection);
        self.delete_selection()
    }

    /// Selects the full text contents.
    pub(crate) fn select_all(&mut self) -> bool {
        self.set_selection(TextSelection {
            anchor: 0,
            focus: self.text.len(),
        })
    }

    /// Collapses the selection to one validated caret index.
    pub(crate) fn set_cursor(&mut self, index: usize) -> bool {
        if !self.index_is_valid(index) {
            return false;
        }
        self.set_selection(TextSelection::collapsed(index))
    }

    /// Extends the current selection focus to one validated caret index.
    pub(crate) fn extend_selection_to(&mut self, index: usize) -> bool {
        if !self.index_is_valid(index) {
            return false;
        }
        self.set_selection(TextSelection {
            anchor: self.selection.anchor,
            focus: index,
        })
    }

    /// Moves the caret left by one character, optionally extending selection.
    pub(crate) fn move_cursor_left(&mut self, extend: bool) -> bool {
        self.debug_assert_selection_boundaries(self.selection);
        if !extend && !self.selection.is_collapsed() {
            return self.set_cursor(self.selection.range().start);
        }

        let Some(previous_index) = self.previous_cursor_index() else {
            return false;
        };

        self.move_cursor_to(previous_index, extend)
    }

    /// Moves the caret right by one character, optionally extending selection.
    pub(crate) fn move_cursor_right(&mut self, extend: bool) -> bool {
        self.debug_assert_selection_boundaries(self.selection);
        if !extend && !self.selection.is_collapsed() {
            return self.set_cursor(self.selection.range().end);
        }

        let Some(next_index) = self.next_cursor_index() else {
            return false;
        };

        self.move_cursor_to(next_index, extend)
    }

    /// Moves the caret to the start of the text, optionally extending selection.
    pub(crate) fn move_cursor_to_start(&mut self, extend: bool) -> bool {
        self.move_cursor_to(0, extend)
    }

    /// Moves the caret to the end of the text, optionally extending selection.
    pub(crate) fn move_cursor_to_end(&mut self, extend: bool) -> bool {
        self.move_cursor_to(self.text.len(), extend)
    }

    fn replace_selection(&mut self, inserted: &str) {
        self.debug_assert_selection_boundaries(self.selection);
        let range = self.selection.range();
        let caret_index = range.start + inserted.len();
        self.text.replace_range(range, inserted);
        self.bump_content_revision();
        self.set_selection(TextSelection::collapsed(caret_index));
    }

    fn delete_selection(&mut self) -> bool {
        let range = self.selection.range();
        if range.is_empty() {
            return false;
        }

        let caret_index = range.start;
        self.text.drain(range);
        self.bump_content_revision();
        self.set_selection(TextSelection::collapsed(caret_index));
        true
    }

    fn move_cursor_to(&mut self, index: usize, extend: bool) -> bool {
        if extend {
            self.extend_selection_to(index)
        } else {
            self.set_cursor(index)
        }
    }

    fn set_selection(&mut self, selection: TextSelection) -> bool {
        self.debug_assert_selection_boundaries(selection);
        if self.selection == selection {
            return false;
        }
        self.selection = selection;
        self.bump_visual_revision();
        true
    }

    fn bump_content_revision(&mut self) {
        self.content_revision = self
            .content_revision
            .checked_add(1)
            .expect("text input content revision exhausted");
    }

    fn bump_visual_revision(&mut self) {
        self.visual_revision = self
            .visual_revision
            .checked_add(1)
            .expect("text input visual revision exhausted");
    }

    fn previous_cursor_index(&self) -> Option<usize> {
        self.text[..self.cursor_index()]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
    }

    fn next_cursor_index(&self) -> Option<usize> {
        let ch = self.text[self.cursor_index()..].chars().next()?;
        Some(self.cursor_index() + ch.len_utf8())
    }

    fn index_is_valid(&self, index: usize) -> bool {
        let valid = index <= self.text.len() && self.text.is_char_boundary(index);
        debug_assert!(valid, "text input selection must stay on UTF-8 boundaries");
        valid
    }

    fn debug_assert_selection_boundaries(&self, selection: TextSelection) {
        debug_assert!(
            self.index_is_valid(selection.anchor) && self.index_is_valid(selection.focus),
            "text input selection must stay on UTF-8 boundaries"
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

    /// Returns a monotonic aggregate revision for text content changes.
    pub(crate) fn content_revision(&self) -> u64 {
        self.states.values().fold(0u64, |revision, state| {
            revision.wrapping_add(state.content_revision())
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
        assert!(!text_input.move_cursor_left(false));
        assert_eq!(text_input.revision(), 0);

        assert!(text_input.insert_char('a'));
        assert_eq!(text_input.content_revision(), 1);
        assert_eq!(text_input.visual_revision(), 1);

        assert!(text_input.move_cursor_left(false));
        assert_eq!(text_input.content_revision(), 1);
        assert_eq!(text_input.visual_revision(), 2);
        assert!(!text_input.move_cursor_left(false));
        assert_eq!(text_input.visual_revision(), 2);

        assert!(text_input.move_cursor_right(false));
        assert_eq!(text_input.visual_revision(), 3);
        assert!(!text_input.move_cursor_right(false));
        assert_eq!(text_input.visual_revision(), 3);

        assert!(text_input.delete_backward());
        assert_eq!(text_input.content_revision(), 2);
        assert_eq!(text_input.visual_revision(), 4);
        assert_eq!(text_input.text(), "");
    }

    #[test]
    fn bulk_insert_updates_at_cursor_and_preserves_utf8_boundaries() {
        let mut text_input = TextInputState::default();

        assert!(text_input.insert_char('a'));
        assert!(text_input.insert_char('中'));
        assert!(text_input.insert_char('d'));
        assert!(text_input.move_cursor_left(false));

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
        assert_eq!(text_input.content_revision(), 1);
        assert_eq!(text_input.visual_revision(), 1);
    }

    #[test]
    fn selection_replacement_and_deletion_preserve_utf8_boundaries() {
        let mut text_input = TextInputState::new("a中βd");
        assert!(text_input.move_cursor_left(true));
        assert!(text_input.move_cursor_left(true));
        assert_eq!(text_input.selected_text(), Some("βd"));

        assert!(text_input.insert_text("ç"));

        assert_eq!(text_input.text(), "a中ç");
        assert_eq!(text_input.cursor_index(), "a中ç".len());
        assert!(text_input
            .text()
            .is_char_boundary(text_input.cursor_index()));

        assert!(text_input.move_cursor_left(true));
        assert_eq!(text_input.selected_text(), Some("ç"));
        assert!(text_input.delete_backward());
        assert_eq!(text_input.text(), "a中");
        assert_eq!(text_input.cursor_index(), "a中".len());
    }

    #[test]
    fn delete_forward_deletes_selection_or_next_character() {
        let mut text_input = TextInputState::new("a中b");
        assert!(text_input.set_cursor(1));
        assert!(text_input.delete_forward());
        assert_eq!(text_input.text(), "ab");
        assert_eq!(text_input.cursor_index(), 1);

        assert!(text_input.move_cursor_right(true));
        assert_eq!(text_input.selected_text(), Some("b"));
        assert!(text_input.delete_forward());
        assert_eq!(text_input.text(), "a");
        assert_eq!(text_input.cursor_index(), 1);
    }

    #[test]
    fn select_all_and_home_end_update_only_visual_revision() {
        let mut text_input = TextInputState::new("a中b");
        let content_revision = text_input.content_revision();

        assert!(text_input.select_all());
        assert_eq!(text_input.selected_text(), Some("a中b"));
        assert_eq!(text_input.content_revision(), content_revision);
        assert_eq!(text_input.visual_revision(), 1);

        assert!(text_input.move_cursor_to_start(false));
        assert_eq!(text_input.selection(), TextSelection::collapsed(0));
        assert!(text_input.move_cursor_to_end(true));
        assert_eq!(
            text_input.selection(),
            TextSelection {
                anchor: 0,
                focus: "a中b".len(),
            }
        );
        assert_eq!(text_input.content_revision(), content_revision);
    }

    #[test]
    fn shift_arrows_extend_selection_and_plain_arrows_collapse() {
        let mut text_input = TextInputState::new("a中b");

        assert!(text_input.move_cursor_left(true));
        assert_eq!(text_input.selected_text(), Some("b"));
        assert!(text_input.move_cursor_left(true));
        assert_eq!(text_input.selected_text(), Some("中b"));
        assert!(text_input.move_cursor_right(false));
        assert_eq!(
            text_input.selection(),
            TextSelection::collapsed("a中b".len())
        );
        assert!(text_input.move_cursor_left(true));
        assert!(text_input.move_cursor_left(false));
        assert_eq!(
            text_input.selection(),
            TextSelection::collapsed("a中".len())
        );
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
