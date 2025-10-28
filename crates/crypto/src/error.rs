//! Crypto errors - using unified error system

// Re-export unified error system
pub use aura_types::{AuraError, ErrorCode, ErrorSeverity, AuraResult as Result};

/// General cryptographic operation error alias
pub type CryptoError = AuraError;
