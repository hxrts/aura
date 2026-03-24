//! Session Management Modules
//!
//! This module contains role-focused session management handlers split by concern:
//! - shared: Common types and utilities
//! - coordination: Session coordination handlers
//! - threshold: Threshold operation session handlers
//! - metadata: Session metadata management
//! - service: Public API wrapper

/// Typed capability family for session coordination choreography.
pub mod capabilities;
pub mod coordination;
pub mod metadata;
pub mod service;
pub mod shared;
pub mod threshold;

// Re-export the main session operations handler
pub use coordination::SessionOperations;

// Re-export coordination role for tests
pub use coordination::SessionCoordinationRole;

// Re-export service API for public API
pub use service::SessionServiceApi;

// Re-export common types
pub use shared::{SessionHandle, SessionStats};
