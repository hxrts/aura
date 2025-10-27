//! Session type infrastructure for coordination protocols
//!
//! This module contains shared session type infrastructure and utilities
//! used across distributed coordination protocols.

pub mod agent;
pub mod context;
pub mod frost;
pub mod wrapper;

// Re-export shared session type infrastructure
pub use agent::*;
pub use context::*;
pub use frost::*;
pub use wrapper::{SessionProtocol, SessionTypedProtocol};
