//! Baseline placement and run construction over broken lines.

use crate::draw_list::PositionedGlyph;
use crate::font::SubpixelBin;

use super::super::document::BlockRect;
use super::super::prepare::PreparedGlyph;
use super::decorations::RunAccumulator;
use super::types::{LayoutLine, LayoutRun};

const SUBPIXEL_BIN_COUNT: f32 = 4.0;

/// The content rectangle available for inline layout after padding.
#[derive(Clone, Copy, Debug)]
pub(super) struct ContentRect {
    x: f32,
    y: f32,
    width: f32,
    // Kept so the padding clamp remains directly assertable in layout unit tests.
    #[allow(dead_code)]
    height: f32,
}

impl ContentRect {
    /// Returns the content origin x.
    pub(super) fn x(self) -> f32 {
        self.x
    }

    /// Returns the content origin y.
    pub(super) fn y(self) -> f32 {
        self.y
    }

    /// Returns the available content width.
    pub(super) fn width(self) -> f32 {
        self.width
    }

    /// Returns the available content height.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn height(self) -> f32 {
        self.height
    }
}

/// Computes the padded content rectangle used by layout.
pub(super) fn content_rect(rect: BlockRect, padding: f32) -> ContentRect {
    let width = (rect.width() - padding * 2.0).max(0.0);
    let height = (rect.height() - padding * 2.0).max(0.0);
    ContentRect {
        x: rect.x() + padding,
        y: rect.y() + padding,
        width,
        height,
    }
}

/// Positions all broken lines inside the block content rectangle.
pub(super) fn place_lines(
    raw_lines: &[Vec<PreparedGlyph>],
    content_rect: ContentRect,
    default_ascent: f32,
    default_line_height: f32,
) -> Vec<LayoutLine> {
    let mut y = content_rect.y();
    raw_lines
        .iter()
        .map(|glyphs| {
            let max_ascent = glyphs
                .iter()
                .map(|glyph| glyph.ascent)
                .fold(default_ascent, f32::max);
            let line_height = glyphs
                .iter()
                .map(|glyph| glyph.line_height)
                .fold(default_line_height, f32::max);
            let baseline = y + max_ascent;
            let runs = build_runs(glyphs, content_rect.x(), y, baseline, line_height);
            let line = LayoutLine {
                runs,
                y,
                line_height,
                baseline,
            };
            y += line_height;
            line
        })
        .collect()
}

fn build_runs(
    glyphs: &[PreparedGlyph],
    line_x: f32,
    line_y: f32,
    baseline: f32,
    line_height: f32,
) -> Vec<LayoutRun> {
    let mut runs = Vec::new();
    let mut x = line_x;
    let mut current: Option<RunAccumulator> = None;

    for glyph in glyphs {
        let positioned = PositionedGlyph {
            font_id: glyph.font_id,
            glyph_id: glyph.glyph_id,
            font_size: glyph.font_size,
            pos: [x, baseline],
            color: glyph.color,
            subpixel_offset: SubpixelBin::new(subpixel_bin(x), subpixel_bin(baseline)),
        };

        if current
            .as_ref()
            .is_some_and(|run| run.span_index() != glyph.span_index)
        {
            runs.push(
                current
                    .take()
                    .expect("run accumulator must exist before switching spans")
                    .finish(line_y, baseline, line_height),
            );
        }

        match current.as_mut() {
            Some(run) => run.push(glyph, positioned),
            None => current = Some(RunAccumulator::new(glyph, positioned, x)),
        }
        x += glyph.advance.max(0.0);
    }

    if let Some(run) = current {
        runs.push(run.finish(line_y, baseline, line_height));
    }
    runs
}

fn subpixel_bin(value: f32) -> u8 {
    let fraction = value.rem_euclid(1.0);
    let bin = (fraction * SUBPIXEL_BIN_COUNT).round() as i32;
    bin.clamp(0, 3) as u8
}

#[cfg(test)]
mod tests {
    use super::{content_rect, place_lines, ContentRect};
    use crate::layout::{line_break::BreakOpportunity, prepare::PreparedGlyph, BlockRect};

    #[test]
    fn place_lines_resets_x_to_content_origin() {
        let content = ContentRect {
            x: 8.0,
            y: 12.0,
            width: 40.0,
            height: 80.0,
        };
        let lines = place_lines(
            &[vec![glyph(0, 1, 10.0, 14.0)], vec![glyph(0, 2, 12.0, 14.0)]],
            content,
            10.0,
            20.0,
        );
        assert_eq!(lines[0].runs[0].glyphs[0].pos[0], 8.0);
        assert_eq!(lines[1].runs[0].glyphs[0].pos[0], 8.0);
        assert!(lines[1].y > lines[0].y);
    }

    #[test]
    fn place_lines_aligns_baseline_for_mixed_font_sizes() {
        let line = &place_lines(
            &[vec![glyph(0, 1, 12.0, 24.0), glyph(1, 2, 8.0, 14.0)]],
            ContentRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            },
            18.0,
            33.6,
        )[0];
        assert_eq!(line.runs.len(), 2);
        assert_eq!(line.runs[0].glyphs[0].pos[1], line.runs[1].glyphs[0].pos[1]);
        assert!((line.line_height - 33.6).abs() < 0.01);
        assert!((line.baseline - 18.0).abs() < 0.01);
    }

    #[test]
    fn content_rect_clamps_padding_to_non_negative_space() {
        let padded = content_rect(
            BlockRect::new(0.0, 0.0, 10.0, 10.0).expect("rect must be valid"),
            8.0,
        );
        assert_eq!(padded.width(), 0.0);
        assert_eq!(padded.height(), 0.0);
    }

    fn glyph(span_index: usize, glyph_id: u16, advance: f32, font_size: f32) -> PreparedGlyph {
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
            break_after: BreakOpportunity::Forbidden,
        }
    }
}
