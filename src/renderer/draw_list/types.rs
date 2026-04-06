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

/// Block-local layer ordering used during block-aware renderer submission.
pub type BlockSubLayer = RenderLayer;

/// Logical clip rectangle applied to one block during rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl ClipRect {
    /// Creates a clip rectangle whose geometry is validated once at construction time.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        debug_assert!(
            x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite(),
            "ClipRect values must stay finite"
        );
        debug_assert!(
            width > 0.0 && height > 0.0,
            "ClipRect size must stay positive"
        );
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the clip rectangle origin in logical pixels.
    pub fn origin(&self) -> [f32; 2] {
        [self.x, self.y]
    }

    /// Returns the clip rectangle size in logical pixels.
    pub fn size(&self) -> [f32; 2] {
        [self.width, self.height]
    }
}

/// Primitives that belong to one block-local layer.
#[derive(Clone, Debug, Default)]
pub struct BlockLayer {
    glyphs: Vec<PositionedGlyph>,
    rects: Vec<RectCmd>,
    paths: Vec<PathCmd>,
    images: Vec<ImageCmd>,
}

impl BlockLayer {
    /// Returns the glyphs assigned to this block-local layer.
    pub fn glyphs(&self) -> &[PositionedGlyph] {
        &self.glyphs
    }

    /// Returns the rectangles assigned to this block-local layer.
    pub fn rects(&self) -> &[RectCmd] {
        &self.rects
    }

    /// Returns the paths assigned to this block-local layer.
    pub fn paths(&self) -> &[PathCmd] {
        &self.paths
    }

    /// Returns the images assigned to this block-local layer.
    pub fn images(&self) -> &[ImageCmd] {
        &self.images
    }
}

/// Renderer-facing draw primitives grouped by stacking block and clip lifetime.
#[derive(Clone, Debug)]
pub struct BlockDrawGroup {
    block_index: usize,
    z_order: u32,
    clip_rect: Option<ClipRect>,
    sub_layers: [BlockLayer; RenderLayer::ALL.len()],
}

impl BlockDrawGroup {
    /// Creates an empty draw group for one block submission unit.
    pub fn new(block_index: usize, z_order: u32, clip_rect: Option<ClipRect>) -> Self {
        Self {
            block_index,
            z_order,
            clip_rect,
            sub_layers: std::array::from_fn(|_| BlockLayer::default()),
        }
    }

    /// Returns the document-order block index used to break z-order ties.
    pub fn block_index(&self) -> usize {
        self.block_index
    }

    /// Returns the stacking order where larger values are visually on top.
    pub fn z_order(&self) -> u32 {
        self.z_order
    }

    /// Returns the clip rectangle applied while drawing this group.
    pub fn clip_rect(&self) -> Option<ClipRect> {
        self.clip_rect
    }

    /// Returns the primitive collections assigned to the requested block-local layer.
    pub fn layer(&self, layer: BlockSubLayer) -> &BlockLayer {
        &self.sub_layers[layer.index()]
    }

    /// Appends glyphs to the requested block-local layer.
    pub fn extend_glyphs(&mut self, layer: BlockSubLayer, glyphs: Vec<PositionedGlyph>) {
        self.sub_layers[layer.index()].glyphs.extend(glyphs);
    }

    /// Appends one rectangle to the layer encoded on the command itself.
    pub fn push_rect(&mut self, rect: RectCmd) {
        self.sub_layers[rect.layer().index()].rects.push(rect);
    }

    /// Appends one path to the layer encoded on the command itself.
    pub fn push_path(&mut self, path: PathCmd) {
        self.sub_layers[path.layer().index()].paths.push(path);
    }

    /// Appends one image to the layer encoded on the command itself.
    pub fn push_image(&mut self, image: ImageCmd) {
        self.sub_layers[image.layer().index()].images.push(image);
    }
}

/// Solid rectangle command used for backgrounds, underlines, and overlay blocks.
#[derive(Clone, Copy, Debug)]
pub struct RectCmd {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    layer: RenderLayer,
}

