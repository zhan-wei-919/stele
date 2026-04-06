//! Stable layer ordering shared by block draw groups.

/// Fixed layer ordering used by the renderer when submitting draw calls.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub(crate) enum RenderLayer {
    Background,
    #[default]
    Content,
    Foreground,
    Overlay,
}

impl RenderLayer {
    pub(crate) const ALL: [Self; 4] = [
        Self::Background,
        Self::Content,
        Self::Foreground,
        Self::Overlay,
    ];

    /// Returns the stable bucket index used by runtime arrays.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Background => 0,
            Self::Content => 1,
            Self::Foreground => 2,
            Self::Overlay => 3,
        }
    }
}

/// Block-local layer ordering used during block-aware renderer submission.
pub(crate) type BlockSubLayer = RenderLayer;
