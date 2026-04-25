//! Prepare-stage cache for the rich-text tree path.

mod embed;
mod paragraph;
mod prepare;
mod text_input;

use std::collections::HashMap;

use crate::layout::tree::BlockStyle;
use crate::layout::tree::{AnchorKey, FlowDirection, NodeId, OverlayAnchor};

pub(crate) use embed::{PreparedEmbed, PreparedEmbedPayload};
pub(crate) use paragraph::{
    PreparedAtomPayload, PreparedInlineAtom, PreparedParagraph, PreparedParagraphItem,
};
#[cfg(test)]
pub(crate) use prepare::prepare_tree;
pub(crate) use prepare::prepare_tree_with_text_inputs;
#[cfg(test)]
pub(crate) use text_input::EmptyTextInputResolver;
pub(crate) use text_input::{PreparedTextInput, TextCaretStop, TextInputResolver, TextInputValue};

#[derive(Clone, Debug)]
pub(crate) struct PreparedTree {
    pub(crate) root: PreparedBlockNode,
    pub(crate) anchor_index: HashMap<AnchorKey, NodeId>,
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedBlockNode {
    Stack(PreparedStack),
    Paragraph(PreparedParagraph),
    Embed(PreparedEmbed),
    TextInput(PreparedTextInput),
    Overlay(PreparedOverlay),
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedStack {
    pub(crate) node_id: NodeId,
    pub(crate) direction: FlowDirection,
    pub(crate) children: Vec<PreparedBlockNode>,
    pub(crate) style: BlockStyle,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedOverlay {
    pub(crate) node_id: NodeId,
    pub(crate) anchor: OverlayAnchor,
    pub(crate) child: Box<PreparedBlockNode>,
}
