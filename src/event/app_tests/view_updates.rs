use std::time::Instant;

use winit::dpi::PhysicalSize;

use super::support::{build_app, sample_patch, sample_scene_buffer, sample_scene_frame};
use crate::event::handlers::ViewportUpdate;
use crate::event::RouteAction;
use crate::io::{AtlasUpdate, ViewUpdate};
use crate::scene::BlockId;

#[test]
fn wake_applies_atlas_update_and_scene_frame_and_rebuilds_once() {
    let mut harness = build_app();
    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Atlas({
            let mut atlas_update = AtlasUpdate::new(0);
            atlas_update.requested_atlas_size = Some(4096);
            atlas_update.patches.push(sample_patch());
            atlas_update
        }))
        .expect("atlas update send must succeed");
    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Scene(sample_scene_frame(
            1,
            Some(0),
            &[7],
            true,
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    let current = harness
        .app
        .current_scene_buffer
        .as_ref()
        .expect("current scene buffer must exist");
    assert_eq!(current.blocks().len(), 1);
    assert_eq!(current.order(), &[BlockId::new(7)]);
    assert_eq!(harness.app.scene_protocol.applied_viewport_revision(), 1);
    assert_eq!(harness.app.scene_protocol.ready_atlas_generation(), Some(0));
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
    harness
        .app
        .scene_protocol
        .set_requested_viewport_revision(2);
    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Scene(sample_scene_frame(1, None, &[7], false)))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.current_scene_buffer.is_none());
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
            event_time: Instant::now(),
        }));

    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Scene(sample_scene_frame(1, None, &[7], false)))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.current_scene_buffer.is_none());
    assert_eq!(harness.app.scene_protocol.applied_viewport_revision(), 0);
    assert_eq!(harness.app.scene_protocol.requested_viewport_revision(), 2);
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
    harness.app.current_scene_buffer = Some(sample_scene_buffer(1, None, &[7], false, None));
    harness.app.scene_protocol.set_applied_viewport_revision(1);
    harness
        .app
        .scene_protocol
        .set_requested_viewport_revision(2);

    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Scene(sample_scene_frame(2, None, &[], false)))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    let current = harness
        .app
        .current_scene_buffer
        .as_ref()
        .expect("current scene buffer must exist");
    assert!(current.blocks().is_empty());
    assert!(current.order().is_empty());
    assert_eq!(harness.app.scene_protocol.applied_viewport_revision(), 2);
    assert_eq!(harness.app.scene_protocol.requested_viewport_revision(), 2);
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
            event_time: Instant::now(),
        }));

    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Scene(sample_scene_frame(
            1,
            Some(1),
            &[9],
            false,
        )))
        .expect("scene frame send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    assert!(harness.app.current_scene_buffer.is_none());
    assert!(harness.app.scene_protocol.pending_scene_buffer().is_some());
    assert!(harness
        .renderer_log
        .lock()
        .expect("renderer log must lock")
        .rebuild_block_counts
        .is_empty());

    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Atlas({
            let mut atlas_update = AtlasUpdate::new(1);
            atlas_update.patches.push(sample_patch());
            atlas_update
        }))
        .expect("atlas update send must succeed");

    let should_exit = harness.app.on_wake();

    assert!(!should_exit);
    let current = harness
        .app
        .current_scene_buffer
        .as_ref()
        .expect("current scene buffer must exist");
    assert_eq!(current.order(), &[BlockId::new(9)]);
    assert_eq!(current.blocks().len(), 1);
    assert_eq!(current.blocks()[0].block_id(), BlockId::new(9));
    assert_eq!(harness.app.scene_protocol.requested_viewport_revision(), 1);
    assert_eq!(harness.app.scene_protocol.applied_viewport_revision(), 1);
    assert!(harness.app.scene_protocol.pending_scene_buffer().is_none());
    assert_eq!(
        harness
            .renderer_log
            .lock()
            .expect("renderer log must lock")
            .rebuild_block_counts,
        vec![1]
    );
}
