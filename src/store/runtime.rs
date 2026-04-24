//! Async store runtime loop that reduces actions, composes full scene buffers, and emits updates.

use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{info, warn};
use tokio::sync::mpsc::error::TryRecvError;

use crate::font::FreeTypeRasterizer;
use crate::io::{Action, InputEvent, IoHandle, SceneFrame, ViewUpdate};
use crate::scene::{SceneBuffer, SceneBufferPool, SceneConfig};

use super::composer::Composer;
use super::delegate::StoreDelegate;
use super::input::{resolve_command, resolve_input_context, InputContext};
use super::logical_atlas::LogicalAtlas;
use super::model::{LayoutCache, Model};
use super::reducer::{ReduceOutcome, Reducer};
use super::text_input::TextInputState;
use super::types::{InputFilter, InteractionConfig, InteractionState, StorePhase, ViewportState};

const INPUT_COALESCE_DRAIN_COUNT: usize = 256;

/// Async-side store that owns the logical app state and full-scene composition pipeline.
pub(crate) struct Store {
    viewport: ViewportState,
    interaction: InteractionState,
    text_input: TextInputState,
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
            text_input: TextInputState::default(),
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
            return self.reduce_to_action_outcome(ReduceOutcome::NoChange, false);
        }

        let context = resolve_input_context(&self.model, &self.interaction);
        let Some(command) = resolve_command(context, event, self.config) else {
            return self.reduce_to_action_outcome(ReduceOutcome::NoChange, false);
        };

        let outcome = match context {
            InputContext::Viewport => {
                self.reducer
                    .apply_command(&mut self.interaction, self.config, command)
            }
            InputContext::TextInput(_) => self
                .reducer
                .apply_text_command(&mut self.text_input, command),
        };
        self.reduce_to_action_outcome(outcome, false)
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
        self.reduce_to_action_outcome(outcome, scale_changed)
    }

    fn reduce_to_action_outcome(
        &mut self,
        outcome: ReduceOutcome,
        clear_tessellation_cache: bool,
    ) -> ActionOutcome {
        match outcome {
            ReduceOutcome::Shutdown => ActionOutcome::Shutdown,
            ReduceOutcome::NoChange => {
                self.phase = StorePhase::Idle;
                ActionOutcome::NoChange
            }
            ReduceOutcome::Changed => {
                if clear_tessellation_cache {
                    self.logical_atlas
                        .reset_for_scale(self.viewport.scale_factor);
                }
                ActionOutcome::Compose {
                    clear_tessellation_cache,
                }
            }
        }
    }

    fn compose_scene_buffer(
        &mut self,
        owner: bumpalo::Bump,
        clear_tessellation_cache: bool,
        scene_config: &SceneConfig,
    ) -> SceneComposeResult {
        self.phase = StorePhase::Laying;
        self.phase = StorePhase::ComposingSnapshot;
        let compose_started = Instant::now();
        let mut content_extent = [0.0, 0.0];
        let scene_buffer = SceneBuffer::new(owner, |owner| {
            let outcome = self.composer.compose_into_buffer(
                owner,
                &self.model,
                &self.layout_cache,
                &mut self.logical_atlas,
                &self.rasterizer,
                self.viewport,
                self.interaction.scroll_offset,
                clear_tessellation_cache,
                scene_config.max_blocks_per_scene,
            );
            content_extent = outcome.content_extent;
            outcome.scene
        });
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
    if !compose_and_emit_with_post_clamp(&mut store, &mut pool, false).await {
        return;
    }

    loop {
        let Some(first_action) = handle.next_action().await else {
            break;
        };
        let batch = drain_action_batch(first_action, &mut handle);
        let pre_offset = store.interaction.scroll_offset;
        let pre_text_input_revision = store.text_input.revision();
        let mut saw_compose_request = false;
        let mut clear_tessellation_cache = false;
        let mut shutdown_seen = false;

        for action in batch.actions {
            match store.handle_action(action) {
                ActionOutcome::Shutdown => {
                    shutdown_seen = true;
                    break;
                }
                ActionOutcome::NoChange => {}
                ActionOutcome::Compose {
                    clear_tessellation_cache: clear_tessellation_cache_this_action,
                } => {
                    saw_compose_request = true;
                    clear_tessellation_cache |= clear_tessellation_cache_this_action;
                }
            }
        }

        if shutdown_seen {
            break;
        }

        let needs_compose = if batch.is_input_batch {
            store.interaction.scroll_offset != pre_offset
                || store.text_input.revision() != pre_text_input_revision
        } else {
            saw_compose_request
        };

        if needs_compose {
            if !compose_and_emit_with_post_clamp(&mut store, &mut pool, clear_tessellation_cache)
                .await
            {
                break;
            }
        } else {
            store.phase = StorePhase::Idle;
        }
    }
}

async fn compose_and_emit_with_post_clamp(
    store: &mut Store,
    pool: &mut SceneBufferPool,
    clear_tessellation_cache: bool,
) -> bool {
    let Some(content_extent) = compose_and_emit_once(store, pool, clear_tessellation_cache).await
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
    clear_tessellation_cache: bool,
) -> Option<[f32; 2]> {
    let owner = match pool.acquire_empty_bump().await {
        Ok(owner) => owner,
        Err(_) => return None,
    };
    let compose_result = store.compose_scene_buffer(owner, clear_tessellation_cache, pool.config());
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
    Compose { clear_tessellation_cache: bool },
}

struct SceneComposeResult {
    scene_buffer: SceneBuffer,
    content_extent: [f32; 2],
}

struct ActionBatch {
    actions: Vec<Action>,
    is_input_batch: bool,
}

fn duration_budget(limit_ms: u32) -> Duration {
    Duration::from_millis(u64::from(limit_ms))
}

fn update_post_compose_state(store: &mut Store, content_extent: [f32; 2]) {
    store.interaction.last_known_viewport = store.viewport.logical_size();
    store.interaction.last_known_content_extent = content_extent;
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

            // Input bursts can collapse to a no-op scroll delta, so drain contiguous input first
            // and decide whether a compose is needed from the batch's net effect.
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
