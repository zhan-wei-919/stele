//! View-thread scene and atlas application for the latest-wins scene protocol.

use std::time::Duration;

use log::{info, warn};

use crate::io::{AtlasUpdate, SceneFrame};
use crate::scene::SceneBuffer;

use super::{AppRenderer, AppRuntime, AppWindow, SteleApp};

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
        self.scene_protocol
            .set_ready_atlas_generation(update.generation);
    }

    pub(super) fn handle_scene_frame(&mut self, scene_frame: SceneFrame) -> bool {
        let scene_buffer = scene_frame.into_buffer();
        if self.scene_buffer_is_stale(scene_buffer.as_ref()) {
            self.retire_scene_buffer(scene_buffer, "stale_revision");
            return false;
        }
        if !self.scene_buffer_atlas_ready(scene_buffer.as_ref()) {
            self.park_scene_buffer(scene_buffer);
            return false;
        }

        if let Some(pending_scene_buffer) = self.scene_protocol.take_pending_scene_buffer() {
            self.retire_scene_buffer(pending_scene_buffer, "replaced_by_newer");
        }
        self.promote_scene_buffer(scene_buffer)
    }

    pub(super) fn apply_pending_scene_buffer_if_ready(&mut self) -> bool {
        let should_apply = self
            .scene_protocol
            .pending_scene_buffer()
            .map(|scene_buffer| self.scene_buffer_atlas_ready(scene_buffer))
            .unwrap_or(false);
        if !should_apply {
            return false;
        }

        let scene_buffer = self
            .scene_protocol
            .take_pending_scene_buffer()
            .expect("pending scene buffer must exist after readiness check");
        if self.scene_buffer_is_stale(scene_buffer.as_ref()) {
            self.retire_scene_buffer(scene_buffer, "stale_revision");
            return false;
        }
        self.promote_scene_buffer(scene_buffer)
    }

    fn promote_scene_buffer(&mut self, scene_buffer: Box<SceneBuffer>) -> bool {
        let metadata = scene_buffer.metadata();
        if !self.clear_tessellation_cache_if_needed(metadata.clear_tessellation_cache) {
            return false;
        }
        if let Some(stale_pending) = self
            .scene_protocol
            .set_applied_viewport_revision(metadata.viewport_revision)
        {
            self.retire_scene_buffer(stale_pending, "stale_revision");
        }
        if let Some(old_current) = self.current_scene_buffer.replace(scene_buffer) {
            self.retire_scene_buffer(old_current, "replaced_by_newer");
        }
        self.warn_if_end_to_end_latency_exceeded(metadata);
        info!(
            "view.apply viewport_revision={} blocks={} required_atlas_generation={:?}",
            metadata.viewport_revision,
            self.current_scene_buffer
                .as_ref()
                .map(|buffer| buffer.blocks().len())
                .unwrap_or(0),
            metadata.required_atlas_generation
        );
        true
    }

    fn atlas_update_is_stale(&self, update: &AtlasUpdate) -> bool {
        self.scene_protocol
            .ready_atlas_generation()
            .map(|ready| update.generation < ready)
            .unwrap_or(false)
    }

    fn scene_buffer_is_stale(&self, scene_buffer: &SceneBuffer) -> bool {
        scene_buffer.metadata().viewport_revision
            < self.scene_protocol.requested_viewport_revision()
    }

    fn scene_buffer_atlas_ready(&self, scene_buffer: &SceneBuffer) -> bool {
        match scene_buffer.metadata().required_atlas_generation {
            None => true,
            Some(required_generation) => self
                .scene_protocol
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

    fn park_scene_buffer(&mut self, scene_buffer: Box<SceneBuffer>) {
        let metadata = scene_buffer.metadata();
        info!(
            "view.park viewport_revision={} waiting_for_atlas_generation={}",
            metadata.viewport_revision,
            metadata.required_atlas_generation.unwrap_or(0)
        );
        if let Some(retired_scene_buffer) = self
            .scene_protocol
            .replace_pending_scene_buffer(scene_buffer)
        {
            let reason = if retired_scene_buffer.metadata().viewport_revision
                < self.scene_protocol.requested_viewport_revision()
            {
                "stale_revision"
            } else {
                "replaced_by_newer"
            };
            self.retire_scene_buffer(retired_scene_buffer, reason);
        }
    }

    fn warn_if_end_to_end_latency_exceeded(&self, metadata: crate::scene::SceneFrameMetadata) {
        let Some(resize_started_at) = metadata.resize_started_at else {
            return;
        };

        let elapsed = resize_started_at.elapsed();
        if elapsed <= Duration::from_millis(u64::from(self.scene_config.end_to_end_latency_ms)) {
            return;
        }

        warn!(
            "scene.budget_exceeded phase=end_to_end elapsed_us={} limit_ms={} viewport_revision={}",
            elapsed.as_micros(),
            self.scene_config.end_to_end_latency_ms,
            metadata.viewport_revision
        );
    }
}
