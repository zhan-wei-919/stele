use log::{info, warn};

use crate::io::{AtlasUpdate, BlockOp, SceneFrame, ScenePayload};

use super::{AppRenderer, AppRuntime, AppWindow, SteleApp};

#[derive(Clone, Copy, Debug, Default)]
struct ApplyStats {
    replaced_blocks: usize,
    removed_blocks: usize,
}

impl<Rt, Win, Rend> SteleApp<Rt, Win, Rend>
where
    Rt: AppRuntime,
    Win: AppWindow,
    Rend: AppRenderer,
{
    pub(super) fn apply_atlas_update(&mut self, update: AtlasUpdate) {
        if self.atlas_update_is_stale(&update) {
            return;
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        if let Some(new_size) = update.requested_atlas_size {
            renderer.recreate_atlas(new_size);
        }
        for patch in &update.patches {
            renderer.write_atlas_patch(patch);
        }
        self.view_state
            .set_ready_atlas_generation(update.generation);
    }

    /// Scene frames may outrun atlas uploads because atlas generation is no longer tied to
    /// viewport revision. We keep only the newest still-requested frame and retry it after
    /// every atlas update so live resize stays latest-only without dropping required glyph data.
    pub(super) fn handle_scene_frame(&mut self, scene_frame: SceneFrame) -> bool {
        if self.scene_frame_is_stale(&scene_frame) {
            return false;
        }
        if !self.scene_frame_atlas_ready(&scene_frame) {
            self.view_state.set_pending_scene_frame(scene_frame);
            return false;
        }

        self.apply_scene_frame(scene_frame)
    }

    pub(super) fn apply_pending_scene_frame_if_ready(&mut self) -> bool {
        let should_apply = self
            .view_state
            .pending_scene_frame()
            .map(|scene_frame| self.scene_frame_atlas_ready(scene_frame))
            .unwrap_or(false);
        if !should_apply {
            return false;
        }

        let scene_frame = self
            .view_state
            .take_pending_scene_frame()
            .expect("pending scene frame must exist after readiness check");
        self.apply_scene_frame(scene_frame)
    }

    fn apply_scene_frame(&mut self, scene_frame: SceneFrame) -> bool {
        if self.scene_frame_is_stale(&scene_frame) {
            return false;
        }
        if !self.clear_tessellation_cache_if_needed(scene_frame.clear_tessellation_cache) {
            return false;
        }

        let required_atlas_generation = scene_frame.required_atlas_generation;
        let viewport_revision = scene_frame.viewport_revision;
        let Some(stats) = self.apply_scene_payload(scene_frame.payload, viewport_revision) else {
            return false;
        };
        self.finish_scene_frame_apply(viewport_revision);
        info!(
            "view.apply viewport_revision={} replaced_blocks={} removed_blocks={} required_atlas_generation={:?}",
            viewport_revision,
            stats.replaced_blocks,
            stats.removed_blocks,
            required_atlas_generation
        );
        true
    }

    fn atlas_update_is_stale(&self, update: &AtlasUpdate) -> bool {
        self.view_state
            .ready_atlas_generation()
            .map(|ready| update.generation < ready)
            .unwrap_or(false)
    }

    fn scene_frame_is_stale(&self, scene_frame: &SceneFrame) -> bool {
        scene_frame.viewport_revision < self.view_state.requested_viewport_revision()
    }

    fn scene_frame_atlas_ready(&self, scene_frame: &SceneFrame) -> bool {
        match scene_frame.required_atlas_generation {
            None => true,
            Some(required_generation) => self
                .view_state
                .ready_atlas_generation()
                .map(|ready_generation| ready_generation >= required_generation)
                .unwrap_or(false),
        }
    }

    fn clear_tessellation_cache_if_needed(&mut self, should_clear: bool) -> bool {
        if !should_clear {
            return true;
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return false;
        };
        renderer.clear_tessellation_cache();
        true
    }

    fn apply_scene_payload(
        &mut self,
        payload: ScenePayload,
        viewport_revision: u64,
    ) -> Option<ApplyStats> {
        match payload {
            ScenePayload::ReplaceAll {
                block_order,
                block_batches,
            } => Some(self.apply_replace_all(block_order, block_batches)),
            ScenePayload::Diff {
                block_order,
                block_ops,
            } => self.apply_diff_payload(viewport_revision, block_order, block_ops),
        }
    }

    fn apply_replace_all(
        &mut self,
        block_order: Vec<crate::scene::BlockId>,
        block_batches: Vec<(crate::scene::BlockId, crate::scene::BlockSceneBatch)>,
    ) -> ApplyStats {
        let mut stats = ApplyStats::default();
        self.view_state.clear_scene();
        self.view_state.set_block_order(block_order);
        for (block_id, batch) in block_batches {
            self.view_state.replace_block(block_id, batch);
            stats.replaced_blocks += 1;
        }
        stats
    }

    fn apply_diff_payload(
        &mut self,
        viewport_revision: u64,
        block_order: Option<Vec<crate::scene::BlockId>>,
        block_ops: Vec<BlockOp>,
    ) -> Option<ApplyStats> {
        if viewport_revision > self.view_state.applied_viewport_revision() {
            warn!(
                "view.drop_non_self_contained_scene_frame viewport_revision={} applied_viewport_revision={}",
                viewport_revision,
                self.view_state.applied_viewport_revision()
            );
            return None;
        }

        if let Some(block_order) = block_order {
            self.view_state.set_block_order(block_order);
        }

        let mut stats = ApplyStats::default();
        for op in block_ops {
            match op {
                BlockOp::Replace { block_id, batch } => {
                    self.view_state.replace_block(block_id, batch);
                    stats.replaced_blocks += 1;
                }
                BlockOp::Remove { block_id } => {
                    self.view_state.remove_block(block_id);
                    stats.removed_blocks += 1;
                }
            }
        }
        Some(stats)
    }

    fn finish_scene_frame_apply(&mut self, viewport_revision: u64) {
        self.view_state
            .set_applied_viewport_revision(viewport_revision);
        self.view_state.clear_pending_scene_frame();
    }
}
