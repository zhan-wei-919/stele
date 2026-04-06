//! Decoration emission for one laid-out text run.

use crate::renderer::{PositionedGlyph, RectCmd, RenderLayer};

use super::super::prepare::PreparedGlyph;
use super::types::LayoutRun;

/// Accumulates one span-local run before turning it into renderer primitives.
#[derive(Clone, Debug)]
pub(super) struct RunAccumulator {
    span_index: usize,
    font_size: f32,
    color: [f32; 4],
    background_color: Option<[f32; 4]>,
    underline: bool,
    strikethrough: bool,
    start_x: f32,
    end_x: f32,
    glyphs: Vec<PositionedGlyph>,
}

impl RunAccumulator {
    /// Starts a new run from the first glyph in that span segment.
    pub(super) fn new(glyph: &PreparedGlyph, positioned: PositionedGlyph, start_x: f32) -> Self {
        Self {
            span_index: glyph.span_index,
            font_size: glyph.font_size,
            color: glyph.color,
            background_color: glyph.background_color,
            underline: glyph.underline,
            strikethrough: glyph.strikethrough,
            start_x,
            end_x: start_x + glyph.advance.max(0.0),
            glyphs: vec![positioned],
        }
    }

    /// Appends another glyph from the same span to the run.
    pub(super) fn push(&mut self, glyph: &PreparedGlyph, positioned: PositionedGlyph) {
        self.end_x += glyph.advance.max(0.0);
        self.glyphs.push(positioned);
    }

    /// Returns the source span index used to decide run boundaries.
    pub(super) fn span_index(&self) -> usize {
        self.span_index
    }

    /// Finalizes background and decoration primitives for this run.
    pub(super) fn finish(self, line_y: f32, baseline: f32, line_height: f32) -> LayoutRun {
        let width = (self.end_x - self.start_x).max(0.0);
        let mut decoration_rects = Vec::new();

        if width > 0.0 {
            if let Some(color) = self.background_color {
                decoration_rects.push(RectCmd::new(
                    [self.start_x, line_y],
                    [width, line_height],
                    color,
                    RenderLayer::Background,
                ));
            }
            if self.underline {
                decoration_rects.push(RectCmd::new(
                    [self.start_x, baseline + underline_offset(self.font_size)],
                    [width, decoration_thickness(self.font_size)],
                    self.color,
                    RenderLayer::Foreground,
                ));
            }
            if self.strikethrough {
                decoration_rects.push(RectCmd::new(
                    [self.start_x, baseline - self.font_size * 0.3],
                    [width, decoration_thickness(self.font_size)],
                    self.color,
                    RenderLayer::Foreground,
                ));
            }
        }

        LayoutRun {
            glyphs: self.glyphs,
            decoration_rects,
        }
    }
}

fn underline_offset(font_size: f32) -> f32 {
    (font_size * 0.1).max(1.0)
}

fn decoration_thickness(font_size: f32) -> f32 {
    (font_size * 0.06).max(1.0)
}

#[cfg(test)]
mod tests {
    use crate::font::SubpixelBin;
    use crate::renderer::{PositionedGlyph, RenderLayer};

    use super::super::super::line_break::BreakOpportunity;
    use super::super::super::prepare::PreparedGlyph;
    use super::RunAccumulator;

    #[test]
    fn run_accumulator_emits_background_and_underline_rects() {
        let glyph = PreparedGlyph {
            span_index: 0,
            font_id: 0,
            glyph_id: 1,
            font_size: 16.0,
            advance: 20.0,
            ascent: 12.0,
            line_height: 22.4,
            color: [1.0, 1.0, 1.0, 1.0],
            background_color: Some([0.2, 0.2, 0.2, 1.0]),
            underline: true,
            strikethrough: false,
            break_after: BreakOpportunity::Forbidden,
        };
        let positioned = PositionedGlyph {
            font_id: 0,
            glyph_id: 1,
            font_size: 16.0,
            pos: [0.0, 12.0],
            color: [1.0, 1.0, 1.0, 1.0],
            subpixel_offset: SubpixelBin::new(0, 0),
        };

        let rects = RunAccumulator::new(&glyph, positioned, 0.0)
            .finish(0.0, 12.0, 22.4)
            .decoration_rects;
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].layer(), RenderLayer::Background);
        assert_eq!(rects[1].layer(), RenderLayer::Foreground);
    }
}
