//! Async store runtime loop that reduces actions, recomputes snapshots, and emits SceneDiffs.

use std::time::Duration;

use log::info;

use crate::demo::build_demo_state;
use crate::font::FreeTypeRasterizer;
use crate::io::{Action, IoHandle};

use super::composer::Composer;
use super::diff::diff_snapshots;
use super::logical_atlas::LogicalAtlas;
use super::model::{LayoutCache, Model};
use super::reducer::{ReduceOutcome, Reducer};
use super::throttle::RedrawThrottle;
use super::types::{SceneSnapshot, StorePhase, ViewportState};

const MIN_SCENE_DIFF_INTERVAL: Duration = Duration::from_millis(16);

/// Async-side store that owns the logical app state and snapshot pipeline.
pub(crate) struct Store {
    viewport: ViewportState,
    model: Model,
    layout_cache: LayoutCache,
    composer: Composer,
    logical_atlas: LogicalAtlas,
    last_emitted_snapshot: SceneSnapshot,
    pending_snapshot: Option<SceneSnapshot>,
    pending_atlas_patches: Vec<crate::io::AtlasPatch>,
    pending_clear_tessellation_cache: bool,
    throttle: RedrawThrottle,
    rasterizer: FreeTypeRasterizer,
    reducer: Reducer,
    phase: StorePhase,
}

impl Store {
    /// Builds the demo-backed store and prepares its initial model state.
    pub(crate) fn new(rasterizer: FreeTypeRasterizer, viewport: ViewportState) -> Self {
        let demo_state = build_demo_state(&rasterizer, super::reducer::logical_viewport(viewport));
        let (model, layout_cache) = Model::from_demo_state(demo_state);
        Self {
            viewport,
            model,
            layout_cache,
            composer: Composer,
            logical_atlas: LogicalAtlas::new(viewport.scale_factor),
            last_emitted_snapshot: SceneSnapshot::empty(viewport.viewport_revision),
            pending_snapshot: None,
            pending_atlas_patches: Vec::new(),
            pending_clear_tessellation_cache: false,
            throttle: RedrawThrottle::new(MIN_SCENE_DIFF_INTERVAL),
            rasterizer,
            reducer: Reducer,
            phase: StorePhase::Idle,
        }
    }

    fn bootstrap(&mut self) {
        self.recompute_snapshot();
    }

    fn handle_action(&mut self, action: Action) -> bool {
        self.phase = StorePhase::Reducing;
        let scale_changed = matches!(
            action,
            Action::Resize { scale_factor, .. }
                if self.viewport.scale_factor.to_bits() != scale_factor.to_bits()
        );

        match self
            .reducer
            .apply(&mut self.model, &mut self.viewport, &action)
        {
            ReduceOutcome::Shutdown => false,
            ReduceOutcome::Continue => {
                if scale_changed {
                    self.logical_atlas
                        .reset_for_scale(self.viewport.scale_factor);
                    self.pending_atlas_patches.clear();
                    self.pending_clear_tessellation_cache = true;
                }
                self.recompute_snapshot();
                true
            }
        }
    }

    fn recompute_snapshot(&mut self) {
        self.phase = StorePhase::Laying;
        let snapshot = self.compose_snapshot();
        self.pending_snapshot = Some(snapshot);
        self.pending_atlas_patches
            .extend(self.logical_atlas.take_pending_patches());
        if !self.throttle.ready_now() {
            self.phase = StorePhase::Throttled;
            if let Some(snapshot) = self.pending_snapshot.as_ref() {
                info!(
                    "store.throttle deferred=true pending_viewport_revision={}",
                    snapshot.viewport_revision
                );
            }
        }
    }

    fn compose_snapshot(&mut self) -> SceneSnapshot {
        self.phase = StorePhase::ComposingSnapshot;
        self.composer.compose_snapshot(
            &self.model,
            &self.layout_cache,
            &mut self.logical_atlas,
            &self.rasterizer,
            self.viewport,
        )
    }

    fn emit_pending_diff(&mut self, handle: &IoHandle) -> bool {
        let Some(snapshot) = self.pending_snapshot.take() else {
            return true;
        };

        self.phase = StorePhase::DiffingSnapshot;
        let force_full_snapshot =
            snapshot.viewport_revision != self.last_emitted_snapshot.viewport_revision;
        let (requested_atlas_size, atlas_patches) = if force_full_snapshot {
            let payload = self.logical_atlas.take_full_snapshot_payload(&self.rasterizer);
            self.pending_atlas_patches.clear();
            payload
        } else {
            (
                self.logical_atlas.take_pending_requested_atlas_size(),
                std::mem::take(&mut self.pending_atlas_patches),
            )
        };
        let diff = diff_snapshots(
            &self.last_emitted_snapshot,
            &snapshot,
            requested_atlas_size,
            atlas_patches,
            self.pending_clear_tessellation_cache,
            force_full_snapshot,
        );
        let should_send = !diff.is_empty();
        if should_send {
            info!(
                "store.snapshot blocks={} changed_blocks={} atlas_patches={}",
                snapshot.blocks.len(),
                diff.block_ops.len(),
                diff.atlas_patches.len()
            );
            if !handle.dispatch_scene_diff(diff) {
                return false;
            }
            self.throttle.record_send();
        }

        self.pending_clear_tessellation_cache = false;
        self.last_emitted_snapshot = snapshot;
        self.phase = StorePhase::Idle;
        true
    }
}

/// Runs the async store until shutdown or channel disconnect.
pub(crate) async fn run_store(mut store: Store, mut handle: IoHandle) {
    store.bootstrap();

    loop {
        if store.pending_snapshot.is_some() && store.throttle.ready_now() {
            if !store.emit_pending_diff(&handle) {
                break;
            }
            continue;
        }

        let delay = store.throttle.delay_until_ready();
        tokio::select! {
            action = handle.next_action() => {
                let Some(action) = action else {
                    break;
                };
                if !store.handle_action(action) {
                    break;
                }
            }
            _ = tokio::time::sleep(delay), if store.pending_snapshot.is_some() => {
                store.phase = StorePhase::Throttled;
            }
        }
    }
}
