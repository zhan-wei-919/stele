//! Validation helpers shared by the document value objects.

use super::DocumentError;

/// Validates one RGBA color at the document boundary.
pub(crate) fn validate_color(color: [f32; 4]) -> Result<(), DocumentError> {
    if color
        .into_iter()
        .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
    {
        Ok(())
    } else {
        Err(DocumentError::InvalidColor)
    }
}

/// Validates an optional RGBA color at the document boundary.
pub(crate) fn validate_optional_color(color: Option<[f32; 4]>) -> Result<(), DocumentError> {
    if let Some(color) = color {
        validate_color(color)
    } else {
        Ok(())
    }
}
