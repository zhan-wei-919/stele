//! Document model consumed by the layout pipeline.

mod block;
mod error;
mod style;
pub(crate) mod validation;

pub(crate) use block::{Block, BlockRect, Document, Span};
pub(crate) use error::DocumentError;
pub(crate) use style::TextStyle;
