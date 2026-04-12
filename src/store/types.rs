//! Store-owned viewport and snapshot types.

use std::collections::HashMap;

use crate::scene::{BlockId, BlockSceneBatch};

/// Physical viewport input used by the store for layout and diff invalidation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewportState {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale_factor: f32,
    pub(crate) viewport_revision: u64,
}

impl ViewportState {
    /// Creates a validated viewport snapshot.
    pub(crate) fn new(width: u32, height: u32, scale_factor: f32, viewport_revision: u64) -> Self {
        debug_assert!(
            scale_factor > 0.0,
            "viewport scale factor must stay positive"
        );
        Self {
            width,
            height,
            scale_factor,
            viewport_revision,
        }
    }
}

/// Store pipeline phase used for logs and debugging.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StorePhase {
    Idle,
    Reducing,
    Laying,
    ComposingSnapshot,
    DiffingSnapshot,
}

/// Full render-ready scene snapshot owned by the async store.
#[derive(Clone, Debug)]
pub(crate) struct SceneSnapshot {
    pub(crate) viewport_revision: u64,
    pub(crate) required_atlas_generation: Option<u64>,
    pub(crate) order: Vec<BlockId>,
    pub(crate) blocks: HashMap<BlockId, BlockSceneBatch>,
}

impl SceneSnapshot {
    /// Creates an empty baseline snapshot for diff bootstrap.
    pub(crate) fn empty(viewport_revision: u64) -> Self {
        Self {
            viewport_revision,
            required_atlas_generation: None,
            order: Vec::new(),
            blocks: HashMap::new(),
        }
    }
}
