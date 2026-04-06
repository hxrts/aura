//! # AppCore: The Portable Application Core
//!
//! This is the main entry point for the application.

pub mod hooks;
pub mod runtime_access;
pub mod signals;
mod state;

#[cfg(feature = "callbacks")]
pub use state::SubscriptionId;
pub use state::{AppConfig, AppCore};
