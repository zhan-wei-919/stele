//! Validation errors produced by shared tree layout types.

/// Validation errors produced while constructing document input types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentError {
    InvalidColor,
    InvalidFontSize,
    InvalidLetterSpacing,
    InvalidEdges,
    InvalidIntrinsicSize,
    InvalidLineHeight,
    InvalidBorderWidth,
    InvalidLocalPaint,
    InvalidAnchorKey,
    DuplicateAnchorKey { key: String },
    DuplicateTextInputId { id: u64 },
    UnknownOverlayTarget { key: String },
    RootOverlay,
}
