//! Incremental mutations applied to the renderer-owned draw list.

mod scene;

#[cfg(test)]
mod tests;

use scene::{LegacyScene, SceneState};

use super::types::{BlockDrawGroup, ClipRect, ImageCmd, PathCmd, PositionedGlyph, RectCmd};

/// Incremental update applied to a renderer-owned line list.
#[derive(Clone, Debug)]
pub enum DrawListOp {
    SetBlocks(Vec<BlockDrawGroup>),
    Insert {
        line_index: usize,
        glyphs: Vec<PositionedGlyph>,
    },
    // M0 only emits Insert operations from the hard-coded layout path.
    // Remove stays in the API because incremental scene updates will need it,
    // and the current unit test is the only non-future caller during cargo run.
    Remove {
        line_index: usize,
    },
    // M0 does not replace lines in normal execution yet.
    // This variant exists to keep the incremental update surface complete ahead
    // of upstream dirty-line updates, so cargo run currently sees it as dead code.
    Replace {
        line_index: usize,
        glyphs: Vec<PositionedGlyph>,
    },
    SetRects(Vec<RectCmd>),
    SetPaths(Vec<PathCmd>),
    SetImages(Vec<ImageCmd>),
}

/// Renderer input state assembled from glyphs plus layer-aware CPU primitives.
#[derive(Clone, Debug, Default)]
pub struct DrawList {
    scene: SceneState,
}

impl DrawList {
    /// Creates an empty draw list with no glyphs or primitive commands.
    ///
    /// This convenience constructor is currently only used by the unit tests.
    /// Production code builds the renderer-owned draw list through Default plus
    /// incremental ops, so cargo run reports this helper as unused for now.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies incremental line and primitive operations in-order.
    pub fn apply_ops<I>(&mut self, ops: I)
    where
        I: IntoIterator<Item = DrawListOp>,
    {
        for op in ops {
            match op {
                DrawListOp::SetBlocks(blocks) => {
                    self.scene = SceneState::Blocks(blocks);
                }
                DrawListOp::Insert { line_index, glyphs } => {
                    let legacy = self.legacy_scene_mut();
                    if !Self::valid_insert_index(legacy, line_index) {
                        continue;
                    }
                    legacy.lines.insert(line_index, glyphs);
                }
                DrawListOp::Remove { line_index } => {
                    let legacy = self.legacy_scene_mut();
                    if !Self::valid_existing_index(legacy, line_index) {
                        continue;
                    }
                    legacy.lines.remove(line_index);
                }
                DrawListOp::Replace { line_index, glyphs } => {
                    let legacy = self.legacy_scene_mut();
                    if !Self::valid_existing_index(legacy, line_index) {
                        continue;
                    }
                    legacy.lines[line_index] = glyphs;
                }
                DrawListOp::SetRects(rects) => {
                    self.legacy_scene_mut().rects = rects;
                }
                DrawListOp::SetPaths(paths) => {
                    self.legacy_scene_mut().paths = paths;
                }
                DrawListOp::SetImages(images) => {
                    self.legacy_scene_mut().images = images;
                }
            }
        }
    }

    /// Returns the active draw groups for the current scene representation.
    pub(crate) fn block_groups(&self, viewport_clip: ClipRect) -> Vec<BlockDrawGroup> {
        let mut groups = match &self.scene {
            SceneState::Blocks(blocks) => blocks.clone(),
            SceneState::Legacy(legacy) => legacy.block_groups(viewport_clip),
        };
        if groups.len() > 1 {
            groups.sort_by_key(|group| (group.z_order(), group.block_index()));
        }
        groups
    }

    fn legacy_scene_mut(&mut self) -> &mut LegacyScene {
        if !matches!(self.scene, SceneState::Legacy(_)) {
            self.scene = SceneState::Legacy(LegacyScene::default());
        }
        let SceneState::Legacy(legacy) = &mut self.scene else {
            unreachable!("scene must be legacy after replacement");
        };
        legacy
    }

    fn valid_insert_index(legacy: &LegacyScene, line_index: usize) -> bool {
        if line_index <= legacy.lines.len() {
            true
        } else {
            debug_assert!(
                false,
                "DrawListOp::Insert line_index out of bounds: {line_index} > {}",
                legacy.lines.len()
            );
            false
        }
    }

    fn valid_existing_index(legacy: &LegacyScene, line_index: usize) -> bool {
        if line_index < legacy.lines.len() {
            true
        } else {
            debug_assert!(
                false,
                "DrawListOp line_index out of bounds: {line_index} >= {}",
                legacy.lines.len()
            );
            false
        }
    }
}
