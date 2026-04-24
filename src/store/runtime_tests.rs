use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bumpalo::Bump;
use tokio::runtime::Runtime;

use super::super::input::TextInputId;
use super::{compose_and_emit_with_post_clamp, Invalidation, Store};
use crate::font::{FontDiscovery, FreeTypeRasterizer};
use crate::io::{
    Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButtonKind, MouseEvent,
    MouseEventKind, MouseScroll, SceneFrame, ViewUpdate,
};
use crate::layout::tree::{
    Align, BlockNode, BlockStyle, BorderStyle, DocumentTree, Edges, FlowDirection, InlineNode,
    ParagraphNode, ParagraphStyle, StackNode, TextInputNode, TextInputStyle, TextRun, TextStyle,
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

fn assert_compose_invalidation(outcome: super::ActionOutcome, expected: Invalidation) {
    assert!(matches!(
        outcome,
        super::ActionOutcome::Compose { invalidation } if invalidation == expected
    ));
}

fn compose_invalidation(outcome: super::ActionOutcome) -> Invalidation {
    let super::ActionOutcome::Compose { invalidation } = outcome else {
        panic!("expected compose outcome");
    };
    invalidation
}

#[test]
fn typed_input_actions_do_not_trigger_scene_recompute() {
    let mut store = build_store_for_test();
    let _ = compose_and_record_state(&mut store);
    let outcome = store.handle_action(Action::Input {
        event: InputEvent::Key(KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
        }),
    });

    assert!(matches!(outcome, super::ActionOutcome::NoChange));
    assert_eq!(store.model.text_inputs().revision(), 0);
    assert!(store.logical_atlas.take_pending_update().is_none());
    assert_eq!(store.phase, StorePhase::Idle);
}

#[test]
fn click_text_input_then_type_updates_that_input_only() {
    let mut store = build_text_input_store();
    let _ = compose_and_record_state(&mut store);
    let first = TextInputId::new(1);
    let second = TextInputId::new(2);

    let focus_action = click_text_input_action(&store, first);
    let focus_outcome = store.handle_action(focus_action);
    assert_compose_invalidation(focus_outcome, Invalidation::RECOMPOSE);
    assert_eq!(store.interaction.focused_text_input, Some(first));

    let outcome = store.handle_action(key_input_action_with_kind(
        KeyCode::Char('a'),
        KeyEventKind::Press,
    ));

    assert_compose_invalidation(outcome, Invalidation::REPREPARE_AND_COMPOSE);
    assert_eq!(text_input_text(&store, first), "a");
    assert_eq!(text_input_text(&store, second), "");
    assert_eq!(store.interaction.scroll_offset, [0.0, 0.0]);
}

#[test]
fn clicking_second_text_input_moves_focus_without_changing_first() {
    let mut store = build_text_input_store();
    let _ = compose_and_record_state(&mut store);
    let first = TextInputId::new(1);
    let second = TextInputId::new(2);

    let first_focus = click_text_input_action(&store, first);
    let _ = store.handle_action(first_focus);
    let _ = store.handle_action(key_input_action_with_kind(
        KeyCode::Char('a'),
        KeyEventKind::Press,
    ));
    let _ = compose_and_record_state(&mut store);
    let second_focus = click_text_input_action(&store, second);
    let outcome = store.handle_action(second_focus);

    assert_compose_invalidation(outcome, Invalidation::RECOMPOSE);
    assert_eq!(store.interaction.focused_text_input, Some(second));
    assert_eq!(text_input_text(&store, first), "a");
    assert_eq!(text_input_text(&store, second), "");

    let _ = store.handle_action(key_input_action_with_kind(
        KeyCode::Char('b'),
        KeyEventKind::Press,
    ));

    assert_eq!(text_input_text(&store, first), "a");
    assert_eq!(text_input_text(&store, second), "b");
}

