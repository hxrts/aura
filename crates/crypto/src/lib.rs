//! Common cryptographic utilities for Aura
//!
//! This crate provides cryptographic primitives for the Aura platform:
//! - P2P Deterministic Key Derivation (DKD)
//! - FROST threshold signatures (Ed25519)
//! - HPKE encryption for guardian shares
//! - Resharing for key rotation
//!
//! # Security Model
//!
//! All operations use:
//! - Ed25519 for signatures
//! - HPKE for public key encryption
//! - FROST for threshold signatures
//! - Curve25519 for all elliptic curve operations
//!
//! # Production Deployment
//!
//! Key shares MUST be stored in platform-specific secure storage:
//! - iOS: Secure Enclave / Keychain
//! - Android: AndroidKeyStore with StrongBox
//! - macOS: Keychain
//! - Windows: DPAPI or Windows Hello
//! - Linux: Secret Service API

/// AES-GCM encryption for content and chunk encryption
pub mod content_encryption;
/// Device key management and secure storage
pub mod device_keys;
/// Deterministic Key Derivation (DKD) for deriving context-specific keys
pub mod dkd;
/// Injectable time and randomness for deterministic testing
pub mod effects;
/// FROST threshold signatures implementation
pub mod frost;
/// HPKE encryption for guardian shares
pub mod hpke_encryption;
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
pub use effects::*; // Export Effects, TimeSource, RandomSource, etc.
pub use frost::*;
pub use hpke_encryption::*;
pub use merkle::*;
pub use resharing::*;
pub use sealing::*;
pub use time::*;
pub use types::*; // Export shared types

use thiserror::Error;

/// Error types for cryptographic operations
#[derive(Error, Debug)]
pub enum CryptoError {
    /// Encryption operation failed
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    /// Decryption operation failed
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    /// Invalid key material provided
    #[error("Invalid key material: {0}")]
    InvalidKey(String),

    /// Invalid signature encountered
    #[error("Invalid signature")]
    InvalidSignature,

    /// General cryptographic error
    #[error("Cryptographic error: {0}")]
    CryptoError(String),

    /// Serialization/deserialization failed
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// System time access failed
    #[error("System time error: {0}")]
    SystemTimeError(String),
}

/// Result type for cryptographic operations
pub type Result<T> = std::result::Result<T, CryptoError>;
