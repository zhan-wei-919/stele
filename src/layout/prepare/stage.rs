//! Span staging helpers used before Unicode break assignment.

use std::collections::HashMap;

use crate::font::{FontSelection, LineMetrics, MeasuredGlyph};

use super::super::document::TextStyle;
use super::super::line_break::BreakOpportunity;
use super::types::{PreparedGlyph, PreparedItem};

/// All inputs required to stage one span into measured glyph items.
pub(super) struct SpanPrepareContext<'a> {
    pub(super) span_index: usize,
    pub(super) text: &'a str,
    pub(super) style: TextStyle,
    pub(super) font_selection: FontSelection,
    pub(super) metrics: LineMetrics,
    pub(super) measured_glyphs: &'a [MeasuredGlyph],
}

/// One span item before Unicode break opportunities are attached.
#[derive(Clone, Debug)]
pub(super) enum StagedItem {
    Glyph {
        byte_end: usize,
        glyph: PreparedGlyph,
    },
    Break,
}

/// Stages one span into glyph and hard-break items while building the full text buffer.
pub(super) fn stage_span_items(
    context: SpanPrepareContext<'_>,
    full_text: &mut String,
    staged_items: &mut Vec<StagedItem>,
) {
    let mut measured_index = 0usize;
    for ch in context.text.chars() {
        full_text.push(ch);
        if ch == '\n' {
            staged_items.push(StagedItem::Break);
            continue;
        }

        let measured = context
            .measured_glyphs
            .get(measured_index)
            .copied()
            .unwrap_or_else(|| {
                MeasuredGlyph::fallback(context.style.font_size(), context.style.letter_spacing())
            });
        measured_index += 1;
        staged_items.push(StagedItem::Glyph {
            byte_end: full_text.len(),
            glyph: PreparedGlyph::from_measurement(
                context.span_index,
                context.style,
                context.font_selection,
                context.metrics,
                measured,
            ),
        });
    }
}

impl StagedItem {
    /// Attaches a Unicode break opportunity to the staged item.
    pub(super) fn into_prepared(
        self,
        break_map: &HashMap<usize, BreakOpportunity>,
    ) -> PreparedItem {
        match self {
            Self::Glyph {
                byte_end,
                mut glyph,
            } => {
                glyph.break_after = break_map
                    .get(&byte_end)
                    .copied()
                    .unwrap_or(BreakOpportunity::Forbidden);
                PreparedItem::Glyph(glyph)
            }
            Self::Break => PreparedItem::Break(BreakOpportunity::Mandatory),
        }
    }
}
