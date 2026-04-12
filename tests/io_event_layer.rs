// The integration test pulls selected production modules in via `#[path]` so
// it exercises the real event-routing and bridge code.
#![allow(dead_code)]

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

use event::handlers::KeyboardInput;
use event::{EventRouter, RouteAction, ViewportSnapshot};
use io::{
    Action, ButtonState, MouseButtonKind, MouseScroll, SceneFrame, ScenePayload, ViewUpdate,
    ViewUpdateDriver, WakeEvent,
};
use tokio::sync::mpsc;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};

fn build_router() -> (EventRouter, mpsc::UnboundedReceiver<Action>) {
    let (action_tx, action_rx) = mpsc::unbounded_channel();
    (EventRouter::new(action_tx), action_rx)
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
    let _ = std::mem::size_of::<Option<io::BlockOp>>();
    let _ = std::mem::size_of::<Option<io::IoHandle>>();
    let _ = std::mem::size_of::<Option<io::IoRuntime>>();
    let _ = std::mem::size_of::<Option<io::SceneFrame>>();
    let _ = std::mem::size_of::<Option<io::ViewUpdate>>();
    let _ = std::mem::size_of::<Option<ViewUpdateDriver>>();
    let _ = std::mem::size_of::<Option<WakeEvent>>();
}

#[test]
fn drain_limit_preserves_overflow_for_the_next_wake() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = ViewUpdateDriver::new(rx);
    for revision in [1, 2, 3] {
        tx.send(ViewUpdate::Scene(SceneFrame::new(
            revision,
            None,
            ScenePayload::ReplaceAll {
                block_order: Vec::new(),
                block_batches: Vec::new(),
            },
        )))
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
fn keyboard_input_is_routed_to_actions() {
    let (router, mut action_rx) = build_router();

    let action = router.dispatch_keyboard_input(KeyboardInput::new(Some("a"), "KeyA", false));

    assert_eq!(action, RouteAction::None);
    assert_eq!(
        action_rx
            .try_recv()
            .expect("keyboard action must be forwarded"),
        Action::KeyInput {
            text: String::from("a"),
        }
    );
}

#[test]
fn mouse_events_are_routed_as_semantic_actions() {
    let (mut router, mut action_rx) = build_router();
    let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

    let button_action = router.dispatch(
        &WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
        },
        viewport,
    );
    assert_eq!(button_action, RouteAction::None);
    assert_eq!(
        action_rx
            .try_recv()
            .expect("mouse button action must be forwarded"),
        Action::MouseButton {
            state: ButtonState::Pressed,
            button: MouseButtonKind::Left,
        }
    );

    let move_action = router.dispatch(
        &WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(42.0, 84.0),
        },
        viewport,
    );
    assert_eq!(move_action, RouteAction::None);
    assert_eq!(
        action_rx
            .try_recv()
            .expect("mouse move action must be forwarded"),
        Action::MouseMove { x: 42.0, y: 84.0 }
    );

    let scroll_action = router.dispatch(
        &WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(1.5, -2.0),
            phase: TouchPhase::Moved,
        },
        viewport,
    );
    assert_eq!(scroll_action, RouteAction::None);
    assert_eq!(
        action_rx
            .try_recv()
            .expect("mouse wheel action must be forwarded"),
        Action::MouseScroll {
            delta: MouseScroll::LineDelta { x: 1.5, y: -2.0 },
        }
    );
}

#[test]
fn resize_events_send_monotonic_viewport_revisions() {
    let (mut router, mut action_rx) = build_router();
    let viewport = ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0);

    let first = router.dispatch(&WindowEvent::Resized(PhysicalSize::new(800, 600)), viewport);
    assert_eq!(
        first,
        RouteAction::Resize(event::handlers::ViewportUpdate {
            size: PhysicalSize::new(800, 600),
            scale_factor: 2.0,
            viewport_revision: 1,
        })
    );
    assert_eq!(
        action_rx
            .try_recv()
            .expect("resize action must be forwarded"),
        Action::Resize {
            width: 800,
            height: 600,
            scale_factor: 2.0,
            viewport_revision: 1,
        }
    );

    let second = router.dispatch(&WindowEvent::Resized(PhysicalSize::new(900, 700)), viewport);
    assert_eq!(
        second,
        RouteAction::Resize(event::handlers::ViewportUpdate {
            size: PhysicalSize::new(900, 700),
            scale_factor: 2.0,
            viewport_revision: 2,
        })
    );
    assert_eq!(
        action_rx
            .try_recv()
            .expect("second resize action must be forwarded"),
        Action::Resize {
            width: 900,
            height: 700,
            scale_factor: 2.0,
            viewport_revision: 2,
        }
    );
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
        ViewUpdate::Scene(scene_frame) => scene_frame.viewport_revision,
        ViewUpdate::Atlas(_) => panic!("expected scene update in driver test"),
    }
}
