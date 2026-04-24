//! Central routing for winit window events.

use std::time::Instant;

use log::{info, warn};
use tokio::sync::mpsc::UnboundedSender;
use winit::event::WindowEvent;

use super::clipboard::{ClipboardProvider, SystemClipboard};
use super::handlers::{
    key_modifiers_from_winit, mouse_button_kind, KeyboardHandler, KeyboardInput, MouseHandler,
    ViewportHandler, ViewportSnapshot, ViewportUpdate,
};
use crate::io::{
    Action, InputEvent, KeyCode, KeyEventKind, KeyModifiers, MouseButtonKind, MouseEventKind,
};

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
    clipboard: Box<dyn ClipboardProvider>,
    current_modifiers: KeyModifiers,
    latest_pointer_physical: Option<[f64; 2]>,
    latest_pointer_logical: Option<[f32; 2]>,
    pressed_mouse_buttons: Vec<MouseButtonKind>,
}

impl EventRouter {
    /// Creates the router and all event-specific handlers.
    pub(crate) fn new(action_tx: UnboundedSender<Action>) -> Self {
        Self::with_clipboard(action_tx, Box::new(SystemClipboard::new()))
    }

    /// Creates the router with an injected clipboard provider for tests.
    #[cfg(test)]
    pub(crate) fn new_with_clipboard(
        action_tx: UnboundedSender<Action>,
        clipboard: Box<dyn ClipboardProvider>,
    ) -> Self {
        Self::with_clipboard(action_tx, clipboard)
    }

