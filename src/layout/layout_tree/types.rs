//! Layout-stage output types for the rich-text tree path.

use std::sync::Arc;

use crate::draw_list::{ImageData, PathVerb, PositionedGlyph, RectCmd};
use crate::layout::tree::{NodeId, PathStroke};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl LayoutRect {
    pub(crate) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        debug_assert!(
            x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite(),
            "layout rect values must be finite"
        );
        debug_assert!(
            width >= 0.0 && height >= 0.0,
            "layout rect size must stay non-negative"
        );
        Self {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
        }
    }

    pub(crate) fn x(self) -> f32 {
        self.x
    }

    pub(crate) fn y(self) -> f32 {
        self.y
    }

    pub(crate) fn width(self) -> f32 {
        self.width
    }

    pub(crate) fn height(self) -> f32 {
        self.height
    }

    pub(crate) fn right(self) -> f32 {
        self.x + self.width
    }

    pub(crate) fn bottom(self) -> f32 {
        self.y + self.height
    }

    pub(crate) fn intersect(self, other: Self) -> Self {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Self::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
    }

    pub(crate) fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LayoutConstraints {
    pub(crate) max_width: f32,
    pub(crate) viewport: [f32; 2],
}

impl LayoutConstraints {
    pub(crate) fn new(max_width: f32, viewport: [f32; 2]) -> Self {
        debug_assert!(max_width.is_finite() && max_width > 0.0);
        Self {
            max_width,
            viewport,
        }
    }

    pub(crate) fn viewport_rect(self) -> LayoutRect {
        LayoutRect::new(
            0.0,
            0.0,
            self.viewport[0].max(0.0),
            self.viewport[1].max(0.0),
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutTree {
    pub(crate) root: LayoutBlock,
    pub(crate) overlays: Vec<LayoutBlock>,
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutBlock {
    pub(crate) node_id: NodeId,
    pub(crate) doc_order: u32,
    pub(crate) rect: LayoutRect,
    pub(crate) clip_rect: LayoutRect,
    pub(crate) z_order: u32,
    pub(crate) background: Option<[f32; 4]>,
    pub(crate) content: LayoutBlockContent,
}

#[derive(Clone, Debug)]
pub(crate) enum LayoutBlockContent {
    Stack { children: Vec<LayoutBlock> },
    Paragraph(LayoutParagraph),
    Embed(LayoutEmbed),
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutParagraph {
    pub(crate) rect: LayoutRect,
    pub(crate) lines: Vec<LayoutLine>,
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutLine {
    pub(crate) line_height: f32,
    pub(crate) y: f32,
    pub(crate) runs: Vec<LayoutRun>,
}

#[derive(Clone, Debug)]
pub(crate) enum LayoutRun {
    Text(LayoutTextRun),
    Atom(LayoutAtomRun),
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutTextRun {
    pub(crate) glyphs: Vec<PositionedGlyph>,
    pub(crate) decoration_rects: Vec<RectCmd>,
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutAtomRun {
    pub(crate) rect: LayoutRect,
    pub(crate) payload: LayoutAtomPayload,
}

#[derive(Clone, Debug)]
pub(crate) enum LayoutAtomPayload {
    Chip {
        background: Option<[f32; 4]>,
        glyphs: Vec<PositionedGlyph>,
    },
    Icon {
        glyph: PositionedGlyph,
    },
    Image {
        data_ref: Arc<ImageData>,
    },
    Custom,
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutEmbed {
    pub(crate) rect: LayoutRect,
    pub(crate) kind: LayoutEmbedKind,
    pub(crate) intrinsic_size: [f32; 2],
}

#[derive(Clone, Debug)]
pub(crate) enum LayoutEmbedKind {
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
