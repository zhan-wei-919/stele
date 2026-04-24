//! Commands that describe store-owned interaction intent.

use crate::layout::tree::TextInputId;

/// Semantic input command consumed by the store reducer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    FocusTextInput(Option<TextInputId>),
    ScrollByLine(i32),
    ScrollByPage(i32),
    ScrollToStart,
    ScrollToEnd,
    ScrollByPixels(f32),
    InsertChar(char),
    InsertText(String),
    DeleteBackward,
    MoveCursorLeft,
    MoveCursorRight,
}
