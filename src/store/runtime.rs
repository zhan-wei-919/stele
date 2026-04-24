//! Async store runtime loop that reduces actions, composes full scene buffers, and emits updates.

use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{info, warn};
use tokio::sync::mpsc::error::TryRecvError;

use crate::font::FreeTypeRasterizer;
use crate::io::{
    Action, InputEvent, IoHandle, MouseButtonKind, MouseEventKind, SceneFrame, ViewUpdate,
};
use crate::scene::{SceneBuffer, SceneBufferPool, SceneConfig};

use super::composer::Composer;
use super::delegate::StoreDelegate;
use super::input::{resolve_command, resolve_input_context, Command, InputContext};
use super::invalidation::Invalidation;
use super::logical_atlas::LogicalAtlas;
use super::model::{LayoutCache, Model};
use super::reducer::{ReduceOutcome, Reducer};
use super::types::{
    InputFilter, InteractionConfig, InteractionState, StorePhase, TextInputHitTarget, ViewportState,
};

const INPUT_COALESCE_DRAIN_COUNT: usize = 256;

/// Async-side store that owns the logical app state and full-scene composition pipeline.
pub(crate) struct Store {
    viewport: ViewportState,
    interaction: InteractionState,
    text_input_hit_targets: Vec<TextInputHitTarget>,
    text_input_hit_targets_revision: u64,
    text_input_hit_targets_scroll_offset: [f32; 2],
    text_input_hit_targets_viewport: [f32; 2],
    config: InteractionConfig,
    model: Model,
    layout_cache: LayoutCache,
    composer: Composer,
    logical_atlas: LogicalAtlas,
    rasterizer: FreeTypeRasterizer,
    delegate: Arc<dyn StoreDelegate>,
    reducer: Reducer,
    phase: StorePhase,
    #[cfg(test)]
    compose_test_delay: Duration,
    #[cfg(test)]
    reprepare_count: usize,
}

impl Store {
    /// Builds the store from application-supplied model state and viewport hooks.
    pub(crate) fn new(
        rasterizer: FreeTypeRasterizer,
        viewport: ViewportState,
        delegate: Arc<dyn StoreDelegate>,
    ) -> Self {
        let config = delegate.interaction_config();
        let config = if config.is_valid() {
            config
        } else {
            debug_assert!(
                config.is_valid(),
                "interaction config must stay finite and positive"
            );
            warn!("store.invalid_interaction_config using_default=true");
            InteractionConfig::default()
        };
        let (model, layout_cache) = delegate
            .bootstrap(&rasterizer, viewport.logical_size())
            .into_parts();
        Self {
            viewport,
            interaction: InteractionState::default(),
            text_input_hit_targets: Vec::new(),
            text_input_hit_targets_revision: 0,
            text_input_hit_targets_scroll_offset: [0.0, 0.0],
            text_input_hit_targets_viewport: [0.0, 0.0],
            config,
            model,
            layout_cache,
            composer: Composer,
            logical_atlas: LogicalAtlas::new(viewport.scale_factor),
            rasterizer,
            delegate,
            reducer: Reducer,
            phase: StorePhase::Idle,
            #[cfg(test)]
            compose_test_delay: Duration::ZERO,
            #[cfg(test)]
            reprepare_count: 0,
        }
    }

    fn handle_action(&mut self, action: Action) -> ActionOutcome {
        self.phase = StorePhase::Reducing;
        match action {
            Action::Input { event } => self.handle_input_event(&event),
            action => self.handle_system_action(&action),
        }
    }

