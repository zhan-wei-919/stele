//! Internal scene representations used by the draw-list API surface.

use super::super::types::{
    BlockDrawGroup, BlockSubLayer, ClipRect, ImageCmd, PathCmd, PositionedGlyph, RectCmd,
};

#[derive(Clone, Debug)]
pub(super) enum SceneState {
    Legacy(LegacyScene),
    Blocks(Vec<BlockDrawGroup>),
}

impl Default for SceneState {
    fn default() -> Self {
        Self::Legacy(LegacyScene::default())
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct LegacyScene {
    pub(super) lines: Vec<Vec<PositionedGlyph>>,
    pub(super) rects: Vec<RectCmd>,
    pub(super) paths: Vec<PathCmd>,
    pub(super) images: Vec<ImageCmd>,
}

impl LegacyScene {
    pub(super) fn block_groups(&self, viewport_clip: ClipRect) -> Vec<BlockDrawGroup> {
        if self.has_content() {
            vec![self.legacy_root_block(viewport_clip)]
        } else {
            Vec::new()
        }
    }

    fn has_content(&self) -> bool {
        !self.lines.is_empty()
            || !self.rects.is_empty()
            || !self.paths.is_empty()
            || !self.images.is_empty()
    }

    fn legacy_root_block(&self, viewport_clip: ClipRect) -> BlockDrawGroup {
        let mut root = BlockDrawGroup::new(0, 0, Some(viewport_clip));
        for line in &self.lines {
            root.extend_glyphs(BlockSubLayer::Content, line.clone());
        }
        for rect in &self.rects {
            root.push_rect(*rect);
        }
        for path in &self.paths {
            root.push_path(path.clone());
        }
        for image in &self.images {
            root.push_image(image.clone());
        }
        root
    }
}
