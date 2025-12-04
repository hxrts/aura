//! AMP Cryptographic Utilities
//!
//! This module provides cryptographic primitives for the Asynchronous Message Protocol (AMP).
//! Following the 8-layer architecture, these are **Layer 1 (Foundation)** pure functions
//! with no side effects or effect handlers.
//!
//! ## Core Operations
//!
//! - **Nonce Derivation**: Deterministic nonce generation from AMP headers
//! - **Key Derivation**: KDF path for deriving AMP message keys from ratchet state
//! - **AEAD Operations**: Pure function wrappers for AES-GCM encryption/decryption
//!
//! ## Security Properties
//!
//! - Nonces are derived deterministically from (ratchet_gen, chan_epoch)
//! - Each nonce is unique per ratchet generation to prevent reuse
//! - Message keys are derived using HKDF with domain separation
//!
//! ## Usage
//!
//! These pure functions can be called directly for deterministic operations.
//! For effect-based crypto operations, use the CryptoEffects trait from aura-effects (Layer 3).

use crate::{AuraError, AuraResult, Hash32};

/// Derive a deterministic nonce from an AMP header.
///
/// The nonce is derived from the ratchet generation and channel epoch to ensure:
/// 1. Each ratchet generation produces a unique nonce
/// 2. Nonces are deterministic for the same (ratchet_gen, chan_epoch) pair
/// 3. 96-bit nonces are sufficient for AES-GCM (NIST recommended)
///
/// # Format
///
/// ```text
/// [ratchet_gen (8 bytes) | chan_epoch (4 bytes)]
/// ```
///
/// # Arguments
///
/// * `ratchet_gen` - Current ratchet generation counter
/// * `chan_epoch` - Channel epoch number
///
/// # Returns
///
/// A 96-bit (12-byte) nonce suitable for AES-GCM
pub fn derive_nonce_from_ratchet(ratchet_gen: u64, chan_epoch: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..8].copy_from_slice(&ratchet_gen.to_le_bytes());
    nonce[8..].copy_from_slice(&chan_epoch.to_le_bytes()[..4]);
    nonce
}

/// Derive an AMP message key using HKDF from a master ratchet key.
///
/// This implements the single KDF path for AMP message keys using HKDF-SHA256
/// with domain separation to prevent key reuse across different contexts.
///
/// # KDF Chain
///
/// ```text
/// message_key = HKDF-Expand(
///     HKDF-Extract(salt=ratchet_gen, ikm=master_key),
///     info="AMP_MESSAGE_v1" || context || channel || chan_epoch || ratchet_gen,
///     L=32
/// )
/// ```
///
/// # Arguments
///
/// * `master_key` - Master ratchet key (32 bytes)
/// * `context` - Context identifier
/// * `channel` - Channel identifier
/// * `chan_epoch` - Channel epoch number
/// * `ratchet_gen` - Ratchet generation number
///
/// # Returns
///
/// A 32-byte message key for AES-GCM encryption
///
/// # Security
///
/// - Domain separation via info string prevents key reuse
/// - Ratchet generation as salt ensures forward secrecy
/// - HKDF provides cryptographic strength key derivation
///
/// **Note**: Uses `sha2::Sha256` directly for HKDF key derivation, which is a
/// cryptographic primitive operation and exempted from the general hash centralization
/// policy. Key derivation requires algorithm-specific properties.
#[allow(clippy::disallowed_types)] // HKDF-SHA256 is a cryptographic primitive for key derivation
pub fn derive_message_key(
    master_key: &Hash32,
    context: &[u8],
    channel: &[u8],
    chan_epoch: u64,
    ratchet_gen: u64,
) -> AuraResult<Hash32> {
    use hkdf::Hkdf;
    use sha2::Sha256;

    // Use ratchet_gen as salt for forward secrecy
    let salt = ratchet_gen.to_le_bytes();
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), master_key.as_bytes());

    // Build info string with domain separation
    let mut info = Vec::with_capacity(128);
    info.extend_from_slice(b"AMP_MESSAGE_v1");
    info.extend_from_slice(context);
    info.extend_from_slice(channel);
    info.extend_from_slice(&chan_epoch.to_le_bytes());
    info.extend_from_slice(&ratchet_gen.to_le_bytes());

    // Expand to 32-byte message key
    let mut output = [0u8; 32];
    hkdf.expand(&info, &mut output)
        .map_err(|e| AuraError::crypto(format!("HKDF expansion failed: {}", e)))?;

    Ok(Hash32::from(output))
}

// XOR cipher function removed - was insecure and deprecated
// Use AES-GCM via CryptoEffects for production encryption

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonce_derivation_deterministic() {
        let nonce1 = derive_nonce_from_ratchet(42, 1);
        let nonce2 = derive_nonce_from_ratchet(42, 1);
        assert_eq!(nonce1, nonce2, "Nonces should be deterministic");
    }

    #[test]
    fn test_nonce_derivation_unique() {
        let nonce1 = derive_nonce_from_ratchet(42, 1);
        let nonce2 = derive_nonce_from_ratchet(43, 1);
        assert_ne!(
            nonce1, nonce2,
            "Different ratchet_gen should produce different nonces"
        );

        let nonce3 = derive_nonce_from_ratchet(42, 2);
        assert_ne!(
            nonce1, nonce3,
            "Different chan_epoch should produce different nonces"
        );
    }

    #[test]
    fn test_nonce_format() {
        let ratchet_gen = 0x0123456789ABCDEFu64;
        let chan_epoch = 0x12345678u64;
        let nonce = derive_nonce_from_ratchet(ratchet_gen, chan_epoch);

        // First 8 bytes should be ratchet_gen in little-endian
        assert_eq!(&nonce[..8], &ratchet_gen.to_le_bytes());
        // Next 4 bytes should be first 4 bytes of chan_epoch in little-endian
        assert_eq!(&nonce[8..], &chan_epoch.to_le_bytes()[..4]);
    }

    #[test]
    fn test_message_key_derivation() {
        let master_key = Hash32::from([1u8; 32]);
        let context = b"test_context";
        let channel = b"test_channel";

        let key1 = derive_message_key(&master_key, context, channel, 1, 42).unwrap();
        let key2 = derive_message_key(&master_key, context, channel, 1, 42).unwrap();

        assert_eq!(key1, key2, "Key derivation should be deterministic");
    }

    #[test]
    fn test_message_key_uniqueness() {
        let master_key = Hash32::from([1u8; 32]);
        let context = b"test_context";
        let channel = b"test_channel";

        let key1 = derive_message_key(&master_key, context, channel, 1, 42).unwrap();
        let key2 = derive_message_key(&master_key, context, channel, 1, 43).unwrap();
        let key3 = derive_message_key(&master_key, context, channel, 2, 42).unwrap();

        assert_ne!(
            key1, key2,
            "Different ratchet_gen should produce different keys"
        );
        assert_ne!(
            key1, key3,
            "Different chan_epoch should produce different keys"
        );
    }
}