    fn handle_input_event(&mut self, event: &InputEvent) -> ActionOutcome {
        if matches!(
            self.delegate.filter_input(&self.interaction, event),
            InputFilter::VetoDefault
        ) {
            return self.reduce_to_action_outcome(ReduceOutcome::NoChange, Invalidation::NONE);
        }

        let context = resolve_input_context(&self.model, &self.interaction);
        if self.is_pointer_focus_event(event) {
            self.refresh_text_input_hit_targets_if_stale();
        }
        if let Some(command) = self.resolve_pointer_focus_command(event) {
            let outcome = self
                .reducer
                .apply_command(&mut self.interaction, self.config, command);
            return self.reduce_to_action_outcome(outcome, Invalidation::RECOMPOSE);
        }

        let Some(command) = resolve_command(context, event, self.config) else {
            return self.reduce_to_action_outcome(ReduceOutcome::NoChange, Invalidation::NONE);
        };

        let (outcome, invalidation) = match context {
            InputContext::Viewport => {
                let outcome =
                    self.reducer
                        .apply_command(&mut self.interaction, self.config, command);
                (outcome, Invalidation::RECOMPOSE)
            }
            InputContext::TextInput(text_input) => {
                let outcome = {
                    let Some(state) = self.model.text_inputs_mut().get_mut(text_input) else {
                        return self
                            .reduce_to_action_outcome(ReduceOutcome::NoChange, Invalidation::NONE);
                    };
                    self.reducer.apply_text_command(state, command)
                };
                (outcome, Invalidation::REPREPARE_AND_COMPOSE)
            }
        };
        self.reduce_to_action_outcome(outcome, invalidation)
    }

    fn is_pointer_focus_event(&self, event: &InputEvent) -> bool {
        let InputEvent::Mouse(mouse_event) = event else {
            return false;
        };
        matches!(
            mouse_event.kind,
            MouseEventKind::Down(MouseButtonKind::Left)
        ) && mouse_event.logical_position.is_some()
    }

    fn refresh_text_input_hit_targets_if_stale(&mut self) {
        if !self.text_input_hit_targets_are_stale() {
            return;
        }
        self.text_input_hit_targets = self.composer.text_input_hit_targets(
            &self.layout_cache,
            self.viewport,
            self.interaction.scroll_offset,
        );
        self.record_text_input_hit_target_state();
    }

    fn text_input_hit_targets_are_stale(&self) -> bool {
        self.text_input_hit_targets_revision != self.model.text_inputs().revision()
            || self.text_input_hit_targets_scroll_offset != self.interaction.scroll_offset
            || self.text_input_hit_targets_viewport != self.viewport.logical_size()
    }

    fn record_text_input_hit_target_state(&mut self) {
        self.text_input_hit_targets_revision = self.model.text_inputs().revision();
        self.text_input_hit_targets_scroll_offset = self.interaction.scroll_offset;
        self.text_input_hit_targets_viewport = self.viewport.logical_size();
    }

    fn resolve_pointer_focus_command(&self, event: &InputEvent) -> Option<Command> {
        let InputEvent::Mouse(mouse_event) = event else {
            return None;
        };
        if !matches!(
            mouse_event.kind,
            MouseEventKind::Down(MouseButtonKind::Left)
        ) {
            return None;
        }
        let position = mouse_event.logical_position?;
        let focused = hit_test_text_inputs(&self.text_input_hit_targets, position)
            .filter(|text_input| self.model.text_inputs().contains(*text_input));
        Some(Command::FocusTextInput(focused))
    }

    fn handle_system_action(&mut self, action: &Action) -> ActionOutcome {
        let scale_changed = matches!(
            action,
            Action::Resize { scale_factor, .. }
                if self.viewport.scale_factor.to_bits() != scale_factor.to_bits()
        );

        let delegate = self.delegate.as_ref();
        let model = &mut self.model;
        let outcome = self.reducer.apply_system_action(
            &mut self.viewport,
            &mut self.interaction,
            action,
            |logical_viewport| delegate.resize(model, logical_viewport),
        );
        let invalidation = if scale_changed {
            Invalidation::RESET_ATLAS_AND_COMPOSE
        } else {
            Invalidation::RECOMPOSE
        };
        self.reduce_to_action_outcome(outcome, invalidation)
    }

