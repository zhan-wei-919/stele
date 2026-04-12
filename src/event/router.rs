//! Central routing for winit window events.

use log::{info, warn};
use tokio::sync::mpsc::UnboundedSender;
use winit::event::WindowEvent;

use super::handlers::{
    KeyboardHandler, KeyboardInput, MouseHandler, ViewportHandler, ViewportSnapshot, ViewportUpdate,
};
use crate::io::Action;

/// Semantic actions returned to `SteleApp` after routing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RouteAction {
    None,
    Resize(ViewportUpdate),
    RedrawRequested,
    CloseRequested,
}

/// Routes raw winit window events to focused handlers.
pub(crate) struct EventRouter {
    action_tx: UnboundedSender<Action>,
    keyboard_handler: KeyboardHandler,
    mouse_handler: MouseHandler,
    viewport_handler: ViewportHandler,
}

impl EventRouter {
    /// Creates the router and all event-specific handlers.
    pub(crate) fn new(action_tx: UnboundedSender<Action>) -> Self {
        Self {
            keyboard_handler: KeyboardHandler::new(action_tx.clone()),
            mouse_handler: MouseHandler::new(action_tx.clone()),
            viewport_handler: ViewportHandler::new(action_tx.clone()),
            action_tx,
        }
    }

    /// Dispatches one winit window event.
    pub(crate) fn dispatch(
        &mut self,
        event: &WindowEvent,
        viewport: ViewportSnapshot,
    ) -> RouteAction {
        info!(
            "event.router.dispatch event_type={}",
            Self::event_type(event)
        );

        match event {
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => self.dispatch_keyboard_input(KeyboardInput::from_winit(event, *is_synthetic)),
            WindowEvent::MouseInput { state, button, .. } => {
                self.mouse_handler.handle_button(*state, *button);
                RouteAction::None
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_handler.handle_move(*position);
                RouteAction::None
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.mouse_handler.handle_scroll(delta);
                RouteAction::None
            }
            WindowEvent::Resized(size) => RouteAction::Resize(
                self.viewport_handler
                    .handle_resize(*size, viewport.scale_factor),
            ),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => RouteAction::Resize(
                self.viewport_handler
                    .handle_scale(viewport.size, *scale_factor as f32),
            ),
            WindowEvent::RedrawRequested => RouteAction::RedrawRequested,
            WindowEvent::CloseRequested => {
                if self.action_tx.send(Action::Shutdown).is_err() {
                    warn!("event.router.send_failed action=shutdown");
                }
                RouteAction::CloseRequested
            }
            _ => RouteAction::None,
        }
    }

    /// Dispatches one normalized keyboard input.
    pub(crate) fn dispatch_keyboard_input(&self, input: KeyboardInput) -> RouteAction {
        self.keyboard_handler.handle(input);
        RouteAction::None
    }

    fn event_type(event: &WindowEvent) -> &'static str {
        match event {
            WindowEvent::KeyboardInput { .. } => "keyboard_input",
            WindowEvent::MouseInput { .. } => "mouse_input",
            WindowEvent::CursorMoved { .. } => "cursor_moved",
            WindowEvent::MouseWheel { .. } => "mouse_wheel",
            WindowEvent::Resized(..) => "resized",
            WindowEvent::ScaleFactorChanged { .. } => "scale_factor_changed",
            WindowEvent::RedrawRequested => "redraw_requested",
            WindowEvent::CloseRequested => "close_requested",
            _ => "other",
        }
    }
}
