//! Pure arithmetic layout over prepared inline measurements.

mod break_lines;
mod decorations;
mod document;
mod place_lines;
mod types;

pub(crate) use document::layout_document;
pub(crate) use types::{LayoutBlock, LayoutLine, LayoutRun};
