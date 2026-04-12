use winit::dpi::PhysicalSize;

use super::support::{build_app, sample_batch, sample_patch};
use crate::event::handlers::ViewportUpdate;
use crate::event::RouteAction;
use crate::io::{AtlasUpdate, SceneFrame, ScenePayload, ViewUpdate};
use crate::scene::BlockId;

#[test]
fn wake_applies_atlas_update_and_scene_frame_and_rebuilds_once() {
    let mut harness = build_app();
    harness
        .view_update_tx
        .send(ViewUpdate::Atlas({
            let mut atlas_update = AtlasUpdate::new(0);
            atlas_update.requested_atlas_size = Some(4096);
            atlas_update.patches.push(sample_patch());
            atlas_update
        }))
        .expect("atlas update send must succeed");
    harness
        .view_update_tx
        .send(ViewUpdate::Scene({
            let mut scene_frame = SceneFrame::new(
                1,
                Some(0),
                ScenePayload::ReplaceAll {
                    block_order: vec![BlockId::new(7)],
                    block_batches: vec![(BlockId::new(7), sample_batch(99))],
                },
            );
            scene_frame.clear_tessellation_cache = true;
            scene_frame
        }))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert_eq!(harness.app.view_state.blocks().len(), 1);
    assert_eq!(harness.app.view_state.block_order(), &[BlockId::new(7)]);
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 1);
    assert_eq!(harness.app.view_state.ready_atlas_generation(), Some(0));
    let renderer_log = harness.renderer_log.lock().expect("renderer log must lock");
    assert_eq!(renderer_log.recreate_atlas_sizes, vec![4096]);
    assert_eq!(renderer_log.clear_tessellation_calls, 1);
    assert_eq!(renderer_log.atlas_patch_writes, 1);
    assert_eq!(renderer_log.rebuild_block_counts, vec![1]);
    drop(renderer_log);
    assert_eq!(
        harness
            .window_log
            .lock()
            .expect("window log must lock")
            .redraw_requests,
        1
    );
}

#[test]
fn stale_scene_frame_is_dropped_before_apply() {
    let mut harness = build_app();
    harness.app.view_state.set_requested_viewport_revision(2);
    harness
        .view_update_tx
        .send(ViewUpdate::Scene(SceneFrame::new(
            1,
            None,
            ScenePayload::ReplaceAll {
                block_order: vec![BlockId::new(7)],
                block_batches: vec![(BlockId::new(7), sample_batch(99))],
            },
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.view_state.blocks().is_empty());
    assert!(harness
        .renderer_log
        .lock()
        .expect("renderer log must lock")
        .rebuild_block_counts
        .is_empty());
}

#[test]
fn stale_scene_frame_is_dropped_after_newer_resize_arrives() {
    let mut harness = build_app();
    harness
        .app
        .apply_route_action(RouteAction::Resize(ViewportUpdate {
            size: PhysicalSize::new(1024, 768),
            scale_factor: 2.0,
            viewport_revision: 2,
        }));

    harness
        .view_update_tx
        .send(ViewUpdate::Scene(SceneFrame::new(
            1,
            None,
            ScenePayload::ReplaceAll {
                block_order: vec![BlockId::new(7)],
                block_batches: vec![(BlockId::new(7), sample_batch(99))],
            },
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.view_state.blocks().is_empty());
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 0);
    assert_eq!(harness.app.view_state.requested_viewport_revision(), 2);
    assert!(harness
        .renderer_log
        .lock()
        .expect("renderer log must lock")
        .rebuild_block_counts
        .is_empty());
}

#[test]
fn newer_viewport_revision_replace_all_clears_old_scene_before_apply() {
    let mut harness = build_app();
    harness.app.view_state.set_block_order(vec![BlockId::new(7)]);
    harness
        .app
        .view_state
        .replace_block(BlockId::new(7), sample_batch(99));
    harness.app.view_state.set_applied_viewport_revision(1);
    harness.app.view_state.set_requested_viewport_revision(2);

    harness
        .view_update_tx
        .send(ViewUpdate::Scene(SceneFrame::new(
            2,
            None,
            ScenePayload::ReplaceAll {
                block_order: Vec::new(),
                block_batches: Vec::new(),
            },
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.view_state.blocks().is_empty());
    assert!(harness.app.view_state.block_order().is_empty());
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 2);
    assert_eq!(harness.app.view_state.requested_viewport_revision(), 2);
    assert_eq!(
        harness
            .renderer_log
            .lock()
            .expect("renderer log must lock")
            .rebuild_block_counts,
        vec![0]
    );
}

#[test]
fn scene_frame_waits_for_required_atlas_generation_before_apply() {
    let mut harness = build_app();
    harness
        .app
        .apply_route_action(RouteAction::Resize(ViewportUpdate {
            size: PhysicalSize::new(1024, 768),
            scale_factor: 2.0,
            viewport_revision: 1,
        }));

    harness
        .view_update_tx
        .send(ViewUpdate::Scene(SceneFrame::new(
            1,
            Some(1),
            ScenePayload::ReplaceAll {
                block_order: vec![BlockId::new(9)],
                block_batches: vec![(BlockId::new(9), sample_batch(90))],
            },
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.view_state.blocks().is_empty());
    assert!(harness.app.view_state.pending_scene_frame().is_some());
    assert!(harness
        .renderer_log
        .lock()
        .expect("renderer log must lock")
        .rebuild_block_counts
        .is_empty());

    harness
        .view_update_tx
        .send(ViewUpdate::Atlas({
            let mut atlas_update = AtlasUpdate::new(1);
            atlas_update.patches.push(sample_patch());
            atlas_update
        }))
        .expect("atlas update send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert_eq!(harness.app.view_state.block_order(), &[BlockId::new(9)]);
    assert_eq!(harness.app.view_state.blocks().len(), 1);
    assert!(harness.app.view_state.blocks().contains_key(&BlockId::new(9)));
    assert_eq!(harness.app.view_state.requested_viewport_revision(), 1);
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 1);
    assert!(harness.app.view_state.pending_scene_frame().is_none());
    assert_eq!(
        harness
            .renderer_log
            .lock()
            .expect("renderer log must lock")
            .rebuild_block_counts,
        vec![1]
    );
}
