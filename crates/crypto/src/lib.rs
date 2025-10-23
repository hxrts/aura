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

pub mod sealing;
pub mod dkd;
pub mod resharing;
pub mod hpke_encryption;
pub mod content_encryption;  // AES-GCM for content/chunk encryption
pub mod frost;
pub mod time;
pub mod effects;  // Injectable time and randomness for deterministic testing
pub mod merkle;   // Merkle tree for DKD commitment roots

pub use sealing::*;
pub use dkd::*;
pub use resharing::*;
pub use hpke_encryption::*;
pub use content_encryption::*;
pub use frost::*;
pub use time::*;
pub use effects::*;  // Export Effects, TimeSource, RandomSource, etc.
pub use merkle::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
    
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),
    
    #[error("Invalid key material: {0}")]
    InvalidKey(String),
    
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Cryptographic error: {0}")]
    CryptoError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("System time error: {0}")]
    SystemTimeError(String),
}

pub type Result<T> = std::result::Result<T, CryptoError>;

