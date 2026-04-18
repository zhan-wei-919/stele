//! Shared text styling, validation, and error types for tree layout.

mod error;
mod style;
pub(crate) mod validation;

pub(crate) use error::DocumentError;
pub(crate) use style::TextStyle;
