//! Validation errors produced by shared tree layout types.

/// Validation errors produced while constructing document input types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DocumentError {
    InvalidColor,
    InvalidFontSize,
    InvalidLetterSpacing,
    InvalidEdges,
    InvalidIntrinsicSize,
    InvalidLineHeight,
    InvalidBorderWidth,
    InvalidAnchorKey,
    DuplicateAnchorKey { key: String },
    UnknownOverlayTarget { key: String },
    RootOverlay,
}
