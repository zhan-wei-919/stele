//! Document layout pipeline from styled blocks to store-owned scene batches.
mod document;
mod flow;
pub(crate) mod layout_tree;
mod line_break;
mod prepare;
pub(crate) mod prepare_tree;
pub(crate) mod tree;

pub(crate) use document::{Block, BlockRect, Document, Span, TextStyle};
pub(crate) use flow::{layout_document, LayoutBlock};
pub(crate) use prepare::{prepare_document, PreparedBlock};
