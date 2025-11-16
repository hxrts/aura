//! Unified rendezvous error handling using core error system
//!
//! **CLEANUP**: Replaced custom RendezvousError enum (195 lines with 10+ variants) with
//! unified AuraError from aura-core. This eliminates redundant error definitions while
//! preserving essential error information through structured messages.
//!
//! Following the pattern established by aura-journal and aura-store, all rendezvous
//! errors now use AuraError with appropriate variant selection and rich error messages.

pub use aura_core::{AuraError, AuraResult};

/// Rendezvous result type alias using unified error system
pub type RendezvousResult<T> = AuraResult<T>;

/// Convenience type alias for backward compatibility
pub type RendezvousError = AuraError;

/// Result type for rendezvous operations (backward compatibility)
pub type Result<T> = AuraResult<T>;
