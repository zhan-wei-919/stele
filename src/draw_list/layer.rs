//! Stable layer ordering shared by block draw groups.

/// Fixed layer ordering used by the renderer when submitting draw calls.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub(crate) enum RenderLayer {
    Background,
    #[default]
    Content,
    Foreground,
    // Reserved for scene elements that must sort above block content once overlay producers land.
    #[allow(dead_code)]
    Overlay,
}
