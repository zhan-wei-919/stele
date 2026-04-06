//! Unit tests for draw-list scene mutation semantics.

use std::sync::Arc;

use super::{DrawList, DrawListOp};
use crate::font::SubpixelBin;
use crate::renderer::draw_list::{
    BlockDrawGroup, ClipRect, ImageCmd, ImageData, PathCmd, PathVerb, PositionedGlyph, RectCmd,
    RenderLayer,
};

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
fn apply_ops_handles_insert_replace_and_remove() {
    let mut draw_list = DrawList::new();
    draw_list.apply_ops([DrawListOp::Insert {
        line_index: 0,
        glyphs: vec![glyph(1)],
    }]);
    assert_eq!(
        draw_list.block_groups(ClipRect::new(0.0, 0.0, 10.0, 10.0))[0]
            .layer(RenderLayer::Content)
            .glyphs()[0]
            .glyph_id,
        1
    );

    draw_list.apply_ops([DrawListOp::Replace {
        line_index: 0,
        glyphs: vec![glyph(2)],
    }]);
    assert_eq!(
        draw_list.block_groups(ClipRect::new(0.0, 0.0, 10.0, 10.0))[0]
            .layer(RenderLayer::Content)
            .glyphs()[0]
            .glyph_id,
        2
    );

    draw_list.apply_ops([DrawListOp::Remove { line_index: 0 }]);
    assert!(draw_list
        .block_groups(ClipRect::new(0.0, 0.0, 10.0, 10.0))
        .is_empty());
}

#[test]
fn apply_ops_replaces_rects_paths_and_images() {
    let mut draw_list = DrawList::new();
    let image = Arc::new(ImageData::new(vec![255, 0, 0, 255], 1, 1));

    draw_list.apply_ops([
        DrawListOp::SetRects(vec![RectCmd::new(
            [1.0, 2.0],
            [3.0, 4.0],
            [0.1, 0.2, 0.3, 1.0],
            RenderLayer::Background,
        )]),
        DrawListOp::SetPaths(vec![PathCmd::new(
            vec![
                PathVerb::MoveTo { to: [0.0, 0.0] },
                PathVerb::LineTo { to: [10.0, 10.0] },
            ],
            Some([0.4, 0.5, 0.6, 1.0]),
            None,
            RenderLayer::Content,
        )]),
        DrawListOp::SetImages(vec![ImageCmd::new(
            [5.0, 6.0],
            [7.0, 8.0],
            image,
            RenderLayer::Overlay,
        )]),
    ]);

    let root = &draw_list.block_groups(ClipRect::new(0.0, 0.0, 10.0, 10.0))[0];
    assert_eq!(root.layer(RenderLayer::Background).rects().len(), 1);
    assert_eq!(root.layer(RenderLayer::Content).paths().len(), 1);
    assert_eq!(root.layer(RenderLayer::Overlay).images().len(), 1);
}

#[test]
fn apply_ops_replaces_explicit_block_groups() {
    let mut draw_list = DrawList::new();
    let mut block = BlockDrawGroup::new(3, 1, Some(ClipRect::new(1.0, 2.0, 3.0, 4.0)));
    block.extend_glyphs(RenderLayer::Content, vec![glyph(9)]);

    draw_list.apply_ops([DrawListOp::SetBlocks(vec![block])]);

    let groups = draw_list.block_groups(ClipRect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].block_index(), 3);
    assert_eq!(
        groups[0].layer(RenderLayer::Content).glyphs()[0].glyph_id,
        9
    );
}

#[test]
fn set_blocks_discards_previous_legacy_scene() {
    let mut draw_list = DrawList::new();
    draw_list.apply_ops([DrawListOp::Insert {
        line_index: 0,
        glyphs: vec![glyph(1)],
    }]);

    let mut block = BlockDrawGroup::new(3, 1, Some(ClipRect::new(1.0, 2.0, 3.0, 4.0)));
    block.extend_glyphs(RenderLayer::Content, vec![glyph(9)]);
    draw_list.apply_ops([DrawListOp::SetBlocks(vec![block])]);

    let groups = draw_list.block_groups(ClipRect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].block_index(), 3);
    assert_eq!(
        groups[0].layer(RenderLayer::Content).glyphs()[0].glyph_id,
        9
    );
}

#[test]
fn legacy_ops_after_set_blocks_switch_back_to_legacy_scene() {
    let mut draw_list = DrawList::new();
    let mut block = BlockDrawGroup::new(3, 1, Some(ClipRect::new(1.0, 2.0, 3.0, 4.0)));
    block.extend_glyphs(RenderLayer::Content, vec![glyph(9)]);
    draw_list.apply_ops([DrawListOp::SetBlocks(vec![block])]);

    draw_list.apply_ops([DrawListOp::Insert {
        line_index: 0,
        glyphs: vec![glyph(4)],
    }]);

    let groups = draw_list.block_groups(ClipRect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].block_index(), 0);
    assert_eq!(
        groups[0].layer(RenderLayer::Content).glyphs()[0].glyph_id,
        4
    );
}