#[test]
fn focused_text_input_paste_writes_to_focused_state() {
    let mut store = build_text_input_store();
    let first = TextInputId::new(1);
    store.interaction.focused_text_input = Some(first);

    let outcome = store.handle_action(Action::Input {
        event: InputEvent::Paste("abc中".to_owned()),
    });

    assert_compose_invalidation(outcome, Invalidation::REPREPARE_AND_COMPOSE);
    assert_eq!(text_input_text(&store, first), "abc中");
    assert_eq!(text_input_cursor(&store, first), "abc中".len());
    assert_eq!(store.interaction.scroll_offset, [0.0, 0.0]);
}

#[test]
fn viewport_paste_without_focused_text_input_is_ignored() {
    let mut store = build_store_for_test();

    let outcome = store.handle_action(Action::Input {
        event: InputEvent::Paste("abc".to_owned()),
    });

    assert!(matches!(outcome, super::ActionOutcome::NoChange));
    assert_eq!(store.model.text_inputs().revision(), 0);
    assert_eq!(store.interaction.scroll_offset, [0.0, 0.0]);
}

#[test]
fn focused_text_input_down_key_does_not_scroll_viewport() {
    let mut store = build_text_input_store();
    prime_scroll_metrics(&mut store, [960.0, 640.0], [960.0, 2_000.0]);
    store.interaction.focused_text_input = Some(TextInputId::new(1));
    store.interaction.scroll_offset = [0.0, 120.0];

    let outcome = store.handle_action(key_input_action_with_kind(
        KeyCode::Down,
        KeyEventKind::Press,
    ));

    assert!(matches!(outcome, super::ActionOutcome::NoChange));
    assert_eq!(store.interaction.scroll_offset, [0.0, 120.0]);
    assert_eq!(text_input_text(&store, TextInputId::new(1)), "");
}

#[test]
fn focused_text_input_navigation_and_backspace_update_text_state() {
    let mut store = build_text_input_store();
    let first = TextInputId::new(1);
    store.interaction.focused_text_input = Some(first);

    for code in [
        KeyCode::Char('a'),
        KeyCode::Char('c'),
        KeyCode::Left,
        KeyCode::Char('b'),
        KeyCode::Right,
        KeyCode::Backspace,
    ] {
        let outcome = store.handle_action(key_input_action_with_kind(code, KeyEventKind::Press));
        assert_compose_invalidation(outcome, Invalidation::REPREPARE_AND_COMPOSE);
    }

    assert_eq!(text_input_text(&store, first), "ab");
    assert_eq!(text_input_cursor(&store, first), 2);
}

#[test]
fn clicking_outside_clears_focus_and_later_text_is_ignored() {
    let mut store = build_text_input_store();
    let _ = compose_and_record_state(&mut store);
    let first = TextInputId::new(1);

    let focus_action = click_text_input_action(&store, first);
    let _ = store.handle_action(focus_action);
    let outcome = store.handle_action(mouse_down_action([900.0, 620.0]));
    assert_compose_invalidation(outcome, Invalidation::RECOMPOSE);
    assert_eq!(store.interaction.focused_text_input, None);

    let ignored = store.handle_action(key_input_action_with_kind(
        KeyCode::Char('a'),
        KeyEventKind::Press,
    ));
    assert!(matches!(ignored, super::ActionOutcome::NoChange));
    assert_eq!(text_input_text(&store, first), "");
    assert_eq!(store.interaction.scroll_offset, [0.0, 0.0]);
}

#[test]
fn text_edit_reprepare_makes_new_text_visible_on_next_compose() {
    let mut store = build_text_input_store();
    let _ = compose_and_record_state(&mut store);
    let first = TextInputId::new(1);

    let focus_action = click_text_input_action(&store, first);
    let _ = store.handle_action(focus_action);
    let invalidation = compose_invalidation(store.handle_action(key_input_action_with_kind(
        KeyCode::Char('z'),
        KeyEventKind::Press,
    )));
    assert_eq!(store.reprepare_count, 0);
    let updates = compose_and_drain_updates(&mut store, invalidation);

    assert!(
        scene_updates_have_glyphs(updates),
        "fresh compose must include glyphs for edited text"
    );
    assert_eq!(store.reprepare_count, 1);
}

