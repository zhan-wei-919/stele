//! Shared text styling, validation, and error types for tree layout.

mod error;
mod style;
pub(crate) mod validation;

pub use error::DocumentError;
pub use style::TextStyle;
