//! # IoContext and Helpers
//!
//! Focused helper modules that compose to form IoContext functionality.
//!
//! This module breaks down the large IoContext struct into smaller, more focused
//! helpers that each handle a specific concern:
//!
//! - `dispatch`: Command dispatch through Intent and Operational handlers
//! - `snapshots`: ViewState snapshot access for initial rendering
//! - `toasts`: Toast notification management
//! - `io_context`: The main IoContext struct that composes these helpers

pub mod dispatch;
pub mod io_context;
pub mod snapshots;
pub mod toasts;

pub use dispatch::DispatchHelper;
pub use io_context::IoContext;
pub use toasts::ToastHelper;

// SnapshotHelper is not yet used - snapshot logic remains in IoContext for now