#[test]
fn batched_text_edits_reprepare_once_after_merge() {
    let mut store = build_text_input_store();
    let first = TextInputId::new(1);
    store.interaction.focused_text_input = Some(first);

    let first_edit = compose_invalidation(store.handle_action(key_input_action_with_kind(
        KeyCode::Char('a'),
        KeyEventKind::Press,
    )));
    let second_edit = compose_invalidation(store.handle_action(key_input_action_with_kind(
        KeyCode::Char('b'),
        KeyEventKind::Press,
    )));
    let invalidation = first_edit.merge(second_edit);

    assert_eq!(store.reprepare_count, 0);
    let _updates = compose_and_drain_updates(&mut store, invalidation);
    assert_eq!(store.reprepare_count, 1);
}

#[test]
fn startup_compose_produces_text_input_hit_targets() {
    let mut store = build_text_input_store();

    let _ = compose_and_record_state(&mut store);

    assert_eq!(store.text_input_hit_targets.len(), 2);
}

#[test]
fn pointer_focus_refreshes_hit_targets_after_text_edit_before_compose() {
    let mut store = build_dynamic_text_input_store();
    let _ = compose_and_record_state(&mut store);
    let input = TextInputId::new(1);
    let old_rect = text_input_hit_rect(&store, input);
    store.interaction.focused_text_input = Some(input);

    let mut pending_invalidation =
        compose_invalidation(
            store.handle_action(Action::Input {
                event: InputEvent::Paste(
                    "wide text that expands the intrinsic input rect before the next compose"
                        .to_owned(),
                ),
            }),
        );
    assert_eq!(pending_invalidation, Invalidation::REPREPARE_AND_COMPOSE);

    let click = mouse_down_action([
        old_rect[0] + old_rect[2] + 10.0,
        old_rect[1] + old_rect[3] * 0.5,
    ]);
    store.flush_invalidation_for_action(&mut pending_invalidation, &click);
    assert_eq!(pending_invalidation, Invalidation::RECOMPOSE);
    assert_eq!(store.reprepare_count, 1);

    let fresh_targets = store.composer.text_input_hit_targets(
        &store.layout_cache,
        store.viewport,
        store.interaction.scroll_offset,
    );
    let fresh_rect = fresh_targets
        .iter()
        .find(|target| target.text_input_id == input)
        .expect("fresh text input target must exist")
        .rect();
    assert!(
        fresh_rect[2] > old_rect[2] + 20.0,
        "test input must grow enough to expose stale hit targets"
    );

    let outcome = store.handle_action(click);

    assert!(matches!(outcome, super::ActionOutcome::NoChange));
    assert_eq!(store.interaction.focused_text_input, Some(input));
    assert_eq!(text_input_hit_rect(&store, input), fresh_rect);
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
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
        }),
    });

    assert_compose_invalidation(outcome, Invalidation::RECOMPOSE);
    assert_eq!(store.interaction.scroll_offset, [0.0, 12.0]);
}

