//! Prepared inline measurements shared by prepare and layout stages.

use crate::font::{FontSelection, LineMetrics, MeasuredGlyph};

use super::super::document::TextStyle;
use super::super::line_break::BreakOpportunity;

/// A block's prepared inline content plus cached metrics for later layout.
#[derive(Clone, Debug)]
pub(crate) struct PreparedBlock {
    pub(crate) block_index: usize,
    pub(crate) items: Vec<PreparedItem>,
    pub(crate) default_ascent: f32,
    pub(crate) default_line_height: f32,
}

/// A layout item that is either visible content or an explicit hard break.
#[derive(Clone, Debug)]
pub(crate) enum PreparedItem {
    Glyph(PreparedGlyph),
    Break(BreakOpportunity),
}

/// One measured glyph plus the styling needed to emit renderer primitives later.
#[derive(Clone, Debug)]
pub(crate) struct PreparedGlyph {
    pub(crate) span_index: usize,
    pub(crate) font_id: u32,
    pub(crate) glyph_id: u16,
    pub(crate) font_size: f32,
    pub(crate) advance: f32,
    pub(crate) ascent: f32,
    pub(crate) line_height: f32,
    pub(crate) color: [f32; 4],
    pub(crate) background_color: Option<[f32; 4]>,
    pub(crate) underline: bool,
    pub(crate) strikethrough: bool,
    pub(crate) break_after: BreakOpportunity,
}

impl PreparedGlyph {
    pub(super) fn from_measurement(
        span_index: usize,
        style: TextStyle,
        font_selection: FontSelection,
        metrics: LineMetrics,
        measured: MeasuredGlyph,
    ) -> Self {
        Self {
            span_index,
            font_id: font_selection.resolved_font_id,
            glyph_id: measured.glyph_id,
            font_size: style.font_size(),
            advance: measured.advance,
            ascent: metrics.ascent,
            line_height: metrics.line_height,
            color: style.color(),
            background_color: style.background_color(),
            underline: style.underline(),
            strikethrough: style.strikethrough(),
            break_after: BreakOpportunity::Forbidden,
        }
    }
}
