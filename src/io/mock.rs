//! Demo-only mock IO task used until PTY/VT modules are wired in.

use std::time::Duration;

use log::info;

use super::{AppCommand, IoEvent, IoHandle};

const MOCK_IO_INTERVAL: Duration = Duration::from_secs(1);

/// Runs the demo mock IO producer on the async side of the bridge.
pub(crate) async fn run_mock_io_task(mut handle: IoHandle) {
    let mut interval = tokio::time::interval(MOCK_IO_INTERVAL);
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if !handle.dispatch_io_event(IoEvent::MockTick {
                    payload: String::from("tick"),
                }) {
                    break;
                }
            }
            command = handle.next_app_command() => {
                match command {
                    Some(AppCommand::Shutdown) => break,
                    Some(AppCommand::MockKeyInput { text }) => {
                        info!("io.command.recv kind=mock_key_input text={:?}", text);
                    }
                    Some(AppCommand::MockMouseInput { event }) => {
                        info!("io.command.recv kind=mock_mouse_input event={:?}", event);
                    }
                    Some(AppCommand::MockResize { width, height }) => {
                        info!(
                            "io.command.recv kind=mock_resize width={} height={}",
                            width,
                            height
                        );
                    }
                    None => break,
                }
            }
        }
    }
}