#[test]
fn keyboard_scroll_commands_update_offset_and_clamp() {
    let mut store = build_store_with_delegate(Arc::new(ConfiguredTestStoreDelegate));
    prime_scroll_metrics(&mut store, [960.0, 640.0], [960.0, 2_000.0]);

    store.interaction.scroll_offset = [0.0, 24.0];
    assert_key_scrolls_to(&mut store, KeyCode::Up, KeyEventKind::Press, 12.0);
    assert_key_scrolls_to(&mut store, KeyCode::Down, KeyEventKind::Repeat, 24.0);

    store.interaction.scroll_offset = [0.0, 620.0];
    assert_key_scrolls_to(&mut store, KeyCode::PageUp, KeyEventKind::Repeat, 20.0);
    assert_key_scrolls_to(&mut store, KeyCode::PageDown, KeyEventKind::Press, 620.0);

    store.interaction.scroll_offset = [0.0, 700.0];
    assert_key_scrolls_to(&mut store, KeyCode::Home, KeyEventKind::Press, 0.0);
    assert_key_scrolls_to(&mut store, KeyCode::End, KeyEventKind::Repeat, 1_360.0);

    store.interaction.scroll_offset = [0.0, 4.0];
    assert_key_scrolls_to(&mut store, KeyCode::PageUp, KeyEventKind::Press, 0.0);

    store.interaction.scroll_offset = [0.0, 1_350.0];
    assert_key_scrolls_to(&mut store, KeyCode::PageDown, KeyEventKind::Press, 1_360.0);
}

#[test]
fn key_release_does_not_request_compose() {
    let mut store = build_store_for_test();
    prime_scroll_metrics(&mut store, [960.0, 640.0], [960.0, 2_000.0]);
    store.interaction.scroll_offset = [0.0, 120.0];

    let outcome = store.handle_action(key_input_action_with_kind(
        KeyCode::Down,
        KeyEventKind::Release,
    ));

    assert!(matches!(outcome, super::ActionOutcome::NoChange));
    assert_eq!(store.interaction.scroll_offset, [0.0, 120.0]);
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
    assert_compose_invalidation(down_outcome, Invalidation::RECOMPOSE);
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
    assert_compose_invalidation(up_outcome, Invalidation::RECOMPOSE);
    assert_eq!(store.interaction.scroll_offset, [0.0, 200.0]);
}

#[test]
fn mouse_pixel_scroll_uses_scaled_delta_signs() {
    let mut store = build_store_with_delegate(Arc::new(PixelScaleStoreDelegate));
    prime_scroll_metrics(&mut store, [960.0, 640.0], [960.0, 2_000.0]);
    store.interaction.scroll_offset = [0.0, 120.0];

    let down_outcome = store.handle_action(mouse_input_action(MouseScroll::PixelDelta {
        x: 0.0,
        y: -15.0,
    }));
    assert_compose_invalidation(down_outcome, Invalidation::RECOMPOSE);
    assert_eq!(store.interaction.scroll_offset, [0.0, 150.0]);

    let up_outcome = store.handle_action(mouse_input_action(MouseScroll::PixelDelta {
        x: 0.0,
        y: 20.0,
    }));
    assert_compose_invalidation(up_outcome, Invalidation::RECOMPOSE);
    assert_eq!(store.interaction.scroll_offset, [0.0, 110.0]);
}

