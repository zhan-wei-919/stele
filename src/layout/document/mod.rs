//! Document model consumed by the layout pipeline.

mod block;
mod error;
mod style;
mod validation;

pub(crate) use block::{Block, BlockRect, Document, Span};
pub(crate) use error::DocumentError;
pub(crate) use style::TextStyle;
