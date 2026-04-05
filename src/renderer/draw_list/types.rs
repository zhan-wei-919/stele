//! Core draw-list types consumed by the renderer runtime.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::font::{GlyphKey, SubpixelBin};

/// Glyph positioned in logical pixel coordinates, ready for atlas lookup.
#[derive(Clone, Copy, Debug)]
pub struct PositionedGlyph {
    pub font_id: u32,
    pub glyph_id: u16,
    pub font_size: f32,
    pub pos: [f32; 2],
    pub color: [f32; 4],
    pub subpixel_offset: SubpixelBin,
}

impl PositionedGlyph {
    /// Builds the rasterization cache key for the current scale factor.
    pub fn glyph_key(&self, scale_factor: f32) -> GlyphKey {
        GlyphKey::new(
            self.font_id,
            self.glyph_id,
            self.font_size,
            scale_factor,
            self.subpixel_offset,
        )
    }
}

/// Fixed layer ordering used by the renderer when submitting draw calls.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum RenderLayer {
    Background,
    #[default]
    Content,
    Foreground,
    Overlay,
}

impl RenderLayer {
    pub const ALL: [Self; 4] = [
        Self::Background,
        Self::Content,
        Self::Foreground,
        Self::Overlay,
    ];

    /// Returns the stable bucket index used by runtime arrays.
    pub const fn index(self) -> usize {
        match self {
            Self::Background => 0,
            Self::Content => 1,
            Self::Foreground => 2,
            Self::Overlay => 3,
        }
    }
}

/// Solid rectangle command used for backgrounds, underlines, and overlay blocks.
#[derive(Clone, Copy, Debug, Default)]
pub struct RectCmd {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub layer: RenderLayer,
}

impl RectCmd {
    /// Returns whether the rectangle can produce visible geometry.
    pub fn is_valid(&self) -> bool {
        self.size[0] > 0.0 && self.size[1] > 0.0 && color_is_valid(self.color)
    }
}

/// High-level path verbs that are later lowered into lyon path events.
#[derive(Clone, Debug)]
pub enum PathVerb {
    MoveTo {
        to: [f32; 2],
    },
    LineTo {
        to: [f32; 2],
    },
    QuadTo {
        ctrl: [f32; 2],
        to: [f32; 2],
    },
    CubicTo {
        ctrl1: [f32; 2],
        ctrl2: [f32; 2],
        to: [f32; 2],
    },
    Close,
}

/// Stroke line-cap style forwarded to lyon.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

/// Stroke line-join style forwarded to lyon.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// Stroke style shared by path commands.
#[derive(Clone, Copy, Debug)]
pub struct StrokeStyle {
    pub color: [f32; 4],
    pub width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
}

impl StrokeStyle {
    /// Returns whether the stroke can generate visible triangles.
    pub fn is_valid(&self) -> bool {
        self.width > 0.0 && color_is_valid(self.color)
    }

    /// Writes the style into a hasher without relying on `f32: Hash`.
    pub fn hash_into(&self, hasher: &mut impl Hasher) {
        hash_color(hasher, self.color);
        hasher.write_u32(self.width.to_bits());
        self.line_cap.hash(hasher);
        self.line_join.hash(hasher);
    }
}

/// Vector path command carrying fill and/or stroke styling.
#[derive(Clone, Debug)]
pub struct PathCmd {
    pub verbs: Vec<PathVerb>,
    pub fill: Option<[f32; 4]>,
    pub stroke: Option<StrokeStyle>,
    pub layer: RenderLayer,
}

impl PathCmd {
    /// Returns `true` when the command can contribute any visible geometry.
    pub fn is_visible(&self) -> bool {
        self.fill.is_some() || self.stroke.is_some()
    }

    /// Returns `true` when the fill color is usable by the runtime.
    pub fn fill_is_valid(&self) -> bool {
        self.fill.map(color_is_valid).unwrap_or(false)
    }

    /// Computes the cache key used by the tessellation cache.
    pub fn content_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        hash_verbs(&mut hasher, &self.verbs);
        match self.fill {
            Some(fill) => {
                true.hash(&mut hasher);
                hash_color(&mut hasher, fill);
            }
            None => false.hash(&mut hasher),
        }
        match self.stroke {
            Some(stroke) => {
                true.hash(&mut hasher);
                stroke.hash_into(&mut hasher);
            }
            None => false.hash(&mut hasher),
        }
        hasher.finish()
    }
}

/// Immutable RGBA image payload whose content hash is computed once at creation.
#[derive(Debug)]
pub struct ImageData {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    content_hash: u64,
}

impl ImageData {
    /// Creates image data and precomputes the deduplication hash.
    pub fn new(rgba: Vec<u8>, width: u32, height: u32) -> Self {
        let content_hash = hash_image(&rgba, width, height);
        Self {
            rgba,
            width,
            height,
            content_hash,
        }
    }

    /// Returns whether the image payload matches its declared dimensions.
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.rgba.len() == self.width as usize * self.height as usize * 4
    }

    /// Returns the deduplication hash derived from dimensions and RGBA bytes.
    pub fn content_hash(&self) -> u64 {
        self.content_hash
    }

    /// Returns the texture width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the texture height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the raw RGBA8 bytes ready for `queue.write_texture`.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// Image draw command referencing shared RGBA data.
#[derive(Clone, Debug)]
pub struct ImageCmd {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub data: Arc<ImageData>,
    pub layer: RenderLayer,
}

impl ImageCmd {
    /// Returns whether the instance and underlying image payload are renderable.
    pub fn is_valid(&self) -> bool {
        self.size[0] > 0.0 && self.size[1] > 0.0 && self.data.is_valid()
    }
}

fn color_is_valid(color: [f32; 4]) -> bool {
    color
        .into_iter()
        .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
}

fn hash_verbs(hasher: &mut impl Hasher, verbs: &[PathVerb]) {
    verbs.len().hash(hasher);
    for verb in verbs {
        match verb {
            PathVerb::MoveTo { to } => {
                0u8.hash(hasher);
                hash_point(hasher, *to);
            }
            PathVerb::LineTo { to } => {
                1u8.hash(hasher);
                hash_point(hasher, *to);
            }
            PathVerb::QuadTo { ctrl, to } => {
                2u8.hash(hasher);
                hash_point(hasher, *ctrl);
                hash_point(hasher, *to);
            }
            PathVerb::CubicTo { ctrl1, ctrl2, to } => {
                3u8.hash(hasher);
                hash_point(hasher, *ctrl1);
                hash_point(hasher, *ctrl2);
                hash_point(hasher, *to);
            }
            PathVerb::Close => 4u8.hash(hasher),
        }
    }
}

fn hash_image(rgba: &[u8], width: u32, height: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    rgba.hash(&mut hasher);
    hasher.finish()
}

fn hash_color(hasher: &mut impl Hasher, color: [f32; 4]) {
    for component in color {
        hasher.write_u32(component.to_bits());
    }
}

fn hash_point(hasher: &mut impl Hasher, point: [f32; 2]) {
    hasher.write_u32(point[0].to_bits());
    hasher.write_u32(point[1].to_bits());
}
