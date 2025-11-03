//! Utility modules for the agent crate
//!
//! This module provides shared utilities to reduce code duplication:
//! - Storage key formatters for consistent key naming
//! - Time utilities for timestamp generation (using Effects system)
//! - Input validation helpers with builder pattern
//!
//! Note: Error context helpers are now in the main error module.
//! Note: Typed identifiers (DataId, CapabilityId, etc.) are re-exported from the crate root.

pub mod storage_keys;
pub mod time;
pub mod validation;

// Re-export commonly used items
pub use storage_keys as keys;
pub use time::*;
pub use validation::*;

/// Extension trait for Result types to add context information
pub trait ResultExt<T, E> {
    /// Add context to an error
    fn with_context<F>(self, f: F) -> Result<T, aura_types::AuraError>
    where
        F: FnOnce() -> String;
}

impl<T, E> ResultExt<T, E> for Result<T, E>
where
    E: std::fmt::Display,
{
    fn with_context<F>(self, f: F) -> Result<T, aura_types::AuraError>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| aura_types::AuraError::Data(aura_types::errors::DataError::LedgerOperationFailed {
            message: f(),
            context: e.to_string(),
        }))
    }
}
