//! Document layout pipeline from styled blocks to renderer draw groups.

mod bridge;
mod document;
mod layout;
mod line_break;
mod prepare;

pub(crate) use bridge::bridge_layout;
pub(crate) use document::{Block, BlockRect, Document, Span, TextStyle};
pub(crate) use layout::{layout_document, LayoutBlock, LayoutLine, LayoutRun};
pub(crate) use prepare::{prepare_document, PreparedBlock};
