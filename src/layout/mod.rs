//! Rich-text tree layout pipeline from semantic nodes to store-owned scene batches.
mod document;
pub(crate) mod layout_tree;
mod line_break;
mod prepare;
pub(crate) mod prepare_tree;
pub(crate) mod tree;

pub use document::DocumentError;
