use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bumpalo::Bump;

use super::Store;
use crate::font::{FontDiscovery, FreeTypeRasterizer};
use crate::io::{Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, SceneFrame};
use crate::layout::prepare_tree::prepare_tree;
use crate::layout::tree::{
    BlockNode, BlockStyle, DocumentTree, FlowDirection, InlineNode, ParagraphNode, ParagraphStyle,
    StackNode, TextRun, TextStyle,
};
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

#[test]
fn tree_resize_reuses_prepared_tree_cache() {
    let delegate = Arc::new(TreeTestStoreDelegate {
        bootstrap_count: AtomicUsize::new(0),
    });
    let mut store = Store::new(
        build_rasterizer_for_perf_test(),
        ViewportState::new(960, 640, 1.0, 0, None),
        delegate.clone(),
    );
    let scene_config = SceneConfig::default();

    let first = store.compose_scene_buffer(Bump::with_capacity(4096), false, &scene_config);
    assert!(
        !first.blocks().is_empty(),
        "tree path must materialize at least one block"
    );
    assert_eq!(delegate.bootstrap_count.load(Ordering::Relaxed), 1);

    let outcome = store.handle_action(Action::Resize {
        width: 1280,
        height: 720,
        scale_factor: 1.0,
        viewport_revision: 2,
        event_time: Instant::now(),
    });
    assert!(matches!(
        outcome,
        super::ActionOutcome::Compose {
            clear_tessellation_cache: false
        }
    ));

    let second = store.compose_scene_buffer(Bump::with_capacity(4096), false, &scene_config);
    assert!(
        !second.blocks().is_empty(),
        "tree path must still materialize after resize"
    );
    assert_eq!(
        delegate.bootstrap_count.load(Ordering::Relaxed),
        1,
        "resize must reuse the prepared tree instead of rebuilding it"
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

struct TreeTestStoreDelegate {
    bootstrap_count: AtomicUsize,
}

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

impl StoreDelegate for TreeTestStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        self.bootstrap_count.fetch_add(1, Ordering::Relaxed);
        let tree = build_tree_test_document();
        let prepared_tree = prepare_tree(&tree, rasterizer);
        StoreBootstrap::new_tree(tree, prepared_tree)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
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

fn build_tree_test_document() -> DocumentTree {
    let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
    let paragraph = ParagraphNode::new(
        vec![InlineNode::Text(TextRun::new(
            "tree layout path for runtime resize testing",
            style,
        ))],
        ParagraphStyle {
            block: BlockStyle {
                padding: crate::layout::tree::Edges::all(12.0).expect("padding must be valid"),
                background: Some([0.12, 0.16, 0.22, 1.0]),
                ..BlockStyle::default()
            },
            ..ParagraphStyle::default()
        },
    )
    .expect("paragraph must be valid");
    DocumentTree::new(BlockNode::Stack(
        StackNode::new(
            FlowDirection::Vertical,
            vec![BlockNode::Paragraph(paragraph)],
            BlockStyle::default(),
        )
        .expect("stack must be valid"),
    ))
    .expect("tree must be valid")
}
