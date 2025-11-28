//! Domain event types for the journal
//!
//! This module contains event types that represent facts and operations
//! that can be recorded in the journal.

pub mod maintenance;

// Re-export commonly used event types
pub use maintenance::{AdminReplaced, MaintenanceEvent};
