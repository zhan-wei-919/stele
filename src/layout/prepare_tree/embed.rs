//! Prepared embed payloads for the rich-text tree path.

use std::sync::Arc;

use crate::draw_list::ImageData;
use crate::draw_list::PathVerb;
use crate::layout::tree::{AnchorKey, BlockStyle, NodeId, PathStroke};

#[derive(Clone, Debug)]
pub(crate) struct PreparedEmbed {
    pub(crate) node_id: NodeId,
    pub(crate) anchor_key: Option<AnchorKey>,
    pub(crate) intrinsic_size: [f32; 2],
    pub(crate) style: BlockStyle,
    pub(crate) payload: PreparedEmbedPayload,
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedEmbedPayload {
    Image {
        data_ref: Arc<ImageData>,
    },
    Path {
        verbs: Vec<PathVerb>,
        fill: Option<[f32; 4]>,
        stroke: Option<PathStroke>,
    },
    Custom,
}
