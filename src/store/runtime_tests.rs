use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bumpalo::Bump;
use tokio::runtime::Runtime;

use super::{compose_and_emit_with_post_clamp, Store};
use crate::font::{FontDiscovery, FreeTypeRasterizer};
use crate::io::{
    Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    MouseScroll, SceneFrame, ViewUpdate,
};
use crate::layout::prepare_tree::prepare_tree;
use crate::layout::tree::{
    BlockNode, BlockStyle, DocumentTree, FlowDirection, InlineNode, ParagraphNode, ParagraphStyle,
    StackNode, TextRun, TextStyle,
};
use crate::renderer::subpixel::detect_subpixel_layout;
use crate::scene::{SceneBufferPool, SceneConfig};
use crate::store::{
    types::{InputFilter, InteractionConfig, InteractionState, StorePhase},
    Model, StoreBootstrap, StoreDelegate, ViewportState,
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
    let _ = compose_and_record_state(&mut store);
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
fn configured_scroll_input_updates_offset_and_requests_compose() {
    let mut store = build_store_with_delegate(Arc::new(ConfiguredTestStoreDelegate));
    let content_extent = compose_and_record_state(&mut store);
    assert!(
        content_extent[1] > store.viewport.logical_size()[1],
        "test document must be scrollable"
    );

    let outcome = store.handle_action(Action::Input {
        event: InputEvent::Key(KeyEvent {
            code: KeyCode::Down,
            text: None,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
        }),
    });

    assert!(matches!(
        outcome,
        super::ActionOutcome::Compose {
            clear_tessellation_cache: false
        }
    ));
    assert_eq!(store.interaction.scroll_offset, [0.0, 12.0]);
}

#[test]
fn mouse_scroll_uses_normalized_delta_signs() {
    let mut store = build_store_for_test();
    let _ = compose_and_record_state(&mut store);
    store.interaction.scroll_offset = [0.0, 120.0];

    let down_outcome = store.handle_action(Action::Input {
        event: InputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            logical_position: None,
            scroll_delta: Some(MouseScroll::LineDelta { x: 0.0, y: -3.0 }),
            modifiers: KeyModifiers::NONE,
            event_time: Instant::now(),
        }),
    });
    assert!(matches!(
        down_outcome,
        super::ActionOutcome::Compose {
            clear_tessellation_cache: false
        }
    ));
    assert_eq!(store.interaction.scroll_offset, [0.0, 240.0]);

    let up_outcome = store.handle_action(Action::Input {
        event: InputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            logical_position: None,
            scroll_delta: Some(MouseScroll::LineDelta { x: 0.0, y: 1.0 }),
            modifiers: KeyModifiers::NONE,
            event_time: Instant::now(),
        }),
    });
    assert!(matches!(
        up_outcome,
        super::ActionOutcome::Compose {
            clear_tessellation_cache: false
        }
    ));
    assert_eq!(store.interaction.scroll_offset, [0.0, 200.0]);
}

#[test]
fn veto_filter_skips_default_scroll_mapping() {
    let mut store = build_store_with_delegate(Arc::new(VetoDownStoreDelegate));
    let _ = compose_and_record_state(&mut store);

    let outcome = store.handle_action(Action::Input {
        event: InputEvent::Key(KeyEvent {
            code: KeyCode::Down,
            text: None,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
        }),
    });

    assert!(matches!(outcome, super::ActionOutcome::NoChange));
    assert_eq!(store.interaction.scroll_offset, [0.0, 0.0]);
}

#[test]
fn resize_pre_clamps_scroll_offset_against_known_extent() {
    let mut store = build_store_for_test();
    store.interaction.last_known_content_extent = [960.0, 1_000.0];
    store.interaction.scroll_offset = [0.0, 400.0];

    let outcome = store.handle_action(Action::Resize {
        width: 960,
        height: 900,
        scale_factor: 1.0,
        viewport_revision: 1,
        event_time: Instant::now(),
    });

    assert!(matches!(
        outcome,
        super::ActionOutcome::Compose {
            clear_tessellation_cache: false
        }
    ));
    assert_eq!(store.interaction.scroll_offset, [0.0, 100.0]);
}

