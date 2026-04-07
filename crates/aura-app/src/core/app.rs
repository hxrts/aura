//! # AppCore: The Portable Application Core
//!
//! This is the main entry point for the application.

mod config;
pub mod hooks;
pub mod runtime_access;
pub mod signals;
mod state;

pub use config::AppConfig;
#[cfg(feature = "callbacks")]
pub use config::SubscriptionId;
pub use state::AppCore;
