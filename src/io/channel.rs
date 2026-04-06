//! Shared channel payloads between the async runtime and the winit thread.

/// Wake-up events delivered through `EventLoopProxy`.
#[derive(Debug, Clone)]
pub(crate) enum WakeEvent {
    Wake,
    DeadlineExpired,
}

/// Async-side IO events consumed by the winit thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IoEvent {
    MockTick { payload: String },
}

impl IoEvent {
    /// Returns a stable event kind for structured logging.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::MockTick { .. } => "mock_tick",
        }
    }
}

/// High-level button state independent of the windowing backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonState {
    Pressed,
    Released,
}

/// High-level mouse button independent of the windowing backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MouseButtonKind {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
}

/// High-level scroll payload independent of the windowing backend.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MouseScroll {
    LineDelta { x: f32, y: f32 },
    PixelDelta { x: f64, y: f64 },
}

/// High-level mouse input emitted toward the async side.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MockMouseEvent {
    Button {
        state: ButtonState,
        button: MouseButtonKind,
    },
    Move {
        x: f64,
        y: f64,
    },
    Scroll {
        delta: MouseScroll,
    },
}

/// Semantic commands emitted by the winit thread toward async tasks.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AppCommand {
    Shutdown,
    MockKeyInput { text: String },
    MockMouseInput { event: MockMouseEvent },
    MockResize { width: u32, height: u32 },
}
