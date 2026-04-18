//! Store-owned viewport and phase types.

use std::time::Instant;

/// Store-owned interaction state derived from input batches and compose results.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct InteractionState {
    pub(crate) scroll_offset: [f32; 2],
    pub(crate) last_known_content_extent: [f32; 2],
    pub(crate) last_known_viewport: [f32; 2],
}

impl InteractionState {
    /// Returns the current vertical scroll limit for the provided viewport/content pair.
    pub(crate) fn max_scroll_y(viewport: [f32; 2], content_extent: [f32; 2]) -> f32 {
        let max_y = content_extent[1] - viewport[1];
        if max_y.is_finite() { max_y.max(0.0) } else { 0.0 }
    }

    /// Clamps the scroll offset into the legal range and returns whether it changed.
    pub(crate) fn clamp_scroll_offset(
        &mut self,
        viewport: [f32; 2],
        content_extent: [f32; 2],
    ) -> bool {
        let previous = self.scroll_offset;
        let max_y = Self::max_scroll_y(viewport, content_extent);
        let next_y = if self.scroll_offset[1].is_nan() {
            0.0
        } else {
            self.scroll_offset[1].clamp(0.0, max_y)
        };
        self.scroll_offset = [0.0, next_y];
        self.scroll_offset != previous
    }
}

/// Runtime-configurable scroll behavior sourced from the store delegate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InteractionConfig {
    pub(crate) line_step_px: f32,
    pub(crate) page_margin_px: f32,
    pub(crate) wheel_line_delta_px: f32,
    pub(crate) wheel_pixel_scale: f32,
}

impl InteractionConfig {
    /// Returns whether every configured scalar is finite and positive.
    pub(crate) fn is_valid(self) -> bool {
        [
            self.line_step_px,
            self.page_margin_px,
            self.wheel_line_delta_px,
            self.wheel_pixel_scale,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0)
    }
}

impl Default for InteractionConfig {
    fn default() -> Self {
        Self {
            line_step_px: 40.0,
            page_margin_px: 40.0,
            wheel_line_delta_px: 40.0,
            wheel_pixel_scale: 1.0,
        }
    }
}

/// Delegate veto result for the default store-side input mapping.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputFilter {
    VetoDefault,
    RunDefault,
}

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
