// The integration test pulls the real `src/event` and `src/io` module trees in
// via `#[path]` so it exercises the production routing and IO bridge code.
// That also compiles helper items which this harness does not reference
// directly, so the test target needs a local dead-code allowance here.
#![allow(dead_code)]

#[path = "../src/event/mod.rs"]
mod event;
#[path = "../src/io/mod.rs"]
mod io;

use std::thread;
use std::time::Duration;

use event::handlers::{KeyboardInput, ViewportUpdate};
use event::{EventRouter, RedrawThrottle, RouteAction, ViewportSnapshot};
use io::{
    run_mock_io_task, AppCommand, ButtonState, IoEvent, IoEventDriver, IoHandle, IoRuntime,
    MockMouseEvent, MouseButtonKind, MouseScroll,
};
use tokio::sync::mpsc;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};

const MIN_INTERVAL: Duration = Duration::from_millis(16);

fn build_router() -> (EventRouter, mpsc::UnboundedReceiver<AppCommand>) {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    (EventRouter::new(command_tx), command_rx)
}

#[test]
fn async_side_exports_remain_available_for_future_mounts() {
    let _ = std::mem::size_of::<Option<IoHandle>>();
    let _ = std::mem::size_of::<Option<IoRuntime>>();
    let _ = run_mock_io_task;
}

#[derive(Debug, PartialEq, Eq)]
struct WakeEffects {
    drained: usize,
    events: Vec<IoEvent>,
    request_redraw: bool,
    redraw_interval: Option<Duration>,
    schedule_deadline: Option<Duration>,
    wake_again: bool,
    disconnected: bool,
}

fn apply_wake(
    driver: &mut IoEventDriver,
    throttle: &mut RedrawThrottle,
    limit: usize,
) -> WakeEffects {
    let outcome = driver.on_wake(limit);
    let mut effects = WakeEffects {
        drained: outcome.drained,
        events: outcome.events,
        request_redraw: false,
        redraw_interval: None,
        schedule_deadline: None,
        wake_again: outcome.wake_again,
        disconnected: outcome.disconnected,
    };

    if effects.disconnected || effects.drained == 0 {
        return effects;
    }

    if throttle.should_redraw_now() {
        effects.request_redraw = true;
        effects.redraw_interval = Some(record_redraw(throttle));
        return effects;
    }

    throttle.mark_dirty();
    if !throttle.pending_deadline() {
        throttle.start_deadline();
        effects.schedule_deadline = Some(throttle.deadline_delay());
    }

    effects
}

fn apply_deadline(throttle: &mut RedrawThrottle) -> Option<Duration> {
    throttle.finish_deadline();
    if throttle.is_dirty() {
        return Some(record_redraw(throttle));
    }

    None
}

fn record_redraw(throttle: &mut RedrawThrottle) -> Duration {
    let interval = throttle.elapsed_since_last_redraw().unwrap_or_default();
    throttle.record_redraw();
    throttle.clear_dirty();
    interval
}

#[test]
fn single_wake_drains_io_and_requests_immediate_redraw() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = IoEventDriver::new(rx);
    let mut throttle = RedrawThrottle::new(MIN_INTERVAL);
    tx.send(IoEvent::MockTick {
        payload: String::from("alpha"),
    })
    .expect("send must succeed");

    let outcome = apply_wake(&mut driver, &mut throttle, 4096);

    assert_eq!(outcome.drained, 1);
    assert_eq!(
        outcome.events,
        vec![IoEvent::MockTick {
            payload: String::from("alpha"),
        }]
    );
    assert!(outcome.request_redraw);
    assert_eq!(outcome.redraw_interval, Some(Duration::ZERO));
    assert_eq!(outcome.schedule_deadline, None);
    assert!(!outcome.wake_again);
    assert!(!outcome.disconnected);
}

#[test]
fn second_wake_inside_interval_is_deferred_until_deadline() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = IoEventDriver::new(rx);
    let mut throttle = RedrawThrottle::new(MIN_INTERVAL);
    tx.send(IoEvent::MockTick {
        payload: String::from("first"),
    })
    .expect("first send must succeed");

    let first = apply_wake(&mut driver, &mut throttle, 4096);
    assert!(first.request_redraw);

    tx.send(IoEvent::MockTick {
        payload: String::from("second"),
    })
    .expect("second send must succeed");
    let second = apply_wake(&mut driver, &mut throttle, 4096);

    assert_eq!(second.drained, 1);
    assert!(!second.request_redraw);
    let delay = second
        .schedule_deadline
        .expect("second wake should schedule a deadline");
    assert!(delay <= MIN_INTERVAL);

    thread::sleep(delay.saturating_add(Duration::from_millis(1)));
    assert!(apply_deadline(&mut throttle).is_some());
}

