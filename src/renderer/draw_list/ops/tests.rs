//! Unit tests for block-scene mutation semantics.

use super::{DrawList, DrawListOp};
use crate::font::SubpixelBin;
use crate::renderer::draw_list::{BlockDrawGroup, ClipRect, PositionedGlyph, RenderLayer};

fn glyph(id: u16) -> PositionedGlyph {
    PositionedGlyph {
        font_id: 0,
        glyph_id: id,
        font_size: 14.0,
        pos: [0.0, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
        subpixel_offset: SubpixelBin::new(0, 0),
    }
}

#[test]
fn apply_ops_replaces_explicit_block_groups() {
    let mut draw_list = DrawList::new();
    draw_list.apply_ops([DrawListOp::SetBlocks(vec![block(3, 1, 9)])]);

    let groups = draw_list.block_groups();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].block_index(), 3);
    assert_eq!(
        groups[0].layer(RenderLayer::Content).glyphs()[0].glyph_id,
        9
    );
}

#[test]
fn set_blocks_replaces_previous_block_scene() {
    let mut draw_list = DrawList::new();
    draw_list.apply_ops([DrawListOp::SetBlocks(vec![block(1, 0, 1)])]);
    draw_list.apply_ops([DrawListOp::SetBlocks(vec![block(3, 1, 9)])]);

    let groups = draw_list.block_groups();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].block_index(), 3);
    assert_eq!(
        groups[0].layer(RenderLayer::Content).glyphs()[0].glyph_id,
        9
    );
}

#[test]
fn set_blocks_sorts_by_z_order_then_block_index() {
    let mut draw_list = DrawList::new();
    draw_list.apply_ops([DrawListOp::SetBlocks(vec![
        block(3, 2, 13),
        block(1, 1, 11),
        block(2, 1, 12),
    ])]);

    let groups = draw_list.block_groups();
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].block_index(), 1);
    assert_eq!(groups[1].block_index(), 2);
    assert_eq!(groups[2].block_index(), 3);
    assert_eq!(
        groups[0].layer(RenderLayer::Content).glyphs()[0].glyph_id,
        11
    );
    assert_eq!(
        groups[1].layer(RenderLayer::Content).glyphs()[0].glyph_id,
        12
    );
    assert_eq!(
        groups[2].layer(RenderLayer::Content).glyphs()[0].glyph_id,
        13
    );
}

fn block(block_index: usize, z_order: u32, glyph_id: u16) -> BlockDrawGroup {
    let mut block = BlockDrawGroup::new(
        block_index,
        z_order,
        Some(ClipRect::new(1.0, 2.0, 3.0, 4.0)),
    );
    block.extend_glyphs(RenderLayer::Content, vec![glyph(glyph_id)]);
    block
}
