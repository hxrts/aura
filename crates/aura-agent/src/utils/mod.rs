//! Utility modules for the agent crate
//!
//! This module provides shared utilities to reduce code duplication:
//! - Error context helpers for cleaner error handling
//! - Storage key formatters for consistent key naming
//! - Time utilities for timestamp generation
//! - ID generation for data, capabilities, etc.
//! - Input validation helpers with builder pattern

pub mod error_ext;
pub mod id_gen;
pub mod storage_keys;
pub mod time;
pub mod validation;

// Re-export commonly used items
pub use error_ext::ResultExt;
pub use id_gen::*;
pub use storage_keys as keys;
pub use time::*;
pub use validation::*;
