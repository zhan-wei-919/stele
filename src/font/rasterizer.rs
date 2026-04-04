use std::path::Path;

use fontdb::Source;
use freetype::face::LoadFlag;
use freetype::{Bitmap, LcdFilter, Library, RenderMode};
use log::warn;

use crate::draw_list::GlyphKey;

use super::FontDiscovery;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubpixelLayout {
    HorizontalRgb,
    HorizontalBgr,
    VerticalRgb,
    VerticalBgr,
    None,
}

#[derive(Clone, Debug, Default)]
pub struct RasterizedGlyph {
    pub width: u32,
    pub height: u32,
    pub bearing_x: i32,
    pub bearing_y: i32,
    pub data: Vec<u8>,
}

impl RasterizedGlyph {
    fn empty(bearing_x: i32, bearing_y: i32) -> Self {
        Self {
            width: 0,
            height: 0,
            bearing_x,
            bearing_y,
            data: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub enum RasterizerError {
    Init(freetype::Error),
    InvalidFontId(u32),
}

pub struct FreeTypeRasterizer {
    library: Library,
    fonts: FontDiscovery,
    subpixel_layout: SubpixelLayout,
}

impl FreeTypeRasterizer {
    pub fn new(
        fonts: FontDiscovery,
        subpixel_layout: SubpixelLayout,
    ) -> Result<Self, RasterizerError> {
        let library = Library::init().map_err(RasterizerError::Init)?;
        let lcd_filter = match subpixel_layout {
            SubpixelLayout::None => LcdFilter::LcdFilterNone,
            _ => LcdFilter::LcdFilterDefault,
        };
        if let Err(error) = library.set_lcd_filter(lcd_filter) {
            warn!("font.rasterizer.set_lcd_filter_failed error={error:?}");
        }

        Ok(Self {
            library,
            fonts,
            subpixel_layout,
        })
    }

    pub fn fonts(&self) -> &FontDiscovery {
        &self.fonts
    }

    pub fn subpixel_layout(&self) -> SubpixelLayout {
        self.subpixel_layout
    }

    pub fn rasterize_lcd(&self, glyph_key: GlyphKey) -> RasterizedGlyph {
        let font_id = self.resolve_font_id(glyph_key.font_id);
        let Ok(face) = self.load_face(font_id) else {
            warn!("font.rasterizer.load_face_failed font_id={font_id}");
            return RasterizedGlyph::default();
        };

        let pixel_height = (glyph_key.font_size() * glyph_key.scale_factor())
            .max(1.0)
            .round() as u32;
        if let Err(error) = face.set_pixel_sizes(0, pixel_height) {
            warn!("font.rasterizer.set_pixel_sizes_failed error={error:?}");
            return RasterizedGlyph::default();
        }

        let load_flags = self.load_flags();
        let glyph_index = glyph_key.glyph_id as u32;
        if let Err(error) = face.load_glyph(glyph_index, load_flags) {
            warn!(
                "font.rasterizer.load_glyph_failed font_id={} glyph_id={} error={error:?}",
                font_id, glyph_key.glyph_id
            );
            if glyph_index != 0 {
                return self.rasterize_lcd(GlyphKey {
                    glyph_id: 0,
                    ..glyph_key
                });
            }
            return RasterizedGlyph::default();
        }

        let glyph = face.glyph();
        if let Err(error) = glyph.render_glyph(self.render_mode()) {
            warn!("font.rasterizer.render_glyph_failed error={error:?}");
            return RasterizedGlyph::default();
        }

        let bitmap = glyph.bitmap();
        if bitmap.width() <= 0 || bitmap.rows() <= 0 {
            return RasterizedGlyph::empty(glyph.bitmap_left(), glyph.bitmap_top());
        }

        let mut data = compact_bitmap_rows(&bitmap);
        match self.subpixel_layout {
            SubpixelLayout::HorizontalBgr => reverse_triplets_per_row(&mut data, bitmap.width()),
            SubpixelLayout::VerticalBgr => reverse_vertical_triplets(&mut data, bitmap.width()),
            _ => {}
        }

        RasterizedGlyph {
            width: bitmap.width() as u32,
            height: bitmap.rows() as u32,
            bearing_x: glyph.bitmap_left(),
            bearing_y: glyph.bitmap_top(),
            data,
        }
    }

    fn resolve_font_id(&self, requested_font_id: u32) -> u32 {
        if self.fonts.db_id_for(requested_font_id).is_some() {
            requested_font_id
        } else {
            self.fonts.default_font_id()
        }
    }

    fn load_face(&self, font_id: u32) -> Result<freetype::Face, RasterizerError> {
        let face_info = self
            .fonts
            .face_info(font_id)
            .ok_or(RasterizerError::InvalidFontId(font_id))?;

        match &face_info.source {
            Source::File(path) | Source::SharedFile(path, _) => self
                .library
                .new_face(Path::new(path), face_info.index as isize),
            Source::Binary(bytes) => self
                .library
                .new_memory_face(bytes.as_ref().as_ref().to_vec(), face_info.index as isize),
        }
        .map_err(RasterizerError::Init)
    }

    fn load_flags(&self) -> LoadFlag {
        match self.subpixel_layout {
            SubpixelLayout::HorizontalRgb | SubpixelLayout::HorizontalBgr => LoadFlag::TARGET_LCD,
            SubpixelLayout::VerticalRgb | SubpixelLayout::VerticalBgr => LoadFlag::TARGET_LCD_V,
            SubpixelLayout::None => LoadFlag::DEFAULT,
        }
    }

    fn render_mode(&self) -> RenderMode {
        match self.subpixel_layout {
            SubpixelLayout::HorizontalRgb | SubpixelLayout::HorizontalBgr => RenderMode::Lcd,
            SubpixelLayout::VerticalRgb | SubpixelLayout::VerticalBgr => RenderMode::LcdV,
            SubpixelLayout::None => RenderMode::Normal,
        }
    }
}

fn compact_bitmap_rows(bitmap: &Bitmap) -> Vec<u8> {
    let width = bitmap.width().max(0) as usize;
    let rows = bitmap.rows().max(0) as usize;
    let pitch = bitmap.pitch().unsigned_abs() as usize;
    let buffer = bitmap.buffer();

    let mut compact = Vec::with_capacity(width.saturating_mul(rows));
    for row in 0..rows {
        let start = row.saturating_mul(pitch);
        let end = start.saturating_add(width).min(buffer.len());
        compact.extend_from_slice(&buffer[start..end]);
    }
    compact
}

fn reverse_triplets_per_row(data: &mut [u8], row_width: i32) {
    let row_width = row_width.max(0) as usize;
    if row_width == 0 {
        return;
    }

    for row in data.chunks_mut(row_width) {
        for pixel in row.chunks_exact_mut(3) {
            pixel.swap(0, 2);
        }
    }
}

fn reverse_vertical_triplets(data: &mut [u8], row_width: i32) {
    let row_width = row_width.max(0) as usize;
    if row_width == 0 {
        return;
    }

    let rows = data.len() / row_width;
    for column in 0..row_width {
        let mut row = 0usize;
        while row + 2 < rows {
            let top = row * row_width + column;
            let bottom = (row + 2) * row_width + column;
            data.swap(top, bottom);
            row += 3;
        }
    }
}
