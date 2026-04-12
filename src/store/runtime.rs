//! Async store runtime loop that reduces actions, recomputes snapshots, and emits view updates.

use log::info;
use tokio::sync::mpsc::error::TryRecvError;

use crate::demo::build_demo_state;
use crate::font::FreeTypeRasterizer;
use crate::io::{Action, IoHandle, SceneFrame, ViewUpdate};

use super::composer::Composer;
use super::diff::{diff_snapshots, replace_all_snapshot};
use super::logical_atlas::LogicalAtlas;
use super::model::{LayoutCache, Model};
use super::reducer::{ReduceOutcome, Reducer};
use super::types::{SceneSnapshot, StorePhase, ViewportState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingSceneMode {
    ReplaceAll,
    Diff,
}

#[derive(Clone, Debug)]
struct PendingScene {
    snapshot: SceneSnapshot,
    mode: PendingSceneMode,
    clear_tessellation_cache: bool,
}

/// Async-side store that owns the logical app state and snapshot pipeline.
pub(crate) struct Store {
    viewport: ViewportState,
    model: Model,
    layout_cache: LayoutCache,
    composer: Composer,
    logical_atlas: LogicalAtlas,
    last_emitted_snapshot: SceneSnapshot,
    pending_scene: Option<PendingScene>,
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
            pending_scene: None,
            rasterizer,
            reducer: Reducer,
            phase: StorePhase::Idle,
        }
    }

    fn bootstrap(&mut self) {
        self.recompute_snapshot(PendingSceneMode::ReplaceAll, false);
    }

    fn handle_action(&mut self, action: Action) -> bool {
        self.phase = StorePhase::Reducing;
        let scale_changed = matches!(
            &action,
            Action::Resize { scale_factor, .. }
                if self.viewport.scale_factor.to_bits() != scale_factor.to_bits()
        );
        let pending_scene_mode = match action {
            Action::Resize { .. } => PendingSceneMode::ReplaceAll,
            _ => PendingSceneMode::Diff,
        };

        match self
            .reducer
            .apply(&mut self.model, &mut self.viewport, &action)
        {
            ReduceOutcome::Shutdown => false,
            ReduceOutcome::Continue => {
                if scale_changed {
                    self.logical_atlas.reset_for_scale(self.viewport.scale_factor);
                }
                self.recompute_snapshot(pending_scene_mode, scale_changed);
                true
            }
        }
    }

    fn recompute_snapshot(
        &mut self,
        mode: PendingSceneMode,
        clear_tessellation_cache: bool,
    ) {
        self.phase = StorePhase::Laying;
        let snapshot = self.compose_snapshot();
        self.pending_scene = Some(PendingScene {
            snapshot,
            mode,
            clear_tessellation_cache,
        });
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

    fn emit_pending_updates(&mut self, handle: &IoHandle) -> bool {
        let Some(pending_scene) = self.pending_scene.take() else {
            return true;
        };

        self.phase = StorePhase::DiffingSnapshot;
        if let Some(atlas_update) = self.logical_atlas.take_pending_update() {
            info!(
                "store.atlas generation={} requested_atlas_size={:?} patches={}",
                atlas_update.generation,
                atlas_update.requested_atlas_size,
                atlas_update.patches.len()
            );
            if !handle.dispatch_view_update(ViewUpdate::Atlas(atlas_update)) {
                return false;
            }
        }

        let payload = match pending_scene.mode {
            PendingSceneMode::ReplaceAll => replace_all_snapshot(&pending_scene.snapshot),
            PendingSceneMode::Diff => diff_snapshots(&self.last_emitted_snapshot, &pending_scene.snapshot),
        };
        let mut scene_frame = SceneFrame::new(
            pending_scene.snapshot.viewport_revision,
            pending_scene.snapshot.required_atlas_generation,
            payload,
        );
        scene_frame.clear_tessellation_cache = pending_scene.clear_tessellation_cache;

        let changed_blocks = match &scene_frame.payload {
            crate::io::ScenePayload::ReplaceAll { block_batches, .. } => block_batches.len(),
            crate::io::ScenePayload::Diff { block_ops, .. } => block_ops.len(),
        };
        if !scene_frame.is_empty() {
            info!(
                "store.scene viewport_revision={} blocks={} changed_blocks={} replace_all={}",
                pending_scene.snapshot.viewport_revision,
                pending_scene.snapshot.blocks.len(),
                changed_blocks,
                matches!(pending_scene.mode, PendingSceneMode::ReplaceAll)
            );
            if !handle.dispatch_view_update(ViewUpdate::Scene(scene_frame)) {
                return false;
            }
        }

        self.last_emitted_snapshot = pending_scene.snapshot;
        self.phase = StorePhase::Idle;
        true
    }
}

