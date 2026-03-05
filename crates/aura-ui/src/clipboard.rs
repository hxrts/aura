//! Clipboard abstraction for cross-platform text operations.
//!
//! Provides a trait-based clipboard interface with platform-specific implementations
//! for native and web environments, plus a memory-backed implementation for testing.

use async_lock::RwLock;

pub trait ClipboardPort: Send + Sync {
    fn write(&self, text: &str);
    fn read(&self) -> String;
}

#[derive(Default)]
pub struct MemoryClipboard {
    text: RwLock<String>,
}

impl ClipboardPort for MemoryClipboard {
    fn write(&self, text: &str) {
        *self.text.write_blocking() = text.to_string();
    }

    fn read(&self) -> String {
        self.text.read_blocking().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipboardPort, MemoryClipboard};

    #[test]
    fn memory_clipboard_round_trip() {
        let clipboard = MemoryClipboard::default();

        assert_eq!(clipboard.read(), "");

        clipboard.write("first");
        assert_eq!(clipboard.read(), "first");

        clipboard.write("second");
        assert_eq!(clipboard.read(), "second");
    }
}
