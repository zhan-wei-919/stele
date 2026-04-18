//! Store-owned viewport and phase types.

use std::time::Instant;

/// Physical viewport input used by the store for layout and diff invalidation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewportState {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale_factor: f32,
    pub(crate) viewport_revision: u64,
    pub(crate) resize_started_at: Option<Instant>,
}

impl ViewportState {
    /// Creates a validated viewport snapshot.
    pub(crate) fn new(
        width: u32,
        height: u32,
        scale_factor: f32,
        viewport_revision: u64,
        resize_started_at: Option<Instant>,
    ) -> Self {
        debug_assert!(
            scale_factor > 0.0,
            "viewport scale factor must stay positive"
        );
        Self {
            width,
            height,
            scale_factor,
            viewport_revision,
            resize_started_at,
        }
    }

    /// Returns the current viewport in logical layout units.
    pub(crate) fn logical_size(self) -> [f32; 2] {
        debug_assert!(
            self.scale_factor > 0.0,
            "viewport scale factor must stay positive"
        );
        [
            self.width as f32 / self.scale_factor,
            self.height as f32 / self.scale_factor,
        ]
    }
}

/// Store pipeline phase used for logs and debugging.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StorePhase {
    Idle,
    Reducing,
    Laying,
    ComposingSnapshot,
}
