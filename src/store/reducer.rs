//! Reducer that updates store-owned state from system actions and input commands.

use log::warn;

use crate::io::Action;

use super::input::Command;
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
        Command::InsertChar(_)
        | Command::DeleteBackward
        | Command::MoveCursorLeft
        | Command::MoveCursorRight => return None,
    };
    Some(next_y.clamp(0.0, max_scroll_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_commands_are_explicit_no_ops() {
        let reducer = Reducer;
        let commands = [
            Command::InsertChar('a'),
            Command::DeleteBackward,
            Command::MoveCursorLeft,
            Command::MoveCursorRight,
        ];

        for command in commands {
            let mut interaction = InteractionState {
                scroll_offset: [0.0, 120.0],
                last_known_viewport: [960.0, 640.0],
                last_known_content_extent: [960.0, 2_000.0],
            };
            let previous = interaction;

            let outcome =
                reducer.apply_command(&mut interaction, InteractionConfig::default(), command);

            assert!(matches!(outcome, ReduceOutcome::NoChange));
            assert_eq!(interaction, previous);
        }
    }
}
