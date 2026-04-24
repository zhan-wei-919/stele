//! Reducer that updates store-owned state from system actions and input commands.

use log::warn;

use crate::io::Action;

use super::input::Command;
use super::text_input::TextInputState;
use super::types::{InteractionConfig, InteractionState, ViewportState};

/// Result of applying one state transition to the store.
pub(crate) enum ReduceOutcome {
    NoChange,
    Changed,
    Shutdown,
}

/// Applies validated commands and system actions to store-owned state.
pub(crate) struct Reducer;

impl Reducer {
    /// Applies one non-input action to the current store state.
    pub(crate) fn apply_system_action(
        &self,
        viewport: &mut ViewportState,
        interaction: &mut InteractionState,
        action: &Action,
        mut resize_model: impl FnMut([f32; 2]),
    ) -> ReduceOutcome {
        match action {
            Action::Shutdown => ReduceOutcome::Shutdown,
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
                resize_model(viewport.logical_size());
                interaction.clamp_scroll_offset(
                    viewport.logical_size(),
                    interaction.last_known_content_extent,
                );
                ReduceOutcome::Changed
            }
            Action::Input { .. } => {
                debug_assert!(false, "input actions must be handled by the store");
                ReduceOutcome::NoChange
            }
        }
    }

    /// Applies one resolved input command to the current interaction state.
    pub(crate) fn apply_command(
        &self,
        interaction: &mut InteractionState,
        config: InteractionConfig,
        command: Command,
    ) -> ReduceOutcome {
        if let Command::FocusTextInput(text_input) = command {
            let previous = interaction.focused_text_input;
            interaction.focused_text_input = text_input;
            return if interaction.focused_text_input == previous {
                ReduceOutcome::NoChange
            } else {
                ReduceOutcome::Changed
            };
        }

        let previous = interaction.scroll_offset;
        let Some(next_y) = next_scroll_y(interaction, config, command) else {
            return ReduceOutcome::NoChange;
        };

        interaction.scroll_offset = [0.0, next_y];
        if interaction.scroll_offset == previous {
            ReduceOutcome::NoChange
        } else {
            ReduceOutcome::Changed
        }
    }

    /// Applies one resolved text edit command to a text input state.
    pub(crate) fn apply_text_command(
        &self,
        text_input: &mut TextInputState,
        command: Command,
    ) -> ReduceOutcome {
        let changed = match command {
            Command::InsertChar(ch) => text_input.insert_char(ch),
            Command::InsertText(text) => text_input.insert_text(&text),
            Command::DeleteBackward => text_input.delete_backward(),
            Command::MoveCursorLeft => text_input.move_cursor_left(),
            Command::MoveCursorRight => text_input.move_cursor_right(),
            Command::FocusTextInput(_)
            | Command::ScrollByLine(_)
            | Command::ScrollByPage(_)
            | Command::ScrollToStart
            | Command::ScrollToEnd
            | Command::ScrollByPixels(_) => false,
        };

        if changed {
            ReduceOutcome::Changed
        } else {
            ReduceOutcome::NoChange
        }
    }
}

