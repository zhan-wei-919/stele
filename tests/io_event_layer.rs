// The integration test pulls selected production modules in via `#[path]` so
// it exercises the real event-routing and bridge code.
#![allow(dead_code)]

use std::time::Instant;

use bumpalo::Bump;

#[path = "../src/draw_list/mod.rs"]
mod draw_list;
#[path = "../src/event/mod.rs"]
mod event;
#[path = "../src/font/mod.rs"]
mod font;
#[path = "../src/io/mod.rs"]
mod io;
#[path = "../src/renderer/mod.rs"]
mod renderer;
#[path = "../src/scene/mod.rs"]
mod scene;

use event::clipboard::{ClipboardProvider, ClipboardReadError, ClipboardWriteError};
use event::handlers::KeyboardInput;
use event::{EventRouter, RouteAction, ViewportSnapshot};
use io::{
    Action, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButtonKind, MouseEvent,
    MouseEventKind, MouseScroll, SceneFrame, UiEffect, UiEffectDriver, ViewUpdate,
    ViewUpdateDriver, WakeEvent,
};
use tokio::sync::mpsc;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::keyboard::ModifiersState;

fn build_router() -> (EventRouter, mpsc::UnboundedReceiver<Action>) {
    let (action_tx, action_rx) = mpsc::unbounded_channel();
    (EventRouter::new(action_tx), action_rx)
}

fn build_router_with_clipboard(
    clipboard: FakeClipboard,
) -> (EventRouter, mpsc::UnboundedReceiver<Action>) {
    let (action_tx, action_rx) = mpsc::unbounded_channel();
    (
        EventRouter::new_with_clipboard(action_tx, Box::new(clipboard)),
        action_rx,
    )
}

#[test]
fn async_side_exports_remain_available_for_future_mounts() {
    let _ = std::mem::size_of::<Option<font::FontDiscovery>>();
    let _ = std::mem::size_of::<Option<font::FontSelection>>();
    let _ = std::mem::size_of::<Option<font::LineMetrics>>();
    let _ = std::mem::size_of::<Option<font::MeasuredGlyph>>();
    let _ = std::mem::size_of::<Option<renderer::Renderer<'static>>>();
    let _ = std::mem::size_of::<Option<io::AtlasPatch>>();
    let _ = std::mem::size_of::<Option<io::AtlasUpdate>>();
    let _ = std::mem::size_of::<Option<io::IoHandle>>();
    let _ = std::mem::size_of::<Option<io::IoRuntime>>();
    let _ = std::mem::size_of::<Option<io::SceneFrame>>();
    let _ = std::mem::size_of::<Option<io::ViewUpdate>>();
    let _ = std::mem::size_of::<Option<ViewUpdateDriver>>();
    let _ = std::mem::size_of::<Option<UiEffectDriver>>();
    let _ = std::mem::size_of::<Option<WakeEvent>>();
    let _ = std::mem::size_of::<Option<scene::SceneProtocolState>>();
}

#[test]
fn drain_limit_preserves_overflow_for_the_next_wake() {
    let (tx, rx) = mpsc::channel(4);
    let mut driver = ViewUpdateDriver::new(rx);
    for revision in [1, 2, 3] {
        tx.blocking_send(ViewUpdate::Scene(empty_scene_frame(revision)))
            .expect("view update send must succeed");
    }

    let first = driver.on_wake(2);
    assert_eq!(first.drained, 2);
    assert!(first.wake_again);
    assert_eq!(scene_revision(&first.updates[0]), 1);
    assert_eq!(scene_revision(&first.updates[1]), 2);

    let second = driver.on_wake(2);
    assert_eq!(second.drained, 1);
    assert_eq!(scene_revision(&second.updates[0]), 3);
}

#[test]
fn ui_effect_drain_limit_preserves_overflow_for_the_next_wake() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = UiEffectDriver::new(rx);
    for text in ["first", "second", "third"] {
        tx.send(UiEffect::ClipboardWrite(text.to_owned()))
            .expect("UI effect send must succeed");
    }

    let first = driver.on_wake(2);
    assert_eq!(first.drained, 2);
    assert!(first.wake_again);
    assert_eq!(
        first.effects,
        vec![
            UiEffect::ClipboardWrite("first".to_owned()),
            UiEffect::ClipboardWrite("second".to_owned()),
        ]
    );

    let second = driver.on_wake(2);
    assert_eq!(second.drained, 1);
    assert_eq!(
        second.effects,
        vec![UiEffect::ClipboardWrite("third".to_owned())]
    );
}

