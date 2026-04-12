//! Cached lyon tessellation output keyed by path content.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::draw_list::PathCmd;

use super::convert::tessellate_path;
use crate::renderer::instance::PathVertex;

/// CPU-side mesh cached for a specific path definition.
#[derive(Clone, Debug, Default)]
pub struct CachedMesh {
    pub vertices: Vec<PathVertex>,
    pub indices: Vec<u32>,
    pub last_used: u64,
}

/// Reuses tessellated meshes across frames until content or scale changes.
#[derive(Default)]
pub struct TessellationCache {
    pub(crate) entries: HashMap<u64, CachedMesh>,
    pub(crate) generation: u64,
}

impl TessellationCache {
    /// Advances the cache generation so rebuild work can refresh liveness.
    pub fn begin_frame(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }

    /// Invalidates every cached mesh after a scale-factor change.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns a cached mesh, tessellating the path on the first miss.
    pub fn get_or_insert(&mut self, cmd: &PathCmd, scale_factor: f32) -> (&CachedMesh, bool) {
        let content_hash = cmd.content_hash();
        match self.entries.entry(content_hash) {
            Entry::Occupied(entry) => {
                let entry = entry.into_mut();
                entry.last_used = self.generation;
                (&*entry, false)
            }
            Entry::Vacant(entry) => {
                let mut mesh = tessellate_path(cmd, scale_factor);
                mesh.last_used = self.generation;
                let entry = entry.insert(mesh);
                (&*entry, true)
            }
        }
    }

    /// Drops meshes that have not been referenced for `max_age` generations.
    pub fn evict_stale(&mut self, max_age: u64) {
        let min_generation = self.generation.saturating_sub(max_age);
        self.entries
            .retain(|_, entry| entry.last_used >= min_generation);
    }
}
