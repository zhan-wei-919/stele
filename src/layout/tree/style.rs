//! Shared styling primitives for the rich-text tree path.

use crate::layout::document::{validation::validate_optional_color, DocumentError};

use super::validation::{validate_dimension, validate_edges, validate_optional_dimension};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Edges {
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) right: f32,
    pub(crate) bottom: f32,
}

impl Edges {
    pub(crate) const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    pub(crate) fn new(left: f32, top: f32, right: f32, bottom: f32) -> Result<Self, DocumentError> {
        validate_edges([left, top, right, bottom])?;
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    pub(crate) fn all(value: f32) -> Result<Self, DocumentError> {
        Self::new(value, value, value, value)
    }

    pub(crate) fn horizontal(self) -> f32 {
        self.left + self.right
    }

    pub(crate) fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ClipMode {
    #[default]
    None,
    Rect,
}

// The semantic tree exposes all supported alignment modes even though the demo only instantiates
// a subset of them today.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Align {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

// The semantic tree exposes all supported paragraph alignment modes even though the demo only
// instantiates a subset of them today.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TextAlign {
    #[default]
    Start,
    End,
    Center,
    Justify,
}

// The semantic tree exposes both wrap policies even though the demo only instantiates wrapping
// paragraphs today.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum WrapMode {
    #[default]
    Wrap,
    NoWrap,
}

// The semantic tree keeps both line-height strategies available to callers even though the demo
// currently relies on the factor-based default.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LineHeight {
    Factor(f32),
    Absolute(f32),
}

impl Default for LineHeight {
    fn default() -> Self {
        Self::Factor(1.4)
    }
}

impl LineHeight {
    pub(crate) fn resolve(self, fallback: f32) -> f32 {
        match self {
            Self::Factor(factor) => (fallback * factor).max(fallback),
            Self::Absolute(value) => value.max(fallback),
        }
    }

    pub(crate) fn validate(self) -> Result<(), DocumentError> {
        match self {
            Self::Factor(value) | Self::Absolute(value) if value.is_finite() && value > 0.0 => {
                Ok(())
            }
            _ => Err(DocumentError::InvalidLineHeight),
        }
    }
}

// The semantic tree supports multiple atom baseline strategies even though the demo only uses one
// of them in production code today.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum AtomBaseline {
    #[default]
    AlphabeticAlignedToLine,
    MiddleOfLine,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BorderStyle {
    pub(crate) color: [f32; 4],
    pub(crate) width: f32,
}

