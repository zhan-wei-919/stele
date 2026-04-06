//! Async IO runtime primitives for bridging background producers with winit.

mod channel;
mod driver;
mod mock;
mod runtime;

pub(crate) use channel::{
    AppCommand, ButtonState, IoEvent, MockMouseEvent, MouseButtonKind, MouseScroll, WakeEvent,
};
pub(crate) use driver::IoEventDriver;
pub(crate) use mock::run_mock_io_task;
pub(crate) use runtime::{IoHandle, IoRuntime};
