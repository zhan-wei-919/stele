//! Prepare-stage cache for the rich-text tree path.

mod embed;
mod paragraph;
mod prepare;

use std::collections::HashMap;

use crate::layout::tree::BlockStyle;
use crate::layout::tree::{AnchorKey, FlowDirection, NodeId, OverlayAnchor};

pub(crate) use embed::{PreparedEmbed, PreparedEmbedPayload};
pub(crate) use paragraph::{
    PreparedAtomPayload, PreparedInline, PreparedInlineAtom, PreparedInlineText, PreparedParagraph,
    PreparedParagraphItem,
};
pub(crate) use prepare::prepare_tree;

#[derive(Clone, Debug)]
pub(crate) struct PreparedTree {
    pub(crate) root: PreparedBlockNode,
    pub(crate) anchor_index: HashMap<AnchorKey, NodeId>,
    pub(crate) default_ascent: f32,
    pub(crate) default_line_height: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedBlockNode {
    Stack(PreparedStack),
    Paragraph(PreparedParagraph),
    Embed(PreparedEmbed),
    Overlay(PreparedOverlay),
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedStack {
    pub(crate) node_id: NodeId,
    pub(crate) anchor_key: Option<AnchorKey>,
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
