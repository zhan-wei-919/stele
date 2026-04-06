//! Validation errors for document input models.

/// Validation errors produced while constructing document input types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DocumentError {
    InvalidRect,
    InvalidPadding,
    InvalidColor,
    InvalidFontSize,
    InvalidLetterSpacing,
    MissingBlock { block_index: usize },
}
