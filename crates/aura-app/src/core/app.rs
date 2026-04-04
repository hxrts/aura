//! # AppCore: The Portable Application Core
//!
//! This is the main entry point for the application.

pub mod hooks;
#[allow(dead_code)]
mod legacy;
pub mod runtime_access;
pub mod signals;

#[cfg(feature = "callbacks")]
pub use legacy::SubscriptionId;
pub use legacy::{AppConfig, AppCore};
