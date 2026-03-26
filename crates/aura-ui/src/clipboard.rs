//! Clipboard abstraction for cross-platform text operations.
//!
//! Re-exported from `aura-app::frontend_primitives` where the canonical
//! definition lives. This module exists for backward compatibility so
//! existing `aura_ui::ClipboardPort` imports continue to resolve.

pub use aura_app::frontend_primitives::{ClipboardPort, MemoryClipboard};
