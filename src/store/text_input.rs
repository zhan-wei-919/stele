//! Store-local text input buffer for focused text editing commands.

/// Editable text and cursor state for a focused text input target.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextInputState {
    text: String,
    cursor_index: usize,
    revision: u64,
}

impl TextInputState {
    /// Returns the current text contents.
    #[cfg(test)]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Returns the cursor as a UTF-8 byte index into the current text.
    #[cfg(test)]
    pub(crate) fn cursor_index(&self) -> usize {
        self.cursor_index
    }

    /// Returns the monotonic state revision used for cheap change detection.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Inserts one character at the cursor and advances past it.
    pub(crate) fn insert_char(&mut self, ch: char) {
        self.debug_assert_cursor_boundary();
        self.text.insert(self.cursor_index, ch);
        self.cursor_index += ch.len_utf8();
        self.bump_revision();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_increments_only_when_state_changes() {
        let mut text_input = TextInputState::default();

        assert_eq!(text_input.revision(), 0);
        assert!(!text_input.delete_backward());
        assert!(!text_input.move_cursor_left());
        assert_eq!(text_input.revision(), 0);

        text_input.insert_char('a');
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
}
