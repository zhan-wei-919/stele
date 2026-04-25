//! Async bridge primitives shared by the store task and the winit view thread.

mod channel;
mod driver;
mod runtime;

pub(crate) use channel::{
    Action, AtlasPatch, AtlasUpdate, InputEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButtonKind, MouseEvent, MouseEventKind, MouseScroll, SceneFrame, UiEffect, ViewUpdate,
    WakeEvent,
};
pub(crate) use driver::{UiEffectDriver, ViewUpdateDriver};
pub(crate) use runtime::{IoHandle, IoRuntime, WakeHandle};
