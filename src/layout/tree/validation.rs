//! Validation helpers shared by the tree input model.

use crate::layout::document::DocumentError;

pub(crate) fn validate_dimension(value: f32, allow_zero: bool) -> Result<(), DocumentError> {
    if !value.is_finite() || value < 0.0 || (!allow_zero && value <= 0.0) {
        return Err(DocumentError::InvalidIntrinsicSize);
    }
    Ok(())
}

pub(crate) fn validate_optional_dimension(value: Option<f32>) -> Result<(), DocumentError> {
    if let Some(value) = value {
        validate_dimension(value, false)?;
    }
    Ok(())
}

pub(crate) fn validate_edges(edges: [f32; 4]) -> Result<(), DocumentError> {
    if edges
        .into_iter()
        .all(|value| value.is_finite() && value >= 0.0)
    {
        Ok(())
    } else {
        Err(DocumentError::InvalidEdges)
    }
}