    fn with_clipboard(
        action_tx: UnboundedSender<Action>,
        clipboard: Box<dyn ClipboardProvider>,
    ) -> Self {
        Self {
            keyboard_handler: KeyboardHandler::new(action_tx.clone()),
            mouse_handler: MouseHandler::new(action_tx.clone()),
            viewport_handler: ViewportHandler::new(action_tx.clone()),
            clipboard,
            action_tx,
            current_modifiers: KeyModifiers::NONE,
            latest_pointer_physical: None,
            latest_pointer_logical: None,
            pressed_mouse_buttons: Vec::new(),
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
            WindowEvent::ModifiersChanged(modifiers) => {
                self.current_modifiers = key_modifiers_from_winit(modifiers.state());
                RouteAction::None
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = mouse_button_kind(*button);
                let kind = match state {
                    winit::event::ElementState::Pressed => {
                        self.note_mouse_button_pressed(button);
                        MouseEventKind::Down(button)
                    }
                    winit::event::ElementState::Released => {
                        self.note_mouse_button_released(button);
                        MouseEventKind::Up(button)
                    }
                };
                self.mouse_handler.handle_button(
                    kind,
                    self.latest_pointer_logical,
                    self.current_modifiers,
                    Instant::now(),
                );
                RouteAction::None
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.update_pointer_snapshot([position.x, position.y], viewport.scale_factor);
                let kind = self
                    .current_drag_button()
                    .map(MouseEventKind::Drag)
                    .unwrap_or(MouseEventKind::Moved);
                self.mouse_handler.handle_move(
                    kind,
                    self.latest_pointer_logical,
                    self.current_modifiers,
                    Instant::now(),
                );
                RouteAction::None
            }
            WindowEvent::CursorLeft { .. } => {
                self.clear_pointer_snapshots();
                self.send_input_event(InputEvent::CursorLeft);
                RouteAction::None
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.mouse_handler.handle_scroll(
                    delta,
                    self.latest_pointer_logical,
                    self.current_modifiers,
                    Instant::now(),
                );
                RouteAction::None
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    self.clear_pointer_snapshots();
                    self.pressed_mouse_buttons.clear();
                    self.current_modifiers = KeyModifiers::NONE;
                }
                self.send_input_event(InputEvent::FocusChanged { focused: *focused });
                RouteAction::None
            }
            WindowEvent::Resized(size) => {
                let update = self
                    .viewport_handler
                    .handle_resize(*size, viewport.scale_factor);
                self.recompute_pointer_logical(update.scale_factor);
                RouteAction::Resize(update)
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let update = self
                    .viewport_handler
                    .handle_scale(viewport.size, *scale_factor as f32);
                self.recompute_pointer_logical(update.scale_factor);
                RouteAction::Resize(update)
            }
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
    pub(crate) fn dispatch_keyboard_input(&mut self, input: KeyboardInput) -> RouteAction {
        if input.is_synthetic() {
            return RouteAction::None;
        }

        let modifiers = self.effective_modifiers_for_key(&input);
        self.update_modifier_snapshot_from_key(&input);
        if is_paste_shortcut(&input, modifiers) {
            self.dispatch_paste();
            return RouteAction::None;
        }

        self.keyboard_handler.handle(input, modifiers);
        RouteAction::None
    }

    fn effective_modifiers_for_key(&self, input: &KeyboardInput) -> KeyModifiers {
        let mut modifiers = self.current_modifiers;
        if let Some(flag) = modifier_flag(input.code()) {
            modifiers.set(flag, input.kind() != KeyEventKind::Release);
        }
        modifiers
    }

    fn update_modifier_snapshot_from_key(&mut self, input: &KeyboardInput) {
        if let Some(flag) = modifier_flag(input.code()) {
            self.current_modifiers
                .set(flag, input.kind() != KeyEventKind::Release);
        }
    }

    fn update_pointer_snapshot(&mut self, physical_position: [f64; 2], scale_factor: f32) {
        self.latest_pointer_physical = Some(physical_position);
        self.latest_pointer_logical = physical_to_logical(physical_position, scale_factor);
    }

    fn recompute_pointer_logical(&mut self, scale_factor: f32) {
        self.latest_pointer_logical = self
            .latest_pointer_physical
            .and_then(|physical| physical_to_logical(physical, scale_factor));
    }

    fn clear_pointer_snapshots(&mut self) {
        self.latest_pointer_physical = None;
        self.latest_pointer_logical = None;
    }

    fn note_mouse_button_pressed(&mut self, button: MouseButtonKind) {
        if let Some(index) = self
            .pressed_mouse_buttons
            .iter()
            .position(|pressed| *pressed == button)
        {
            self.pressed_mouse_buttons.remove(index);
        }
        self.pressed_mouse_buttons.push(button);
    }

    fn note_mouse_button_released(&mut self, button: MouseButtonKind) {
        if let Some(index) = self
            .pressed_mouse_buttons
            .iter()
            .position(|pressed| *pressed == button)
        {
            self.pressed_mouse_buttons.remove(index);
        }
    }

    fn current_drag_button(&self) -> Option<MouseButtonKind> {
        self.pressed_mouse_buttons.last().copied()
    }

    fn dispatch_paste(&mut self) {
        // TODO(input): Move clipboard reads behind a serial input producer so desktop
        // clipboard IPC cannot block winit dispatch while paste ordering stays deterministic.
        match self.clipboard.read_text() {
            Ok(text) => self.send_input_event(InputEvent::Paste(text)),
            Err(error) => warn!("event.clipboard_read_failed error={}", error),
        }
    }

    fn send_input_event(&self, event: InputEvent) {
        if self.action_tx.send(Action::Input { event }).is_err() {
            warn!("event.router.send_failed action=input");
        }
    }

    fn event_type(event: &WindowEvent) -> &'static str {
        match event {
            WindowEvent::KeyboardInput { .. } => "keyboard_input",
            WindowEvent::ModifiersChanged(..) => "modifiers_changed",
            WindowEvent::Focused(..) => "focused",
            WindowEvent::MouseInput { .. } => "mouse_input",
            WindowEvent::CursorMoved { .. } => "cursor_moved",
            WindowEvent::CursorLeft { .. } => "cursor_left",
            WindowEvent::MouseWheel { .. } => "mouse_wheel",
            WindowEvent::Resized(..) => "resized",
            WindowEvent::ScaleFactorChanged { .. } => "scale_factor_changed",
            WindowEvent::RedrawRequested => "redraw_requested",
            WindowEvent::CloseRequested => "close_requested",
            _ => "other",
        }
    }
}

fn is_paste_shortcut(input: &KeyboardInput, modifiers: KeyModifiers) -> bool {
    if input.kind() != KeyEventKind::Press {
        return false;
    }
    if modifiers.alt() {
        return false;
    }
    if !modifiers.control() && !modifiers.super_key() {
        return false;
    }

    matches!(input.code(), KeyCode::Char('v' | 'V'))
}

fn physical_to_logical(physical_position: [f64; 2], scale_factor: f32) -> Option<[f32; 2]> {
    if scale_factor <= 0.0 {
        return None;
    }

    Some([
        physical_position[0] as f32 / scale_factor,
        physical_position[1] as f32 / scale_factor,
    ])
}

