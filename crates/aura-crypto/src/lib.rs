//! Common cryptographic utilities for Aura
#![allow(clippy::result_large_err)]
#![allow(clippy::disallowed_types)] // Crypto crate needs OsRng for actual cryptographic operations
#![allow(clippy::expect_used)] // Crypto operations sometimes need expect for invariants

/// AES-GCM encryption for content and chunk encryption
pub mod content_encryption;
/// Device key management and secure storage
pub mod device_keys;
/// Deterministic Key Derivation (DKD) for deriving context-specific keys
pub mod dkd;
/// Injectable time and randomness for deterministic testing
pub mod effects;
/// Symmetric encryption abstractions (ChaCha20Poly1305)
pub mod encryption;
/// Unified error handling for cryptographic operations
pub mod error;
/// FROST threshold signatures implementation
pub mod frost;
/// Hash function abstractions (Blake3)
pub mod hash;
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
/// Serialization utilities for cryptographic types (serde helpers)
pub mod serde;
/// Digital signature abstractions (Ed25519)
pub mod signature;
/// Time utilities with proper error handling
pub mod time;
/// Shared types (DeviceId, AccountId, etc.)
pub mod types;
/// UUID utilities and abstractions
pub mod uuid_utils;

pub use content_encryption::*;
pub use device_keys::*;
pub use dkd::*;
pub use effects::*;
pub use encryption::*;
pub use error::*;
pub use frost::*;
pub use hash::*;
pub use hpke_encryption::*;
pub use key_derivation::*;
pub use key_rotation::*;
pub use merkle::*;
pub use resharing::*;
pub use sealing::*;
pub use serde::{signature_serde, verifying_key_serde};
pub use signature::*;
pub use time::*;
pub use types::*;
pub use uuid_utils::*;
