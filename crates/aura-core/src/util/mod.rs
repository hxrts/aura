//! Utility Modules
//!
//! Common utilities for serialization, type conversions, context derivation,
//! and testing infrastructure.
//!
//! **Layer 1**: Foundation utilities with no domain logic.

pub mod context;
pub mod conversions;
pub mod serialization;
#[doc(hidden)]
pub mod test_utils;

// Re-export public types for convenience
pub use context::{
    ContextDerivationService, ContextParams, DkdContextDerivation, GroupConfiguration,
    GroupContextDerivation, RelayContextDerivation,
};
pub use serialization::{
    from_slice, hash_canonical, to_vec, SemanticVersion as SerVersion, SerializationError,
    VersionedMessage,
};
// conversions module is internal helpers, no re-exports
// test_utils is hidden and used internally
