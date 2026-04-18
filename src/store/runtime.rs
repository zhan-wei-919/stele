//! Async store runtime loop that reduces actions, composes full scene buffers, and emits updates.

use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{info, warn};
use tokio::sync::mpsc::error::TryRecvError;

use crate::font::FreeTypeRasterizer;
use crate::io::{Action, IoHandle, SceneFrame, ViewUpdate};
use crate::scene::{SceneBuffer, SceneBufferPool, SceneConfig};

use super::composer::Composer;
use super::delegate::StoreDelegate;
use super::logical_atlas::LogicalAtlas;
use super::model::{LayoutCache, Model};
use super::reducer::{ReduceOutcome, Reducer};
use super::types::{StorePhase, ViewportState};

/// Async-side store that owns the logical app state and full-scene composition pipeline.
pub(crate) struct Store {
    viewport: ViewportState,
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
        let (model, layout_cache) = delegate
            .bootstrap(&rasterizer, viewport.logical_size())
            .into_parts();
        Self {
            viewport,
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
        let scale_changed = matches!(
            &action,
            Action::Resize { scale_factor, .. }
                if self.viewport.scale_factor.to_bits() != scale_factor.to_bits()
        );

        match self.reducer.apply(
            &mut self.model,
            &mut self.viewport,
            &action,
            self.delegate.as_ref(),
        ) {
            ReduceOutcome::Shutdown => ActionOutcome::Shutdown,
            ReduceOutcome::NoChange => {
                self.phase = StorePhase::Idle;
                ActionOutcome::NoChange
            }
            ReduceOutcome::Changed => {
                if scale_changed {
                    self.logical_atlas
                        .reset_for_scale(self.viewport.scale_factor);
                }
                ActionOutcome::Compose {
                    clear_tessellation_cache: scale_changed,
                }
            }
        }
    }

    fn compose_scene_buffer(
        &mut self,
        owner: bumpalo::Bump,
        clear_tessellation_cache: bool,
        scene_config: &SceneConfig,
    ) -> SceneBuffer {
        self.phase = StorePhase::Laying;
        self.phase = StorePhase::ComposingSnapshot;
        let compose_started = Instant::now();
        let scene_buffer = SceneBuffer::new(owner, |owner| {
            self.composer.compose_into_buffer(
                owner,
                &self.model,
                &self.layout_cache,
                &mut self.logical_atlas,
                &self.rasterizer,
                self.viewport,
                clear_tessellation_cache,
                scene_config.max_blocks_per_scene,
            )
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
        scene_buffer
    }
}

/// Runs the async store until shutdown or channel disconnect.
pub(crate) async fn run_store(mut store: Store, mut handle: IoHandle, mut pool: SceneBufferPool) {
    if !compose_and_emit(&mut store, &mut pool, false).await {
        return;
    }

    loop {
        let Some(action) = handle.next_action().await else {
            break;
        };
        let action = coalesce_resize_action(action, &mut handle);
        match store.handle_action(action) {
            ActionOutcome::Shutdown => break,
            ActionOutcome::NoChange => {}
            ActionOutcome::Compose {
                clear_tessellation_cache,
            } => {
                if !compose_and_emit(&mut store, &mut pool, clear_tessellation_cache).await {
                    break;
                }
            }
        }
    }
}

async fn compose_and_emit(
    store: &mut Store,
    pool: &mut SceneBufferPool,
    clear_tessellation_cache: bool,
) -> bool {
    let owner = match pool.acquire_empty_bump().await {
        Ok(owner) => owner,
        Err(_) => return false,
    };
    let scene_buffer = store.compose_scene_buffer(owner, clear_tessellation_cache, pool.config());

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
            return false;
        }
    }

    let metadata = scene_buffer.metadata();
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
        return false;
    }

    store.phase = StorePhase::Idle;
    true
}

enum ActionOutcome {
    Shutdown,
    NoChange,
    Compose { clear_tessellation_cache: bool },
}

fn duration_budget(limit_ms: u32) -> Duration {
    Duration::from_millis(u64::from(limit_ms))
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
