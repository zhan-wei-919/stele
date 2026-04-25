//! Clipboard access used by event-layer paste producers.

use std::fmt;

/// Error returned when the event layer cannot read clipboard text.
#[derive(Clone, Debug)]
pub(crate) enum ClipboardReadError {
    Unavailable(String),
    ReadFailed(String),
}

/// Error returned when the event layer cannot write clipboard text.
#[derive(Clone, Debug)]
pub(crate) enum ClipboardWriteError {
    Unavailable(String),
    WriteFailed(String),
}

impl fmt::Display for ClipboardReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "unavailable:{error}"),
            Self::ReadFailed(error) => write!(formatter, "read_failed:{error}"),
        }
    }
}

impl fmt::Display for ClipboardWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "unavailable:{error}"),
            Self::WriteFailed(error) => write!(formatter, "write_failed:{error}"),
        }
    }
}

/// Text clipboard provider used by the event router.
pub(crate) trait ClipboardProvider {
    /// Reads UTF-8 text from the platform clipboard.
    fn read_text(&mut self) -> Result<String, ClipboardReadError>;

    /// Writes UTF-8 text to the platform clipboard.
    fn write_text(&mut self, text: String) -> Result<(), ClipboardWriteError>;
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
        self.ensure_clipboard_for_read()?;

        self.clipboard
            .as_mut()
            .expect("clipboard must be initialized")
            .get_text()
            .map_err(|error| ClipboardReadError::ReadFailed(format!("{error:?}")))
    }

    fn write_text(&mut self, text: String) -> Result<(), ClipboardWriteError> {
        self.ensure_clipboard_for_write()?;

        self.clipboard
            .as_mut()
            .expect("clipboard must be initialized")
            .set_text(text)
            .map_err(|error| ClipboardWriteError::WriteFailed(format!("{error:?}")))
    }
}

impl SystemClipboard {
    fn ensure_clipboard_for_read(&mut self) -> Result<(), ClipboardReadError> {
        if self.clipboard.is_none() {
            self.clipboard = Some(
                arboard::Clipboard::new()
                    .map_err(|error| ClipboardReadError::Unavailable(format!("{error:?}")))?,
            );
        }
        Ok(())
    }

    fn ensure_clipboard_for_write(&mut self) -> Result<(), ClipboardWriteError> {
        if self.clipboard.is_none() {
            self.clipboard = Some(
                arboard::Clipboard::new()
                    .map_err(|error| ClipboardWriteError::Unavailable(format!("{error:?}")))?,
            );
        }
        Ok(())
    }
}
