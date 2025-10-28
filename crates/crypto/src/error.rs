//! Crypto errors - using unified error system

// Re-export unified error system
pub use aura_errors::{AuraError, ErrorCode, ErrorSeverity, Result};

// Type aliases for backward compatibility
/// General cryptographic operation error alias
pub type CryptoError = AuraError;
/// FROST threshold signature error alias
pub type FrostError = AuraError;
/// Deterministic Key Derivation error alias
pub type DkdError = AuraError;
