//! Pure arithmetic layout for the prepared rich-text tree.

mod paragraph;
mod solve;
mod types;

pub(crate) use solve::layout_tree;
pub(crate) use types::{
    LayoutAtomPayload, LayoutBlock, LayoutBlockContent, LayoutConstraints, LayoutEmbedKind,
    LayoutRect, LayoutRun, LayoutTree,
};
