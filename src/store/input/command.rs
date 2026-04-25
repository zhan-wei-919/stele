//! Commands that describe store-owned interaction intent.

use crate::layout::tree::TextInputId;

/// Semantic input command consumed by the store reducer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    // Delegates and tests can still focus fields directly; pointer focus now uses caret placement.
    #[cfg_attr(not(test), allow(dead_code))]
    FocusTextInput(Option<TextInputId>),
    ScrollByLine(i32),
    ScrollByPage(i32),
    ScrollToStart,
    ScrollToEnd,
    ScrollByPixels(f32),
    InsertChar(char),
    InsertText(String),
    DeleteBackward,
    DeleteForward,
    SelectAll,
    CopySelection,
    CutSelection,
    MoveCursorLeft {
        extend: bool,
    },
    MoveCursorRight {
        extend: bool,
    },
    MoveCursorToStart {
        extend: bool,
    },
    MoveCursorToEnd {
        extend: bool,
    },
    SetCursorFromPoint {
        point: [f32; 2],
    },
    ExtendSelectionFromPoint {
        point: [f32; 2],
    },
    EndSelectionDrag,
}
