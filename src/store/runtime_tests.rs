use std::sync::Arc;
use std::time::Instant;

use super::Store;
use crate::font::{FontDiscovery, FreeTypeRasterizer};
use crate::io::{Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ViewUpdate};
use crate::layout::{prepare_document, Block, BlockRect, Document};
use crate::renderer::subpixel::detect_subpixel_layout;
use crate::store::{
    types::StorePhase, BlockDrawCommands, Model, StoreBootstrap, StoreDelegate, ViewportState,
};

fn build_rasterizer_for_perf_test() -> FreeTypeRasterizer {
    let font_discovery = FontDiscovery::new().expect("failed to discover system fonts");
    FreeTypeRasterizer::new(font_discovery, detect_subpixel_layout())
        .expect("failed to initialize FreeType rasterizer")
}

fn build_pending_updates_for_test(store: &mut Store) -> Vec<ViewUpdate> {
    let pending_scene = store
        .pending_scene
        .take()
        .expect("pending scene must exist");
    let mut updates = Vec::new();
    if let Some(atlas_update) = store.logical_atlas.take_pending_update() {
        updates.push(ViewUpdate::Atlas(atlas_update));
    }

    let payload = match pending_scene.mode {
        super::PendingSceneMode::ReplaceAll => super::replace_all_snapshot(&pending_scene.snapshot),
        super::PendingSceneMode::Diff => {
            super::diff_snapshots(&store.last_emitted_snapshot, &pending_scene.snapshot)
        }
    };
    let mut scene_frame = crate::io::SceneFrame::new(
        pending_scene.snapshot.viewport_revision,
        pending_scene.snapshot.required_atlas_generation,
        payload,
    );
    scene_frame.clear_tessellation_cache = pending_scene.clear_tessellation_cache;
    if !scene_frame.is_empty() {
        updates.push(ViewUpdate::Scene(scene_frame));
    }

    store.last_emitted_snapshot = pending_scene.snapshot;
    updates
}

#[test]
fn typed_input_actions_do_not_trigger_snapshot_recompute() {
    let mut store = build_store_for_test();

    store.bootstrap();
    let _ = build_pending_updates_for_test(&mut store);
    assert!(store.pending_scene.is_none());

    assert!(store.handle_action(Action::Input {
        event: InputEvent::Key(KeyEvent {
            code: KeyCode::Character(String::from("a")),
            text: Some(String::from("a")),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
        }),
    }));

    assert!(store.pending_scene.is_none());
    assert!(store.logical_atlas.take_pending_update().is_none());
    assert_eq!(store.phase, StorePhase::Idle);
}

#[test]
#[ignore = "manual perf smoke test"]
fn reports_resize_recompute_and_diff_perf() {
    let mut store = build_store_for_test();

    store.bootstrap();
    let _ = build_pending_updates_for_test(&mut store);

    let recompute_started = Instant::now();
    assert!(store.handle_action(Action::Resize {
        width: 1440,
        height: 900,
        scale_factor: 1.0,
        viewport_revision: 1,
    }));
    let recompute_elapsed = recompute_started.elapsed();

    let emit_started = Instant::now();
    let updates = build_pending_updates_for_test(&mut store);
    let emit_elapsed = emit_started.elapsed();

    let total_compute_us = recompute_elapsed.as_micros() + emit_elapsed.as_micros();
    let atlas_update_count = updates
        .iter()
        .filter(|update| matches!(update, ViewUpdate::Atlas(_)))
        .count();
    let atlas_patch_count = updates
        .iter()
        .filter_map(|update| match update {
            ViewUpdate::Atlas(atlas_update) => Some(atlas_update.patches.len()),
            ViewUpdate::Scene(_) => None,
        })
        .sum::<usize>();
    let scene_frame_count = updates
        .iter()
        .filter(|update| matches!(update, ViewUpdate::Scene(_)))
        .count();
    let replace_all_block_count = updates
        .iter()
        .filter_map(|update| match update {
            ViewUpdate::Scene(scene_frame) => match &scene_frame.payload {
                crate::io::ScenePayload::ReplaceAll { block_batches, .. } => {
                    Some(block_batches.len())
                }
                crate::io::ScenePayload::Diff { .. } => None,
            },
            ViewUpdate::Atlas(_) => None,
        })
        .sum::<usize>();
    let diff_op_count = updates
        .iter()
        .filter_map(|update| match update {
            ViewUpdate::Scene(scene_frame) => match &scene_frame.payload {
                crate::io::ScenePayload::Diff { block_ops, .. } => Some(block_ops.len()),
                crate::io::ScenePayload::ReplaceAll { .. } => None,
            },
            ViewUpdate::Atlas(_) => None,
        })
        .sum::<usize>();

    println!(
        "perf.store resize_recompute_us={} emit_updates_us={} total_compute_us={} atlas_updates={} atlas_patches={} scene_frames={} replace_all_blocks={} diff_ops={}",
        recompute_elapsed.as_micros(),
        emit_elapsed.as_micros(),
        total_compute_us,
        atlas_update_count,
        atlas_patch_count,
        scene_frame_count,
        replace_all_block_count,
        diff_op_count,
    );
}

fn build_store_for_test() -> Store {
    Store::new(
        build_rasterizer_for_perf_test(),
        ViewportState::new(960, 640, 1.0, 0),
        Arc::new(TestStoreDelegate),
    )
}

struct TestStoreDelegate;

impl StoreDelegate for TestStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let document = build_test_document(logical_viewport);
        let prepared_blocks = prepare_document(&document, rasterizer);
        StoreBootstrap::new(document, prepared_blocks, BlockDrawCommands::default())
    }

    fn resize(&self, model: &mut Model, logical_viewport: [f32; 2]) {
        let rect = BlockRect::new(0.0, 0.0, logical_viewport[0], logical_viewport[1])
            .expect("test viewport rect must be valid");
        model
            .document_mut()
            .set_block_rect(0, rect)
            .expect("test block must exist");
        model.set_block_draw_commands(BlockDrawCommands::default());
    }
}

fn build_test_document(logical_viewport: [f32; 2]) -> Document {
    let block = Block::new(
        BlockRect::new(0.0, 0.0, logical_viewport[0], logical_viewport[1])
            .expect("test block rect must be valid"),
        0.0,
        None,
        Vec::new(),
        0,
    )
    .expect("test block must be valid");
    Document::new(vec![block])
}