impl RectCmd {
    /// Creates a rectangle command whose size and color are validated at the source.
    pub fn new(pos: [f32; 2], size: [f32; 2], color: [f32; 4], layer: RenderLayer) -> Self {
        debug_assert!(
            size[0] > 0.0 && size[1] > 0.0,
            "RectCmd size must stay positive"
        );
        debug_assert!(
            color_is_valid(color),
            "RectCmd color must stay within [0, 1]"
        );
        Self {
            pos,
            size,
            color,
            layer,
        }
    }

    /// Returns the rectangle origin in logical pixels.
    pub fn pos(&self) -> [f32; 2] {
        self.pos
    }

    /// Returns the rectangle size in logical pixels.
    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    /// Returns the rectangle color in normalized RGBA.
    pub fn color(&self) -> [f32; 4] {
        self.color
    }

    /// Returns the layer bucket that should contain this rectangle.
    pub fn layer(&self) -> RenderLayer {
        self.layer
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
    color: [f32; 4],
    width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
}

impl StrokeStyle {
    /// Creates a stroke style whose width and color are validated at construction time.
    pub fn new(color: [f32; 4], width: f32, line_cap: LineCap, line_join: LineJoin) -> Self {
        debug_assert!(width > 0.0, "StrokeStyle width must stay positive");
        debug_assert!(
            color_is_valid(color),
            "StrokeStyle color must stay within [0, 1]"
        );
        Self {
            color,
            width,
            line_cap,
            line_join,
        }
    }

    /// Returns the normalized RGBA color used for the stroke.
    pub fn color(&self) -> [f32; 4] {
        self.color
    }

    /// Returns the logical stroke width.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Returns the line-cap style forwarded to lyon.
    pub fn line_cap(&self) -> LineCap {
        self.line_cap
    }

    /// Returns the line-join style forwarded to lyon.
    pub fn line_join(&self) -> LineJoin {
        self.line_join
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
    verbs: Vec<PathVerb>,
    fill: Option<[f32; 4]>,
    stroke: Option<StrokeStyle>,
    layer: RenderLayer,
}

impl PathCmd {
    /// Creates a path command whose fill and stroke invariants are checked once.
    pub fn new(
        verbs: Vec<PathVerb>,
        fill: Option<[f32; 4]>,
        stroke: Option<StrokeStyle>,
        layer: RenderLayer,
    ) -> Self {
        debug_assert!(
            fill.is_some() || stroke.is_some(),
            "PathCmd must define a fill or stroke"
        );
        debug_assert!(
            fill.map(color_is_valid).unwrap_or(true),
            "PathCmd fill color must stay within [0, 1]"
        );
        Self {
            verbs,
            fill,
            stroke,
            layer,
        }
    }

    /// Returns the high-level path verbs that define this command.
    pub fn verbs(&self) -> &[PathVerb] {
        &self.verbs
    }

    /// Returns the optional normalized RGBA fill color.
    pub fn fill(&self) -> Option<[f32; 4]> {
        self.fill
    }

    /// Returns the optional stroke style.
    pub fn stroke(&self) -> Option<StrokeStyle> {
        self.stroke
    }

    /// Returns the layer bucket that should contain this path.
    pub fn layer(&self) -> RenderLayer {
        self.layer
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
    pos: [f32; 2],
    size: [f32; 2],
    data: Arc<ImageData>,
    layer: RenderLayer,
}

impl ImageCmd {
    /// Creates an image command whose geometry and payload are validated at the source.
    pub fn new(pos: [f32; 2], size: [f32; 2], data: Arc<ImageData>, layer: RenderLayer) -> Self {
        debug_assert!(
            size[0] > 0.0 && size[1] > 0.0,
            "ImageCmd size must stay positive"
        );
        debug_assert!(
            data.is_valid(),
            "ImageCmd payload dimensions must match the RGBA bytes"
        );
        Self {
            pos,
            size,
            data,
            layer,
        }
    }

    /// Returns the image origin in logical pixels.
    pub fn pos(&self) -> [f32; 2] {
        self.pos
    }

    /// Returns the image size in logical pixels.
    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    /// Returns the immutable image payload.
    pub fn data(&self) -> &ImageData {
        self.data.as_ref()
    }

    /// Returns the layer bucket that should contain this image.
    pub fn layer(&self) -> RenderLayer {
        self.layer
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