    fn reduce_to_action_outcome(
        &mut self,
        outcome: ReduceOutcome,
        invalidation: Invalidation,
    ) -> ActionOutcome {
        match outcome {
            ReduceOutcome::Shutdown => ActionOutcome::Shutdown,
            ReduceOutcome::NoChange => {
                self.phase = StorePhase::Idle;
                ActionOutcome::NoChange
            }
            ReduceOutcome::Changed => {
                debug_assert!(
                    invalidation.needs_compose(),
                    "changed state must request a compose invalidation"
                );
                ActionOutcome::Compose { invalidation }
            }
        }
    }

    fn apply_invalidation(&mut self, invalidation: Invalidation) {
        if invalidation.needs_reprepare() {
            #[cfg(test)]
            {
                self.reprepare_count += 1;
            }
            self.layout_cache
                .rebuild_from_model(&self.model, &self.rasterizer);
        }
        if invalidation.resets_atlas() {
            self.logical_atlas
                .reset_for_scale(self.viewport.scale_factor);
        }
    }

    fn compose_scene_buffer(
        &mut self,
        owner: bumpalo::Bump,
        reset_tessellation_cache: bool,
        scene_config: &SceneConfig,
    ) -> SceneComposeResult {
        self.phase = StorePhase::Laying;
        self.phase = StorePhase::ComposingSnapshot;
        let compose_started = Instant::now();
        let mut content_extent = [0.0, 0.0];
        let mut text_input_hit_targets = Vec::new();
        let scene_buffer = SceneBuffer::new(owner, |owner| {
            let outcome = self.composer.compose_into_buffer(
                owner,
                &self.model,
                &self.layout_cache,
                &mut self.logical_atlas,
                &self.rasterizer,
                self.viewport,
                self.interaction.scroll_offset,
                self.interaction.focused_text_input,
                reset_tessellation_cache,
                scene_config.max_blocks_per_scene,
            );
            content_extent = outcome.content_extent;
            text_input_hit_targets = outcome.text_input_hit_targets;
            outcome.scene
        });
        self.text_input_hit_targets = text_input_hit_targets;
        self.record_text_input_hit_target_state();
        #[cfg(test)]
        if !self.compose_test_delay.is_zero() {
            std::thread::sleep(self.compose_test_delay);
        }
        let elapsed = compose_started.elapsed();
        if elapsed > duration_budget(scene_config.compose_budget_ms) {
            warn!(
                "scene.budget_exceeded phase=compose elapsed_us={} limit_ms={}",
                elapsed.as_micros(),
                scene_config.compose_budget_ms
            );
        }
        SceneComposeResult {
            scene_buffer,
            content_extent,
        }
    }
}

/// Runs the async store until shutdown or channel disconnect.
pub(crate) async fn run_store(mut store: Store, mut handle: IoHandle, mut pool: SceneBufferPool) {
    if !compose_and_emit_with_post_clamp(&mut store, &mut pool, Invalidation::RECOMPOSE).await {
        return;
    }

    loop {
        let Some(first_action) = handle.next_action().await else {
            break;
        };
        let batch = drain_action_batch(first_action, &mut handle);
        let input_snapshot = batch
            .is_input_batch
            .then(|| InputBatchSnapshot::capture(&store));
        let mut invalidation = Invalidation::NONE;
        let mut pending_side_effects = Invalidation::NONE;
        let mut shutdown_seen = false;

        for action in batch.actions {
            store.flush_invalidation_for_action(&mut pending_side_effects, &action);
            match store.handle_action(action) {
                ActionOutcome::Shutdown => {
                    shutdown_seen = true;
                    break;
                }
                ActionOutcome::NoChange => {}
                ActionOutcome::Compose {
                    invalidation: action_invalidation,
                } => {
                    invalidation = invalidation.merge(action_invalidation);
                    pending_side_effects = pending_side_effects.merge(action_invalidation);
                }
            }
        }

        if shutdown_seen {
            break;
        }

        if input_snapshot.is_some_and(|snapshot| !snapshot.changed(&store)) {
            invalidation = Invalidation::NONE;
            pending_side_effects = Invalidation::NONE;
        }

        if invalidation.needs_compose() {
            store.apply_invalidation(pending_side_effects);
            if !compose_and_emit_with_post_clamp(&mut store, &mut pool, invalidation).await {
                break;
            }
        } else {
            store.phase = StorePhase::Idle;
        }
    }
}

