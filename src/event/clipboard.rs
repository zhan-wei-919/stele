//! Clipboard access used by event-layer paste producers.

use std::fmt;

/// Error returned when the event layer cannot read clipboard text.
#[derive(Clone, Debug)]
pub(crate) enum ClipboardReadError {
    Unavailable(String),
    ReadFailed(String),
}

impl fmt::Display for ClipboardReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "unavailable:{error}"),
            Self::ReadFailed(error) => write!(formatter, "read_failed:{error}"),
        }
    }
}

/// Text clipboard provider used by the event router.
pub(crate) trait ClipboardProvider {
    /// Reads UTF-8 text from the platform clipboard.
    fn read_text(&mut self) -> Result<String, ClipboardReadError>;
}

/// System clipboard provider backed by `arboard`.
#[derive(Default)]
pub(crate) struct SystemClipboard {
    clipboard: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    /// Creates a provider that initializes the platform clipboard on first use.
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

impl ClipboardProvider for SystemClipboard {
    fn read_text(&mut self) -> Result<String, ClipboardReadError> {
        if self.clipboard.is_none() {
            self.clipboard = Some(
                arboard::Clipboard::new()
                    .map_err(|error| ClipboardReadError::Unavailable(format!("{error:?}")))?,
            );
        }

        self.clipboard
            .as_mut()
            .expect("clipboard must be initialized")
            .get_text()
            .map_err(|error| ClipboardReadError::ReadFailed(format!("{error:?}")))
    }
}
