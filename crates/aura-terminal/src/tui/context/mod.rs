//! # IoContext and Helpers
//!
//! Focused helper modules that compose to form IoContext functionality.
//!
//! This module breaks down the large IoContext struct into smaller, more focused
//! helpers that each handle a specific concern:
//!
//! - `dispatch`: Command dispatch through Intent and Operational handlers
//! - `snapshots`: Best-effort AppCore snapshots (dispatch context + tests; screens should use signals)
//! - `toasts`: Toast notification management
//! - `io_context`: The main IoContext struct that composes these helpers

pub mod dispatch;
pub mod io_context;
pub mod snapshots;
pub mod toasts;
pub mod initialized_app_core;

pub use dispatch::{AccountFilesHelper, DispatchHelper};
pub use io_context::IoContext;
pub use snapshots::SnapshotHelper;
pub use toasts::ToastHelper;
pub use initialized_app_core::InitializedAppCore;
