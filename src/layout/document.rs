//! Document model consumed by the layout pipeline.

/// A document made of stacking blocks laid out independently.
#[derive(Clone, Debug, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

impl Document {
    /// Creates a document from the provided block list.
    pub fn new(blocks: Vec<Block>) -> Self {
        Self { blocks }
    }
}

/// A rectangle in logical pixels used for block geometry and clipping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl BlockRect {
    /// Creates a rectangle whose dimensions stay finite and positive.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        debug_assert!(
            width > 0.0 && height > 0.0,
            "BlockRect size must stay positive"
        );
        debug_assert!(
            x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite(),
            "BlockRect values must stay finite"
        );
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns whether the rectangle remains finite and positive.
    pub fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }

    /// Insets the rectangle by uniform padding on all four edges.
    pub fn inset(self, padding: f32) -> Self {
        let inset = padding.max(0.0);
        let width = (self.width - inset * 2.0).max(0.0);
        let height = (self.height - inset * 2.0).max(0.0);
        Self {
            x: self.x + inset,
            y: self.y + inset,
            width,
            height,
        }
    }
}

/// A stacking and clipping unit in the document tree.
#[derive(Clone, Debug)]
pub struct Block {
    pub rect: BlockRect,
    pub padding: f32,
    pub background_color: Option<[f32; 4]>,
    pub spans: Vec<Span>,
    pub z_order: u32,
}

impl Block {
    /// Creates a block with validated styling inputs.
    pub fn new(
        rect: BlockRect,
        padding: f32,
        background_color: Option<[f32; 4]>,
        spans: Vec<Span>,
        z_order: u32,
    ) -> Self {
        debug_assert!(
            padding.is_finite() && padding >= 0.0,
            "padding must stay finite"
        );
        Self {
            rect,
            padding,
            background_color,
            spans,
            z_order,
        }
    }
}

/// A styled text span participating in a block's inline flow.
#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub style: TextStyle,
}

impl Span {
    /// Creates a text span.
    pub fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

/// Inline styling applied to a span's text.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub font_id: u32,
    pub font_size: f32,
    pub color: [f32; 4],
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub background_color: Option<[f32; 4]>,
    pub letter_spacing: f32,
}

impl TextStyle {
    /// Creates a style with the required font and color inputs.
    pub fn new(font_id: u32, font_size: f32, color: [f32; 4]) -> Self {
        Self {
            font_id,
            font_size,
            color,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            background_color: None,
            letter_spacing: 0.0,
        }
    }
}
