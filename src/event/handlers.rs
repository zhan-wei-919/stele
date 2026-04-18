//! Event-specific handlers used by the winit router.

use std::time::Instant;

use log::warn;
use tokio::sync::mpsc::UnboundedSender;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent as WinitKeyEvent, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};

use crate::io::{
    Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButtonKind, MouseEvent,
    MouseEventKind, MouseScroll,
};

/// Keyboard input normalized at the winit boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyboardInput {
    text: Option<String>,
    code: KeyCode,
    kind: KeyEventKind,
    is_synthetic: bool,
}

impl KeyboardInput {
    /// Builds normalized keyboard input from one winit key event.
    pub(crate) fn from_winit(event: &WinitKeyEvent, is_synthetic: bool) -> Self {
        let kind = normalize_key_kind(event.state, event.repeat);
        Self {
            text: normalize_text(event.text.as_deref(), kind),
            code: map_key_code(event.physical_key, event.logical_key.as_ref()),
            kind,
            is_synthetic,
        }
    }

    /// Builds normalized keyboard input for tests and internal callers.
    #[cfg(test)]
    pub(crate) fn new(
        text: Option<&str>,
        code: KeyCode,
        kind: KeyEventKind,
        is_synthetic: bool,
    ) -> Self {
        Self {
            text: normalize_text(text, kind),
            code,
            kind,
            is_synthetic,
        }
    }

    pub(crate) fn code(&self) -> &KeyCode {
        &self.code
    }

    pub(crate) fn kind(&self) -> KeyEventKind {
        self.kind
    }

    pub(crate) fn is_synthetic(&self) -> bool {
        self.is_synthetic
    }

