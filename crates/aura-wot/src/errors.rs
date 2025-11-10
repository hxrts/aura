//! TODO fix - Simplified Web of Trust operations using unified error system
//!
//! **CLEANUP**: Replaced custom WotError enum with unified AuraError from aura-core.
//! This eliminates 33 lines of redundant error definitions and conversion boilerplate.

pub use aura_core::{AuraError, AuraResult};

/// WoT result type alias using unified error system
pub type WotResult<T> = AuraResult<T>;

/// Type alias for backward compatibility during migration from custom WotError to unified AuraError
pub type WotError = AuraError;
