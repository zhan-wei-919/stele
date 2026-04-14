//! Async bridge primitives shared by the store task and the winit view thread.

mod channel;
mod driver;
mod runtime;

pub(crate) use channel::{
    Action, AtlasPatch, AtlasUpdate, BlockOp, InputEvent, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButtonKind, MouseEvent, MouseEventKind, MouseScroll, SceneFrame,
    ScenePayload, ViewUpdate, WakeEvent,
};
pub(crate) use driver::ViewUpdateDriver;
pub(crate) use runtime::{IoHandle, IoRuntime};
