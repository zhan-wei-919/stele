//! Reducer that updates store-owned model state from incoming actions.

use log::{trace, warn};

use crate::io::{
    Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind, MouseScroll,
};

use super::delegate::StoreDelegate;
use super::model::Model;
use super::types::{InputFilter, InteractionConfig, InteractionState, ViewportState};

/// Result of applying one action to the store.
pub(crate) enum ReduceOutcome {
    NoChange,
    Changed,
    Shutdown,
}

/// Applies actions to the logical model and viewport state.
pub(crate) struct Reducer;

impl Reducer {
    /// Applies one action to the current store state.
    pub(crate) fn apply(
        &self,
        model: &mut Model,
        viewport: &mut ViewportState,
        interaction: &mut InteractionState,
        config: InteractionConfig,
        action: &Action,
        delegate: &dyn StoreDelegate,
    ) -> ReduceOutcome {
        match action {
            Action::Shutdown => ReduceOutcome::Shutdown,
            Action::Input { event } => self.apply_input(interaction, config, event, delegate),
            Action::Resize {
                width,
                height,
                scale_factor,
                viewport_revision,
                event_time,
            } => {
                if *scale_factor <= 0.0 {
                    debug_assert!(
                        *scale_factor > 0.0,
                        "viewport scale factor must stay positive"
                    );
                    warn!(
                        "store.invalid_scale_factor scale_factor={} viewport_revision={}",
                        scale_factor, viewport_revision
                    );
                    return ReduceOutcome::NoChange;
                }
                *viewport = ViewportState::new(
                    *width,
                    *height,
                    *scale_factor,
                    *viewport_revision,
                    Some(*event_time),
                );
                delegate.resize(model, viewport.logical_size());
                interaction.clamp_scroll_offset(
                    viewport.logical_size(),
                    interaction.last_known_content_extent,
                );
                ReduceOutcome::Changed
            }
        }
    }

    fn apply_input(
        &self,
        interaction: &mut InteractionState,
        config: InteractionConfig,
        event: &InputEvent,
        delegate: &dyn StoreDelegate,
    ) -> ReduceOutcome {
        if matches!(
            delegate.filter_input(interaction, event),
            InputFilter::VetoDefault
        ) {
            return ReduceOutcome::NoChange;
        }

        let previous = interaction.scroll_offset;
        let next_y = match map_input_to_next_scroll_y(event, interaction, config) {
            Some(next_y) => next_y,
            None => return ReduceOutcome::NoChange,
        };

        interaction.scroll_offset = [0.0, next_y];
        if interaction.scroll_offset == previous {
            ReduceOutcome::NoChange
        } else {
            ReduceOutcome::Changed
        }
    }
}

fn map_input_to_next_scroll_y(
    event: &InputEvent,
    interaction: &InteractionState,
    config: InteractionConfig,
) -> Option<f32> {
    let max_scroll_y = InteractionState::max_scroll_y(
        interaction.last_known_viewport,
        interaction.last_known_content_extent,
    );
    let current_y = interaction.scroll_offset[1];

    match event {
        InputEvent::Key(key_event) => map_key_input_to_next_scroll_y(
            key_event,
            current_y,
            max_scroll_y,
            interaction.last_known_viewport,
            config,
        ),
        InputEvent::Mouse(mouse_event) => {
            let delta = map_mouse_input_to_delta(mouse_event, config)?;
            Some((current_y + delta).clamp(0.0, max_scroll_y))
        }
        InputEvent::Paste(_) | InputEvent::CursorLeft | InputEvent::FocusChanged { .. } => None,
    }
}

fn map_key_input_to_next_scroll_y(
    event: &KeyEvent,
    current_y: f32,
    max_scroll_y: f32,
    last_known_viewport: [f32; 2],
    config: InteractionConfig,
) -> Option<f32> {
    if !matches!(event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    let page_step = (last_known_viewport[1] - config.page_margin_px).max(1.0);
    let next_y = match &event.code {
        KeyCode::Up => current_y - config.line_step_px,
        KeyCode::Down => current_y + config.line_step_px,
        KeyCode::PageUp => current_y - page_step,
        KeyCode::PageDown => current_y + page_step,
        KeyCode::Home => 0.0,
        KeyCode::End => max_scroll_y,
        _ => {
            trace!(
                "store.input_unhandled code={:?} kind={:?}",
                event.code,
                event.kind
            );
            return None;
        }
    };
    Some(next_y.clamp(0.0, max_scroll_y))
}

fn map_mouse_input_to_delta(event: &MouseEvent, config: InteractionConfig) -> Option<f32> {
    match event.kind {
        MouseEventKind::ScrollUp
        | MouseEventKind::ScrollDown
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => {}
        _ => return None,
    }

    match event.scroll_delta {
        Some(MouseScroll::LineDelta { y, .. }) => {
            if !y.is_finite() {
                debug_assert!(y.is_finite(), "mouse line scroll delta must stay finite");
                return None;
            }
            Some(-y * config.wheel_line_delta_px)
        }
        Some(MouseScroll::PixelDelta { y, .. }) => {
            if !y.is_finite() {
                debug_assert!(y.is_finite(), "mouse pixel scroll delta must stay finite");
                return None;
            }
            Some(-(y as f32) * config.wheel_pixel_scale)
        }
        None => None,
    }
}
