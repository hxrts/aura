//! Error types for native Quint API
//!
//! This module provides error handling for Quint API operations using the unified
//! AuraError type from aura-core. Previously contained 115 lines of custom error
//! definitions that have been consolidated.

// Re-export the unified error system
pub use aura_core::{AuraError, Result as AuraResult};

/// Result type for Quint API operations
///
/// DEPRECATED: Use `AuraResult<T>` directly for new code.
/// This type alias is maintained for backward compatibility.
pub type QuintResult<T> = AuraResult<T>;

// QuintError removed - use AuraError directly instead
// Legacy type alias has been superseded by unified error system
