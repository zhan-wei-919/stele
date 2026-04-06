//! Text measurement helpers backed by FreeType metrics and glyph advances.

use freetype::face::LoadFlag;
use log::warn;

use super::FreeTypeRasterizer;

const DEFAULT_LINE_HEIGHT_FACTOR: f32 = 1.4;

/// Logical line metrics derived from a font face at a concrete pixel size.
#[derive(Clone, Copy, Debug)]
pub struct LineMetrics {
    pub ascent: f32,
    pub line_height: f32,
}

/// The resolved face used to measure one styled span.
#[derive(Clone, Copy, Debug)]
pub struct FontSelection {
    pub requested_font_id: u32,
    pub resolved_font_id: u32,
}

/// Glyph measurement produced during prepare without any final positioning.
#[derive(Clone, Copy, Debug)]
pub struct MeasuredGlyph {
    pub glyph_id: u16,
    pub advance: f32,
}

impl MeasuredGlyph {
    /// Returns a conservative fallback width when the font backend cannot measure a glyph.
    pub fn fallback(font_size: f32, letter_spacing: f32) -> Self {
        Self {
            glyph_id: 0,
            advance: font_size * 0.6 + letter_spacing.max(0.0),
        }
    }
}

impl FreeTypeRasterizer {
    /// Resolves a styled face ID in the same family as the requested base face.
    pub(crate) fn resolve_font(
        &self,
        requested_font_id: u32,
        bold: bool,
        italic: bool,
    ) -> FontSelection {
        FontSelection {
            requested_font_id,
            resolved_font_id: self.resolve_styled_font_id(requested_font_id, bold, italic),
        }
    }

    /// Measures ascent and line height for the requested font at the given size.
    pub(crate) fn line_metrics(&self, font_id: u32, font_size: f32) -> LineMetrics {
        let Ok(face) = self.load_face_for_layout(font_id) else {
            return fallback_line_metrics(font_size);
        };
        if face.set_pixel_sizes(0, pixel_height(font_size)).is_err() {
            return fallback_line_metrics(font_size);
        }

        face.size_metrics()
            .map(|metrics| LineMetrics {
                ascent: metrics.ascender as f32 / 64.0,
                line_height: (metrics.height as f32 / 64.0).max(font_size),
            })
            .unwrap_or_else(|| fallback_line_metrics(font_size))
    }

    /// Measures visible characters in order while skipping hard line breaks.
    pub(crate) fn measure_text(
        &self,
        text: &str,
        font_id: u32,
        font_size: f32,
        letter_spacing: f32,
    ) -> Vec<MeasuredGlyph> {
        let visible = text.chars().filter(|ch| *ch != '\n').collect::<Vec<_>>();
        if visible.is_empty() {
            return Vec::new();
        }

        let Ok(face) = self.load_face_for_layout(font_id) else {
            warn!("layout.load_face_failed font_id={font_id}");
            return fallback_glyphs(visible.len(), font_size, letter_spacing);
        };
        if face.set_pixel_sizes(0, pixel_height(font_size)).is_err() {
            warn!("layout.set_pixel_sizes_failed font_id={font_id} size={font_size}");
            return fallback_glyphs(visible.len(), font_size, letter_spacing);
        }

        visible
            .iter()
            .enumerate()
            .map(|(index, ch)| {
                measure_glyph(
                    &face,
                    *ch,
                    font_size,
                    letter_spacing,
                    index + 1 == visible.len(),
                )
            })
            .collect()
    }
}

fn measure_glyph(
    face: &freetype::Face,
    ch: char,
    font_size: f32,
    letter_spacing: f32,
    is_last: bool,
) -> MeasuredGlyph {
    let glyph_id = face.get_char_index(ch as usize).unwrap_or(0);
    if let Err(error) = face.load_glyph(glyph_id, LoadFlag::DEFAULT) {
        warn!("layout.load_glyph_failed glyph_id={glyph_id} error={error:?}");
        return MeasuredGlyph::fallback(font_size, trailing_spacing(letter_spacing, is_last));
    }

    MeasuredGlyph {
        glyph_id: glyph_id.min(u16::MAX as u32) as u16,
        advance: (face.glyph().advance().x as f32 / 64.0).max(0.0)
            + trailing_spacing(letter_spacing, is_last),
    }
}

fn fallback_glyphs(count: usize, font_size: f32, letter_spacing: f32) -> Vec<MeasuredGlyph> {
    (0..count)
        .map(|index| {
            MeasuredGlyph::fallback(
                font_size,
                trailing_spacing(letter_spacing, index + 1 == count),
            )
        })
        .collect()
}

fn fallback_line_metrics(font_size: f32) -> LineMetrics {
    LineMetrics {
        ascent: font_size,
        line_height: font_size * DEFAULT_LINE_HEIGHT_FACTOR,
    }
}

fn trailing_spacing(letter_spacing: f32, is_last: bool) -> f32 {
    if is_last {
        0.0
    } else {
        letter_spacing.max(0.0)
    }
}

fn pixel_height(font_size: f32) -> u32 {
    font_size.max(1.0).round() as u32
}
