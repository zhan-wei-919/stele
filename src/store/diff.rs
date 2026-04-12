//! Snapshot diffing from full store state to scene payloads.

use crate::io::{BlockOp, ScenePayload};

use super::types::SceneSnapshot;

/// Builds a self-contained scene payload that replaces the full view-owned cache.
pub(crate) fn replace_all_snapshot(snapshot: &SceneSnapshot) -> ScenePayload {
    let block_batches = snapshot
        .order
        .iter()
        .filter_map(|block_id| {
            snapshot
                .blocks
                .get(block_id)
                .cloned()
                .map(|batch| (*block_id, batch))
        })
        .collect::<Vec<_>>();
    ScenePayload::ReplaceAll {
        block_order: snapshot.order.clone(),
        block_batches,
    }
}

/// Computes the incremental scene payload between the old and new full snapshots.
pub(crate) fn diff_snapshots(old: &SceneSnapshot, new: &SceneSnapshot) -> ScenePayload {
    let mut block_order = None;
    let mut block_ops = Vec::new();

    if old.order != new.order {
        block_order = Some(new.order.clone());
    }

    for block_id in &new.order {
        match (old.blocks.get(block_id), new.blocks.get(block_id)) {
            (None, Some(batch)) => block_ops.push(BlockOp::Replace {
                block_id: *block_id,
                batch: batch.clone(),
            }),
            (Some(old_batch), Some(new_batch))
                if old_batch.fingerprint() != new_batch.fingerprint() =>
            {
                block_ops.push(BlockOp::Replace {
                    block_id: *block_id,
                    batch: new_batch.clone(),
                });
            }
            _ => {}
        }
    }

    for block_id in &old.order {
        if !new.blocks.contains_key(block_id) {
            block_ops.push(BlockOp::Remove {
                block_id: *block_id,
            });
        }
    }

    ScenePayload::Diff {
        block_order,
        block_ops,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{diff_snapshots, replace_all_snapshot};
    use crate::draw_list::ClipRect;
    use crate::io::ScenePayload;
    use crate::scene::{BlockId, BlockSceneBatch};
    use crate::store::types::SceneSnapshot;

    #[test]
    fn stable_block_ids_keep_existing_blocks_mapped_across_insertion() {
        let old = snapshot(1, &[(10, 10), (20, 20)]);
        let new = snapshot(2, &[(99, 99), (10, 10), (20, 20)]);

        let ScenePayload::Diff {
            block_order,
            block_ops,
        } = diff_snapshots(&old, &new)
        else {
            panic!("expected diff payload");
        };

        assert_eq!(
            block_order,
            Some(vec![BlockId::new(99), BlockId::new(10), BlockId::new(20)])
        );
        let replaced = block_ops
            .iter()
            .filter_map(|op| match op {
                crate::io::BlockOp::Replace { block_id, batch } => {
                    Some((*block_id, batch.fingerprint()))
                }
                crate::io::BlockOp::Remove { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(replaced, vec![(BlockId::new(99), 99)]);
        assert_eq!(
            block_ops
                .iter()
                .filter(|op| matches!(op, crate::io::BlockOp::Remove { .. }))
                .count(),
            0
        );
    }

    #[test]
    fn replace_all_snapshot_includes_every_block_in_order() {
        let snapshot = snapshot(1, &[(10, 10), (20, 20)]);

        let ScenePayload::ReplaceAll {
            block_order,
            block_batches,
        } = replace_all_snapshot(&snapshot)
        else {
            panic!("expected replace-all payload");
        };

        assert_eq!(block_order, vec![BlockId::new(10), BlockId::new(20)]);
        let replaced = block_batches
            .iter()
            .map(|(block_id, _)| *block_id)
            .collect::<Vec<_>>();
        assert_eq!(replaced, vec![BlockId::new(10), BlockId::new(20)]);
    }

    #[test]
    fn replace_all_snapshot_keeps_empty_scene_visible() {
        let snapshot = snapshot(1, &[]);

        let ScenePayload::ReplaceAll {
            block_order,
            block_batches,
        } = replace_all_snapshot(&snapshot)
        else {
            panic!("expected replace-all payload");
        };

        assert!(block_order.is_empty());
        assert!(block_batches.is_empty());
    }

    fn snapshot(viewport_revision: u64, blocks: &[(u64, u64)]) -> SceneSnapshot {
        let order = blocks
            .iter()
            .map(|(block_id, _)| BlockId::new(*block_id))
            .collect::<Vec<_>>();
        let blocks = blocks
            .iter()
            .map(|(block_id, fingerprint)| (BlockId::new(*block_id), sample_batch(*fingerprint)))
            .collect::<HashMap<_, _>>();
        SceneSnapshot {
            viewport_revision,
            required_atlas_generation: None,
            order,
            blocks,
        }
    }

    fn sample_batch(fingerprint: u64) -> BlockSceneBatch {
        BlockSceneBatch::new(
            ClipRect::new(0.0, 0.0, 100.0, 80.0),
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            fingerprint,
        )
    }
}
