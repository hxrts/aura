//! Serialization utilities for cryptographic types
//!
//! This module provides reusable serde helpers for common cryptographic types,
//! eliminating duplication across the codebase. All serialization modules should
//! be imported from this central location.
//!
//! # Usage
//!
//! For Ed25519 signatures:
//! ```ignore
//! use aura_crypto::serde::signature_serde;
//!
//! #[derive(Serialize, Deserialize)]
//! struct MyType {
//!     #[serde(with = "signature_serde")]
//!     signature: Ed25519Signature,
//! }
//! ```
//!
//! For Ed25519 verifying keys:
//! ```ignore
//! use aura_crypto::serde::verifying_key_serde;
//!
//! #[derive(Serialize, Deserialize)]
//! struct MyType {
//!     #[serde(with = "verifying_key_serde")]
//!     public_key: Ed25519VerifyingKey,
//! }
//! ```

use crate::{Ed25519Signature, Ed25519VerifyingKey};
use serde::{Deserialize, Deserializer, Serializer};

/// Serde module for Ed25519 signatures
///
/// Serializes Ed25519 signatures as byte arrays for compact representation.
/// Use with `#[serde(with = "signature_serde")]` attribute.
pub mod signature_serde {
    use super::*;

    /// Serialize an Ed25519 signature to bytes
    pub fn serialize<S>(sig: &Ed25519Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&crate::ed25519_signature_to_bytes(sig))
    }

    /// Deserialize an Ed25519 signature from bytes
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Ed25519Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        let bytes_array: [u8; 64] = bytes
            .as_slice()
            .try_into()
            .map_err(serde::de::Error::custom)?;
        crate::ed25519_signature_from_bytes(&bytes_array).map_err(serde::de::Error::custom)
    }
}

/// Serde module for Ed25519 verifying keys (public keys)
///
/// Serializes Ed25519 verifying keys as byte arrays for compact representation.
/// Use with `#[serde(with = "verifying_key_serde")]` attribute.
pub mod verifying_key_serde {
    use super::*;

    /// Serialize an Ed25519 verifying key to bytes
    pub fn serialize<S>(key: &Ed25519VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&crate::ed25519_verifying_key_to_bytes(key))
    }

    /// Deserialize an Ed25519 verifying key from bytes
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Ed25519VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        let bytes_array: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(serde::de::Error::custom)?;
        crate::ed25519_verifying_key_from_bytes(&bytes_array).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct SignatureContainer {
        #[serde(with = "signature_serde")]
        sig: Ed25519Signature,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct KeyContainer {
        #[serde(with = "verifying_key_serde")]
        key: Ed25519VerifyingKey,
    }

    #[test]
    fn test_signature_serde() {
        let effects = Effects::test();
        let (sk, pk) = crate::ed25519_keypair(&effects);

        // Sign some data
        let data = b"test data";
        let sig = crate::ed25519_sign(&sk, data);

        // Serialize and deserialize
        let container = SignatureContainer { sig: sig.clone() };
        let json = serde_json::to_string(&container).expect("should serialize");
        let restored: SignatureContainer = serde_json::from_str(&json).expect("should deserialize");

        assert_eq!(container.sig, restored.sig);
    }

    #[test]
    fn test_verifying_key_serde() {
        let effects = Effects::test();
        let (_sk, pk) = crate::ed25519_keypair(&effects);

        // Serialize and deserialize
        let container = KeyContainer { key: pk.clone() };
        let json = serde_json::to_string(&container).expect("should serialize");
        let restored: KeyContainer = serde_json::from_str(&json).expect("should deserialize");

        assert_eq!(container.key, restored.key);
    }

    #[test]
    fn test_cbor_signature_serde() {
        let effects = Effects::test();
        let (sk, _pk) = crate::ed25519_keypair(&effects);

        // Sign some data
        let data = b"test data";
        let sig = crate::ed25519_sign(&sk, data);

        // Serialize and deserialize with CBOR
        let container = SignatureContainer { sig: sig.clone() };
        let cbor = serde_cbor::to_vec(&container).expect("should serialize");
        let restored: SignatureContainer =
            serde_cbor::from_slice(&cbor).expect("should deserialize");

        assert_eq!(container.sig, restored.sig);
    }
}
