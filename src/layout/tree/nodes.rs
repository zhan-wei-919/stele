//! Rich-text tree input nodes.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::draw_list::{ImageData, LineCap, LineJoin, PathVerb};
use crate::layout::document::DocumentError;

use super::style::{BlockStyle, InlineAtomStyle, ParagraphStyle};
use super::text_style::TextStyle;
use super::validation::validate_dimension;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct NodeId(u64);

impl NodeId {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct AnchorKey(String);

impl AnchorKey {
    pub(crate) fn new(value: impl Into<String>) -> Result<Self, DocumentError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DocumentError::InvalidAnchorKey);
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

// The input tree supports both flow directions even though the demo currently instantiates only
// vertical stacks in production code.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum FlowDirection {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Clone, Debug)]
pub(crate) struct DocumentTree {
    root: BlockNode,
    anchor_index: HashMap<AnchorKey, NodeId>,
}

impl DocumentTree {
    pub(crate) fn new(mut root: BlockNode) -> Result<Self, DocumentError> {
        if matches!(root, BlockNode::Overlay(_)) {
            return Err(DocumentError::RootOverlay);
        }

        let mut next_node_id = 0u64;
        let mut seen_anchors = HashSet::new();
        let mut flow_anchor_index = HashMap::new();
        assign_node_ids(
            &mut root,
            &mut next_node_id,
            &mut seen_anchors,
            &mut flow_anchor_index,
            false,
        )?;
        validate_overlay_targets(&root, &flow_anchor_index)?;

        Ok(Self {
            root,
            anchor_index: flow_anchor_index,
        })
    }

    pub(crate) fn root(&self) -> &BlockNode {
        &self.root
    }

