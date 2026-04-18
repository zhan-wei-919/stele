use std::sync::Arc;
use std::time::{Duration, Instant};

use bumpalo::Bump;

use super::Store;
use crate::font::{FontDiscovery, FreeTypeRasterizer};
use crate::io::{Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, SceneFrame};
use crate::layout::{prepare_document, Block, BlockRect, Document};
use crate::renderer::subpixel::detect_subpixel_layout;
use crate::scene::SceneConfig;
use crate::store::{
    types::StorePhase, BlockDrawCommands, Model, StoreBootstrap, StoreDelegate, ViewportState,
};
use crate::test_support::log_capture::LogCapture;

fn build_rasterizer_for_perf_test() -> FreeTypeRasterizer {
    let font_discovery = FontDiscovery::new().expect("failed to discover system fonts");
    FreeTypeRasterizer::new(font_discovery, detect_subpixel_layout())
        .expect("failed to initialize FreeType rasterizer")
}

#[test]
fn typed_input_actions_do_not_trigger_scene_recompute() {
    let mut store = build_store_for_test();
    let outcome = store.handle_action(Action::Input {
        event: InputEvent::Key(KeyEvent {
            code: KeyCode::Character(String::from("a")),
            text: Some(String::from("a")),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
        }),
    });

    assert!(matches!(outcome, super::ActionOutcome::NoChange));
    assert!(store.logical_atlas.take_pending_update().is_none());
    assert_eq!(store.phase, StorePhase::Idle);
}

#[test]
#[ignore = "manual perf smoke test"]
fn reports_resize_recompute_and_diff_perf() {
    let mut store = build_store_for_test();
    let scene_config = SceneConfig::default();

    let bootstrap_started = Instant::now();
    let bootstrap_scene =
        store.compose_scene_buffer(Bump::with_capacity(4096), false, &scene_config);
    let bootstrap_elapsed = bootstrap_started.elapsed();
    let bootstrap_atlas_patches = store
        .logical_atlas
        .take_pending_update()
        .map(|update| update.patches.len())
        .unwrap_or(0);

    let recompute_started = Instant::now();
    let outcome = store.handle_action(Action::Resize {
        width: 1440,
        height: 900,
        scale_factor: 1.0,
        viewport_revision: 1,
        event_time: Instant::now(),
    });
    let recompute_elapsed = recompute_started.elapsed();
    assert!(matches!(
        outcome,
        super::ActionOutcome::Compose {
            clear_tessellation_cache: false
        }
    ));

    let emit_started = Instant::now();
    let scene_buffer = store.compose_scene_buffer(Bump::with_capacity(4096), false, &scene_config);
    let scene_block_count = scene_buffer.blocks().len();
    let atlas_update = store.logical_atlas.take_pending_update();
    let _scene_frame = SceneFrame::new(Box::new(scene_buffer));
    let emit_elapsed = emit_started.elapsed();

    let total_compute_us = recompute_elapsed.as_micros() + emit_elapsed.as_micros();
    let atlas_update_count = usize::from(atlas_update.is_some());
    let atlas_patch_count = atlas_update.map(|update| update.patches.len()).unwrap_or(0);

    println!(
        "perf.store bootstrap_us={} bootstrap_blocks={} bootstrap_atlas_patches={} resize_recompute_us={} emit_updates_us={} total_compute_us={} atlas_updates={} atlas_patches={} scene_blocks={}",
        bootstrap_elapsed.as_micros(),
        bootstrap_scene.blocks().len(),
        bootstrap_atlas_patches,
        recompute_elapsed.as_micros(),
        emit_elapsed.as_micros(),
        total_compute_us,
        atlas_update_count,
        atlas_patch_count,
        scene_block_count,
    );
}

#[test]
fn compose_budget_warns_for_sub_millisecond_overrun() {
    let mut store = build_store_for_test();
    store.compose_test_delay = Duration::from_micros(1_500);
    let scene_config = SceneConfig {
        compose_budget_ms: 1,
        ..SceneConfig::default()
    };
    let capture = LogCapture::begin();

    let _scene = store.compose_scene_buffer(Bump::with_capacity(4096), false, &scene_config);

    assert!(
        capture.contains("scene.budget_exceeded phase=compose elapsed_us="),
        "expected compose budget warning, got {:?}",
        capture.entries()
    );
}

fn build_store_for_test() -> Store {
    Store::new(
        build_rasterizer_for_perf_test(),
        ViewportState::new(960, 640, 1.0, 0, None),
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
