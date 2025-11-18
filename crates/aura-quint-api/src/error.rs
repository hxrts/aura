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

/// DEPRECATED: Legacy QuintError type, replaced with unified AuraError
///
/// This type is maintained for backward compatibility. All variants now map
/// to appropriate AuraError variants. New code should use AuraError directly.
#[deprecated(since = "0.1.0", note = "Use AuraError directly instead")]
pub type QuintError = AuraError;