#[test]
fn post_compose_clamp_triggers_one_recompose_after_reflow_shrinks_content() {
    let mut store = build_store_with_viewport(
        Arc::new(ReflowingResizeStoreDelegate),
        ViewportState::new(320, 240, 1.0, 0, None),
    );
    let initial_extent = compose_and_record_state(&mut store);
    let initial_viewport = store.viewport.logical_size();
    let old_max_scroll = initial_extent[1] - initial_viewport[1];
    assert!(
        old_max_scroll > 0.0,
        "initial wrapping document must be scrollable"
    );

    store.interaction.scroll_offset = [0.0, old_max_scroll];

    let outcome = store.handle_action(Action::Resize {
        width: 960,
        height: 240,
        scale_factor: 1.0,
        viewport_revision: 1,
        event_time: Instant::now(),
    });
    assert!(matches!(
        outcome,
        super::ActionOutcome::Compose {
            clear_tessellation_cache: false
        }
    ));

    let (mut view_update_rx, mut pool) = build_pool_for_test();
    let completed = Runtime::new()
        .expect("tokio runtime must build")
        .block_on(async { compose_and_emit_with_post_clamp(&mut store, &mut pool, false).await });

    assert!(completed);
    assert_eq!(
        scene_update_count(&drain_view_updates(&mut view_update_rx)),
        2
    );
    assert!(
        store.interaction.scroll_offset[1] < old_max_scroll,
        "post-compose clamp must shrink the old scroll offset"
    );
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
    let scene_block_count = scene_buffer.scene_buffer.blocks().len();
    let atlas_update = store.logical_atlas.take_pending_update();
    let _scene_frame = SceneFrame::new(Box::new(scene_buffer.scene_buffer));
    let emit_elapsed = emit_started.elapsed();

    let total_compute_us = recompute_elapsed.as_micros() + emit_elapsed.as_micros();
    let atlas_update_count = usize::from(atlas_update.is_some());
    let atlas_patch_count = atlas_update.map(|update| update.patches.len()).unwrap_or(0);

    println!(
        "perf.store bootstrap_us={} bootstrap_blocks={} bootstrap_atlas_patches={} resize_recompute_us={} emit_updates_us={} total_compute_us={} atlas_updates={} atlas_patches={} scene_blocks={}",
        bootstrap_elapsed.as_micros(),
        bootstrap_scene.scene_buffer.blocks().len(),
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
        !first.scene_buffer.blocks().is_empty(),
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
        !second.scene_buffer.blocks().is_empty(),
        "tree path must still materialize after resize"
    );
    assert_eq!(
        delegate.bootstrap_count.load(Ordering::Relaxed),
        1,
        "resize must reuse the prepared tree instead of rebuilding it"
    );
}

fn build_store_for_test() -> Store {
    build_store_with_delegate(Arc::new(TestStoreDelegate))
}

struct TestStoreDelegate;
struct ConfiguredTestStoreDelegate;
struct ReflowingResizeStoreDelegate;
struct VetoDownStoreDelegate;

struct TreeTestStoreDelegate {
    bootstrap_count: AtomicUsize,
}

impl StoreDelegate for TestStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let tree = build_tree_test_document();
        let prepared_tree = prepare_tree(&tree, rasterizer);
        StoreBootstrap::new(tree, prepared_tree)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
}

impl StoreDelegate for ConfiguredTestStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let tree = build_tree_test_document();
        let prepared_tree = prepare_tree(&tree, rasterizer);
        StoreBootstrap::new(tree, prepared_tree)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}

    fn interaction_config(&self) -> InteractionConfig {
        InteractionConfig {
            line_step_px: 12.0,
            ..InteractionConfig::default()
        }
    }
}