#[test]
fn keyboard_input_is_routed_to_actions() {
    let (mut router, mut action_rx) = build_router();

    let action = router.dispatch_keyboard_input(KeyboardInput::new(
        KeyCode::Char('a'),
        KeyEventKind::Press,
        false,
    ));

    assert_eq!(action, RouteAction::None);
    assert_eq!(
        action_rx
            .try_recv()
            .expect("keyboard action must be forwarded"),
        Action::Input {
            event: InputEvent::Key(KeyEvent {
                code: KeyCode::Char('a'),
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
            }),
        }
    );
}

#[test]
fn paste_shortcut_is_routed_to_paste_action() {
    let (mut router, mut action_rx) =
        build_router_with_clipboard(FakeClipboard::with_text("pasted text"));
    let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

    assert_eq!(
        router.dispatch(
            &WindowEvent::ModifiersChanged(ModifiersState::CONTROL.into()),
            viewport,
        ),
        RouteAction::None
    );
    assert_eq!(
        router.dispatch_keyboard_input(KeyboardInput::new(
            KeyCode::Char('v'),
            KeyEventKind::Press,
            false,
        )),
        RouteAction::None
    );

    assert_eq!(
        action_rx.try_recv().expect("paste action must be emitted"),
        Action::Input {
            event: InputEvent::Paste("pasted text".to_owned()),
        }
    );
}

#[test]
fn mouse_events_are_routed_as_semantic_actions() {
    let (mut router, mut action_rx) = build_router();
    let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

    let move_before = Instant::now();
    let move_action = router.dispatch(
        &WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(42.0, 84.0),
        },
        viewport,
    );
    let move_after = Instant::now();
    assert_eq!(move_action, RouteAction::None);
    let move_event = expect_mouse_event(
        action_rx
            .try_recv()
            .expect("mouse move action must be forwarded"),
        move_before,
        move_after,
    );
    assert_eq!(move_event.kind, MouseEventKind::Moved);
    assert_eq!(move_event.logical_position, Some([21.0, 42.0]));
    assert_eq!(move_event.scroll_delta, None);
    assert_eq!(move_event.modifiers, KeyModifiers::NONE);

    let button_before = Instant::now();
    let button_action = router.dispatch(
        &WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
        },
        viewport,
    );
    let button_after = Instant::now();
    assert_eq!(button_action, RouteAction::None);
    let button_event = expect_mouse_event(
        action_rx
            .try_recv()
            .expect("mouse button action must be forwarded"),
        button_before,
        button_after,
    );
    assert_eq!(
        button_event.kind,
        MouseEventKind::Down(MouseButtonKind::Left)
    );
    assert_eq!(button_event.logical_position, Some([21.0, 42.0]));
    assert_eq!(button_event.scroll_delta, None);
    assert_eq!(button_event.modifiers, KeyModifiers::NONE);

    let drag_before = Instant::now();
    router.dispatch(
        &WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(84.0, 42.0),
        },
        viewport,
    );
    let drag_after = Instant::now();
    let drag_event = expect_mouse_event(
        action_rx
            .try_recv()
            .expect("drag move action must be forwarded"),
        drag_before,
        drag_after,
    );
    assert_eq!(drag_event.kind, MouseEventKind::Drag(MouseButtonKind::Left));
    assert_eq!(drag_event.logical_position, Some([42.0, 21.0]));
    assert_eq!(drag_event.scroll_delta, None);
    assert_eq!(drag_event.modifiers, KeyModifiers::NONE);

    let release_before = Instant::now();
    router.dispatch(
        &WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
        },
        viewport,
    );
    let release_after = Instant::now();
    let release_event = expect_mouse_event(
        action_rx
            .try_recv()
            .expect("mouse up action must be forwarded"),
        release_before,
        release_after,
    );
    assert_eq!(
        release_event.kind,
        MouseEventKind::Up(MouseButtonKind::Left)
    );
    assert_eq!(release_event.logical_position, Some([42.0, 21.0]));
    assert_eq!(release_event.scroll_delta, None);
    assert_eq!(release_event.modifiers, KeyModifiers::NONE);

    let scroll_before = Instant::now();
    let scroll_action = router.dispatch(
        &WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(1.5, -2.0),
            phase: TouchPhase::Moved,
        },
        viewport,
    );
    let scroll_after = Instant::now();
    assert_eq!(scroll_action, RouteAction::None);
    let scroll_event = expect_mouse_event(
        action_rx
            .try_recv()
            .expect("vertical scroll action must be forwarded"),
        scroll_before,
        scroll_after,
    );
    assert_eq!(scroll_event.kind, MouseEventKind::ScrollDown);
    assert_eq!(scroll_event.logical_position, Some([42.0, 21.0]));
    assert_eq!(
        scroll_event.scroll_delta,
        Some(MouseScroll::LineDelta { x: 0.0, y: -2.0 })
    );
    assert_eq!(scroll_event.modifiers, KeyModifiers::NONE);

    let horizontal_scroll_event = expect_mouse_event(
        action_rx
            .try_recv()
            .expect("horizontal scroll action must be forwarded"),
        scroll_before,
        scroll_after,
    );
    assert_eq!(horizontal_scroll_event.kind, MouseEventKind::ScrollRight);
    assert_eq!(horizontal_scroll_event.logical_position, Some([42.0, 21.0]));
    assert_eq!(
        horizontal_scroll_event.scroll_delta,
        Some(MouseScroll::LineDelta { x: 1.5, y: 0.0 })
    );
    assert_eq!(horizontal_scroll_event.modifiers, KeyModifiers::NONE);
}

