//! Block-level document flow layout.

use log::{info, warn};

use crate::renderer::{RectCmd, RenderLayer};

use super::break_lines::greedy_line_break;
use super::place_lines::{content_rect, place_lines};
use super::types::LayoutBlock;
use crate::layout::document::Document;
use crate::layout::prepare::{PreparedBlock, PreparedGlyph};

/// Positions all prepared blocks using the current document geometry.
pub(crate) fn layout_document(
    document: &Document,
    prepared_blocks: &[PreparedBlock],
) -> Vec<LayoutBlock> {
    prepared_blocks
        .iter()
        .filter_map(|prepared| layout_block(document, prepared))
        .collect()
}

fn layout_block(document: &Document, prepared: &PreparedBlock) -> Option<LayoutBlock> {
    let Some(block) = document.block(prepared.block_index) else {
        warn!(
            "layout.warn.skip_block block_index={} reason=missing_document_block",
            prepared.block_index
        );
        return None;
    };

    let content_bounds = content_rect(block.rect(), block.padding());
    let raw_lines = break_block_lines(prepared, content_bounds.width());
    let lines = place_lines(
        &raw_lines,
        content_bounds,
        prepared.default_ascent,
        prepared.default_line_height,
    );

    info!(
        "layout.layout block_index={} line_count={}",
        prepared.block_index,
        lines.len()
    );
    Some(LayoutBlock {
        block_index: prepared.block_index,
        z_order: block.z_order(),
        lines,
        background_rect: block_background_rect(block),
        clip_rect: block.rect(),
    })
}

fn break_block_lines(prepared: &PreparedBlock, content_width: f32) -> Vec<Vec<PreparedGlyph>> {
    if content_width > 0.0 {
        greedy_line_break(prepared, content_width)
    } else {
        Vec::new()
    }
}

fn block_background_rect(block: &crate::layout::document::Block) -> Option<RectCmd> {
    block.background_color().map(|color| {
        RectCmd::new(
            [block.rect().x(), block.rect().y()],
            [block.rect().width(), block.rect().height()],
            color,
            RenderLayer::Background,
        )
    })
}
