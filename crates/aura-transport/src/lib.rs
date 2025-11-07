//! Transport Middleware System
//!
//! This crate provides a composable middleware system for transport operations.
//! All transport functionality is implemented as middleware layers that can be
//! stacked to create custom transport behaviors.

pub mod middleware;
pub mod peers;

// Re-export all middleware components
pub use middleware::*;
pub use peers::*;
