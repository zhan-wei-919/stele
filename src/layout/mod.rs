//! Document layout pipeline from styled blocks to store-owned scene batches.
mod document;
mod flow;
mod line_break;
mod prepare;

pub(crate) use document::{Block, BlockRect, Document, Span, TextStyle};
pub(crate) use flow::{layout_document, LayoutBlock};
pub(crate) use prepare::{prepare_document, PreparedBlock};
