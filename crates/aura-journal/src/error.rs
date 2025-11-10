//! TODO fix - Simplified journal error handling using unified error system
//!
//! **CLEANUP**: Replaced custom Error enum with unified AuraError from aura-core.
//! This eliminates 133 lines of redundant error definitions while preserving
//! essential error information through structured messages.

pub use aura_core::{AuraError, AuraResult};

/// Journal result type alias using unified error system
pub type Result<T> = AuraResult<T>;
