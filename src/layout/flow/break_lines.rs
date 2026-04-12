//! Greedy line breaking over prepared inline measurements.

use super::super::line_break::BreakOpportunity;
use super::super::prepare::{PreparedBlock, PreparedGlyph, PreparedItem};

/// Breaks one prepared block into lines using cached break opportunities.
pub(super) fn greedy_line_break(
    prepared: &PreparedBlock,
    max_width: f32,
) -> Vec<Vec<PreparedGlyph>> {
    let mut lines = Vec::new();
    let mut current = PendingLine::default();

    for item in &prepared.items {
        match item {
            PreparedItem::Break(BreakOpportunity::Mandatory) => {
                lines.push(current.take());
            }
            PreparedItem::Break(_) => {}
            PreparedItem::Glyph(glyph) => {
                current.push(glyph.clone());
                if current.width <= max_width || current.glyphs.len() == 1 {
                    if current.width > max_width && current.glyphs.len() == 1 {
                        lines.push(current.take());
                    }
                    continue;
                }

                if let Some(break_len) = current
                    .last_allowed_break
                    .filter(|break_len| *break_len < current.glyphs.len())
                {
                    let remainder = current.split_off(break_len);
                    lines.push(current.take());
                    current = PendingLine::from_glyphs(remainder);
                } else if let Some(overflow) = current.pop_last() {
                    lines.push(current.take());
                    current = PendingLine::from_glyphs(vec![overflow]);
                }
            }
        }
    }

    if !current.is_empty() {
        lines.push(current.take());
    }
    lines
}

#[derive(Clone, Debug, Default)]
struct PendingLine {
    glyphs: Vec<PreparedGlyph>,
    width: f32,
    last_allowed_break: Option<usize>,
}

impl PendingLine {
    fn from_glyphs(glyphs: Vec<PreparedGlyph>) -> Self {
        let mut pending = Self::default();
        for glyph in glyphs {
            pending.push(glyph);
        }
        pending
    }

    fn push(&mut self, glyph: PreparedGlyph) {
        self.width += glyph.advance.max(0.0);
        self.glyphs.push(glyph);
        if self
            .glyphs
            .last()
            .is_some_and(|glyph| glyph.break_after == BreakOpportunity::Allowed)
        {
            self.last_allowed_break = Some(self.glyphs.len());
        }
    }

    fn pop_last(&mut self) -> Option<PreparedGlyph> {
        let glyph = self.glyphs.pop()?;
        self.width = (self.width - glyph.advance.max(0.0)).max(0.0);
        self.last_allowed_break = self
            .glyphs
            .iter()
            .enumerate()
            .rev()
            .find(|(_, glyph)| glyph.break_after == BreakOpportunity::Allowed)
            .map(|(index, _)| index + 1);
        Some(glyph)
    }

    fn split_off(&mut self, at: usize) -> Vec<PreparedGlyph> {
        let remainder = self.glyphs.split_off(at);
        self.width = self.glyphs.iter().map(|glyph| glyph.advance.max(0.0)).sum();
        self.last_allowed_break = self
            .glyphs
            .iter()
            .enumerate()
            .rev()
            .find(|(_, glyph)| glyph.break_after == BreakOpportunity::Allowed)
            .map(|(index, _)| index + 1);
        remainder
    }

    fn take(&mut self) -> Vec<PreparedGlyph> {
        self.width = 0.0;
        self.last_allowed_break = None;
        std::mem::take(&mut self.glyphs)
    }

    fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::greedy_line_break;
    use crate::layout::{
        line_break::BreakOpportunity,
        prepare::{PreparedBlock, PreparedGlyph, PreparedItem},
    };
    use crate::scene::BlockId;

    #[test]
    fn greedy_break_wraps_at_allowed_boundaries() {
        let prepared = PreparedBlock {
            block_id: BlockId::new(0),
            document_index: 0,
            items: vec![
                PreparedItem::Glyph(glyph(0, 1, 10.0, 14.0, BreakOpportunity::Forbidden)),
                PreparedItem::Glyph(glyph(0, 2, 10.0, 14.0, BreakOpportunity::Forbidden)),
                PreparedItem::Glyph(glyph(0, 3, 5.0, 14.0, BreakOpportunity::Allowed)),
                PreparedItem::Glyph(glyph(0, 4, 12.0, 14.0, BreakOpportunity::Forbidden)),
                PreparedItem::Glyph(glyph(0, 5, 12.0, 14.0, BreakOpportunity::Forbidden)),
            ],
            default_ascent: 10.0,
            default_line_height: 20.0,
        };

        let lines = greedy_line_break(&prepared, 30.0);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 3);
        assert_eq!(lines[1].len(), 2);
    }

    fn glyph(
        span_index: usize,
        glyph_id: u16,
        advance: f32,
        font_size: f32,
        break_after: BreakOpportunity,
    ) -> PreparedGlyph {
        PreparedGlyph {
            span_index,
            font_id: 0,
            glyph_id,
            font_size,
            advance,
            ascent: font_size * 0.75,
            line_height: font_size * 1.4,
            color: [1.0, 1.0, 1.0, 1.0],
            background_color: None,
            underline: false,
            strikethrough: false,
            break_after,
        }
    }
}