    pub(crate) fn anchor_index(&self) -> &HashMap<AnchorKey, NodeId> {
        &self.anchor_index
    }
}

#[derive(Clone, Debug)]
pub(crate) enum BlockNode {
    Stack(StackNode),
    Paragraph(ParagraphNode),
    Embed(BlockEmbedNode),
    Overlay(OverlayNode),
}

impl BlockNode {
    fn set_node_id(&mut self, node_id: NodeId) {
        match self {
            Self::Stack(node) => node.node_id = node_id,
            Self::Paragraph(node) => node.node_id = node_id,
            Self::Embed(node) => node.node_id = node_id,
            Self::Overlay(node) => node.node_id = node_id,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StackNode {
    pub(crate) node_id: NodeId,
    pub(crate) anchor_key: Option<AnchorKey>,
    pub(crate) direction: FlowDirection,
    pub(crate) children: Vec<BlockNode>,
    pub(crate) style: BlockStyle,
}

impl StackNode {
    pub(crate) fn new(
        direction: FlowDirection,
        children: Vec<BlockNode>,
        style: BlockStyle,
    ) -> Result<Self, DocumentError> {
        style.validate()?;
        Ok(Self {
            node_id: NodeId::new(0),
            anchor_key: None,
            direction,
            children,
            style,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ParagraphNode {
    pub(crate) node_id: NodeId,
    pub(crate) anchor_key: Option<AnchorKey>,
    pub(crate) inlines: Vec<InlineNode>,
    pub(crate) style: ParagraphStyle,
}

impl ParagraphNode {
    pub(crate) fn new(
        inlines: Vec<InlineNode>,
        style: ParagraphStyle,
    ) -> Result<Self, DocumentError> {
        style.validate()?;
        Ok(Self {
            node_id: NodeId::new(0),
            anchor_key: None,
            inlines,
            style,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) enum InlineNode {
    Text(TextRun),
    Atom(InlineAtom),
}

#[derive(Clone, Debug)]
pub(crate) struct TextRun {
    pub(crate) text: String,
    pub(crate) style: TextStyle,
}

impl TextRun {
    pub(crate) fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct InlineAtom {
    pub(crate) kind: InlineAtomKind,
    pub(crate) style: InlineAtomStyle,
}

impl InlineAtom {
    pub(crate) fn new(kind: InlineAtomKind, style: InlineAtomStyle) -> Result<Self, DocumentError> {
        style.validate()?;
        kind.validate()?;
        Ok(Self { kind, style })
    }
}

// The semantic tree supports multiple inline atom payloads even though the demo only instantiates
// a subset of them in production code.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum InlineAtomKind {
    Chip {
        label: String,
        text_style: TextStyle,
    },
    Icon {
        glyph_id: u16,
        font_id: u32,
        size: f32,
        color: [f32; 4],
    },
    Image {
        data_ref: Arc<ImageData>,
    },
    Custom {
        measured_size: [f32; 2],
    },
}

impl InlineAtomKind {
    fn validate(&self) -> Result<(), DocumentError> {
        match self {
            Self::Chip { .. } | Self::Image { .. } => Ok(()),
            Self::Icon { size, .. } => validate_dimension(*size, false),
            Self::Custom { measured_size } => {
                validate_dimension(measured_size[0], false)?;
                validate_dimension(measured_size[1], false)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BlockEmbedNode {
    pub(crate) node_id: NodeId,
    pub(crate) anchor_key: Option<AnchorKey>,
    pub(crate) kind: BlockEmbedKind,
    pub(crate) style: BlockStyle,
}

impl BlockEmbedNode {
    pub(crate) fn new(kind: BlockEmbedKind, style: BlockStyle) -> Result<Self, DocumentError> {
        style.validate()?;
        kind.validate()?;
        Ok(Self {
            node_id: NodeId::new(0),
            anchor_key: None,
            kind,
            style,
        })
    }
}

// The semantic tree supports custom embeds even though the demo currently instantiates only image
// and path embeds in production code.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum BlockEmbedKind {
    Image {
        data_ref: Arc<ImageData>,
        intrinsic_size: [f32; 2],
    },
    Path {
        verbs: Vec<PathVerb>,
        fill: Option<[f32; 4]>,
        stroke: Option<PathStroke>,
        intrinsic_size: [f32; 2],
    },
    Custom {
        intrinsic_size: [f32; 2],
    },
}

impl BlockEmbedKind {
    fn validate(&self) -> Result<(), DocumentError> {
        let intrinsic_size = match self {
            Self::Image { intrinsic_size, .. }
            | Self::Path { intrinsic_size, .. }
            | Self::Custom { intrinsic_size } => intrinsic_size,
        };
        validate_dimension(intrinsic_size[0], false)?;
        validate_dimension(intrinsic_size[1], false)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PathStroke {
    pub(crate) color: [f32; 4],
    pub(crate) width: f32,
    pub(crate) line_cap: LineCap,
    pub(crate) line_join: LineJoin,
}

#[derive(Clone, Debug)]
pub(crate) struct OverlayNode {
    pub(crate) node_id: NodeId,
    pub(crate) anchor: OverlayAnchor,
    pub(crate) child: Box<BlockNode>,
}

impl OverlayNode {
    pub(crate) fn new(anchor: OverlayAnchor, child: BlockNode) -> Self {
        Self {
            node_id: NodeId::new(0),
            anchor,
            child: Box::new(child),
        }
    }
}

// Overlay anchors can target either the viewport or a block-relative anchor; the demo currently
// instantiates only the block-relative path in production code.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum OverlayAnchor {
    Viewport { offset: [f32; 2] },
    BlockRelative { target: AnchorKey, offset: [f32; 2] },
}

fn assign_node_ids(
    node: &mut BlockNode,
    next_node_id: &mut u64,
    seen_anchors: &mut HashSet<AnchorKey>,
    flow_anchor_index: &mut HashMap<AnchorKey, NodeId>,
    in_overlay_subtree: bool,
) -> Result<(), DocumentError> {
    let node_id = NodeId::new(*next_node_id);
    *next_node_id += 1;
    node.set_node_id(node_id);

    match node {
        BlockNode::Stack(stack) => {
            register_anchor(
                stack.anchor_key.as_ref(),
                node_id,
                seen_anchors,
                flow_anchor_index,
                in_overlay_subtree,
            )?;
            for child in &mut stack.children {
                assign_node_ids(
                    child,
                    next_node_id,
                    seen_anchors,
                    flow_anchor_index,
                    in_overlay_subtree,
                )?;
            }
        }
        BlockNode::Paragraph(paragraph) => {
            register_anchor(
                paragraph.anchor_key.as_ref(),
                node_id,
                seen_anchors,
                flow_anchor_index,
                in_overlay_subtree,
            )?;
        }
        BlockNode::Embed(embed) => {
            register_anchor(
                embed.anchor_key.as_ref(),
                node_id,
                seen_anchors,
                flow_anchor_index,
                in_overlay_subtree,
            )?;
        }
        BlockNode::Overlay(overlay) => {
            assign_node_ids(
                overlay.child.as_mut(),
                next_node_id,
                seen_anchors,
                flow_anchor_index,
                true,
            )?;
        }
    }
    Ok(())
}

fn register_anchor(
    anchor_key: Option<&AnchorKey>,
    node_id: NodeId,
    seen_anchors: &mut HashSet<AnchorKey>,
    flow_anchor_index: &mut HashMap<AnchorKey, NodeId>,
    in_overlay_subtree: bool,
) -> Result<(), DocumentError> {
    let Some(anchor_key) = anchor_key.cloned() else {
        return Ok(());
    };
    if !seen_anchors.insert(anchor_key.clone()) {
        return Err(DocumentError::DuplicateAnchorKey {
            key: anchor_key.as_str().to_owned(),
        });
    }
    if !in_overlay_subtree {
        flow_anchor_index.insert(anchor_key, node_id);
    }
    Ok(())
}

fn validate_overlay_targets(
    node: &BlockNode,
    flow_anchor_index: &HashMap<AnchorKey, NodeId>,
) -> Result<(), DocumentError> {
    match node {
        BlockNode::Stack(stack) => {
            for child in &stack.children {
                validate_overlay_targets(child, flow_anchor_index)?;
            }
        }
        BlockNode::Overlay(overlay) => {
            if let OverlayAnchor::BlockRelative { target, .. } = &overlay.anchor {
                if !flow_anchor_index.contains_key(target) {
                    return Err(DocumentError::UnknownOverlayTarget {
                        key: target.as_str().to_owned(),
                    });
                }
            }
            validate_overlay_targets(overlay.child.as_ref(), flow_anchor_index)?;
        }
        BlockNode::Paragraph(_) | BlockNode::Embed(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AnchorKey, BlockNode, DocumentTree, FlowDirection, OverlayAnchor, OverlayNode,
        ParagraphNode, StackNode, TextRun,
    };
    use crate::layout::document::DocumentError;
    use crate::layout::tree::style::ParagraphStyle;
    use crate::layout::tree::text_style::TextStyle;

    fn body_style() -> TextStyle {
        TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid")
    }

    #[test]
    fn rejects_overlay_root() {
        let paragraph = BlockNode::Paragraph(
            ParagraphNode::new(
                vec![super::InlineNode::Text(TextRun::new("body", body_style()))],
                ParagraphStyle::default(),
            )
            .expect("paragraph must be valid"),
        );
        let root = BlockNode::Overlay(OverlayNode::new(
            OverlayAnchor::Viewport { offset: [0.0, 0.0] },
            paragraph,
        ));
        assert!(matches!(
            DocumentTree::new(root),
            Err(DocumentError::RootOverlay)
        ));
    }

    #[test]
    fn validates_duplicate_anchor_keys() {
        let mut left = ParagraphNode::new(
            vec![super::InlineNode::Text(TextRun::new("left", body_style()))],
            ParagraphStyle::default(),
        )
        .expect("paragraph must be valid");
        left.anchor_key = Some(AnchorKey::new("dup").expect("anchor must be valid"));
        let mut right = ParagraphNode::new(
            vec![super::InlineNode::Text(TextRun::new("right", body_style()))],
            ParagraphStyle::default(),
        )
        .expect("paragraph must be valid");
        right.anchor_key = Some(AnchorKey::new("dup").expect("anchor must be valid"));

        let root = BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Paragraph(left), BlockNode::Paragraph(right)],
                super::super::style::BlockStyle::default(),
            )
            .expect("stack must be valid"),
        );
        assert!(matches!(
            DocumentTree::new(root),
            Err(DocumentError::DuplicateAnchorKey { key }) if key == "dup"
        ));
    }

    #[test]
    fn validates_overlay_targets_against_flow_nodes_only() {
        let mut paragraph = ParagraphNode::new(
            vec![super::InlineNode::Text(TextRun::new("body", body_style()))],
            ParagraphStyle::default(),
        )
        .expect("paragraph must be valid");
        paragraph.anchor_key = Some(AnchorKey::new("body").expect("anchor must be valid"));
        let overlay = BlockNode::Overlay(OverlayNode::new(
            OverlayAnchor::BlockRelative {
                target: AnchorKey::new("body").expect("anchor must be valid"),
                offset: [0.0, 0.0],
            },
            BlockNode::Paragraph(
                ParagraphNode::new(
                    vec![super::InlineNode::Text(TextRun::new(
                        "overlay",
                        body_style(),
                    ))],
                    ParagraphStyle::default(),
                )
                .expect("paragraph must be valid"),
            ),
        ));
        let root = BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Paragraph(paragraph), overlay],
                super::super::style::BlockStyle::default(),
            )
            .expect("stack must be valid"),
        );

        DocumentTree::new(root).expect("overlay target must resolve");
    }
}