/// Runs the async store until shutdown or channel disconnect.
pub(crate) async fn run_store(mut store: Store, mut handle: IoHandle) {
    store.bootstrap();
    if !store.emit_pending_updates(&handle) {
        return;
    }

    loop {
        let Some(action) = handle.next_action().await else {
            break;
        };
        let action = coalesce_resize_action(action, &mut handle);
        if !store.handle_action(action) {
            break;
        }
        if !store.emit_pending_updates(&handle) {
            break;
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
mod tests {
    use std::time::Instant;

    use super::Store;
    use crate::font::{FontDiscovery, FreeTypeRasterizer};
    use crate::io::{Action, ViewUpdate};
    use crate::renderer::subpixel::detect_subpixel_layout;
    use crate::store::ViewportState;

    fn build_rasterizer_for_perf_test() -> FreeTypeRasterizer {
        let font_discovery = FontDiscovery::new().expect("failed to discover system fonts");
        FreeTypeRasterizer::new(font_discovery, detect_subpixel_layout())
            .expect("failed to initialize FreeType rasterizer")
    }

    fn build_pending_updates_for_test(store: &mut Store) -> Vec<ViewUpdate> {
        let pending_scene = store
            .pending_scene
            .take()
            .expect("pending scene must exist");
        let mut updates = Vec::new();
        if let Some(atlas_update) = store.logical_atlas.take_pending_update() {
            updates.push(ViewUpdate::Atlas(atlas_update));
        }

        let payload = match pending_scene.mode {
            super::PendingSceneMode::ReplaceAll => super::replace_all_snapshot(&pending_scene.snapshot),
            super::PendingSceneMode::Diff => {
                super::diff_snapshots(&store.last_emitted_snapshot, &pending_scene.snapshot)
            }
        };
        let mut scene_frame = crate::io::SceneFrame::new(
            pending_scene.snapshot.viewport_revision,
            pending_scene.snapshot.required_atlas_generation,
            payload,
        );
        scene_frame.clear_tessellation_cache = pending_scene.clear_tessellation_cache;
        if !scene_frame.is_empty() {
            updates.push(ViewUpdate::Scene(scene_frame));
        }

        store.last_emitted_snapshot = pending_scene.snapshot;
        updates
    }

    #[test]
    #[ignore = "manual perf smoke test"]
    fn reports_resize_recompute_and_diff_perf() {
        let rasterizer = build_rasterizer_for_perf_test();
        let mut store = Store::new(rasterizer, ViewportState::new(960, 640, 1.0, 0));

        store.bootstrap();
        let _ = build_pending_updates_for_test(&mut store);

        let recompute_started = Instant::now();
        assert!(store.handle_action(Action::Resize {
            width: 1440,
            height: 900,
            scale_factor: 1.0,
            viewport_revision: 1,
        }));
        let recompute_elapsed = recompute_started.elapsed();

        let emit_started = Instant::now();
        let updates = build_pending_updates_for_test(&mut store);
        let emit_elapsed = emit_started.elapsed();

        let total_compute_us = recompute_elapsed.as_micros() + emit_elapsed.as_micros();
        let atlas_update_count = updates
            .iter()
            .filter(|update| matches!(update, ViewUpdate::Atlas(_)))
            .count();
        let atlas_patch_count = updates
            .iter()
            .filter_map(|update| match update {
                ViewUpdate::Atlas(atlas_update) => Some(atlas_update.patches.len()),
                ViewUpdate::Scene(_) => None,
            })
            .sum::<usize>();
        let scene_frame_count = updates
            .iter()
            .filter(|update| matches!(update, ViewUpdate::Scene(_)))
            .count();
        let replace_all_block_count = updates
            .iter()
            .filter_map(|update| match update {
                ViewUpdate::Scene(scene_frame) => match &scene_frame.payload {
                    crate::io::ScenePayload::ReplaceAll { block_batches, .. } => {
                        Some(block_batches.len())
                    }
                    crate::io::ScenePayload::Diff { .. } => None,
                },
                ViewUpdate::Atlas(_) => None,
            })
            .sum::<usize>();
        let diff_op_count = updates
            .iter()
            .filter_map(|update| match update {
                ViewUpdate::Scene(scene_frame) => match &scene_frame.payload {
                    crate::io::ScenePayload::Diff { block_ops, .. } => Some(block_ops.len()),
                    crate::io::ScenePayload::ReplaceAll { .. } => None,
                },
                ViewUpdate::Atlas(_) => None,
            })
            .sum::<usize>();

        println!(
            "perf.store resize_recompute_us={} emit_updates_us={} total_compute_us={} atlas_updates={} atlas_patches={} scene_frames={} replace_all_blocks={} diff_ops={}",
            recompute_elapsed.as_micros(),
            emit_elapsed.as_micros(),
            total_compute_us,
            atlas_update_count,
            atlas_patch_count,
            scene_frame_count,
            replace_all_block_count,
            diff_op_count,
        );
    }
}
