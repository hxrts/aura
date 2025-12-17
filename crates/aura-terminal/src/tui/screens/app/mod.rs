//! # App Screen Module
//!
//! Main application shell with screen navigation and modal management.

mod account_setup_modal;
mod modal_overlays;
mod shell;

// Shell exports
pub use shell::{run_app_with_context, IoApp};
