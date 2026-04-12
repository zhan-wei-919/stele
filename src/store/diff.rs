//! Snapshot diffing from full store state to SceneDiff payloads.

use crate::io::{AtlasPatch, BlockOp, SceneDiff};

use super::types::SceneSnapshot;

/// Computes the incremental SceneDiff between the old and new full snapshots.
pub(crate) fn diff_snapshots(
    old: &SceneSnapshot,
    new: &SceneSnapshot,
    requested_atlas_size: Option<u32>,
    atlas_patches: Vec<AtlasPatch>,
    clear_tessellation_cache: bool,
    force_full_snapshot: bool,
) -> SceneDiff {
    let mut diff = SceneDiff::new(new.viewport_revision);
    diff.requested_atlas_size = requested_atlas_size;
    diff.atlas_patches = atlas_patches;
    diff.clear_tessellation_cache = clear_tessellation_cache;

    if force_full_snapshot {
        diff.block_order = Some(new.order.clone());
        diff.block_ops.extend(new.order.iter().filter_map(|block_id| {
            new.blocks.get(block_id).cloned().map(|batch| BlockOp::Replace {
                block_id: *block_id,
                batch,
            })
        }));
        return diff;
    }

    if old.order != new.order {
        diff.block_order = Some(new.order.clone());
    }

    for block_id in &new.order {
        match (old.blocks.get(block_id), new.blocks.get(block_id)) {
            (None, Some(batch)) => diff.block_ops.push(BlockOp::Replace {
                block_id: *block_id,
                batch: batch.clone(),
            }),
            (Some(old_batch), Some(new_batch))
                if old_batch.fingerprint() != new_batch.fingerprint() =>
            {
                diff.block_ops.push(BlockOp::Replace {
                    block_id: *block_id,
                    batch: new_batch.clone(),
                });
            }
            _ => {}
        }
    }

    for block_id in &old.order {
        if !new.blocks.contains_key(block_id) {
            diff.block_ops.push(BlockOp::Remove {
                block_id: *block_id,
            });
        }
    }

    diff
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::diff_snapshots;
    use crate::draw_list::ClipRect;
    use crate::scene::{BlockId, BlockSceneBatch};
    use crate::store::types::SceneSnapshot;

    #[test]
    fn stable_block_ids_keep_existing_blocks_mapped_across_insertion() {
        let old = snapshot(1, &[(10, 10), (20, 20)]);
        let new = snapshot(2, &[(99, 99), (10, 10), (20, 20)]);

        let diff = diff_snapshots(&old, &new, None, Vec::new(), false, false);

        assert_eq!(diff.viewport_revision, 2);
        assert_eq!(
            diff.block_order,
            Some(vec![BlockId::new(99), BlockId::new(10), BlockId::new(20)])
        );
        let replaced = diff
            .block_ops
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
            diff.block_ops
                .iter()
                .filter(|op| matches!(op, crate::io::BlockOp::Remove { .. }))
                .count(),
            0
        );
    }

    #[test]
    fn viewport_revision_change_forces_full_snapshot_even_when_fingerprints_match() {
        let old = snapshot(0, &[(10, 10), (20, 20)]);
        let new = snapshot(1, &[(10, 10), (20, 20)]);

        let diff = diff_snapshots(&old, &new, None, Vec::new(), false, true);

        assert_eq!(
            diff.block_order,
            Some(vec![BlockId::new(10), BlockId::new(20)])
        );
        let replaced = diff
            .block_ops
            .iter()
            .filter_map(|op| match op {
                crate::io::BlockOp::Replace { block_id, .. } => Some(*block_id),
                crate::io::BlockOp::Remove { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(replaced, vec![BlockId::new(10), BlockId::new(20)]);
    }

    #[test]
    fn viewport_revision_change_emits_empty_block_order_for_empty_scene() {
        let old = snapshot(0, &[(10, 10)]);
        let new = snapshot(1, &[]);

        let diff = diff_snapshots(&old, &new, None, Vec::new(), false, true);

        assert_eq!(diff.block_order, Some(Vec::new()));
        assert!(diff.block_ops.is_empty());
        assert!(!diff.is_empty());
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
