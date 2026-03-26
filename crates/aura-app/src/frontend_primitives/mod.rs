//! Shared frontend primitives for all Layer 7 shells.
//!
//! This module provides platform-agnostic types that every frontend
//! (Dioxus-based or otherwise) consumes. Shell crates may re-export
//! these types under their own aliases but should not fork them.

mod clipboard;
mod operations;
mod task_owner;

pub use clipboard::{ClipboardPort, MemoryClipboard};
pub use operations::FrontendUiOperation;
pub use task_owner::{FrontendTaskOwner, FrontendTaskRuntime};
