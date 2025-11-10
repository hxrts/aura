//! TODO fix - Simplified agent error handling using unified error system
//!
//! **CLEANUP**: Replaced complex error extension traits with simple re-exports.
//! This eliminates 100+ lines of error context boilerplate.

pub use aura_core::{AuraError, AuraResult};

/// Agent result type alias using unified error system
pub type Result<T> = AuraResult<T>;