#[test]
fn modifiers_are_attached_to_keyboard_and_mouse_events() {
    let (mut router, mut action_rx) = build_router();
    let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

    assert_eq!(
        router.dispatch(
            &WindowEvent::ModifiersChanged(ModifiersState::CONTROL.into()),
            viewport,
        ),
        RouteAction::None
    );

    router.dispatch_keyboard_input(KeyboardInput::new(
        KeyCode::Char('c'),
        KeyEventKind::Press,
        false,
    ));
    assert_eq!(
        action_rx
            .try_recv()
            .expect("keyboard modifier snapshot must be attached"),
        Action::Input {
            event: InputEvent::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
            }),
        }
    );

    let move_before = Instant::now();
    router.dispatch(
        &WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(40.0, 20.0),
        },
        viewport,
    );
    let move_after = Instant::now();
    let move_event = expect_mouse_event(
        action_rx
            .try_recv()
            .expect("mouse move must inherit modifier snapshot"),
        move_before,
        move_after,
    );
    assert_eq!(move_event.kind, MouseEventKind::Moved);
    assert_eq!(move_event.logical_position, Some([20.0, 10.0]));
    assert_eq!(move_event.scroll_delta, None);
    assert_eq!(move_event.modifiers, KeyModifiers::CONTROL);
}

#[test]
fn cursor_left_and_focus_events_clear_transient_mouse_state() {
    let (mut router, mut action_rx) = build_router();
    let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

    router.dispatch(
        &WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(50.0, 30.0),
        },
        viewport,
    );
    let _ = action_rx
        .try_recv()
        .expect("cursor move must emit one mouse move");

    assert_eq!(
        router.dispatch(
            &WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            },
            viewport,
        ),
        RouteAction::None
    );
    assert_eq!(
        action_rx
            .try_recv()
            .expect("cursor left event must be forwarded"),
        Action::Input {
            event: InputEvent::CursorLeft,
        }
    );

    let button_before = Instant::now();
    router.dispatch(
        &WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
        },
        viewport,
    );
    let button_after = Instant::now();
    let button_event = expect_mouse_event(
        action_rx
            .try_recv()
            .expect("button event must still be preserved"),
        button_before,
        button_after,
    );
    assert_eq!(
        button_event.kind,
        MouseEventKind::Down(MouseButtonKind::Left)
    );
    assert_eq!(button_event.logical_position, None);
    assert_eq!(button_event.scroll_delta, None);

    assert_eq!(
        router.dispatch(&WindowEvent::Focused(false), viewport),
        RouteAction::None
    );
    assert_eq!(
        action_rx
            .try_recv()
            .expect("focus lost event must be forwarded"),
        Action::Input {
            event: InputEvent::FocusChanged { focused: false },
        }
    );

    let after_focus_before = Instant::now();
    router.dispatch(
        &WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
        },
        viewport,
    );
    let after_focus_after = Instant::now();
    let after_focus_event = expect_mouse_event(
        action_rx
            .try_recv()
            .expect("button event after focus lost must still be preserved"),
        after_focus_before,
        after_focus_after,
    );
    assert_eq!(
        after_focus_event.kind,
        MouseEventKind::Down(MouseButtonKind::Left)
    );
    assert_eq!(after_focus_event.logical_position, None);
    assert_eq!(after_focus_event.scroll_delta, None);
    assert_eq!(after_focus_event.modifiers, KeyModifiers::NONE);

    assert_eq!(
        router.dispatch(&WindowEvent::Focused(true), viewport),
        RouteAction::None
    );
    assert_eq!(
        action_rx
            .try_recv()
            .expect("focus gain event must be forwarded"),
        Action::Input {
            event: InputEvent::FocusChanged { focused: true },
        }
    );
}