fn modifier_flag(code: &KeyCode) -> Option<KeyModifiers> {
    match code {
        KeyCode::Shift => Some(KeyModifiers::SHIFT),
        KeyCode::Control => Some(KeyModifiers::CONTROL),
        KeyCode::Alt => Some(KeyModifiers::ALT),
        KeyCode::Super => Some(KeyModifiers::SUPER),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;
    use winit::dpi::PhysicalSize;
    use winit::event::{DeviceId, WindowEvent};
    use winit::keyboard::ModifiersState;

    use super::EventRouter;
    use crate::event::clipboard::{ClipboardProvider, ClipboardReadError};
    use crate::event::handlers::KeyboardInput;
    use crate::event::{RouteAction, ViewportSnapshot};
    use crate::io::{
        Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButtonKind,
        MouseEventKind,
    };

    fn build_router() -> (EventRouter, mpsc::UnboundedReceiver<Action>) {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        (EventRouter::new(action_tx), action_rx)
    }

    fn build_router_with_clipboard(
        clipboard: FakeClipboard,
    ) -> (EventRouter, mpsc::UnboundedReceiver<Action>) {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        (
            EventRouter::new_with_clipboard(action_tx, Box::new(clipboard)),
            action_rx,
        )
    }

    #[test]
    fn control_v_press_emits_paste_from_clipboard() {
        let (mut router, mut action_rx) =
            build_router_with_clipboard(FakeClipboard::with_text("from clipboard"));
        router.current_modifiers = KeyModifiers::CONTROL;

        assert_eq!(
            router.dispatch_keyboard_input(KeyboardInput::new(
                KeyCode::Char('v'),
                KeyEventKind::Press,
                false,
            )),
            RouteAction::None
        );

        assert_eq!(
            action_rx.try_recv().expect("paste action must be emitted"),
            Action::Input {
                event: InputEvent::Paste("from clipboard".to_owned()),
            }
        );
    }

    #[test]
    fn super_v_press_emits_paste_from_clipboard() {
        let (mut router, mut action_rx) =
            build_router_with_clipboard(FakeClipboard::with_text("command paste"));
        router.current_modifiers = KeyModifiers::SUPER;

        router.dispatch_keyboard_input(KeyboardInput::new(
            KeyCode::Char('V'),
            KeyEventKind::Press,
            false,
        ));

        assert_eq!(
            action_rx.try_recv().expect("paste action must be emitted"),
            Action::Input {
                event: InputEvent::Paste("command paste".to_owned()),
            }
        );
    }

    #[test]
    fn shift_does_not_block_paste_but_alt_does() {
        let (mut shift_router, mut shift_rx) =
            build_router_with_clipboard(FakeClipboard::with_text("shift paste"));
        shift_router.current_modifiers = KeyModifiers::CONTROL | KeyModifiers::SHIFT;

        shift_router.dispatch_keyboard_input(KeyboardInput::new(
            KeyCode::Char('V'),
            KeyEventKind::Press,
            false,
        ));

        assert_eq!(
            shift_rx.try_recv().expect("shift paste must be emitted"),
            Action::Input {
                event: InputEvent::Paste("shift paste".to_owned()),
            }
        );

        let (mut alt_router, mut alt_rx) =
            build_router_with_clipboard(FakeClipboard::with_text("blocked"));
        alt_router.current_modifiers = KeyModifiers::CONTROL | KeyModifiers::ALT;

        alt_router.dispatch_keyboard_input(KeyboardInput::new(
            KeyCode::Char('v'),
            KeyEventKind::Press,
            false,
        ));

        assert_eq!(
            alt_rx
                .try_recv()
                .expect("alt-modified key must stay a key fact"),
            Action::Input {
                event: InputEvent::Key(KeyEvent {
                    code: KeyCode::Char('v'),
                    modifiers: KeyModifiers::CONTROL | KeyModifiers::ALT,
                    kind: KeyEventKind::Press,
                }),
            }
        );
    }

    #[test]
    fn synthetic_repeat_release_and_clipboard_errors_do_not_emit_paste() {
        let (mut synthetic_router, mut synthetic_rx) =
            build_router_with_clipboard(FakeClipboard::with_text("synthetic"));
        synthetic_router.current_modifiers = KeyModifiers::CONTROL;
        synthetic_router.dispatch_keyboard_input(KeyboardInput::new(
            KeyCode::Char('v'),
            KeyEventKind::Press,
            true,
        ));
        assert!(synthetic_rx.try_recv().is_err());

        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            let (mut router, mut action_rx) =
                build_router_with_clipboard(FakeClipboard::with_text("repeat"));
            router.current_modifiers = KeyModifiers::CONTROL;
            router.dispatch_keyboard_input(KeyboardInput::new(KeyCode::Char('v'), kind, false));

            assert_eq!(
                action_rx
                    .try_recv()
                    .expect("non-press paste shortcut must stay a key fact"),
                Action::Input {
                    event: InputEvent::Key(KeyEvent {
                        code: KeyCode::Char('v'),
                        modifiers: KeyModifiers::CONTROL,
                        kind,
                    }),
                }
            );
        }

        let (mut error_router, mut error_rx) =
            build_router_with_clipboard(FakeClipboard::with_error());
        error_router.current_modifiers = KeyModifiers::CONTROL;
        error_router.dispatch_keyboard_input(KeyboardInput::new(
            KeyCode::Char('v'),
            KeyEventKind::Press,
            false,
        ));
        assert!(error_rx.try_recv().is_err());
    }

    #[test]
    fn modifier_key_events_keep_canonical_modifier_state() {
        let (mut router, mut action_rx) = build_router();

        assert_eq!(
            router.dispatch_keyboard_input(KeyboardInput::new(
                KeyCode::Control,
                KeyEventKind::Press,
                false,
            )),
            RouteAction::None
        );
        assert_eq!(router.current_modifiers, KeyModifiers::CONTROL);
        assert_eq!(
            action_rx
                .try_recv()
                .expect("control press must emit one key event"),
            Action::Input {
                event: InputEvent::Key(KeyEvent {
                    code: KeyCode::Control,
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                }),
            }
        );

        router.dispatch_keyboard_input(KeyboardInput::new(
            KeyCode::Char('c'),
            KeyEventKind::Press,
            false,
        ));
        assert_eq!(
            action_rx
                .try_recv()
                .expect("character key must inherit control"),
            Action::Input {
                event: InputEvent::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                }),
            }
        );

        router.dispatch_keyboard_input(KeyboardInput::new(
            KeyCode::Control,
            KeyEventKind::Release,
            false,
        ));
        assert_eq!(router.current_modifiers, KeyModifiers::NONE);
        assert_eq!(
            action_rx
                .try_recv()
                .expect("control release must clear the modifier bit"),
            Action::Input {
                event: InputEvent::Key(KeyEvent {
                    code: KeyCode::Control,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Release,
                }),
            }
        );
    }

    #[test]
    fn modifiers_changed_updates_router_snapshot() {
        let (mut router, mut action_rx) = build_router();
        let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

        assert_eq!(
            router.dispatch(
                &WindowEvent::ModifiersChanged(ModifiersState::ALT.into()),
                viewport,
            ),
            RouteAction::None
        );
        router.dispatch_keyboard_input(KeyboardInput::new(
            KeyCode::Char('x'),
            KeyEventKind::Press,
            false,
        ));

        assert_eq!(router.current_modifiers, KeyModifiers::ALT);
        assert_eq!(
            action_rx
                .try_recv()
                .expect("character key must inherit modifier snapshot"),
            Action::Input {
                event: InputEvent::Key(KeyEvent {
                    code: KeyCode::Char('x'),
                    modifiers: KeyModifiers::ALT,
                    kind: KeyEventKind::Press,
                }),
            }
        );
    }

    #[test]
    fn pointer_snapshots_are_recomputed_and_cleared() {
        let (mut router, mut action_rx) = build_router();
        let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

        router.dispatch(
            &WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: winit::dpi::PhysicalPosition::new(48.0, 96.0),
            },
            viewport,
        );
        let _ = action_rx
            .try_recv()
            .expect("cursor move must emit one mouse event");
        assert_eq!(router.latest_pointer_physical, Some([48.0, 96.0]));
        assert_eq!(router.latest_pointer_logical, Some([24.0, 48.0]));
        assert_eq!(router.current_drag_button(), None);

        router.recompute_pointer_logical(4.0);
        assert_eq!(router.latest_pointer_logical, Some([12.0, 24.0]));

        router.dispatch(
            &WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            },
            viewport,
        );
        assert_eq!(router.latest_pointer_physical, None);
        assert_eq!(router.latest_pointer_logical, None);
        assert_eq!(router.current_drag_button(), None);
        assert_eq!(
            action_rx
                .try_recv()
                .expect("cursor left must emit typed cursor-left event"),
            Action::Input {
                event: InputEvent::CursorLeft,
            }
        );
    }

    #[test]
    fn focus_lost_clears_transient_router_state() {
        let (mut router, mut action_rx) = build_router();
        let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);
        router.current_modifiers = KeyModifiers::SUPER;
        router.latest_pointer_physical = Some([20.0, 40.0]);
        router.latest_pointer_logical = Some([10.0, 20.0]);
        router.pressed_mouse_buttons.push(MouseButtonKind::Left);

        assert_eq!(
            router.dispatch(&WindowEvent::Focused(false), viewport),
            RouteAction::None
        );
        assert_eq!(router.current_modifiers, KeyModifiers::NONE);
        assert_eq!(router.latest_pointer_physical, None);
        assert_eq!(router.latest_pointer_logical, None);
        assert!(router.pressed_mouse_buttons.is_empty());
        assert_eq!(
            action_rx
                .try_recv()
                .expect("focus lost must emit one action"),
            Action::Input {
                event: InputEvent::FocusChanged { focused: false },
            }
        );

        assert_eq!(
            router.dispatch(&WindowEvent::Focused(true), viewport),
            RouteAction::None
        );
        assert_eq!(
            action_rx
                .try_recv()
                .expect("focus gain must emit one action"),
            Action::Input {
                event: InputEvent::FocusChanged { focused: true },
            }
        );
    }

    #[test]
    fn pressed_mouse_button_turns_cursor_move_into_drag() {
        let (mut router, mut action_rx) = build_router();
        let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

        router.dispatch(
            &WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state: winit::event::ElementState::Pressed,
                button: winit::event::MouseButton::Left,
            },
            viewport,
        );
        let _ = action_rx
            .try_recv()
            .expect("mouse down must emit one event");

        router.dispatch(
            &WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: winit::dpi::PhysicalPosition::new(64.0, 32.0),
            },
            viewport,
        );

        let action = action_rx.try_recv().expect("drag move must emit one event");
        match action {
            Action::Input {
                event: InputEvent::Mouse(mouse_event),
            } => {
                assert_eq!(
                    mouse_event.kind,
                    MouseEventKind::Drag(MouseButtonKind::Left)
                );
                assert_eq!(mouse_event.logical_position, Some([32.0, 16.0]));
                assert_eq!(mouse_event.modifiers, KeyModifiers::NONE);
            }
            other => panic!("expected drag mouse event, got {other:?}"),
        }
    }

    #[test]
    fn cursor_left_does_not_drop_drag_state_without_release() {
        let (mut router, mut action_rx) = build_router();
        let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

        router.dispatch(
            &WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state: winit::event::ElementState::Pressed,
                button: winit::event::MouseButton::Left,
            },
            viewport,
        );
        let _ = action_rx
            .try_recv()
            .expect("mouse down must emit one event");

        router.dispatch(
            &WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            },
            viewport,
        );
        assert_eq!(
            action_rx
                .try_recv()
                .expect("cursor left must emit one event"),
            Action::Input {
                event: InputEvent::CursorLeft,
            }
        );
        assert_eq!(router.current_drag_button(), Some(MouseButtonKind::Left));

        router.dispatch(
            &WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: winit::dpi::PhysicalPosition::new(32.0, 48.0),
            },
            viewport,
        );

        let action = action_rx
            .try_recv()
            .expect("cursor move after leaving must still drag");
        match action {
            Action::Input {
                event: InputEvent::Mouse(mouse_event),
            } => {
                assert_eq!(
                    mouse_event.kind,
                    MouseEventKind::Drag(MouseButtonKind::Left)
                );
                assert_eq!(mouse_event.logical_position, Some([16.0, 24.0]));
                assert_eq!(mouse_event.modifiers, KeyModifiers::NONE);
            }
            other => panic!("expected drag mouse event, got {other:?}"),
        }
    }

    struct FakeClipboard {
        text: Result<String, ClipboardReadError>,
    }

    impl FakeClipboard {
        fn with_text(text: &str) -> Self {
            Self {
                text: Ok(text.to_owned()),
            }
        }

        fn with_error() -> Self {
            Self {
                text: Err(ClipboardReadError::ReadFailed("test failure".to_owned())),
            }
        }
    }

    impl ClipboardProvider for FakeClipboard {
        fn read_text(&mut self) -> Result<String, ClipboardReadError> {
            self.text.clone()
        }
    }
}
