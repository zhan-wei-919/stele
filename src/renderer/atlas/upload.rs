//! Conversion from FreeType raster output into atlas-ready RGBA texels.

use crate::font::{RasterizedGlyph, SubpixelLayout};

pub(crate) struct AtlasUpload {
    pub width: u32,
    pub height: u32,
    pub bearing: [f32; 2],
    pub rgba: Vec<u8>,
}

impl AtlasUpload {
    /// Converts a rasterized glyph into the texture layout expected by the atlas.
    pub fn from_rasterized(rasterized: &RasterizedGlyph, layout: SubpixelLayout) -> Self {
        let bearing = [rasterized.bearing_x as f32, rasterized.bearing_y as f32];
        if rasterized.width == 0 || rasterized.height == 0 || rasterized.data.is_empty() {
            return Self {
                width: 0,
                height: 0,
                bearing,
                rgba: Vec::new(),
            };
        }

        let (width, height, rgba) = match layout {
            SubpixelLayout::HorizontalRgb | SubpixelLayout::HorizontalBgr => {
                horizontal_lcd_to_rgba(rasterized)
            }
            SubpixelLayout::VerticalRgb | SubpixelLayout::VerticalBgr => {
                vertical_lcd_to_rgba(rasterized)
            }
            SubpixelLayout::None => grayscale_to_rgba(rasterized),
        };

        Self {
            width,
            height,
            bearing,
            rgba,
        }
    }
}

fn horizontal_lcd_to_rgba(rasterized: &RasterizedGlyph) -> (u32, u32, Vec<u8>) {
    // FreeType LCD mode produces 3x width (one byte per R/G/B subpixel).
    debug_assert_eq!(rasterized.width % 3, 0);
    let width = rasterized.width / 3;
    let height = rasterized.height;
    let src_row_width = rasterized.width as usize;
    let dst_row_width = width as usize;
    let mut rgba = vec![0; dst_row_width * height as usize * 4];

    for y in 0..height as usize {
        let src_row = &rasterized.data[y * src_row_width..(y + 1) * src_row_width];
        for x in 0..dst_row_width {
            let src = x * 3;
            let dst = (y * dst_row_width + x) * 4;
            rgba[dst..dst + 4].copy_from_slice(&[
                src_row[src],
                src_row[src + 1],
                src_row[src + 2],
                255,
            ]);
        }
    }

    (width, height, rgba)
}

fn vertical_lcd_to_rgba(rasterized: &RasterizedGlyph) -> (u32, u32, Vec<u8>) {
    // FreeType vertical LCD mode produces 3x height (one row per R/G/B subpixel).
    debug_assert_eq!(rasterized.height % 3, 0);
    let width = rasterized.width;
    let height = rasterized.height / 3;
    let src_row_width = width as usize;
    let dst_row_width = width as usize;
    let mut rgba = vec![0; dst_row_width * height as usize * 4];

    for y in 0..height as usize {
        for x in 0..dst_row_width {
            let src_r = (y * 3) * src_row_width + x;
            let src_g = (y * 3 + 1) * src_row_width + x;
            let src_b = (y * 3 + 2) * src_row_width + x;
            let dst = (y * dst_row_width + x) * 4;
            rgba[dst..dst + 4].copy_from_slice(&[
                rasterized.data[src_r],
                rasterized.data[src_g],
                rasterized.data[src_b],
                255,
            ]);
        }
    }

    (width, height, rgba)
}

fn grayscale_to_rgba(rasterized: &RasterizedGlyph) -> (u32, u32, Vec<u8>) {
    let width = rasterized.width;
    let height = rasterized.height;
    let mut rgba = vec![0; width as usize * height as usize * 4];

    for (index, coverage) in rasterized.data.iter().copied().enumerate() {
        let dst = index * 4;
        rgba[dst..dst + 4].copy_from_slice(&[coverage, coverage, coverage, 255]);
    }

    (width, height, rgba)
}
