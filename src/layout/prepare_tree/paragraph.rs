//! Prepared paragraph types shared by the tree prepare and layout stages.

use std::sync::Arc;

use crate::draw_list::ImageData;
use crate::layout::line_break::BreakOpportunity;
use crate::layout::prepare::PreparedGlyph;
use crate::layout::tree::{
    AtomBaseline, InlineAtomStyle, LocalPaintCommand, NodeId, ParagraphStyle,
};

#[derive(Clone, Debug)]
pub(crate) struct PreparedParagraph {
    pub(crate) node_id: NodeId,
    pub(crate) atoms: Vec<PreparedInlineAtom>,
    pub(crate) items: Vec<PreparedParagraphItem>,
    pub(crate) default_ascent: f32,
    pub(crate) default_line_height: f32,
    pub(crate) style: ParagraphStyle,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedInlineAtom {
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
    Chip { measured_text: Vec<PreparedGlyph> },
    Icon { glyph: PreparedGlyph },
    Image { data_ref: Arc<ImageData> },
    Custom { paint: Arc<[LocalPaintCommand]> },
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
