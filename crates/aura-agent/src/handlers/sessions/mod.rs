//! Session Management Modules
//!
//! This module contains role-focused session management handlers split by concern:
//! - shared: Common types and utilities
//! - coordination: Session coordination handlers
//! - threshold: Threshold operation session handlers
//! - metadata: Session metadata management
//! - service: Public API wrapper

pub mod coordination;
pub mod metadata;
pub mod service;
pub mod shared;
pub mod threshold;

// Re-export the main session operations handler
pub use coordination::SessionOperations;

// Re-export service API for public API
pub use service::SessionServiceApi;

#[deprecated(note = "Use SessionServiceApi instead.")]
pub type SessionService = SessionServiceApi;

// Re-export common types
pub use shared::{SessionHandle, SessionStats};
