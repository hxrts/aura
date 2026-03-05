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
        *write_guard(&self.text) = text.to_string();
    }

    fn read(&self) -> String {
        read_guard(&self.text).clone()
    }
}

fn read_guard<T>(lock: &RwLock<T>) -> async_lock::RwLockReadGuard<'_, T> {
    loop {
        if let Some(guard) = lock.try_read() {
            return guard;
        }
        std::hint::spin_loop();
    }
}

fn write_guard<T>(lock: &RwLock<T>) -> async_lock::RwLockWriteGuard<'_, T> {
    loop {
        if let Some(guard) = lock.try_write() {
            return guard;
        }
        std::hint::spin_loop();
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