#[test]
fn resize_events_send_monotonic_viewport_revisions() {
    let (mut router, mut action_rx) = build_router();
    let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

    let first_started = Instant::now();
    let first = router.dispatch(&WindowEvent::Resized(PhysicalSize::new(800, 600)), viewport);
    let first_finished = Instant::now();
    let first_event_time = match first {
        RouteAction::Resize(event::handlers::ViewportUpdate {
            size,
            scale_factor,
            viewport_revision,
            event_time,
        }) => {
            assert_eq!(size, PhysicalSize::new(800, 600));
            assert_eq!(scale_factor, 2.0);
            assert_eq!(viewport_revision, 1);
            assert!(event_time >= first_started);
            assert!(event_time <= first_finished);
            event_time
        }
        _ => panic!("expected resize route action"),
    };
    match action_rx
        .try_recv()
        .expect("resize action must be forwarded")
    {
        Action::Resize {
            width,
            height,
            scale_factor,
            viewport_revision,
            event_time,
        } => {
            assert_eq!(width, 800);
            assert_eq!(height, 600);
            assert_eq!(scale_factor, 2.0);
            assert_eq!(viewport_revision, 1);
            assert_eq!(event_time, first_event_time);
        }
        _ => panic!("expected resize action"),
    }

    let second_started = Instant::now();
    let second = router.dispatch(&WindowEvent::Resized(PhysicalSize::new(900, 700)), viewport);
    let second_finished = Instant::now();
    let second_event_time = match second {
        RouteAction::Resize(event::handlers::ViewportUpdate {
            size,
            scale_factor,
            viewport_revision,
            event_time,
        }) => {
            assert_eq!(size, PhysicalSize::new(900, 700));
            assert_eq!(scale_factor, 2.0);
            assert_eq!(viewport_revision, 2);
            assert!(event_time >= second_started);
            assert!(event_time <= second_finished);
            event_time
        }
        _ => panic!("expected second resize route action"),
    };
    match action_rx
        .try_recv()
        .expect("second resize action must be forwarded")
    {
        Action::Resize {
            width,
            height,
            scale_factor,
            viewport_revision,
            event_time,
        } => {
            assert_eq!(width, 900);
            assert_eq!(height, 700);
            assert_eq!(scale_factor, 2.0);
            assert_eq!(viewport_revision, 2);
            assert_eq!(event_time, second_event_time);
        }
        _ => panic!("expected second resize action"),
    }
}

#[test]
fn close_requested_routes_shutdown_action() {
    let (mut router, mut action_rx) = build_router();
    let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

    let action = router.dispatch(&WindowEvent::CloseRequested, viewport);

    assert_eq!(action, RouteAction::CloseRequested);
    assert_eq!(
        action_rx
            .try_recv()
            .expect("shutdown action must be forwarded"),
        Action::Shutdown
    );
}

fn scene_revision(update: &ViewUpdate) -> u64 {
    match update {
        ViewUpdate::Scene(scene_frame) => scene_frame.metadata().viewport_revision,
        ViewUpdate::Atlas(_) => panic!("expected scene update in driver test"),
    }
}

fn empty_scene_frame(viewport_revision: u64) -> SceneFrame {
    let metadata = scene::SceneFrameMetadata {
        viewport_revision,
        required_atlas_generation: None,
        clear_tessellation_cache: false,
        resize_started_at: None,
    };
    let scene_buffer = scene::SceneBuffer::new(Bump::with_capacity(4096), |owner| {
        scene::SceneBufferInner::empty_in(owner, metadata)
    });
    SceneFrame::new(Box::new(scene_buffer))
}

fn expect_mouse_event(action: Action, before: Instant, after: Instant) -> MouseEvent {
    match action {
        Action::Input {
            event: InputEvent::Mouse(mouse_event),
        } => {
            assert!(mouse_event.event_time >= before);
            assert!(mouse_event.event_time <= after);
            mouse_event
        }
        other => panic!("expected mouse event, got {other:?}"),
    }
}

struct FakeClipboard {
    text: Result<String, ClipboardReadError>,
    written: Vec<String>,
}

impl FakeClipboard {
    fn with_text(text: &str) -> Self {
        Self {
            text: Ok(text.to_owned()),
            written: Vec::new(),
        }
    }
}

impl ClipboardProvider for FakeClipboard {
    fn read_text(&mut self) -> Result<String, ClipboardReadError> {
        self.text.clone()
    }

    fn write_text(&mut self, text: String) -> Result<(), ClipboardWriteError> {
        self.written.push(text);
        Ok(())
    }
}
