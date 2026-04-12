use winit::dpi::PhysicalSize;

use super::super::RUNTIME_SHUTDOWN_TIMEOUT;
use super::support::build_app;
use crate::event::handlers::ViewportUpdate;
use crate::event::RouteAction;

#[test]
fn resize_action_updates_renderer_and_requests_redraw() {
    let mut harness = build_app();

    let should_exit = harness
        .app
        .apply_route_action(RouteAction::Resize(ViewportUpdate {
            size: PhysicalSize::new(800, 600),
            scale_factor: 2.0,
            viewport_revision: 1,
        }));

    assert!(!should_exit);
    assert_eq!(
        harness
            .renderer_log
            .lock()
            .expect("renderer log must lock")
            .resize_calls,
        vec![(PhysicalSize::new(800, 600), 2.0)]
    );
    assert_eq!(
        harness
            .window_log
            .lock()
            .expect("window log must lock")
            .redraw_requests,
        1
    );
    assert_eq!(harness.app.view_state.requested_viewport_revision(), 1);
    assert_eq!(harness.app.view_state.applied_viewport_revision(), 0);
}

#[test]
fn redraw_action_notifies_window_and_frames_renderer() {
    let mut harness = build_app();

    let should_exit = harness.app.apply_route_action(RouteAction::RedrawRequested);

    assert!(!should_exit);
    assert_eq!(
        harness
            .window_log
            .lock()
            .expect("window log must lock")
            .pre_present_notify_calls,
        1
    );
    assert_eq!(
        harness
            .renderer_log
            .lock()
            .expect("renderer log must lock")
            .frame_calls,
        1
    );
}

#[test]
fn close_action_clears_state_and_shuts_down_runtime() {
    let mut harness = build_app();

    let should_exit = harness.app.apply_route_action(RouteAction::CloseRequested);

    assert!(should_exit);
    assert!(harness.app.shutting_down);
    assert!(harness.app.window.is_none());
    assert!(harness.app.renderer.is_none());
    assert!(harness.app.router.is_none());
    assert!(harness.app.io_runtime.is_none());
    assert_eq!(
        harness
            .runtime_log
            .lock()
            .expect("runtime log must lock")
            .shutdown_timeouts,
        vec![RUNTIME_SHUTDOWN_TIMEOUT]
    );
}
