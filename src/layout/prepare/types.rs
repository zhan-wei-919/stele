//! Prepared inline measurements shared by prepare and layout stages.

use super::super::document::TextStyle;
use super::super::line_break::BreakOpportunity;
use crate::font::{FontSelection, LineMetrics, MeasuredGlyph};

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
    pub(crate) fn from_measurement(
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
