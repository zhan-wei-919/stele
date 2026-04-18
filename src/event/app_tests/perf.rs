use std::time::Instant;

use super::support::{build_app, sample_scene_frame};
use crate::event::RouteAction;
use crate::io::ViewUpdate;

#[test]
#[ignore = "manual perf smoke test"]
fn reports_view_dispatch_to_render_perf() {
    let mut harness = build_app();
    let scene_frame = sample_scene_frame(1, None, &[7], false);

    let dispatch_started = Instant::now();
    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Scene(scene_frame))
        .expect("scene frame send must succeed");

    assert!(!harness.app.on_wake());
    let applied_at = Instant::now();

    assert!(!harness.app.apply_route_action(RouteAction::RedrawRequested));
    let frame_finished = Instant::now();

    println!(
        "perf.view dispatch_to_apply_us={} apply_to_frame_us={} dispatch_to_frame_us={}",
        applied_at.duration_since(dispatch_started).as_micros(),
        frame_finished.duration_since(applied_at).as_micros(),
        frame_finished.duration_since(dispatch_started).as_micros(),
    );
}
