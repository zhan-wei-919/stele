//! Prepared paragraph types shared by the tree prepare and layout stages.

use std::collections::HashMap;
use std::sync::Arc;

use crate::draw_list::ImageData;
use crate::layout::line_break::BreakOpportunity;
use crate::layout::prepare::PreparedGlyph;
use crate::layout::tree::{
    AnchorKey, AtomBaseline, InlineAtomStyle, NodeId, ParagraphStyle, TextStyle,
};

#[derive(Clone, Debug)]
pub(crate) struct PreparedParagraph {
    pub(crate) node_id: NodeId,
    pub(crate) anchor_key: Option<AnchorKey>,
    pub(crate) inlines: Vec<PreparedInline>,
    pub(crate) items: Vec<PreparedParagraphItem>,
    pub(crate) break_map: HashMap<usize, BreakOpportunity>,
    pub(crate) default_ascent: f32,
    pub(crate) default_line_height: f32,
    pub(crate) style: ParagraphStyle,
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedInline {
    Text(PreparedInlineText),
    Atom(PreparedInlineAtom),
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedInlineText {
    pub(crate) run_index: usize,
    pub(crate) glyphs: Vec<PreparedGlyph>,
    pub(crate) style: TextStyle,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedInlineAtom {
    pub(crate) atom_index: usize,
    pub(crate) intrinsic_size: [f32; 2],
    pub(crate) baseline: AtomBaseline,
    pub(crate) style: InlineAtomStyle,
    pub(crate) payload: PreparedAtomPayload,
}

impl PreparedInlineAtom {
    pub(crate) fn outer_width(&self) -> f32 {
        self.style.margin.horizontal() + self.intrinsic_size[0]
    }

    pub(crate) fn outer_height(&self) -> f32 {
        self.style.margin.vertical() + self.intrinsic_size[1]
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedAtomPayload {
    Chip {
        foreground: [f32; 4],
        background: Option<[f32; 4]>,
        measured_text: Vec<PreparedGlyph>,
    },
    Icon {
        glyph: PreparedGlyph,
    },
    Image {
        data_ref: Arc<ImageData>,
    },
    Custom,
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedParagraphItem {
    Glyph(PreparedGlyph),
    Atom {
        atom_index: usize,
        break_after: BreakOpportunity,
    },
    Break(BreakOpportunity),
}

impl PreparedParagraphItem {
    pub(crate) fn break_after(&self) -> BreakOpportunity {
        match self {
            Self::Glyph(glyph) => glyph.break_after,
            Self::Atom { break_after, .. } | Self::Break(break_after) => *break_after,
        }
    }
}
