// The integration test pulls selected production modules in via `#[path]` so
// it exercises the real store/runtime bridge without widening production visibility.
// Those imported modules keep their production re-exports intact, which is useful for fidelity
// but leaves many items intentionally unused inside this test crate.
#![allow(dead_code, unused_imports)]

use std::sync::Arc;
use std::time::Instant;

use tokio::runtime::Runtime;

#[path = "../src/draw_list/mod.rs"]
mod draw_list;
#[path = "../src/font/mod.rs"]
mod font;
#[path = "../src/io/mod.rs"]
mod io;
#[path = "../src/layout/mod.rs"]
mod layout;
#[path = "../src/renderer/mod.rs"]
mod renderer;
#[path = "../src/scene/mod.rs"]
mod scene;
#[path = "../src/store/mod.rs"]
mod store;
#[path = "../src/test_support/mod.rs"]
mod test_support;

use font::{FontDiscovery, FreeTypeRasterizer};
use io::{Action, InputEvent, IoHandle, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ViewUpdate};
use layout::prepare_tree::prepare_tree;
use layout::tree::{
    BlockNode, BlockStyle, DocumentTree, FlowDirection, InlineNode, ParagraphNode, ParagraphStyle,
    StackNode, TextRun, TextStyle,
};
use renderer::subpixel::detect_subpixel_layout;
use scene::{SceneBufferPool, SceneConfig};
use store::types::InteractionConfig;
use store::{run_store, Model, Store, StoreBootstrap, StoreDelegate, ViewportState};

#[test]
fn run_store_skips_compose_for_zero_net_input_batch() {
    let updates = run_store_with_actions(
        build_store_with_delegate(Arc::new(ConfiguredTestStoreDelegate)),
        vec![
            key_input_action(KeyCode::Down),
            key_input_action(KeyCode::Up),
            Action::Shutdown,
        ],
    );

    assert_eq!(scene_update_count(&updates), 1);
}

#[test]
fn run_store_drains_input_batch_before_processing_resize() {
    let updates = run_store_with_actions(
        build_store_with_delegate(Arc::new(ConfiguredTestStoreDelegate)),
        vec![
            key_input_action(KeyCode::Down),
            key_input_action(KeyCode::Down),
            key_input_action(KeyCode::Down),
            Action::Resize {
                width: 960,
                height: 640,
                scale_factor: 1.0,
                viewport_revision: 1,
                event_time: Instant::now(),
            },
            Action::Shutdown,
        ],
    );

    assert_eq!(scene_update_count(&updates), 3);
}

#[test]
fn run_store_composes_scrolled_frame_before_honoring_shutdown() {
    let updates = run_store_with_actions(
        build_store_with_delegate(Arc::new(ConfiguredTestStoreDelegate)),
        vec![key_input_action(KeyCode::Down), Action::Shutdown],
    );

    assert_eq!(scene_update_count(&updates), 2);
}

struct ConfiguredTestStoreDelegate;

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

fn build_store_with_delegate(delegate: Arc<dyn StoreDelegate>) -> Store {
    Store::new(
        build_rasterizer_for_test(),
        ViewportState::new(960, 640, 1.0, 0, None),
        delegate,
    )
}

fn build_rasterizer_for_test() -> FreeTypeRasterizer {
    let font_discovery = FontDiscovery::new().expect("failed to discover system fonts");
    FreeTypeRasterizer::new(font_discovery, detect_subpixel_layout())
        .expect("failed to initialize FreeType rasterizer")
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
                    padding: layout::tree::Edges::all(12.0).expect("padding must be valid"),
                    margin: layout::tree::Edges::new(0.0, 0.0, 0.0, 12.0)
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

fn key_input_action(code: KeyCode) -> Action {
    Action::Input {
        event: InputEvent::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
        }),
    }
}

fn run_store_with_actions(store: Store, actions: Vec<Action>) -> Vec<ViewUpdate> {
    let (action_tx, handle) = IoHandle::new_for_test();
    let (pool, mut view_update_rx) = SceneBufferPool::new_for_test(SceneConfig::default());
    for action in actions {
        action_tx
            .send(action)
            .expect("test action send must succeed");
    }
    drop(action_tx);

    Runtime::new()
        .expect("tokio runtime must build")
        .block_on(async { run_store(store, handle, pool).await });

    drain_view_updates(&mut view_update_rx)
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