#[test]
fn veto_filter_skips_default_scroll_mapping() {
    let mut store = build_store_with_delegate(Arc::new(VetoDownStoreDelegate));
    let _ = compose_and_record_state(&mut store);

    let outcome = store.handle_action(Action::Input {
        event: InputEvent::Key(KeyEvent {
            code: KeyCode::Down,
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

    assert_compose_invalidation(outcome, Invalidation::RECOMPOSE);
    assert_eq!(store.interaction.scroll_offset, [0.0, 100.0]);
}

#[test]
fn resize_scale_change_resets_atlas_and_requests_compose() {
    let mut store = build_store_for_test();

    let invalidation = compose_invalidation(store.handle_action(Action::Resize {
        width: 960,
        height: 640,
        scale_factor: 2.0,
        viewport_revision: 1,
        event_time: Instant::now(),
    }));

    assert_eq!(invalidation, Invalidation::RESET_ATLAS_AND_COMPOSE);
    assert_eq!(store.logical_atlas.generation, 0);
    assert_eq!(store.reprepare_count, 0);
    let updates = compose_and_drain_updates(&mut store, invalidation);
    assert_eq!(store.logical_atlas.generation, 1);
    assert_eq!(store.reprepare_count, 0);
    assert_eq!(store.logical_atlas.scale_factor_bits, 2.0f32.to_bits());
    assert!(updates
        .iter()
        .any(|update| matches!(update, ViewUpdate::Atlas(atlas) if atlas.generation == 1)));
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
    assert_compose_invalidation(outcome, Invalidation::RECOMPOSE);

    let (mut view_update_rx, mut pool) = build_pool_for_test();
    let completed = Runtime::new()
        .expect("tokio runtime must build")
        .block_on(async {
            compose_and_emit_with_post_clamp(&mut store, &mut pool, Invalidation::RECOMPOSE).await
        });

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
    assert_compose_invalidation(outcome, Invalidation::RECOMPOSE);

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
    assert_compose_invalidation(outcome, Invalidation::RECOMPOSE);

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

fn build_text_input_store() -> Store {
    build_store_with_delegate(Arc::new(TextInputTestStoreDelegate))
}

fn build_dynamic_text_input_store() -> Store {
    build_store_with_delegate(Arc::new(DynamicTextInputTestStoreDelegate))
}

struct TestStoreDelegate;
struct TextInputTestStoreDelegate;
struct DynamicTextInputTestStoreDelegate;
struct ConfiguredTestStoreDelegate;
struct PixelScaleStoreDelegate;
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
        StoreBootstrap::new(tree, rasterizer)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
}

impl StoreDelegate for TextInputTestStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let tree = build_text_input_test_document();
        StoreBootstrap::new(tree, rasterizer)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
}

impl StoreDelegate for DynamicTextInputTestStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let tree = build_dynamic_text_input_test_document();
        StoreBootstrap::new(tree, rasterizer)
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
        StoreBootstrap::new(tree, rasterizer)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}

    fn interaction_config(&self) -> InteractionConfig {
        InteractionConfig {
            line_step_px: 12.0,
            ..InteractionConfig::default()
        }
    }
}

impl StoreDelegate for PixelScaleStoreDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        let tree = build_tree_test_document();
        StoreBootstrap::new(tree, rasterizer)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}

    fn interaction_config(&self) -> InteractionConfig {
        InteractionConfig {
            wheel_pixel_scale: 2.0,
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
        StoreBootstrap::new(tree, rasterizer)
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
        StoreBootstrap::new(tree, rasterizer)
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
        StoreBootstrap::new(tree, rasterizer)
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

fn build_text_input_test_document() -> DocumentTree {
    let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
    let input_style = TextInputStyle {
        block: BlockStyle {
            padding: Edges::all(8.0).expect("padding must be valid"),
            margin: Edges::new(0.0, 0.0, 0.0, 10.0).expect("margin must be valid"),
            background: Some([0.12, 0.16, 0.22, 1.0]),
            min_width: Some(240.0),
            ..BlockStyle::default()
        },
        border: Some(BorderStyle::new([0.35, 0.48, 0.62, 1.0], 1.0).expect("border must be valid")),
        caret_color: [1.0, 1.0, 1.0, 1.0],
    };
    let children = [TextInputId::new(1), TextInputId::new(2)]
        .into_iter()
        .map(|id| {
            BlockNode::TextInput(
                TextInputNode::new(id, "", style, input_style).expect("text input must be valid"),
            )
        })
        .collect();

    DocumentTree::new(BlockNode::Stack(
        StackNode::new(FlowDirection::Vertical, children, BlockStyle::default())
            .expect("stack must be valid"),
    ))
    .expect("text input document must be valid")
}

fn build_dynamic_text_input_test_document() -> DocumentTree {
    let style = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
    let input_style = TextInputStyle {
        block: BlockStyle {
            align_self: Align::Start,
            padding: Edges::all(4.0).expect("padding must be valid"),
            background: Some([0.12, 0.16, 0.22, 1.0]),
            ..BlockStyle::default()
        },
        border: Some(BorderStyle::new([0.35, 0.48, 0.62, 1.0], 1.0).expect("border must be valid")),
        caret_color: [1.0, 1.0, 1.0, 1.0],
    };
    let input = TextInputNode::new(TextInputId::new(1), "", style, input_style)
        .expect("text input must be valid");

    DocumentTree::new(BlockNode::Stack(
        StackNode::new(
            FlowDirection::Vertical,
            vec![BlockNode::TextInput(input)],
            BlockStyle::default(),
        )
        .expect("stack must be valid"),
    ))
    .expect("dynamic text input document must be valid")
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

fn prime_scroll_metrics(store: &mut Store, viewport: [f32; 2], content_extent: [f32; 2]) {
    store.interaction.last_known_viewport = viewport;
    store.interaction.last_known_content_extent = content_extent;
}

fn assert_key_scrolls_to(store: &mut Store, code: KeyCode, kind: KeyEventKind, expected_y: f32) {
    let outcome = store.handle_action(key_input_action_with_kind(code, kind));
    assert_compose_invalidation(outcome, Invalidation::RECOMPOSE);
    assert_eq!(store.interaction.scroll_offset, [0.0, expected_y]);
}

fn key_input_action_with_kind(code: KeyCode, kind: KeyEventKind) -> Action {
    Action::Input {
        event: InputEvent::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind,
        }),
    }
}

fn mouse_input_action(scroll_delta: MouseScroll) -> Action {
    Action::Input {
        event: InputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            logical_position: None,
            scroll_delta: Some(scroll_delta),
            modifiers: KeyModifiers::NONE,
            event_time: Instant::now(),
        }),
    }
}

