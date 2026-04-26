//! Inline text styling used by the document model.

use super::validation::{validate_color, validate_optional_color};
use super::DocumentError;

/// Inline styling applied to a span's text.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    font_id: u32,
    font_size: f32,
    color: [f32; 4],
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    background_color: Option<[f32; 4]>,
    letter_spacing: f32,
}

impl TextStyle {
    /// Creates a style with validated font and color inputs.
    pub fn new(font_id: u32, font_size: f32, color: [f32; 4]) -> Result<Self, DocumentError> {
        if !font_size.is_finite() || font_size <= 0.0 {
            return Err(DocumentError::InvalidFontSize);
        }
        validate_color(color)?;

        Ok(Self {
            font_id,
            font_size,
            color,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            background_color: None,
            letter_spacing: 0.0,
        })
    }

    /// Returns the base font id requested by the span.
    pub fn font_id(self) -> u32 {
        self.font_id
    }

    /// Returns the validated font size in logical pixels.
    pub fn font_size(self) -> f32 {
        self.font_size
    }

    /// Returns the validated text color.
    pub fn color(self) -> [f32; 4] {
        self.color
    }

    /// Returns whether bold face selection is requested.
    pub fn bold(self) -> bool {
        self.bold
    }

    /// Returns whether italic face selection is requested.
    pub fn italic(self) -> bool {
        self.italic
    }

    /// Returns whether underline decoration is enabled.
    pub fn underline(self) -> bool {
        self.underline
    }

    /// Returns whether strikethrough decoration is enabled.
    pub fn strikethrough(self) -> bool {
        self.strikethrough
    }

    /// Returns the optional background color behind this span.
    pub fn background_color(self) -> Option<[f32; 4]> {
        self.background_color
    }

    /// Returns the additional spacing inserted after each non-final glyph.
    pub fn letter_spacing(self) -> f32 {
        self.letter_spacing
    }

    /// Returns the same style with bold selection enabled or disabled.
    pub fn with_bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }

    /// Returns the same style with italic selection enabled or disabled.
    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    /// Returns the same style with underline decoration enabled or disabled.
    pub fn with_underline(mut self, underline: bool) -> Self {
        self.underline = underline;
        self
    }

    /// Returns the same style with strikethrough decoration enabled or disabled.
    pub fn with_strikethrough(mut self, strikethrough: bool) -> Self {
        self.strikethrough = strikethrough;
        self
    }

    /// Returns the same style with an optional validated background color.
    pub fn with_background_color(
        mut self,
        background_color: Option<[f32; 4]>,
    ) -> Result<Self, DocumentError> {
        validate_optional_color(background_color)?;
        self.background_color = background_color;
        Ok(self)
    }

    /// Returns the same style with validated letter spacing.
    pub fn with_letter_spacing(mut self, letter_spacing: f32) -> Result<Self, DocumentError> {
        if !letter_spacing.is_finite() || letter_spacing < 0.0 {
            return Err(DocumentError::InvalidLetterSpacing);
        }
        self.letter_spacing = letter_spacing;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{DocumentError, TextStyle};

    #[test]
    fn text_style_rejects_invalid_inputs() {
        assert!(matches!(
            TextStyle::new(0, 0.0, [1.0, 1.0, 1.0, 1.0]),
            Err(DocumentError::InvalidFontSize)
        ));
        assert!(matches!(
            TextStyle::new(0, 12.0, [1.5, 1.0, 1.0, 1.0]),
            Err(DocumentError::InvalidColor)
        ));
        let style = TextStyle::new(0, 12.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        assert!(matches!(
            style.with_letter_spacing(-1.0),
            Err(DocumentError::InvalidLetterSpacing)
        ));
    }
}
