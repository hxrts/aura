//! Serde utilities for cryptographic types

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Serde module for VerifyingKey serialization
pub mod verifying_key_serde {
    use super::*;

    /// Serialize a VerifyingKey as bytes
    pub fn serialize<S>(key: &VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        key.as_bytes().serialize(serializer)
    }

    /// Deserialize a VerifyingKey from bytes
    pub fn deserialize<'de, D>(deserializer: D) -> Result<VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: [u8; 32] = Deserialize::deserialize(deserializer)?;
        VerifyingKey::from_bytes(&bytes)
            .map_err(|e| serde::de::Error::custom(format!("Invalid verifying key: {}", e)))
    }
}

/// Serde module for Ed25519 Signature serialization
pub mod signature_serde {
    use super::*;

    /// Serialize a Signature as bytes
    pub fn serialize<S>(sig: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        sig.to_bytes().to_vec().serialize(serializer)
    }

    /// Deserialize a Signature from bytes
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        let sig_bytes: [u8; 64] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| serde::de::Error::invalid_length(bytes.len(), &"64 bytes"))?;
        Ok(Signature::from_bytes(&sig_bytes))
    }
}