fn mouse_down_action(position: [f32; 2]) -> Action {
    Action::Input {
        event: InputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButtonKind::Left),
            logical_position: Some(position),
            scroll_delta: None,
            modifiers: KeyModifiers::NONE,
            event_time: Instant::now(),
        }),
    }
}

fn click_text_input_action(store: &Store, text_input: TextInputId) -> Action {
    let rect = text_input_hit_rect(store, text_input);
    mouse_down_action([rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5])
}

fn text_input_hit_rect(store: &Store, text_input: TextInputId) -> [f32; 4] {
    store
        .text_input_hit_targets
        .iter()
        .find(|target| target.text_input_id == text_input)
        .expect("text input hit target must exist after compose")
        .rect()
}

fn text_input_text(store: &Store, text_input: TextInputId) -> &str {
    store
        .model
        .text_inputs()
        .get(text_input)
        .expect("text input state must exist")
        .text()
}

fn text_input_cursor(store: &Store, text_input: TextInputId) -> usize {
    store
        .model
        .text_inputs()
        .get(text_input)
        .expect("text input state must exist")
        .cursor_index()
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

fn compose_and_drain_updates(store: &mut Store, invalidation: Invalidation) -> Vec<ViewUpdate> {
    let (mut view_update_rx, mut pool) = build_pool_for_test();
    let completed = Runtime::new()
        .expect("tokio runtime must build")
        .block_on(async { compose_and_emit_with_post_clamp(store, &mut pool, invalidation).await });
    assert!(completed);
    drain_view_updates(&mut view_update_rx)
}

fn scene_updates_have_glyphs(updates: Vec<ViewUpdate>) -> bool {
    updates.into_iter().any(|update| {
        let ViewUpdate::Scene(frame) = update else {
            return false;
        };
        frame
            .into_buffer()
            .blocks()
            .iter()
            .any(|block| !block.glyphs().is_empty())
    })
}

fn build_pool_for_test() -> (tokio::sync::mpsc::Receiver<ViewUpdate>, SceneBufferPool) {
    let (pool, view_update_rx) = SceneBufferPool::new_for_test(SceneConfig::default());
    (view_update_rx, pool)
}