#[test]
fn disconnected_channel_requests_shutdown() {
    let (tx, rx) = mpsc::unbounded_channel::<IoEvent>();
    let mut driver = IoEventDriver::new(rx);
    drop(tx);

    let outcome = driver.on_wake(4096);

    assert_eq!(outcome.drained, 0);
    assert!(outcome.disconnected);
    assert!(outcome.events.is_empty());
}

#[test]
fn drain_limit_preserves_overflow_for_the_next_wake() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = IoEventDriver::new(rx);
    for payload in ["one", "two", "three"] {
        tx.send(IoEvent::MockTick {
            payload: payload.to_string(),
        })
        .expect("send must succeed");
    }

    let first = driver.on_wake(2);
    assert_eq!(first.drained, 2);
    assert!(first.wake_again);
    assert_eq!(
        first.events,
        vec![
            IoEvent::MockTick {
                payload: String::from("one"),
            },
            IoEvent::MockTick {
                payload: String::from("two"),
            },
        ]
    );

    let second = driver.on_wake(2);
    assert_eq!(second.drained, 1);
    assert_eq!(
        second.events,
        vec![IoEvent::MockTick {
            payload: String::from("three"),
        }]
    );
}

#[test]
fn keyboard_input_is_routed_to_async_commands() {
    let (router, mut command_rx) = build_router();

    let action = router.dispatch_keyboard_input(KeyboardInput::new(Some("a"), "KeyA", false));

    assert_eq!(action, RouteAction::None);
    assert_eq!(
        command_rx
            .try_recv()
            .expect("keyboard command must be forwarded"),
        AppCommand::MockKeyInput {
            text: String::from("a"),
        }
    );
}

#[test]
fn mouse_events_are_routed_as_semantic_mouse_commands() {
    let (router, mut command_rx) = build_router();
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
        command_rx
            .try_recv()
            .expect("mouse button command must be forwarded"),
        AppCommand::MockMouseInput {
            event: MockMouseEvent::Button {
                state: ButtonState::Pressed,
                button: MouseButtonKind::Left,
            },
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
        command_rx
            .try_recv()
            .expect("mouse move command must be forwarded"),
        AppCommand::MockMouseInput {
            event: MockMouseEvent::Move { x: 42.0, y: 84.0 },
        }
    );

    let scroll_action = router.dispatch(
        &WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(1.5, -2.5),
            phase: TouchPhase::Moved,
        },
        viewport,
    );
    assert_eq!(scroll_action, RouteAction::None);
    assert_eq!(
        command_rx
            .try_recv()
            .expect("mouse scroll command must be forwarded"),
        AppCommand::MockMouseInput {
            event: MockMouseEvent::Scroll {
                delta: MouseScroll::LineDelta { x: 1.5, y: -2.5 },
            },
        }
    );
}

#[test]
fn resize_event_routes_and_emits_resize_command() {
    let (router, mut command_rx) = build_router();

    let action = router.dispatch(
        &WindowEvent::Resized(PhysicalSize::new(800, 600)),
        ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0),
    );

    assert_eq!(
        action,
        RouteAction::Resize(ViewportUpdate {
            size: PhysicalSize::new(800, 600),
            scale_factor: 2.0,
        })
    );
    assert_eq!(
        command_rx
            .try_recv()
            .expect("resize command must be forwarded"),
        AppCommand::MockResize {
            width: 800,
            height: 600,
        }
    );
}

#[test]
fn close_requested_routes_shutdown_command() {
    let (router, mut command_rx) = build_router();

    let action = router.dispatch(
        &WindowEvent::CloseRequested,
        ViewportSnapshot::new(PhysicalSize::new(1280, 720), 2.0),
    );

    assert_eq!(action, RouteAction::CloseRequested);
    assert_eq!(
        command_rx
            .try_recv()
            .expect("shutdown command must be forwarded"),
        AppCommand::Shutdown
    );
}
