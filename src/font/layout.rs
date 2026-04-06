//! Text layout helpers backed by FreeType metrics and glyph advances.

use freetype::face::LoadFlag;
use log::warn;

use super::{FreeTypeRasterizer, SubpixelBin};

const DEFAULT_LINE_HEIGHT_FACTOR: f32 = 1.4;
const SUBPIXEL_BIN_COUNT: f32 = 4.0;

/// Logical line metrics derived from a font face at a concrete pixel size.
#[derive(Clone, Copy, Debug)]
pub struct LineMetrics {
    pub ascent: f32,
    pub line_height: f32,
}

/// Glyph positioning produced by the font module without renderer styling.
#[derive(Clone, Copy, Debug)]
pub struct LaidOutGlyph {
    pub glyph_id: u16,
    pub pos: [f32; 2],
    pub subpixel_offset: SubpixelBin,
}

impl FreeTypeRasterizer {
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

    /// Positions one line of text using FreeType glyph advances from the selected face.
    pub(crate) fn layout_line(
        &self,
        text: &str,
        font_id: u32,
        font_size: f32,
        x_offset: f32,
        y_offset: f32,
    ) -> Vec<LaidOutGlyph> {
        let Ok(face) = self.load_face_for_layout(font_id) else {
            warn!("layout.load_face_failed font_id={font_id}");
            return Vec::new();
        };
        if face.set_pixel_sizes(0, pixel_height(font_size)).is_err() {
            warn!("layout.set_pixel_sizes_failed font_id={font_id} size={font_size}");
            return Vec::new();
        }

        let ascent = face
            .size_metrics()
            .map(|metrics| metrics.ascender as f32 / 64.0)
            .unwrap_or(font_size);
        let baseline_y = y_offset + ascent;
        let mut x = x_offset;
        let mut glyphs = Vec::with_capacity(text.chars().count());

        for ch in text.chars() {
            let glyph_id = face.get_char_index(ch as usize).unwrap_or(0);
            if let Err(error) = face.load_glyph(glyph_id, LoadFlag::DEFAULT) {
                warn!("layout.load_glyph_failed glyph_id={glyph_id} error={error:?}");
                continue;
            }

            let advance = face.glyph().advance().x as f32 / 64.0;
            glyphs.push(LaidOutGlyph {
                glyph_id: glyph_id.min(u16::MAX as u32) as u16,
                pos: [x, baseline_y],
                subpixel_offset: SubpixelBin::new(subpixel_bin(x), subpixel_bin(baseline_y)),
            });
            x += advance.max(0.0);
        }

        glyphs
    }
}

fn fallback_line_metrics(font_size: f32) -> LineMetrics {
    LineMetrics {
        ascent: font_size,
        line_height: font_size * DEFAULT_LINE_HEIGHT_FACTOR,
    }
}

fn pixel_height(font_size: f32) -> u32 {
    font_size.max(1.0).round() as u32
}

fn subpixel_bin(value: f32) -> u8 {
    let bin = (value.fract() * SUBPIXEL_BIN_COUNT).round() as i32;
    bin.clamp(0, 3) as u8
}
