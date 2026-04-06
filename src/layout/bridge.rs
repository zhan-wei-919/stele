//! Bridge from layout output into renderer-owned draw groups.

use log::info;

use crate::renderer::{BlockDrawGroup, ClipRect, DrawListOp, RenderLayer};

use super::layout::LayoutBlock;

/// Converts laid-out blocks into renderer scene updates.
pub(crate) fn bridge_layout(layout_blocks: &[LayoutBlock]) -> Vec<DrawListOp> {
    let mut draw_groups = layout_blocks
        .iter()
        .map(|block| {
            let mut group = BlockDrawGroup::new(
                block.block_index,
                block.z_order,
                Some(ClipRect::new(
                    block.clip_rect.x(),
                    block.clip_rect.y(),
                    block.clip_rect.width(),
                    block.clip_rect.height(),
                )),
            );

            if let Some(rect) = block.background_rect {
                group.push_rect(rect);
            }
            for line in &block.lines {
                for run in &line.runs {
                    group.extend_glyphs(RenderLayer::Content, run.glyphs.clone());
                    for rect in &run.decoration_rects {
                        group.push_rect(*rect);
                    }
                }
            }
            group
        })
        .collect::<Vec<_>>();

    draw_groups.sort_by_key(|group| (group.z_order(), group.block_index()));
    info!("layout.bridge block_count={}", draw_groups.len());
    vec![DrawListOp::SetBlocks(draw_groups)]
}

#[cfg(test)]
mod tests {
    use super::bridge_layout;
    use crate::layout::{BlockRect, LayoutBlock, LayoutLine, LayoutRun};
    use crate::renderer::{DrawListOp, RectCmd, RenderLayer};

    #[test]
    fn bridge_sorts_blocks_by_z_order_then_document_order() {
        let ops = bridge_layout(&[
            layout_block(3, 2, 20.0),
            layout_block(1, 1, 10.0),
            layout_block(2, 1, 15.0),
        ]);

        let DrawListOp::SetBlocks(blocks) = &ops[0] else {
            panic!("bridge must emit SetBlocks");
        };
        assert_eq!(blocks[0].block_index(), 1);
        assert_eq!(blocks[1].block_index(), 2);
        assert_eq!(blocks[2].block_index(), 3);
    }

    fn layout_block(block_index: usize, z_order: u32, x: f32) -> LayoutBlock {
        LayoutBlock {
            block_index,
            z_order,
            lines: vec![LayoutLine {
                runs: vec![LayoutRun {
                    glyphs: Vec::new(),
                    decoration_rects: vec![RectCmd::new(
                        [x, 0.0],
                        [10.0, 10.0],
                        [1.0, 0.0, 0.0, 1.0],
                        RenderLayer::Background,
                    )],
                }],
                y: 0.0,
                line_height: 10.0,
                baseline: 8.0,
            }],
            background_rect: None,
            clip_rect: BlockRect::new(x, 0.0, 40.0, 20.0).expect("rect must be valid"),
        }
    }
}
