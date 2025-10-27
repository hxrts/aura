//! Common cryptographic utilities for Aura
#![allow(clippy::result_large_err)]

/// AES-GCM encryption for content and chunk encryption
pub mod content_encryption;
/// Device key management and secure storage
pub mod device_keys;
/// Deterministic Key Derivation (DKD) for deriving context-specific keys
pub mod dkd;
/// Injectable time and randomness for deterministic testing
pub mod effects;
/// Unified error handling for cryptographic operations
pub mod error;
/// FROST threshold signatures implementation
pub mod frost;
/// HPKE encryption for guardian shares
pub mod hpke_encryption;
/// Separated key derivation for identity and permission keys
pub mod key_derivation;
/// Coordinated key rotation for independent subsystems
pub mod key_rotation;
/// Merkle tree implementation for commitment verification
pub mod merkle;
/// Key resharing and threshold share management
pub mod resharing;
/// Sealing and encryption of sensitive data
pub mod sealing;
/// Time utilities with proper error handling
pub mod time;
/// Shared types (DeviceId, AccountId, etc.)
pub mod types;

pub use content_encryption::*;
pub use device_keys::*;
pub use dkd::*;
pub use effects::*;
pub use error::*;
pub use frost::*;
pub use hpke_encryption::*;
pub use key_derivation::*;
pub use key_rotation::*;
pub use merkle::*;
pub use resharing::*;
pub use sealing::*;
pub use time::*;
pub use types::*;
