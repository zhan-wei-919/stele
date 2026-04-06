//! Renderer-owned block scene mutations.

#[cfg(test)]
mod tests;

use super::types::BlockDrawGroup;

/// Whole-scene update applied to the renderer-owned block draw list.
#[derive(Clone, Debug)]
pub enum DrawListOp {
    SetBlocks(Vec<BlockDrawGroup>),
}

/// Renderer input state assembled as block-aware draw groups.
#[derive(Clone, Debug, Default)]
pub struct DrawList {
    blocks: Vec<BlockDrawGroup>,
}

impl DrawList {
    /// Creates an empty draw list with no block draw groups.
    ///
    /// This convenience constructor is currently only used by the unit tests.
    /// Production code builds the renderer-owned draw list through Default plus
    /// whole-scene updates, so cargo run reports this helper as unused for now.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies whole-scene block updates in-order.
    pub fn apply_ops<I>(&mut self, ops: I)
    where
        I: IntoIterator<Item = DrawListOp>,
    {
        for op in ops {
            match op {
                DrawListOp::SetBlocks(blocks) => {
                    self.blocks = blocks;
                    if self.blocks.len() > 1 {
                        self.blocks
                            .sort_by_key(|group| (group.z_order(), group.block_index()));
                    }
                }
            }
        }
    }

    /// Returns the current block scene in stable draw order.
    pub(crate) fn block_groups(&self) -> &[BlockDrawGroup] {
        &self.blocks
    }
}
