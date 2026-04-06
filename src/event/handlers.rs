//! Event-specific handlers used by the router.

use log::warn;
use tokio::sync::mpsc::UnboundedSender;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta};

use crate::io::{AppCommand, ButtonState, MockMouseEvent, MouseButtonKind, MouseScroll};

/// Keyboard input normalized at the winit boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyboardInput {
    text: Option<String>,
    logical_key: String,
    is_synthetic: bool,
}

impl KeyboardInput {
    /// Builds a normalized keyboard input from a winit event.
    pub(crate) fn from_winit(event: &KeyEvent, is_synthetic: bool) -> Self {
        Self {
            text: event.text.as_deref().map(str::to_owned),
            logical_key: format!("{:?}", event.logical_key),
            is_synthetic,
        }
    }

    /// Builds a normalized keyboard input for tests and internal callers.
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

    fn into_command(self) -> Option<AppCommand> {
        if self.is_synthetic {
            return None;
        }

        Some(AppCommand::MockKeyInput {
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

/// Renderer-facing viewport updates derived from window events.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewportUpdate {
    pub(crate) size: PhysicalSize<u32>,
    pub(crate) scale_factor: f32,
}

/// Handles keyboard input and forwards it as mock semantic commands.
pub(crate) struct KeyboardHandler {
    command_tx: UnboundedSender<AppCommand>,
}

impl KeyboardHandler {
    /// Creates a keyboard handler backed by the shared app-command channel.
    pub(crate) fn new(command_tx: UnboundedSender<AppCommand>) -> Self {
        Self { command_tx }
    }

    /// Forwards keyboard input to the async side.
    pub(crate) fn handle(&self, input: KeyboardInput) {
        if let Some(command) = input.into_command() {
            self.send_command(command);
        }
    }

    fn send_command(&self, command: AppCommand) {
        if self.command_tx.send(command).is_err() {
            warn!("event.handler.send_failed handler=keyboard");
        }
    }
}

/// Handles mouse input and forwards it through the shared mock command path.
pub(crate) struct MouseHandler {
    command_tx: UnboundedSender<AppCommand>,
}

impl MouseHandler {
    /// Creates a mouse handler backed by the shared app-command channel.
    pub(crate) fn new(command_tx: UnboundedSender<AppCommand>) -> Self {
        Self { command_tx }
    }

    /// Forwards mouse button activity.
    pub(crate) fn handle_button(&self, state: ElementState, button: MouseButton) {
        self.send_command(MockMouseEvent::Button {
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
        });
    }

    /// Forwards cursor movement.
    pub(crate) fn handle_move(&self, position: PhysicalPosition<f64>) {
        self.send_command(MockMouseEvent::Move {
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
        self.send_command(MockMouseEvent::Scroll { delta });
    }

    fn send_command(&self, event: MockMouseEvent) {
        if self
            .command_tx
            .send(AppCommand::MockMouseInput { event })
            .is_err()
        {
            warn!("event.handler.send_failed handler=mouse");
        }
    }
}

/// Handles window-size changes and exposes viewport updates to the app layer.
pub(crate) struct ViewportHandler {
    command_tx: UnboundedSender<AppCommand>,
}

impl ViewportHandler {
    /// Creates a viewport handler backed by the shared app-command channel.
    pub(crate) fn new(command_tx: UnboundedSender<AppCommand>) -> Self {
        Self { command_tx }
    }

    /// Processes a resize event.
    pub(crate) fn handle_resize(
        &self,
        size: PhysicalSize<u32>,
        scale_factor: f32,
    ) -> ViewportUpdate {
        self.send_resize_command(size);
        ViewportUpdate { size, scale_factor }
    }

    /// Processes a scale-factor change.
    pub(crate) fn handle_scale(
        &self,
        size: PhysicalSize<u32>,
        scale_factor: f32,
    ) -> ViewportUpdate {
        self.send_resize_command(size);
        ViewportUpdate { size, scale_factor }
    }

    fn send_resize_command(&self, size: PhysicalSize<u32>) {
        let command = AppCommand::MockResize {
            width: size.width,
            height: size.height,
        };

        if self.command_tx.send(command).is_err() {
            warn!("event.handler.send_failed handler=viewport");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KeyboardInput;
    use crate::io::AppCommand;

    #[test]
    fn keyboard_input_prefers_text_payload() {
        let input = KeyboardInput::new(Some("a"), "KeyA", false);

        assert_eq!(
            input.into_command(),
            Some(AppCommand::MockKeyInput {
                text: String::from("a"),
            })
        );
    }

    #[test]
    fn synthetic_keyboard_input_is_filtered_at_source() {
        let input = KeyboardInput::new(Some("a"), "KeyA", true);

        assert_eq!(input.into_command(), None);
    }

    #[test]
    fn keyboard_input_falls_back_to_logical_key_when_text_is_missing() {
        let input = KeyboardInput::new(None, "Named(Enter)", false);

        assert_eq!(
            input.into_command(),
            Some(AppCommand::MockKeyInput {
                text: String::from("Named(Enter)"),
            })
        );
    }
}
