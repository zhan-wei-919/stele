//! Arena-backed scene buffers that keep one full viewport revision self-contained.

use std::time::Instant;

use bumpalo::collections::Vec as BumpVec;
use self_cell::self_cell;

use super::{BlockDataArena, BlockId};

self_cell! {
    /// Self-contained scene buffer whose dependent payload borrows one reusable bump arena.
    pub(crate) struct SceneBuffer {
        owner: bumpalo::Bump,

        #[covariant]
        dependent: SceneBufferInner,
    }
}

/// Per-frame metadata carried with one self-contained scene buffer.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SceneFrameMetadata {
    pub(crate) viewport_revision: u64,
    pub(crate) required_atlas_generation: Option<u64>,
    pub(crate) clear_tessellation_cache: bool,
    pub(crate) resize_started_at: Option<Instant>,
}

/// Arena-backed block ordering plus render-ready payloads borrowed from the owner bump.
#[derive(Debug)]
pub(crate) struct SceneBufferInner<'a> {
    metadata: SceneFrameMetadata,
    order: BumpVec<'a, BlockId>,
    blocks: BumpVec<'a, BlockDataArena<'a>>,
}

impl<'a> SceneBufferInner<'a> {
    /// Creates an empty scene payload inside the provided bump arena.
    pub(crate) fn empty_in(owner: &'a bumpalo::Bump, metadata: SceneFrameMetadata) -> Self {
        Self {
            metadata,
            order: BumpVec::new_in(owner),
            blocks: BumpVec::new_in(owner),
        }
    }

    /// Returns immutable frame metadata.
    pub(crate) fn metadata(&self) -> SceneFrameMetadata {
        self.metadata
    }

    /// Returns the ordered block ids for diagnostics and tests.
    pub(crate) fn order(&self) -> &[BlockId] {
        self.order.as_slice()
    }

    /// Returns the ordered block payloads consumed by the renderer.
    pub(crate) fn blocks(&self) -> &[BlockDataArena<'a>] {
        self.blocks.as_slice()
    }

    /// Returns the mutable block order while the composer is assembling this scene.
    pub(crate) fn order_mut(&mut self) -> &mut BumpVec<'a, BlockId> {
        &mut self.order
    }

    /// Returns the mutable block payload arena while the composer is assembling this scene.
    pub(crate) fn blocks_mut(&mut self) -> &mut BumpVec<'a, BlockDataArena<'a>> {
        &mut self.blocks
    }
}

impl SceneBuffer {
    /// Returns the immutable metadata snapshot for this scene.
    pub(crate) fn metadata(&self) -> SceneFrameMetadata {
        self.borrow_dependent().metadata()
    }

    /// Returns the ordered block ids captured in this scene.
    pub(crate) fn order(&self) -> &[BlockId] {
        self.borrow_dependent().order()
    }

    /// Returns the ordered block payloads captured in this scene.
    pub(crate) fn blocks(&self) -> &[BlockDataArena<'_>] {
        self.borrow_dependent().blocks()
    }
}

impl std::fmt::Debug for SceneBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let metadata = self.metadata();
        f.debug_struct("SceneBuffer")
            .field("viewport_revision", &metadata.viewport_revision)
            .field(
                "required_atlas_generation",
                &metadata.required_atlas_generation,
            )
            .field(
                "clear_tessellation_cache",
                &metadata.clear_tessellation_cache,
            )
            .field("blocks", &self.blocks().len())
            .finish()
    }
}

// SAFETY:
// 1. `SceneBufferInner` borrows the owner `Bump` only inside `self_cell`; callers only observe
//    `&SceneBuffer` or `Box<SceneBuffer>`, and any `borrow_dependent()` lifetime is tied to `&self`.
// 2. Renderer rebuilds read dependent slices only during one `rebuild_gpu_data` call; the buffer is
//    not retired, dropped, or converted back into its owner bump while those borrows are alive.
// 3. Cross-thread transfer happens only by move through Tokio mpsc channels, so the value is never
//    shared concurrently.
// 4. The dependent fields only contain `Send` elements (`BlockId`, `ClipRect`, `u32`, `u64`,
//    `GlyphInstance`, `RectInstance`, `PathCmd`, `ImageCmd`); new fields must be re-audited.
// 5. `Bump::reset` is called only after `SceneBuffer::into_owner` has dropped the dependent payload.
// 6. `SceneBuffer` is move-only; do not add `Sync`.
unsafe impl Send for SceneBuffer {}