fn next_scroll_y(
    interaction: &InteractionState,
    config: InteractionConfig,
    command: Command,
) -> Option<f32> {
    let max_scroll_y = InteractionState::max_scroll_y(
        interaction.last_known_viewport,
        interaction.last_known_content_extent,
    );
    let current_y = interaction.scroll_offset[1];
    let page_step = (interaction.last_known_viewport[1] - config.page_margin_px).max(1.0);

    let next_y = match command {
        Command::ScrollByLine(lines) => current_y + lines as f32 * config.line_step_px,
        Command::ScrollByPage(pages) => current_y + pages as f32 * page_step,
        Command::ScrollToStart => 0.0,
        Command::ScrollToEnd => max_scroll_y,
        Command::ScrollByPixels(pixels) => current_y + pixels,
        Command::FocusTextInput(_)
        | Command::InsertChar(_)
        | Command::InsertText(_)
        | Command::DeleteBackward
        | Command::MoveCursorLeft
        | Command::MoveCursorRight => return None,
    };
    Some(next_y.clamp(0.0, max_scroll_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::text_input::TextInputState;

    #[test]
    fn text_commands_update_text_state() {
        let reducer = Reducer;
        let mut text_input = TextInputState::default();

        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::InsertChar('a')),
            ReduceOutcome::Changed
        ));
        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::InsertChar('c')),
            ReduceOutcome::Changed
        ));
        assert_eq!(text_input.text(), "ac");
        assert_eq!(text_input.cursor_index(), 2);

        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::MoveCursorLeft),
            ReduceOutcome::Changed
        ));
        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::InsertChar('b')),
            ReduceOutcome::Changed
        ));

        assert_eq!(text_input.text(), "abc");
        assert_eq!(text_input.cursor_index(), 2);
    }

    #[test]
    fn focus_command_updates_only_focused_text_input() {
        let reducer = Reducer;
        let text_input = crate::layout::tree::TextInputId::new(7);
        let mut interaction = InteractionState {
            scroll_offset: [0.0, 80.0],
            ..InteractionState::default()
        };

        assert!(matches!(
            reducer.apply_command(
                &mut interaction,
                InteractionConfig::default(),
                Command::FocusTextInput(Some(text_input))
            ),
            ReduceOutcome::Changed
        ));
        assert_eq!(interaction.focused_text_input, Some(text_input));
        assert_eq!(interaction.scroll_offset, [0.0, 80.0]);

        assert!(matches!(
            reducer.apply_command(
                &mut interaction,
                InteractionConfig::default(),
                Command::FocusTextInput(Some(text_input))
            ),
            ReduceOutcome::NoChange
        ));

        assert!(matches!(
            reducer.apply_command(
                &mut interaction,
                InteractionConfig::default(),
                Command::FocusTextInput(None)
            ),
            ReduceOutcome::Changed
        ));
        assert_eq!(interaction.focused_text_input, None);
    }

    #[test]
    fn delete_backward_removes_character_before_cursor() {
        let reducer = Reducer;
        let mut text_input = TextInputState::default();

        for command in [
            Command::InsertChar('a'),
            Command::InsertChar('b'),
            Command::InsertChar('c'),
            Command::MoveCursorLeft,
            Command::DeleteBackward,
        ] {
            reducer.apply_text_command(&mut text_input, command);
        }

        assert_eq!(text_input.text(), "ac");
        assert_eq!(text_input.cursor_index(), 1);
    }

    #[test]
    fn text_cursor_boundaries_report_no_change() {
        let reducer = Reducer;
        let mut text_input = TextInputState::default();

        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::MoveCursorLeft),
            ReduceOutcome::NoChange
        ));
        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::DeleteBackward),
            ReduceOutcome::NoChange
        ));

        reducer.apply_text_command(&mut text_input, Command::InsertChar('a'));

        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::MoveCursorRight),
            ReduceOutcome::NoChange
        ));
        assert_eq!(text_input.text(), "a");
        assert_eq!(text_input.cursor_index(), 1);
    }

    #[test]
    fn unicode_editing_preserves_utf8_boundaries() {
        let reducer = Reducer;
        let mut text_input = TextInputState::default();

        for command in [
            Command::InsertChar('a'),
            Command::InsertChar('中'),
            Command::InsertChar('é'),
            Command::MoveCursorLeft,
            Command::DeleteBackward,
        ] {
            reducer.apply_text_command(&mut text_input, command);
            assert!(text_input
                .text()
                .is_char_boundary(text_input.cursor_index()));
        }

        assert_eq!(text_input.text(), "aé");
        assert_eq!(text_input.cursor_index(), 1);

        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::MoveCursorRight),
            ReduceOutcome::Changed
        ));
        assert_eq!(text_input.cursor_index(), "aé".len());
    }

    #[test]
    fn bulk_insert_text_updates_at_cursor_and_preserves_utf8_boundaries() {
        let reducer = Reducer;
        let mut text_input = TextInputState::default();

        for command in [
            Command::InsertChar('a'),
            Command::InsertChar('中'),
            Command::InsertChar('d'),
            Command::MoveCursorLeft,
        ] {
            reducer.apply_text_command(&mut text_input, command);
        }

        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::InsertText("βc".to_owned())),
            ReduceOutcome::Changed
        ));

        assert_eq!(text_input.text(), "a中βcd");
        assert_eq!(text_input.cursor_index(), "a中βc".len());
        assert!(text_input
            .text()
            .is_char_boundary(text_input.cursor_index()));
    }

    #[test]
    fn text_commands_reject_control_characters() {
        let reducer = Reducer;
        let mut text_input = TextInputState::default();

        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::InsertChar('\n')),
            ReduceOutcome::NoChange
        ));
        assert!(matches!(
            reducer.apply_text_command(&mut text_input, Command::InsertText("a\nb\rc".to_owned())),
            ReduceOutcome::Changed
        ));

        assert_eq!(text_input.text(), "abc");
        assert_eq!(text_input.cursor_index(), 3);
    }

    #[test]
    fn scroll_commands_do_not_modify_text_state() {
        let reducer = Reducer;
        let mut text_input = TextInputState::default();

        reducer.apply_text_command(&mut text_input, Command::InsertChar('a'));
        let previous = text_input.clone();

        for command in [
            Command::ScrollByLine(1),
            Command::ScrollByPage(-1),
            Command::ScrollToStart,
            Command::ScrollToEnd,
            Command::ScrollByPixels(12.0),
        ] {
            let outcome = reducer.apply_text_command(&mut text_input, command);

            assert!(matches!(outcome, ReduceOutcome::NoChange));
            assert_eq!(text_input, previous);
        }
    }

    #[test]
    fn viewport_reducer_still_ignores_text_commands() {
        let reducer = Reducer;
        let previous = InteractionState {
            scroll_offset: [0.0, 120.0],
            last_known_viewport: [960.0, 640.0],
            last_known_content_extent: [960.0, 2_000.0],
            ..InteractionState::default()
        };

        for command in [
            Command::InsertChar('a'),
            Command::InsertText("abc".to_owned()),
            Command::DeleteBackward,
            Command::MoveCursorLeft,
            Command::MoveCursorRight,
        ] {
            let mut interaction = previous;
            let outcome =
                reducer.apply_command(&mut interaction, InteractionConfig::default(), command);

            assert!(matches!(outcome, ReduceOutcome::NoChange));
            assert_eq!(interaction, previous);
        }
    }
}
