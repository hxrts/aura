//! Web of Trust error handling using unified error system
//!
//! **DESIGN**: Uses unified AuraError from aura-core for consistency across crates.
//! This eliminates redundant error definitions and provides seamless integration
//! with the broader Aura error ecosystem.

pub use aura_core::{AuraError, AuraResult};

/// WoT result type alias using unified error system
pub type WotResult<T> = AuraResult<T>;

/// Type alias for backward compatibility during migration from custom WotError to unified AuraError
pub type WotError = AuraError;
