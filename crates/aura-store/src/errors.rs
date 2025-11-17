//! Unified storage error handling using core error system
//!
//! **CLEANUP**: Replaced custom StorageError enum (300+ lines with 30+ variants) with
//! unified AuraError from aura-core. This eliminates redundant error definitions while
//! preserving essential error information through structured messages.
//!
//! Following the pattern established by aura-journal, all storage errors now use
//! AuraError with appropriate variant selection and rich error messages.

pub use aura_core::{AuraError, AuraResult};

/// Storage result type alias using unified error system
pub type StorageResult<T> = AuraResult<T>;

/// Convenience type alias for backward compatibility
pub type StorageError = AuraError;

/// Result type for storage operations (backward compatibility)
pub type Result<T> = AuraResult<T>;
