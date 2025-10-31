//! Crypto errors - using unified error system

// Re-export unified error system
pub use aura_types::{AuraError, AuraResult as Result, ErrorCode, ErrorSeverity};

/// General cryptographic operation error alias
pub type CryptoError = AuraError;
