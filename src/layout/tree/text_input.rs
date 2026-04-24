//! Shared normalization rules for semantic text input values.

/// Returns a single-line text input string by dropping control characters.
pub(crate) fn single_line_text(text: &str) -> String {
    text.chars()
        .filter(|ch| is_insertable_text_input_char(*ch))
        .collect()
}

/// Returns whether a character can be inserted into Stele's single-line text inputs.
pub(crate) fn is_insertable_text_input_char(ch: char) -> bool {
    !ch.is_control()
}
