//! Pure arithmetic layout over prepared inline measurements.

mod break_lines;
mod decorations;
mod place_lines;
mod types;

use log::{info, warn};

use crate::renderer::{RectCmd, RenderLayer};

use super::document::Document;
use super::prepare::PreparedBlock;
use break_lines::greedy_line_break;
use place_lines::{content_rect, place_lines};

pub(crate) use types::{LayoutBlock, LayoutLine, LayoutRun};

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

    let content_rect = content_rect(block.rect(), block.padding());
    let raw_lines = if content_rect.width() > 0.0 {
        greedy_line_break(prepared, content_rect.width())
    } else {
        Vec::new()
    };
    let lines = place_lines(
        &raw_lines,
        content_rect,
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
        background_rect: block.background_color().map(|color| {
            RectCmd::new(
                [block.rect().x(), block.rect().y()],
                [block.rect().width(), block.rect().height()],
                color,
                RenderLayer::Background,
            )
        }),
        clip_rect: block.rect(),
    })
}
