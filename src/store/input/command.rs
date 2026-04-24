//! Commands that describe store-owned interaction intent.

/// Semantic input command consumed by the store reducer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Command {
    ScrollByLine(i32),
    ScrollByPage(i32),
    ScrollToStart,
    ScrollToEnd,
    ScrollByPixels(f32),
    InsertChar(char),
    DeleteBackward,
    MoveCursorLeft,
    MoveCursorRight,
}
