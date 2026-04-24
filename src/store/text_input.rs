//! Store-local text input buffer used by reducer-level edit command tests.

/// Editable text and cursor state for a focused text input target.
// Reserved for focus plumbing; reducer tests lock down the state contract first.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextInputState {
    text: String,
    cursor_index: usize,
}

// Reserved for focus plumbing; reducer tests lock down the state contract first.
#[cfg_attr(not(test), allow(dead_code))]
impl TextInputState {
    /// Returns the current text contents.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Returns the cursor as a UTF-8 byte index into the current text.
    pub(crate) fn cursor_index(&self) -> usize {
        self.cursor_index
    }

    /// Inserts one character at the cursor and advances past it.
    pub(crate) fn insert_char(&mut self, ch: char) {
        self.debug_assert_cursor_boundary();
        self.text.insert(self.cursor_index, ch);
        self.cursor_index += ch.len_utf8();
    }

    /// Deletes the character before the cursor when one exists.
    pub(crate) fn delete_backward(&mut self) -> bool {
        self.debug_assert_cursor_boundary();
        let Some(previous_index) = self.previous_cursor_index() else {
            return false;
        };

        self.text.drain(previous_index..self.cursor_index);
        self.cursor_index = previous_index;
        true
    }

    /// Moves the cursor left by one character when possible.
    pub(crate) fn move_cursor_left(&mut self) -> bool {
        self.debug_assert_cursor_boundary();
        let Some(previous_index) = self.previous_cursor_index() else {
            return false;
        };

        self.cursor_index = previous_index;
        true
    }

    /// Moves the cursor right by one character when possible.
    pub(crate) fn move_cursor_right(&mut self) -> bool {
        self.debug_assert_cursor_boundary();
        let Some(next_index) = self.next_cursor_index() else {
            return false;
        };

        self.cursor_index = next_index;
        true
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
