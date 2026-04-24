//! Reducer that updates store-owned model state from incoming actions.

use log::warn;

use crate::io::Action;

use super::delegate::StoreDelegate;
use super::input::{resolve_command, Command};
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
            Action::Input { event } => {
                if matches!(
                    delegate.filter_input(interaction, event),
                    InputFilter::VetoDefault
                ) {
                    return ReduceOutcome::NoChange;
                }

                let Some(command) = resolve_command(event, config) else {
                    return ReduceOutcome::NoChange;
                };

                self.apply_command(interaction, config, command)
            }
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

    fn apply_command(
        &self,
        interaction: &mut InteractionState,
        config: InteractionConfig,
        command: Command,
    ) -> ReduceOutcome {
        let previous = interaction.scroll_offset;
        let next_y = next_scroll_y(interaction, config, command);

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
) -> f32 {
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
    };
    next_y.clamp(0.0, max_scroll_y)
}