impl BorderStyle {
    pub(crate) fn new(color: [f32; 4], width: f32) -> Result<Self, DocumentError> {
        validate_optional_color(Some(color))?;
        validate_dimension(width, false).map_err(|_| DocumentError::InvalidBorderWidth)?;
        Ok(Self { color, width })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlockStyle {
    pub(crate) padding: Edges,
    pub(crate) margin: Edges,
    pub(crate) background: Option<[f32; 4]>,
    pub(crate) clip: ClipMode,
    pub(crate) align_self: Align,
    pub(crate) z_index: u32,
    pub(crate) min_width: Option<f32>,
    pub(crate) max_width: Option<f32>,
    pub(crate) min_height: Option<f32>,
    pub(crate) max_height: Option<f32>,
}

impl Default for BlockStyle {
    fn default() -> Self {
        Self {
            padding: Edges::ZERO,
            margin: Edges::ZERO,
            background: None,
            clip: ClipMode::None,
            align_self: Align::Stretch,
            z_index: 0,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
        }
    }
}

impl BlockStyle {
    pub(crate) fn validate(self) -> Result<(), DocumentError> {
        validate_edges([
            self.padding.left,
            self.padding.top,
            self.padding.right,
            self.padding.bottom,
        ])?;
        validate_edges([
            self.margin.left,
            self.margin.top,
            self.margin.right,
            self.margin.bottom,
        ])?;
        validate_optional_color(self.background)?;
        validate_optional_dimension(self.min_width)?;
        validate_optional_dimension(self.max_width)?;
        validate_optional_dimension(self.min_height)?;
        validate_optional_dimension(self.max_height)?;
        if let (Some(min), Some(max)) = (self.min_width, self.max_width) {
            if min > max {
                return Err(DocumentError::InvalidIntrinsicSize);
            }
        }
        if let (Some(min), Some(max)) = (self.min_height, self.max_height) {
            if min > max {
                return Err(DocumentError::InvalidIntrinsicSize);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ParagraphStyle {
    pub(crate) block: BlockStyle,
    pub(crate) line_height: LineHeight,
    pub(crate) text_align: TextAlign,
    pub(crate) wrap: WrapMode,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            block: BlockStyle::default(),
            line_height: LineHeight::default(),
            text_align: TextAlign::Start,
            wrap: WrapMode::Wrap,
        }
    }
}

impl ParagraphStyle {
    pub(crate) fn validate(self) -> Result<(), DocumentError> {
        self.block.validate()?;
        self.line_height.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InlineAtomStyle {
    pub(crate) margin: Edges,
    pub(crate) padding: Edges,
    pub(crate) baseline: AtomBaseline,
    pub(crate) background: Option<[f32; 4]>,
    pub(crate) border: Option<BorderStyle>,
}

impl Default for InlineAtomStyle {
    fn default() -> Self {
        Self {
            margin: Edges::ZERO,
            padding: Edges::ZERO,
            baseline: AtomBaseline::AlphabeticAlignedToLine,
            background: None,
            border: None,
        }
    }
}

impl InlineAtomStyle {
    pub(crate) fn validate(self) -> Result<(), DocumentError> {
        validate_edges([
            self.margin.left,
            self.margin.top,
            self.margin.right,
            self.margin.bottom,
        ])?;
        validate_edges([
            self.padding.left,
            self.padding.top,
            self.padding.right,
            self.padding.bottom,
        ])?;
        validate_optional_color(self.background)?;
        if let Some(border) = self.border {
            let _ = BorderStyle::new(border.color, border.width)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockStyle, BorderStyle, Edges, InlineAtomStyle, LineHeight, ParagraphStyle};
    use crate::layout::document::DocumentError;

    #[test]
    fn edges_reject_negative_values() {
        assert_eq!(
            Edges::new(-1.0, 0.0, 0.0, 0.0),
            Err(DocumentError::InvalidEdges)
        );
    }

    #[test]
    fn line_height_rejects_non_positive_values() {
        assert_eq!(
            LineHeight::Absolute(0.0).validate(),
            Err(DocumentError::InvalidLineHeight)
        );
    }

    #[test]
    fn block_style_rejects_inverted_constraints() {
        let style = BlockStyle {
            min_width: Some(200.0),
            max_width: Some(100.0),
            ..BlockStyle::default()
        };
        assert_eq!(style.validate(), Err(DocumentError::InvalidIntrinsicSize));
    }

    #[test]
    fn inline_atom_style_validates_border_once() {
        let border = BorderStyle::new([1.0, 1.0, 1.0, 1.0], 1.0).expect("valid border");
        let style = InlineAtomStyle {
            border: Some(border),
            ..InlineAtomStyle::default()
        };
        style.validate().expect("style must be valid");
        ParagraphStyle::default()
            .validate()
            .expect("paragraph must be valid");
    }

    #[test]
    fn block_style_rejects_negative_struct_literal_edges() {
        let style = BlockStyle {
            margin: Edges {
                left: -1.0,
                ..Edges::ZERO
            },
            ..BlockStyle::default()
        };
        assert_eq!(style.validate(), Err(DocumentError::InvalidEdges));
    }

    #[test]
    fn inline_atom_style_rejects_negative_struct_literal_edges() {
        let style = InlineAtomStyle {
            padding: Edges {
                top: -1.0,
                ..Edges::ZERO
            },
            ..InlineAtomStyle::default()
        };
        assert_eq!(style.validate(), Err(DocumentError::InvalidEdges));
    }
}