impl Store {
    fn flush_invalidation_for_action(&mut self, pending: &mut Invalidation, action: &Action) {
        if !pending.needs_reprepare() || !self.action_needs_fresh_hit_targets(action) {
            return;
        }
        self.apply_invalidation(*pending);
        *pending = Invalidation::NONE;
    }

    fn action_needs_fresh_hit_targets(&self, action: &Action) -> bool {
        let Action::Input { event } = action else {
            return false;
        };
        self.is_pointer_focus_event(event)
    }
}

async fn compose_and_emit_with_post_clamp(
    store: &mut Store,
    pool: &mut SceneBufferPool,
    invalidation: Invalidation,
) -> bool {
    let Some(content_extent) =
        compose_and_emit_once(store, pool, invalidation.resets_atlas()).await
    else {
        return false;
    };
    update_post_compose_state(store, content_extent);

    let pre_clamp = store.interaction.scroll_offset;
    // Content height can change after a resize or reflow, so clamp only after one fresh compose
    // has measured the new extent. If the clamp moves the viewport, emit one corrected frame.
    if !store.interaction.clamp_scroll_offset(
        store.interaction.last_known_viewport,
        store.interaction.last_known_content_extent,
    ) {
        store.phase = StorePhase::Idle;
        return true;
    }

    info!(
        "store.scroll_clamped from=({:.1},{:.1}) to=({:.1},{:.1}) reason=post_compose",
        pre_clamp[0],
        pre_clamp[1],
        store.interaction.scroll_offset[0],
        store.interaction.scroll_offset[1]
    );

    let Some(recomposed_extent) = compose_and_emit_once(store, pool, false).await else {
        return false;
    };
    update_post_compose_state(store, recomposed_extent);

    let second_pre_clamp = store.interaction.scroll_offset;
    if store.interaction.clamp_scroll_offset(
        store.interaction.last_known_viewport,
        store.interaction.last_known_content_extent,
    ) {
        warn!(
            "store.scroll_oscillation from=({:.1},{:.1}) to=({:.1},{:.1})",
            second_pre_clamp[0],
            second_pre_clamp[1],
            store.interaction.scroll_offset[0],
            store.interaction.scroll_offset[1]
        );
    }

    store.phase = StorePhase::Idle;
    true
}

async fn compose_and_emit_once(
    store: &mut Store,
    pool: &mut SceneBufferPool,
    reset_tessellation_cache: bool,
) -> Option<[f32; 2]> {
    let owner = match pool.acquire_empty_bump().await {
        Ok(owner) => owner,
        Err(_) => return None,
    };
    let compose_result = store.compose_scene_buffer(owner, reset_tessellation_cache, pool.config());
    let content_extent = compose_result.content_extent;
    let scene_buffer = compose_result.scene_buffer;

    if let Some(atlas_update) = store.logical_atlas.take_pending_update() {
        info!(
            "store.atlas generation={} requested_atlas_size={:?} patches={}",
            atlas_update.generation,
            atlas_update.requested_atlas_size,
            atlas_update.patches.len()
        );
        if pool
            .dispatch_view_update(ViewUpdate::Atlas(atlas_update))
            .await
            .is_err()
        {
            return None;
        }
    }

    let metadata = scene_buffer.metadata();
    if store.interaction.scroll_offset != [0.0, 0.0] {
        info!(
            "store.scroll offset=({:.1},{:.1}) extent=({:.1},{:.1}) viewport_revision={}",
            store.interaction.scroll_offset[0],
            store.interaction.scroll_offset[1],
            content_extent[0],
            content_extent[1],
            metadata.viewport_revision
        );
    }
    info!(
        "store.scene viewport_revision={} blocks={} required_atlas_generation={:?}",
        metadata.viewport_revision,
        scene_buffer.blocks().len(),
        metadata.required_atlas_generation
    );
    if pool
        .dispatch_view_update(ViewUpdate::Scene(SceneFrame::new(Box::new(scene_buffer))))
        .await
        .is_err()
    {
        return None;
    }

    Some(content_extent)
}

