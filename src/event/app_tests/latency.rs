use std::time::{Duration, Instant};

use crate::io::ViewUpdate;
use crate::scene::SceneConfig;

use super::support::{
    build_app_with_scene_config, sample_scene_frame, sample_scene_frame_with_resize_started_at,
    LogCapture,
};

#[test]
fn rebuild_budget_warns_for_sub_millisecond_overrun() {
    let mut harness = build_app_with_scene_config(SceneConfig {
        rebuild_budget_ms: 1,
        ..SceneConfig::default()
    });
    harness.set_rebuild_delay(Duration::from_micros(1_500));
    let capture = LogCapture::begin();

    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Scene(sample_scene_frame(1, None, &[7], false)))
        .expect("scene frame send must succeed");

    assert!(!harness.app.on_wake());
    assert!(
        capture.contains("scene.budget_exceeded phase=rebuild elapsed_us="),
        "expected rebuild budget warning, got {:?}",
        capture.entries()
    );
}

#[test]
fn end_to_end_latency_budget_warns_when_resize_apply_exceeds_limit() {
    let mut harness = build_app_with_scene_config(SceneConfig {
        end_to_end_latency_ms: 1,
        ..SceneConfig::default()
    });
    let capture = LogCapture::begin();

    harness
        .view_update_tx
        .blocking_send(ViewUpdate::Scene(
            sample_scene_frame_with_resize_started_at(
                9,
                None,
                &[7],
                false,
                Some(Instant::now() - Duration::from_micros(1_500)),
            ),
        ))
        .expect("scene frame send must succeed");

    assert!(!harness.app.on_wake());
    assert!(
        capture.contains("scene.budget_exceeded phase=end_to_end elapsed_us="),
        "expected end-to-end latency warning, got {:?}",
        capture.entries()
    );
    assert!(
        capture.contains("viewport_revision=9"),
        "expected viewport revision in end-to-end warning, got {:?}",
        capture.entries()
    );
}
