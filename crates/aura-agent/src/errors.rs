//! TODO fix - Simplified agent error handling using unified error system
//!
//! **CLEANUP**: Replaced custom AgentError enum with unified AuraError from aura-core.
//! This eliminates 150+ lines of redundant error definitions and conversion boilerplate.

pub use aura_core::{AuraError, AuraResult};

/// Agent result type alias using unified error system
pub type Result<T> = AuraResult<T>;