enum ActionOutcome {
    Shutdown,
    NoChange,
    Compose { invalidation: Invalidation },
}

struct SceneComposeResult {
    scene_buffer: SceneBuffer,
    content_extent: [f32; 2],
}

struct ActionBatch {
    actions: Vec<Action>,
    is_input_batch: bool,
}

struct InputBatchSnapshot {
    scroll_offset: [f32; 2],
    focused_text_input: Option<crate::layout::tree::TextInputId>,
    text_input_revision: u64,
}

impl InputBatchSnapshot {
    fn capture(store: &Store) -> Self {
        Self {
            scroll_offset: store.interaction.scroll_offset,
            focused_text_input: store.interaction.focused_text_input,
            text_input_revision: store.model.text_inputs().revision(),
        }
    }

    fn changed(self, store: &Store) -> bool {
        self.scroll_offset != store.interaction.scroll_offset
            || self.focused_text_input != store.interaction.focused_text_input
            || self.text_input_revision != store.model.text_inputs().revision()
    }
}

fn duration_budget(limit_ms: u32) -> Duration {
    Duration::from_millis(u64::from(limit_ms))
}

fn update_post_compose_state(store: &mut Store, content_extent: [f32; 2]) {
    store.interaction.last_known_viewport = store.viewport.logical_size();
    store.interaction.last_known_content_extent = content_extent;
}

fn hit_test_text_inputs(
    targets: &[TextInputHitTarget],
    position: [f32; 2],
) -> Option<crate::layout::tree::TextInputId> {
    targets
        .iter()
        .copied()
        .filter(|target| target.contains(position))
        .max_by_key(|target| target.paint_order())
        .map(|target| target.text_input_id)
}

fn drain_action_batch(first_action: Action, handle: &mut IoHandle) -> ActionBatch {
    match first_action {
        Action::Resize { .. } => ActionBatch {
            actions: vec![coalesce_resize_action(first_action, handle)],
            is_input_batch: false,
        },
        Action::Shutdown => ActionBatch {
            actions: vec![Action::Shutdown],
            is_input_batch: false,
        },
        input @ Action::Input { .. } => {
            let mut actions = Vec::with_capacity(4);
            actions.push(input);

            // Input bursts often contain many small deltas, so drain contiguous input first
            // and compose once for the highest-cost invalidation in the batch.
            while actions.len() < INPUT_COALESCE_DRAIN_COUNT {
                match handle.try_next_action() {
                    Ok(next @ Action::Input { .. }) => actions.push(next),
                    Ok(other_action) => {
                        handle.push_front_action(other_action);
                        break;
                    }
                    Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
                }
            }

            ActionBatch {
                actions,
                is_input_batch: true,
            }
        }
    }
}

fn coalesce_resize_action(initial_action: Action, handle: &mut IoHandle) -> Action {
    if !matches!(initial_action, Action::Resize { .. }) {
        return initial_action;
    }

    let mut latest_resize = initial_action;
    loop {
        match handle.try_next_action() {
            Ok(next_resize @ Action::Resize { .. }) => latest_resize = next_resize,
            Ok(other_action) => {
                handle.push_front_action(other_action);
                return latest_resize;
            }
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return latest_resize,
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
