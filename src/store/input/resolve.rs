//! Translation from backend input facts into store commands.

use log::trace;

use crate::io::{
    InputEvent, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind, MouseScroll,
};

use super::{Command, InputContext};
use crate::store::types::InteractionConfig;

/// Resolves one input fact into the store command it requests.
pub(crate) fn resolve_command(
    context: InputContext,
    event: &InputEvent,
    config: InteractionConfig,
) -> Option<Command> {
    match context {
        InputContext::Viewport => resolve_viewport_command(event, config),
    }
}

fn resolve_viewport_command(event: &InputEvent, config: InteractionConfig) -> Option<Command> {
    match event {
        InputEvent::Key(key_event) => resolve_key_command(key_event),
        InputEvent::Mouse(mouse_event) => resolve_mouse_command(mouse_event, config),
        InputEvent::Paste(_) | InputEvent::CursorLeft | InputEvent::FocusChanged { .. } => None,
    }
}

fn resolve_key_command(event: &KeyEvent) -> Option<Command> {
    if !matches!(event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    match event.code {
        KeyCode::Up => Some(Command::ScrollByLine(-1)),
        KeyCode::Down => Some(Command::ScrollByLine(1)),
        KeyCode::PageUp => Some(Command::ScrollByPage(-1)),
        KeyCode::PageDown => Some(Command::ScrollByPage(1)),
        KeyCode::Home => Some(Command::ScrollToStart),
        KeyCode::End => Some(Command::ScrollToEnd),
        _ => {
            trace!(
                "store.input_unhandled code={:?} kind={:?}",
                event.code,
                event.kind
            );
            None
        }
    }
}

fn resolve_mouse_command(event: &MouseEvent, config: InteractionConfig) -> Option<Command> {
    match event.kind {
        MouseEventKind::ScrollUp
        | MouseEventKind::ScrollDown
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => {}
        _ => return None,
    }

    let pixels = match event.scroll_delta {
        Some(MouseScroll::LineDelta { y, .. }) => line_scroll_pixels(y, config)?,
        Some(MouseScroll::PixelDelta { y, .. }) => pixel_scroll_pixels(y, config)?,
        None => return None,
    };
    if pixels == 0.0 {
        return None;
    }
    Some(Command::ScrollByPixels(pixels))
}

fn line_scroll_pixels(y: f32, config: InteractionConfig) -> Option<f32> {
    if !y.is_finite() {
        debug_assert!(y.is_finite(), "mouse line scroll delta must stay finite");
        return None;
    }
    Some(-y * config.wheel_line_delta_px)
}

fn pixel_scroll_pixels(y: f64, config: InteractionConfig) -> Option<f32> {
    if !y.is_finite() {
        debug_assert!(y.is_finite(), "mouse pixel scroll delta must stay finite");
        return None;
    }
    Some(-(y as f32) * config.wheel_pixel_scale)
}

#[cfg(test)]
mod tests {
    use std::panic;
    use std::time::Instant;

    use crate::io::{KeyModifiers, MouseButtonKind};

    use super::*;

    #[test]
    fn key_press_and_repeat_resolve_to_scroll_commands() {
        assert_eq!(
            resolve_command(&key_event(KeyCode::Up, KeyEventKind::Press)),
            Some(Command::ScrollByLine(-1))
        );
        assert_eq!(
            resolve_command(&key_event(KeyCode::Down, KeyEventKind::Repeat)),
            Some(Command::ScrollByLine(1))
        );
        assert_eq!(
            resolve_command(&key_event(KeyCode::PageUp, KeyEventKind::Press)),
            Some(Command::ScrollByPage(-1))
        );
        assert_eq!(
            resolve_command(&key_event(KeyCode::PageDown, KeyEventKind::Repeat)),
            Some(Command::ScrollByPage(1))
        );
        assert_eq!(
            resolve_command(&key_event(KeyCode::Home, KeyEventKind::Press)),
            Some(Command::ScrollToStart)
        );
        assert_eq!(
            resolve_command(&key_event(KeyCode::End, KeyEventKind::Repeat)),
            Some(Command::ScrollToEnd)
        );
    }

    #[test]
    fn key_release_and_unhandled_keys_do_not_resolve() {
        assert_eq!(
            resolve_command(&key_event(KeyCode::Down, KeyEventKind::Release)),
            None
        );
        assert_eq!(
            resolve_command(&key_event(KeyCode::Char('a'), KeyEventKind::Press)),
            None
        );
    }

    #[test]
    fn non_scroll_input_facts_do_not_resolve() {
        assert_eq!(resolve_command(&InputEvent::CursorLeft), None);
        assert_eq!(
            resolve_command(&InputEvent::FocusChanged { focused: false }),
            None
        );
        assert_eq!(resolve_command(&InputEvent::Paste("text".to_owned())), None);
    }

    #[test]
    fn mouse_wheel_resolves_vertical_delta_to_pixel_command() {
        assert_eq!(
            resolve_command(&mouse_scroll(MouseScroll::LineDelta { x: 0.0, y: -3.0 })),
            Some(Command::ScrollByPixels(120.0))
        );
        assert_eq!(
            resolve_command(&mouse_scroll(MouseScroll::PixelDelta { x: 0.0, y: 24.0 })),
            Some(Command::ScrollByPixels(-24.0))
        );
    }

    #[test]
    fn mouse_wheel_ignores_non_scroll_and_zero_vertical_delta() {
        assert_eq!(
            resolve_command(&mouse_event(MouseEventKind::Moved, None)),
            None
        );
        assert_eq!(
            resolve_command(&mouse_scroll(MouseScroll::LineDelta { x: 1.0, y: 0.0 })),
            None
        );
    }

    #[test]
    fn non_finite_mouse_delta_is_rejected() {
        assert_rejects_non_finite(&mouse_scroll(MouseScroll::LineDelta {
            x: 0.0,
            y: f32::NAN,
        }));
        assert_rejects_non_finite(&mouse_scroll(MouseScroll::PixelDelta {
            x: 0.0,
            y: f64::INFINITY,
        }));
    }

    fn resolve_command(event: &InputEvent) -> Option<Command> {
        super::resolve_command(InputContext::Viewport, event, InteractionConfig::default())
    }

    fn key_event(code: KeyCode, kind: KeyEventKind) -> InputEvent {
        InputEvent::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind,
        })
    }

    fn mouse_scroll(scroll_delta: MouseScroll) -> InputEvent {
        mouse_event(MouseEventKind::ScrollDown, Some(scroll_delta))
    }

    fn mouse_event(kind: MouseEventKind, scroll_delta: Option<MouseScroll>) -> InputEvent {
        InputEvent::Mouse(MouseEvent {
            kind,
            logical_position: None,
            scroll_delta,
            modifiers: KeyModifiers::NONE,
            event_time: Instant::now(),
        })
    }

    fn assert_rejects_non_finite(event: &InputEvent) {
        #[cfg(debug_assertions)]
        assert!(panic::catch_unwind(|| resolve_command(event)).is_err());

        #[cfg(not(debug_assertions))]
        assert_eq!(resolve_command(event), None);
    }

    #[test]
    fn non_vertical_mouse_facts_do_not_resolve() {
        assert_eq!(
            resolve_command(&mouse_event(
                MouseEventKind::Down(MouseButtonKind::Left),
                Some(MouseScroll::LineDelta { x: 0.0, y: -1.0 })
            )),
            None
        );
    }
}
