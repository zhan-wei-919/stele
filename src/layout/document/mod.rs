//! Document model consumed by the layout pipeline.

mod block;
mod error;
mod style;

pub(crate) use block::{Block, BlockRect, Document, Span};
pub(crate) use error::DocumentError;
pub(crate) use style::TextStyle;

fn validate_color(color: [f32; 4]) -> Result<(), DocumentError> {
    if color
        .into_iter()
        .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
    {
        Ok(())
    } else {
        Err(DocumentError::InvalidColor)
    }
}

fn validate_optional_color(color: Option<[f32; 4]>) -> Result<(), DocumentError> {
    if let Some(color) = color {
        validate_color(color)
    } else {
        Ok(())
    }
}
