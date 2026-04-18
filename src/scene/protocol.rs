//! View-side protocol state for latest-wins scene application and atlas readiness.

use super::SceneBuffer;

/// View-side protocol state that tracks revision monotonicity, atlas readiness, and pending scene.
#[derive(Debug, Default)]
pub(crate) struct SceneProtocolState {
    requested_viewport_revision: u64,
    applied_viewport_revision: u64,
    ready_atlas_generation: Option<u64>,
    pending_scene_buffer: Option<Box<SceneBuffer>>,
}

impl SceneProtocolState {
    /// Creates an empty protocol state.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns the latest requested viewport revision.
    pub(crate) fn requested_viewport_revision(&self) -> u64 {
        self.requested_viewport_revision
    }

    /// Returns the latest applied viewport revision.
    #[cfg(test)]
    pub(crate) fn applied_viewport_revision(&self) -> u64 {
        self.applied_viewport_revision
    }

    /// Returns the latest renderer-ready atlas generation.
    pub(crate) fn ready_atlas_generation(&self) -> Option<u64> {
        self.ready_atlas_generation
    }

    /// Returns the current pending scene buffer, if any.
    pub(crate) fn pending_scene_buffer(&self) -> Option<&SceneBuffer> {
        self.pending_scene_buffer.as_deref()
    }

    /// Monotonically raises the requested viewport revision and ejects stale pending buffers.
    pub(crate) fn set_requested_viewport_revision(
        &mut self,
        viewport_revision: u64,
    ) -> Option<Box<SceneBuffer>> {
        self.requested_viewport_revision = self.requested_viewport_revision.max(viewport_revision);
        self.eject_stale_pending()
    }

    /// Monotonically raises the applied viewport revision and ejects stale pending buffers.
    pub(crate) fn set_applied_viewport_revision(
        &mut self,
        viewport_revision: u64,
    ) -> Option<Box<SceneBuffer>> {
        self.applied_viewport_revision = self.applied_viewport_revision.max(viewport_revision);
        self.requested_viewport_revision = self.requested_viewport_revision.max(viewport_revision);
        self.eject_stale_pending()
    }

    /// Monotonically raises the ready atlas generation.
    pub(crate) fn set_ready_atlas_generation(&mut self, generation: u64) {
        self.ready_atlas_generation = Some(
            self.ready_atlas_generation
                .map(|ready| ready.max(generation))
                .unwrap_or(generation),
        );
    }

    /// Replaces the current pending scene buffer and returns the retired older value, if any.
    pub(crate) fn replace_pending_scene_buffer(
        &mut self,
        scene_buffer: Box<SceneBuffer>,
    ) -> Option<Box<SceneBuffer>> {
        if scene_buffer.metadata().viewport_revision < self.requested_viewport_revision {
            return Some(scene_buffer);
        }
        let previous = self.pending_scene_buffer.replace(scene_buffer);
        if self
            .pending_scene_buffer
            .as_ref()
            .map(|buffer| buffer.metadata().viewport_revision < self.requested_viewport_revision)
            .unwrap_or(false)
        {
            return self.pending_scene_buffer.take().or(previous);
        }
        previous
    }

    /// Removes and returns the current pending scene buffer.
    pub(crate) fn take_pending_scene_buffer(&mut self) -> Option<Box<SceneBuffer>> {
        self.pending_scene_buffer.take()
    }

    fn eject_stale_pending(&mut self) -> Option<Box<SceneBuffer>> {
        let should_eject = self
            .pending_scene_buffer
            .as_ref()
            .map(|buffer| buffer.metadata().viewport_revision < self.requested_viewport_revision)
            .unwrap_or(false);
        if should_eject {
            return self.pending_scene_buffer.take();
        }
        None
    }
}
