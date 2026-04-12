//! Event-specific handlers used by the winit router.

use log::warn;
use tokio::sync::mpsc::UnboundedSender;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta};

use crate::io::{Action, ButtonState, MouseButtonKind, MouseScroll};

/// Keyboard input normalized at the winit boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyboardInput {
    text: Option<String>,
    logical_key: String,
    is_synthetic: bool,
}

impl KeyboardInput {
    /// Builds normalized keyboard input from one winit key event.
    pub(crate) fn from_winit(event: &KeyEvent, is_synthetic: bool) -> Self {
        Self {
            text: event.text.as_deref().map(str::to_owned),
            logical_key: format!("{:?}", event.logical_key),
            is_synthetic,
        }
    }

    /// Builds normalized keyboard input for tests and internal callers.
    #[cfg(test)]
    pub(crate) fn new(
        text: Option<&str>,
        logical_key: impl Into<String>,
        is_synthetic: bool,
    ) -> Self {
        Self {
            text: text.map(str::to_owned),
            logical_key: logical_key.into(),
            is_synthetic,
        }
    }

    fn into_action(self) -> Option<Action> {
        if self.is_synthetic {
            return None;
        }

        Some(Action::KeyInput {
            text: self.text.unwrap_or(self.logical_key),
        })
    }
}

/// Window metrics snapshot captured at the routing boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewportSnapshot {
    pub(crate) size: PhysicalSize<u32>,
    pub(crate) scale_factor: f32,
}

impl ViewportSnapshot {
    /// Captures the current window metrics needed by resize routing.
    pub(crate) fn new(size: PhysicalSize<u32>, scale_factor: f32) -> Self {
        Self { size, scale_factor }
    }
}

/// View-facing viewport updates derived from window events.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewportUpdate {
    pub(crate) size: PhysicalSize<u32>,
    pub(crate) scale_factor: f32,
    pub(crate) viewport_revision: u64,
}

/// Handles keyboard input and forwards it through the shared action channel.
pub(crate) struct KeyboardHandler {
    action_tx: UnboundedSender<Action>,
}

impl KeyboardHandler {
    /// Creates a keyboard handler backed by the shared action channel.
    pub(crate) fn new(action_tx: UnboundedSender<Action>) -> Self {
        Self { action_tx }
    }

    /// Forwards keyboard input to the async store.
    pub(crate) fn handle(&self, input: KeyboardInput) {
        if let Some(action) = input.into_action() {
            self.send_action(action);
        }
    }

    fn send_action(&self, action: Action) {
        if self.action_tx.send(action).is_err() {
            warn!("event.handler.send_failed handler=keyboard");
        }
    }
}

/// Handles mouse input and forwards it through the shared action channel.
pub(crate) struct MouseHandler {
    action_tx: UnboundedSender<Action>,
}

impl MouseHandler {
    /// Creates a mouse handler backed by the shared action channel.
    pub(crate) fn new(action_tx: UnboundedSender<Action>) -> Self {
        Self { action_tx }
    }

    /// Forwards mouse button activity.
    pub(crate) fn handle_button(&self, state: ElementState, button: MouseButton) {
        let action = Action::MouseButton {
            state: match state {
                ElementState::Pressed => ButtonState::Pressed,
                ElementState::Released => ButtonState::Released,
            },
            button: match button {
                MouseButton::Left => MouseButtonKind::Left,
                MouseButton::Right => MouseButtonKind::Right,
                MouseButton::Middle => MouseButtonKind::Middle,
                MouseButton::Back => MouseButtonKind::Back,
                MouseButton::Forward => MouseButtonKind::Forward,
                MouseButton::Other(value) => MouseButtonKind::Other(value),
            },
        };
        self.send_action(action);
    }

    /// Forwards cursor movement.
    pub(crate) fn handle_move(&self, position: PhysicalPosition<f64>) {
        self.send_action(Action::MouseMove {
            x: position.x,
            y: position.y,
        });
    }

    /// Forwards wheel activity.
    pub(crate) fn handle_scroll(&self, delta: &MouseScrollDelta) {
        let delta = match delta {
            MouseScrollDelta::LineDelta(x, y) => MouseScroll::LineDelta { x: *x, y: *y },
            MouseScrollDelta::PixelDelta(position) => MouseScroll::PixelDelta {
                x: position.x,
                y: position.y,
            },
        };
        self.send_action(Action::MouseScroll { delta });
    }

    fn send_action(&self, action: Action) {
        if self.action_tx.send(action).is_err() {
            warn!("event.handler.send_failed handler=mouse");
        }
    }
}

/// Handles viewport changes and emits monotonic viewport revisions.
pub(crate) struct ViewportHandler {
    action_tx: UnboundedSender<Action>,
    next_viewport_revision: u64,
}

impl ViewportHandler {
    /// Creates a viewport handler backed by the shared action channel.
    pub(crate) fn new(action_tx: UnboundedSender<Action>) -> Self {
        Self {
            action_tx,
            next_viewport_revision: 0,
        }
    }

    /// Processes a resize event.
    pub(crate) fn handle_resize(
        &mut self,
        size: PhysicalSize<u32>,
        scale_factor: f32,
    ) -> ViewportUpdate {
        self.send_resize_action(size, scale_factor)
    }

    /// Processes a scale-factor change.
    pub(crate) fn handle_scale(
        &mut self,
        size: PhysicalSize<u32>,
        scale_factor: f32,
    ) -> ViewportUpdate {
        self.send_resize_action(size, scale_factor)
    }

    fn send_resize_action(&mut self, size: PhysicalSize<u32>, scale_factor: f32) -> ViewportUpdate {
        self.next_viewport_revision += 1;
        let update = ViewportUpdate {
            size,
            scale_factor,
            viewport_revision: self.next_viewport_revision,
        };
        let action = Action::Resize {
            width: size.width,
            height: size.height,
            scale_factor,
            viewport_revision: update.viewport_revision,
        };
        if self.action_tx.send(action).is_err() {
            warn!("event.handler.send_failed handler=viewport");
        }
        update
    }
}

#[cfg(test)]
mod tests {
    use super::KeyboardInput;
    use crate::io::Action;

    #[test]
    fn keyboard_input_prefers_text_payload() {
        let input = KeyboardInput::new(Some("a"), "KeyA", false);

        assert_eq!(
            input.into_action(),
            Some(Action::KeyInput {
                text: String::from("a"),
            })
        );
    }

    #[test]
    fn synthetic_keyboard_input_is_filtered_at_source() {
        let input = KeyboardInput::new(Some("a"), "KeyA", true);

        assert_eq!(input.into_action(), None);
    }

    #[test]
    fn keyboard_input_falls_back_to_logical_key_when_text_is_missing() {
        let input = KeyboardInput::new(None, "Named(Enter)", false);

        assert_eq!(
            input.into_action(),
            Some(Action::KeyInput {
                text: String::from("Named(Enter)"),
            })
        );
    }
}
