//! Shared validation helpers for draw-list value objects.

/// Returns whether a normalized RGBA color is finite and inside `[0, 1]`.
pub(super) fn color_is_valid(color: [f32; 4]) -> bool {
    color
        .into_iter()
        .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
}
