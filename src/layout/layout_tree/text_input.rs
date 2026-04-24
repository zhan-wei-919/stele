//! Single-line text input placement for the tree layout path.

use crate::draw_list::PositionedGlyph;
use crate::font::SubpixelBin;
use crate::layout::prepare::PreparedGlyph;
use crate::layout::prepare_tree::PreparedTextInput;

use super::types::{LayoutRect, LayoutTextInput};

const SUBPIXEL_BIN_COUNT: f32 = 4.0;
const CARET_WIDTH: f32 = 1.0;

/// Measures the text input's visible text width without block padding or borders.
pub(crate) fn measure_text_input_content_width(text_input: &PreparedTextInput) -> f32 {
    text_input.content_width
}

/// Positions the text input's glyphs and caret inside the provided content rect.
pub(crate) fn layout_text_input(
    text_input: &PreparedTextInput,
    content_rect: LayoutRect,
) -> LayoutTextInput {
    let baseline = content_rect.y() + text_input.default_ascent;
    let mut x = content_rect.x();
    let glyphs = text_input
        .glyphs
        .iter()
        .map(|glyph| {
            let positioned = position_glyph(glyph, x, baseline);
            x += glyph.advance.max(0.0);
            positioned
        })
        .collect();
    let caret_rect = LayoutRect::new(
        content_rect.x() + text_input.caret_advance,
        content_rect.y(),
        CARET_WIDTH,
        text_input.default_line_height,
    );

    LayoutTextInput {
        text_input_id: text_input.text_input_id,
        rect: LayoutRect::new(
            content_rect.x(),
            content_rect.y(),
            content_rect.width(),
            text_input.default_line_height,
        ),
        glyphs,
        caret_rect,
        caret_color: text_input.style.caret_color,
    }
}

fn position_glyph(glyph: &PreparedGlyph, x: f32, baseline: f32) -> PositionedGlyph {
    PositionedGlyph {
        font_id: glyph.font_id,
        glyph_id: glyph.glyph_id,
        font_size: glyph.font_size,
        pos: [x, baseline],
        color: glyph.color,
        subpixel_offset: SubpixelBin::new(subpixel_bin(x), subpixel_bin(baseline)),
    }
}

fn subpixel_bin(value: f32) -> u8 {
    let fraction = value.rem_euclid(1.0);
    let bin = (fraction * SUBPIXEL_BIN_COUNT).round() as i32;
    bin.clamp(0, 3) as u8
}
