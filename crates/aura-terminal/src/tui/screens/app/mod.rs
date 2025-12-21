//! # App Screen Module
//!
//! Main application shell with screen navigation and modal management.

mod modal_overlays;
mod shell;
pub mod subscriptions;

// Shell exports
pub use shell::{run_app_with_context, IoApp};
