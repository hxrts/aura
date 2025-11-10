//! Cryptographic effects trait definitions
//!
//! This module re-exports the CryptoEffects trait from aura-core to provide
//! a unified interface for cryptographic operations across the system.
//! Effect handlers that integrate aura-crypto are provided by aura-protocol handlers.

// Re-export the comprehensive CryptoEffects trait from aura-core
pub use aura_core::effects::crypto::{FrostSigningPackage, KeyDerivationContext};
pub use aura_core::effects::{CryptoEffects, CryptoError};
