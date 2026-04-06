//! Block, span, and document geometry used by layout.

use super::{validate_optional_color, DocumentError, TextStyle};

/// A document made of stacking blocks laid out independently.
#[derive(Clone, Debug, Default)]
pub struct Document {
    blocks: Vec<Block>,
}

impl Document {
    /// Creates a document from validated blocks.
    pub fn new(blocks: Vec<Block>) -> Self {
        Self { blocks }
    }

    /// Returns the document blocks in document order.
    pub(crate) fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Returns one block by document order index.
    pub(crate) fn block(&self, block_index: usize) -> Option<&Block> {
        self.blocks.get(block_index)
    }

    /// Updates one block rectangle while preserving the validated geometry type.
    pub fn set_block_rect(
        &mut self,
        block_index: usize,
        rect: BlockRect,
    ) -> Result<(), DocumentError> {
        let block = self
            .blocks
            .get_mut(block_index)
            .ok_or(DocumentError::MissingBlock { block_index })?;
        block.rect = rect;
        Ok(())
    }

    /// Updates one block background color.
    pub fn set_block_background_color(
        &mut self,
        block_index: usize,
        background_color: Option<[f32; 4]>,
    ) -> Result<(), DocumentError> {
        let block = self
            .blocks
            .get_mut(block_index)
            .ok_or(DocumentError::MissingBlock { block_index })?;
        validate_optional_color(background_color)?;
        block.background_color = background_color;
        Ok(())
    }
}

/// A rectangle in logical pixels used for block geometry and clipping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl BlockRect {
    /// Creates a block rectangle with finite, positive geometry.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Result<Self, DocumentError> {
        if !(x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite()) {
            return Err(DocumentError::InvalidRect);
        }
        if width <= 0.0 || height <= 0.0 {
            return Err(DocumentError::InvalidRect);
        }

        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    /// Returns the x origin in logical pixels.
    pub fn x(self) -> f32 {
        self.x
    }

    /// Returns the y origin in logical pixels.
    pub fn y(self) -> f32 {
        self.y
    }

    /// Returns the width in logical pixels.
    pub fn width(self) -> f32 {
        self.width
    }

    /// Returns the height in logical pixels.
    pub fn height(self) -> f32 {
        self.height
    }
}

/// A stacking and clipping unit in the document tree.
#[derive(Clone, Debug)]
pub struct Block {
    rect: BlockRect,
    padding: f32,
    background_color: Option<[f32; 4]>,
    spans: Vec<Span>,
    z_order: u32,
}

impl Block {
    /// Creates a block with validated styling inputs.
    pub fn new(
        rect: BlockRect,
        padding: f32,
        background_color: Option<[f32; 4]>,
        spans: Vec<Span>,
        z_order: u32,
    ) -> Result<Self, DocumentError> {
        if !padding.is_finite() || padding < 0.0 {
            return Err(DocumentError::InvalidPadding);
        }
        validate_optional_color(background_color)?;

        Ok(Self {
            rect,
            padding,
            background_color,
            spans,
            z_order,
        })
    }

    /// Returns the validated block rectangle.
    pub(crate) fn rect(&self) -> BlockRect {
        self.rect
    }

    /// Returns the block padding in logical pixels.
    pub(crate) fn padding(&self) -> f32 {
        self.padding
    }

    /// Returns the optional block background color.
    pub(crate) fn background_color(&self) -> Option<[f32; 4]> {
        self.background_color
    }

    /// Returns the block spans in inline order.
    pub(crate) fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Returns the block z-order.
    pub(crate) fn z_order(&self) -> u32 {
        self.z_order
    }
}

/// A styled text span participating in a block's inline flow.
#[derive(Clone, Debug)]
pub struct Span {
    text: String,
    style: TextStyle,
}

impl Span {
    /// Creates a text span.
    pub fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    /// Returns the span text.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Returns the validated span style.
    pub(crate) fn style(&self) -> TextStyle {
        self.style
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, BlockRect, Document, DocumentError, Span, TextStyle};

    #[test]
    fn block_rect_rejects_invalid_geometry() {
        assert_eq!(
            BlockRect::new(0.0, 0.0, 0.0, 20.0),
            Err(DocumentError::InvalidRect)
        );
        assert_eq!(
            BlockRect::new(0.0, 0.0, 20.0, f32::NAN),
            Err(DocumentError::InvalidRect)
        );
    }

    #[test]
    fn document_updates_block_geometry_without_exposing_fields() {
        let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let block = Block::new(
            BlockRect::new(0.0, 0.0, 10.0, 10.0).expect("rect must be valid"),
            4.0,
            None,
            vec![Span::new("text", style)],
            0,
        )
        .expect("block must be valid");
        let mut document = Document::new(vec![block]);

        let rect = BlockRect::new(2.0, 3.0, 20.0, 30.0).expect("rect must be valid");
        document
            .set_block_rect(0, rect)
            .expect("block index must exist");
        assert_eq!(document.block(0).expect("block must exist").rect(), rect);
        assert_eq!(
            document.set_block_background_color(1, None),
            Err(DocumentError::MissingBlock { block_index: 1 })
        );
    }
}
