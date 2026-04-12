//! Block clip geometry used by the renderer.

/// Logical clip rectangle applied to one block during rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClipRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl ClipRect {
    /// Creates a clip rectangle whose geometry is validated once at construction time.
    pub(crate) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
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
    pub(crate) fn origin(&self) -> [f32; 2] {
        [self.x, self.y]
    }

    /// Returns the clip rectangle size in logical pixels.
    pub(crate) fn size(&self) -> [f32; 2] {
        [self.width, self.height]
    }
}