    fn into_action(self, modifiers: KeyModifiers) -> Option<Action> {
        if self.is_synthetic {
            return None;
        }

        let KeyboardInput {
            text,
            code,
            kind,
            is_synthetic: _,
        } = self;

        Some(Action::Input {
            event: InputEvent::Key(KeyEvent {
                code,
                text,
                modifiers,
                kind,
            }),
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
    pub(crate) event_time: Instant,
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
    pub(crate) fn handle(&self, input: KeyboardInput, modifiers: KeyModifiers) {
        if let Some(action) = input.into_action(modifiers) {
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
    pub(crate) fn handle_button(
        &self,
        kind: MouseEventKind,
        logical_position: Option<[f32; 2]>,
        modifiers: KeyModifiers,
        event_time: Instant,
    ) {
        self.send_mouse_event(kind, logical_position, None, modifiers, event_time);
    }

    /// Forwards cursor movement or drag activity.
    pub(crate) fn handle_move(
        &self,
        kind: MouseEventKind,
        logical_position: Option<[f32; 2]>,
        modifiers: KeyModifiers,
        event_time: Instant,
    ) {
        self.send_mouse_event(kind, logical_position, None, modifiers, event_time);
    }

    /// Forwards wheel activity.
    pub(crate) fn handle_scroll(
        &self,
        delta: &MouseScrollDelta,
        logical_position: Option<[f32; 2]>,
        modifiers: KeyModifiers,
        event_time: Instant,
    ) {
        for (kind, scroll_delta) in scroll_events_from_delta(delta) {
            self.send_mouse_event(
                kind,
                logical_position,
                Some(scroll_delta),
                modifiers,
                event_time,
            );
        }
    }

    fn send_mouse_event(
        &self,
        kind: MouseEventKind,
        logical_position: Option<[f32; 2]>,
        scroll_delta: Option<MouseScroll>,
        modifiers: KeyModifiers,
        event_time: Instant,
    ) {
        self.send_action(Action::Input {
            event: InputEvent::Mouse(MouseEvent {
                kind,
                logical_position,
                scroll_delta,
                modifiers,
                event_time,
            }),
        });
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
        let event_time = Instant::now();
        let update = ViewportUpdate {
            size,
            scale_factor,
            viewport_revision: self.next_viewport_revision,
            event_time,
        };
        let action = Action::Resize {
            width: size.width,
            height: size.height,
            scale_factor,
            viewport_revision: update.viewport_revision,
            event_time,
        };
        if self.action_tx.send(action).is_err() {
            warn!("event.handler.send_failed handler=viewport");
        }
        update
    }
}

pub(crate) fn key_modifiers_from_winit(state: ModifiersState) -> KeyModifiers {
    let mut modifiers = KeyModifiers::NONE;
    modifiers.set(KeyModifiers::SHIFT, state.shift_key());
    modifiers.set(KeyModifiers::CONTROL, state.control_key());
    modifiers.set(KeyModifiers::ALT, state.alt_key());
    modifiers.set(KeyModifiers::SUPER, state.super_key());
    modifiers
}

pub(crate) fn mouse_button_kind(button: MouseButton) -> MouseButtonKind {
    match button {
        MouseButton::Left => MouseButtonKind::Left,
        MouseButton::Right => MouseButtonKind::Right,
        MouseButton::Middle => MouseButtonKind::Middle,
        MouseButton::Back => MouseButtonKind::Back,
        MouseButton::Forward => MouseButtonKind::Forward,
        MouseButton::Other(value) => MouseButtonKind::Other(value),
    }
}

fn normalize_key_kind(state: ElementState, repeat: bool) -> KeyEventKind {
    match state {
        ElementState::Released => KeyEventKind::Release,
        ElementState::Pressed if repeat => KeyEventKind::Repeat,
        ElementState::Pressed => KeyEventKind::Press,
    }
}

fn scroll_events_from_delta(delta: &MouseScrollDelta) -> Vec<(MouseEventKind, MouseScroll)> {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => {
            let mut events = Vec::with_capacity(2);
            if *y > 0.0 {
                events.push((
                    MouseEventKind::ScrollUp,
                    MouseScroll::LineDelta { x: 0.0, y: *y },
                ));
            } else if *y < 0.0 {
                events.push((
                    MouseEventKind::ScrollDown,
                    MouseScroll::LineDelta { x: 0.0, y: *y },
                ));
            }

            if *x > 0.0 {
                events.push((
                    MouseEventKind::ScrollRight,
                    MouseScroll::LineDelta { x: *x, y: 0.0 },
                ));
            } else if *x < 0.0 {
                events.push((
                    MouseEventKind::ScrollLeft,
                    MouseScroll::LineDelta { x: *x, y: 0.0 },
                ));
            }

            events
        }
        MouseScrollDelta::PixelDelta(position) => {
            let mut events = Vec::with_capacity(2);
            if position.y > 0.0 {
                events.push((
                    MouseEventKind::ScrollUp,
                    MouseScroll::PixelDelta {
                        x: 0.0,
                        y: position.y,
                    },
                ));
            } else if position.y < 0.0 {
                events.push((
                    MouseEventKind::ScrollDown,
                    MouseScroll::PixelDelta {
                        x: 0.0,
                        y: position.y,
                    },
                ));
            }

            if position.x > 0.0 {
                events.push((
                    MouseEventKind::ScrollRight,
                    MouseScroll::PixelDelta {
                        x: position.x,
                        y: 0.0,
                    },
                ));
            } else if position.x < 0.0 {
                events.push((
                    MouseEventKind::ScrollLeft,
                    MouseScroll::PixelDelta {
                        x: position.x,
                        y: 0.0,
                    },
                ));
            }

            events
        }
    }
}

fn normalize_text(text: Option<&str>, kind: KeyEventKind) -> Option<String> {
    if kind == KeyEventKind::Release {
        return None;
    }

    let text = text?;
    if text.is_empty() || text.chars().all(char::is_control) {
        return None;
    }
    Some(text.to_owned())
}

fn map_key_code(physical_key: PhysicalKey, logical_key: Key<&str>) -> KeyCode {
    match physical_key {
        PhysicalKey::Code(code) => map_physical_key_code(code),
        PhysicalKey::Unidentified(_) => map_logical_key_code(logical_key),
    }
}

fn map_physical_key_code(code: WinitKeyCode) -> KeyCode {
    match code {
        WinitKeyCode::KeyA => key_code_char("a"),
        WinitKeyCode::KeyB => key_code_char("b"),
        WinitKeyCode::KeyC => key_code_char("c"),
        WinitKeyCode::KeyD => key_code_char("d"),
        WinitKeyCode::KeyE => key_code_char("e"),
        WinitKeyCode::KeyF => key_code_char("f"),
        WinitKeyCode::KeyG => key_code_char("g"),
        WinitKeyCode::KeyH => key_code_char("h"),
        WinitKeyCode::KeyI => key_code_char("i"),
        WinitKeyCode::KeyJ => key_code_char("j"),
        WinitKeyCode::KeyK => key_code_char("k"),
        WinitKeyCode::KeyL => key_code_char("l"),
        WinitKeyCode::KeyM => key_code_char("m"),
        WinitKeyCode::KeyN => key_code_char("n"),
        WinitKeyCode::KeyO => key_code_char("o"),
        WinitKeyCode::KeyP => key_code_char("p"),
        WinitKeyCode::KeyQ => key_code_char("q"),
        WinitKeyCode::KeyR => key_code_char("r"),
        WinitKeyCode::KeyS => key_code_char("s"),
        WinitKeyCode::KeyT => key_code_char("t"),
        WinitKeyCode::KeyU => key_code_char("u"),
        WinitKeyCode::KeyV => key_code_char("v"),
        WinitKeyCode::KeyW => key_code_char("w"),
        WinitKeyCode::KeyX => key_code_char("x"),
        WinitKeyCode::KeyY => key_code_char("y"),
        WinitKeyCode::KeyZ => key_code_char("z"),
        WinitKeyCode::Digit0 | WinitKeyCode::Numpad0 => key_code_char("0"),
        WinitKeyCode::Digit1 | WinitKeyCode::Numpad1 => key_code_char("1"),
        WinitKeyCode::Digit2 | WinitKeyCode::Numpad2 => key_code_char("2"),
        WinitKeyCode::Digit3 | WinitKeyCode::Numpad3 => key_code_char("3"),
        WinitKeyCode::Digit4 | WinitKeyCode::Numpad4 => key_code_char("4"),
        WinitKeyCode::Digit5 | WinitKeyCode::Numpad5 => key_code_char("5"),
        WinitKeyCode::Digit6 | WinitKeyCode::Numpad6 => key_code_char("6"),
        WinitKeyCode::Digit7 | WinitKeyCode::Numpad7 => key_code_char("7"),
        WinitKeyCode::Digit8 | WinitKeyCode::Numpad8 => key_code_char("8"),
        WinitKeyCode::Digit9 | WinitKeyCode::Numpad9 => key_code_char("9"),
        WinitKeyCode::Space => key_code_char(" "),
        WinitKeyCode::Minus | WinitKeyCode::NumpadSubtract => key_code_char("-"),
        WinitKeyCode::Equal | WinitKeyCode::NumpadEqual => key_code_char("="),
        WinitKeyCode::BracketLeft => key_code_char("["),
        WinitKeyCode::BracketRight => key_code_char("]"),
        WinitKeyCode::Backslash | WinitKeyCode::IntlBackslash => key_code_char("\\"),
        WinitKeyCode::Semicolon => key_code_char(";"),
        WinitKeyCode::Quote => key_code_char("'"),
        WinitKeyCode::Backquote => key_code_char("`"),
        WinitKeyCode::Comma | WinitKeyCode::NumpadComma => key_code_char(","),
        WinitKeyCode::Period | WinitKeyCode::NumpadDecimal => key_code_char("."),
        WinitKeyCode::Slash | WinitKeyCode::NumpadDivide => key_code_char("/"),
        WinitKeyCode::NumpadAdd => key_code_char("+"),
        WinitKeyCode::NumpadMultiply => key_code_char("*"),
        WinitKeyCode::Enter | WinitKeyCode::NumpadEnter => KeyCode::Enter,
        WinitKeyCode::Tab => KeyCode::Tab,
        WinitKeyCode::Escape => KeyCode::Escape,
        WinitKeyCode::Backspace => KeyCode::Backspace,
        WinitKeyCode::Delete => KeyCode::Delete,
        WinitKeyCode::Insert => KeyCode::Insert,
        WinitKeyCode::ShiftLeft | WinitKeyCode::ShiftRight => KeyCode::Shift,
        WinitKeyCode::ControlLeft | WinitKeyCode::ControlRight => KeyCode::Control,
        WinitKeyCode::AltLeft | WinitKeyCode::AltRight => KeyCode::Alt,
        WinitKeyCode::SuperLeft | WinitKeyCode::SuperRight => KeyCode::Super,
        WinitKeyCode::ArrowLeft => KeyCode::Left,
        WinitKeyCode::ArrowRight => KeyCode::Right,
        WinitKeyCode::ArrowUp => KeyCode::Up,
        WinitKeyCode::ArrowDown => KeyCode::Down,
        WinitKeyCode::Home => KeyCode::Home,
        WinitKeyCode::End => KeyCode::End,
        WinitKeyCode::PageUp => KeyCode::PageUp,
        WinitKeyCode::PageDown => KeyCode::PageDown,
        WinitKeyCode::F1 => KeyCode::F(1),
        WinitKeyCode::F2 => KeyCode::F(2),
        WinitKeyCode::F3 => KeyCode::F(3),
        WinitKeyCode::F4 => KeyCode::F(4),
        WinitKeyCode::F5 => KeyCode::F(5),
        WinitKeyCode::F6 => KeyCode::F(6),
        WinitKeyCode::F7 => KeyCode::F(7),
        WinitKeyCode::F8 => KeyCode::F(8),
        WinitKeyCode::F9 => KeyCode::F(9),
        WinitKeyCode::F10 => KeyCode::F(10),
        WinitKeyCode::F11 => KeyCode::F(11),
        WinitKeyCode::F12 => KeyCode::F(12),
        WinitKeyCode::F13 => KeyCode::F(13),
        WinitKeyCode::F14 => KeyCode::F(14),
        WinitKeyCode::F15 => KeyCode::F(15),
        WinitKeyCode::F16 => KeyCode::F(16),
        WinitKeyCode::F17 => KeyCode::F(17),
        WinitKeyCode::F18 => KeyCode::F(18),
        WinitKeyCode::F19 => KeyCode::F(19),
        WinitKeyCode::F20 => KeyCode::F(20),
        WinitKeyCode::F21 => KeyCode::F(21),
        WinitKeyCode::F22 => KeyCode::F(22),
        WinitKeyCode::F23 => KeyCode::F(23),
        WinitKeyCode::F24 => KeyCode::F(24),
        WinitKeyCode::F25 => KeyCode::F(25),
        WinitKeyCode::F26 => KeyCode::F(26),
        WinitKeyCode::F27 => KeyCode::F(27),
        WinitKeyCode::F28 => KeyCode::F(28),
        WinitKeyCode::F29 => KeyCode::F(29),
        WinitKeyCode::F30 => KeyCode::F(30),
        WinitKeyCode::F31 => KeyCode::F(31),
        WinitKeyCode::F32 => KeyCode::F(32),
        WinitKeyCode::F33 => KeyCode::F(33),
        WinitKeyCode::F34 => KeyCode::F(34),
        WinitKeyCode::F35 => KeyCode::F(35),
        _ => KeyCode::Unknown,
    }
}

fn map_logical_key_code(key: Key<&str>) -> KeyCode {
    match key {
        Key::Character(text) => key_code_char_from_logical(text),
        Key::Named(NamedKey::Space) => key_code_char(" "),
        Key::Named(named) => map_named_key(named),
        Key::Dead(Some(ch)) => {
            let normalized: String = ch.to_lowercase().collect();
            key_code_char_from_logical(&normalized)
        }
        Key::Dead(None) | Key::Unidentified(_) => KeyCode::Unknown,
    }
}

fn key_code_char(text: &str) -> KeyCode {
    KeyCode::Character(String::from(text))
}

fn key_code_char_from_logical(text: &str) -> KeyCode {
    if text.is_empty() {
        return KeyCode::Unknown;
    }

    KeyCode::Character(text.chars().flat_map(char::to_lowercase).collect())
}

fn map_named_key(named: NamedKey) -> KeyCode {
    match named {
        NamedKey::Enter => KeyCode::Enter,
        NamedKey::Tab => KeyCode::Tab,
        NamedKey::Escape => KeyCode::Escape,
        NamedKey::Backspace => KeyCode::Backspace,
        NamedKey::Delete => KeyCode::Delete,
        NamedKey::Insert => KeyCode::Insert,
        NamedKey::Shift => KeyCode::Shift,
        NamedKey::Control => KeyCode::Control,
        NamedKey::Alt => KeyCode::Alt,
        NamedKey::Super => KeyCode::Super,
        NamedKey::ArrowLeft => KeyCode::Left,
        NamedKey::ArrowRight => KeyCode::Right,
        NamedKey::ArrowUp => KeyCode::Up,
        NamedKey::ArrowDown => KeyCode::Down,
        NamedKey::Home => KeyCode::Home,
        NamedKey::End => KeyCode::End,
        NamedKey::PageUp => KeyCode::PageUp,
        NamedKey::PageDown => KeyCode::PageDown,
        NamedKey::F1 => KeyCode::F(1),
        NamedKey::F2 => KeyCode::F(2),
        NamedKey::F3 => KeyCode::F(3),
        NamedKey::F4 => KeyCode::F(4),
        NamedKey::F5 => KeyCode::F(5),
        NamedKey::F6 => KeyCode::F(6),
        NamedKey::F7 => KeyCode::F(7),
        NamedKey::F8 => KeyCode::F(8),
        NamedKey::F9 => KeyCode::F(9),
        NamedKey::F10 => KeyCode::F(10),
        NamedKey::F11 => KeyCode::F(11),
        NamedKey::F12 => KeyCode::F(12),
        NamedKey::F13 => KeyCode::F(13),
        NamedKey::F14 => KeyCode::F(14),
        NamedKey::F15 => KeyCode::F(15),
        NamedKey::F16 => KeyCode::F(16),
        NamedKey::F17 => KeyCode::F(17),
        NamedKey::F18 => KeyCode::F(18),
        NamedKey::F19 => KeyCode::F(19),
        NamedKey::F20 => KeyCode::F(20),
        NamedKey::F21 => KeyCode::F(21),
        NamedKey::F22 => KeyCode::F(22),
        NamedKey::F23 => KeyCode::F(23),
        NamedKey::F24 => KeyCode::F(24),
        NamedKey::F25 => KeyCode::F(25),
        NamedKey::F26 => KeyCode::F(26),
        NamedKey::F27 => KeyCode::F(27),
        NamedKey::F28 => KeyCode::F(28),
        NamedKey::F29 => KeyCode::F(29),
        NamedKey::F30 => KeyCode::F(30),
        NamedKey::F31 => KeyCode::F(31),
        NamedKey::F32 => KeyCode::F(32),
        NamedKey::F33 => KeyCode::F(33),
        NamedKey::F34 => KeyCode::F(34),
        NamedKey::F35 => KeyCode::F(35),
        _ => KeyCode::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use winit::dpi::PhysicalPosition;
    use winit::event::{MouseButton, MouseScrollDelta};
    use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};

    use super::{
        key_modifiers_from_winit, map_key_code, mouse_button_kind, scroll_events_from_delta,
        KeyboardInput,
    };
    use crate::io::{
        Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButtonKind,
        MouseEventKind, MouseScroll,
    };

    #[test]
    fn keyboard_input_keeps_text_on_the_key_event() {
        let input = KeyboardInput::new(
            Some("a"),
            KeyCode::Character(String::from("a")),
            KeyEventKind::Press,
            false,
        );

        assert_eq!(
            input.into_action(KeyModifiers::CONTROL),
            Some(Action::Input {
                event: InputEvent::Key(KeyEvent {
                    code: KeyCode::Character(String::from("a")),
                    text: Some(String::from("a")),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                }),
            })
        );
    }

    #[test]
    fn synthetic_keyboard_input_is_filtered_at_source() {
        let input = KeyboardInput::new(
            Some("a"),
            KeyCode::Character(String::from("a")),
            KeyEventKind::Press,
            true,
        );

        assert_eq!(input.into_action(KeyModifiers::NONE), None);
    }

    #[test]
    fn release_keyboard_input_clears_text_on_the_key_event() {
        let input = KeyboardInput::new(
            Some("a"),
            KeyCode::Character(String::from("a")),
            KeyEventKind::Release,
            false,
        );

        assert_eq!(
            input.into_action(KeyModifiers::NONE),
            Some(Action::Input {
                event: InputEvent::Key(KeyEvent {
                    code: KeyCode::Character(String::from("a")),
                    text: None,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Release,
                }),
            })
        );
    }

    #[test]
    fn control_keys_do_not_smuggle_text_through_key_events() {
        let input = KeyboardInput::new(Some("\r"), KeyCode::Enter, KeyEventKind::Press, false);

        assert_eq!(
            input.into_action(KeyModifiers::NONE),
            Some(Action::Input {
                event: InputEvent::Key(KeyEvent {
                    code: KeyCode::Enter,
                    text: None,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                }),
            })
        );
    }

    #[test]
    fn key_code_mapping_is_backend_agnostic() {
        assert_eq!(
            map_key_code(PhysicalKey::Code(WinitKeyCode::KeyA), Key::Character("q")),
            KeyCode::Character(String::from("a"))
        );
        assert_eq!(
            map_key_code(
                PhysicalKey::Code(WinitKeyCode::Enter),
                Key::Named(NamedKey::Enter),
            ),
            KeyCode::Enter
        );
        assert_eq!(
            map_key_code(
                PhysicalKey::Code(WinitKeyCode::ShiftLeft),
                Key::Named(NamedKey::Shift),
            ),
            KeyCode::Shift
        );
        assert_eq!(
            map_key_code(
                PhysicalKey::Code(WinitKeyCode::PageDown),
                Key::Named(NamedKey::PageDown),
            ),
            KeyCode::PageDown
        );
        assert_eq!(
            map_key_code(
                PhysicalKey::Code(WinitKeyCode::F12),
                Key::Named(NamedKey::F12),
            ),
            KeyCode::F(12)
        );
        assert_eq!(
            map_key_code(
                PhysicalKey::Code(WinitKeyCode::CapsLock),
                Key::Named(NamedKey::CapsLock),
            ),
            KeyCode::Unknown
        );
    }

    #[test]
    fn winit_modifiers_are_mapped_to_internal_snapshot_bits() {
        let state = ModifiersState::SHIFT | ModifiersState::ALT | ModifiersState::SUPER;
        let modifiers = key_modifiers_from_winit(state);

        assert!(modifiers.contains(KeyModifiers::SHIFT));
        assert!(!modifiers.contains(KeyModifiers::CONTROL));
        assert!(modifiers.contains(KeyModifiers::ALT));
        assert!(modifiers.contains(KeyModifiers::SUPER));
    }

    #[test]
    fn mouse_button_mapping_is_backend_agnostic() {
        assert_eq!(mouse_button_kind(MouseButton::Left), MouseButtonKind::Left);
        assert_eq!(
            mouse_button_kind(MouseButton::Other(7)),
            MouseButtonKind::Other(7)
        );
    }

    #[test]
    fn scroll_delta_expands_to_directional_events() {
        assert_eq!(
            scroll_events_from_delta(&MouseScrollDelta::LineDelta(-3.0, 2.0)),
            vec![
                (
                    MouseEventKind::ScrollUp,
                    MouseScroll::LineDelta { x: 0.0, y: 2.0 },
                ),
                (
                    MouseEventKind::ScrollLeft,
                    MouseScroll::LineDelta { x: -3.0, y: 0.0 },
                ),
            ]
        );
        assert_eq!(
            scroll_events_from_delta(&MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                0.0, -4.0
            ),)),
            vec![(
                MouseEventKind::ScrollDown,
                MouseScroll::PixelDelta { x: 0.0, y: -4.0 },
            )]
        );
        assert!(scroll_events_from_delta(&MouseScrollDelta::LineDelta(0.0, 0.0)).is_empty());
    }
}