impl StoreDelegate for ReflowingResizeStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let tree = build_reflowing_test_document();
        let prepared_tree = prepare_tree(&tree, rasterizer);
        StoreBootstrap::new(tree, prepared_tree)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
}

impl StoreDelegate for VetoDownStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let tree = build_tree_test_document();
        let prepared_tree = prepare_tree(&tree, rasterizer);
        StoreBootstrap::new(tree, prepared_tree)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}

    fn filter_input(&self, _state: &InteractionState, event: &InputEvent) -> InputFilter {
        if matches!(
            event,
            InputEvent::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            })
        ) {
            InputFilter::VetoDefault
        } else {
            InputFilter::RunDefault
        }
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
        StoreBootstrap::new(tree, prepared_tree)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
}

fn build_tree_test_document() -> DocumentTree {
    let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
    let mut children = Vec::new();
    for index in 0..24 {
        let paragraph = ParagraphNode::new(
            vec![InlineNode::Text(TextRun::new(
                format!("tree layout path for runtime scroll testing paragraph {index}"),
                style,
            ))],
            ParagraphStyle {
                block: BlockStyle {
                    padding: crate::layout::tree::Edges::all(12.0).expect("padding must be valid"),
                    margin: crate::layout::tree::Edges::new(0.0, 0.0, 0.0, 12.0)
                        .expect("margin must be valid"),
                    background: Some([0.12, 0.16, 0.22, 1.0]),
                    ..BlockStyle::default()
                },
                ..ParagraphStyle::default()
            },
        )
        .expect("paragraph must be valid");
        children.push(BlockNode::Paragraph(paragraph));
    }

    DocumentTree::new(BlockNode::Stack(
        StackNode::new(FlowDirection::Vertical, children, BlockStyle::default())
            .expect("stack must be valid"),
    ))
    .expect("tree must be valid")
}

fn build_reflowing_test_document() -> DocumentTree {
    let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
    let long_text = "runtime post clamp recompose coverage requires a paragraph that wraps aggressively when the viewport is narrow and becomes much shorter when the viewport grows wider. ".repeat(32);
    let paragraph = ParagraphNode::new(
        vec![InlineNode::Text(TextRun::new(long_text, style))],
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

fn build_store_with_delegate(delegate: Arc<dyn StoreDelegate>) -> Store {
    build_store_with_viewport(delegate, ViewportState::new(960, 640, 1.0, 0, None))
}

fn build_store_with_viewport(delegate: Arc<dyn StoreDelegate>, viewport: ViewportState) -> Store {
    Store::new(build_rasterizer_for_perf_test(), viewport, delegate)
}

fn compose_and_record_state(store: &mut Store) -> [f32; 2] {
    let compose_result =
        store.compose_scene_buffer(Bump::with_capacity(4096), false, &SceneConfig::default());
    store.interaction.last_known_viewport = store.viewport.logical_size();
    store.interaction.last_known_content_extent = compose_result.content_extent;
    let _ = store.logical_atlas.take_pending_update();
    compose_result.content_extent
}

fn scene_update_count(updates: &[ViewUpdate]) -> usize {
    updates
        .iter()
        .filter(|update| matches!(update, ViewUpdate::Scene(_)))
        .count()
}

fn drain_view_updates(
    view_update_rx: &mut tokio::sync::mpsc::Receiver<ViewUpdate>,
) -> Vec<ViewUpdate> {
    let mut updates = Vec::new();
    while let Ok(update) = view_update_rx.try_recv() {
        updates.push(update);
    }
    updates
}

fn build_pool_for_test() -> (tokio::sync::mpsc::Receiver<ViewUpdate>, SceneBufferPool) {
    let (pool, view_update_rx) = SceneBufferPool::new_for_test(SceneConfig::default());
    (view_update_rx, pool)
}
