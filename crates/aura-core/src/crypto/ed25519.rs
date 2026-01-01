//! Ed25519 signature types and operations

use ed25519_dalek::Signer;
use serde::{Deserialize, Serialize};

/// Basic Ed25519 signature wrapper (bytes form for serialization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ed25519Signature(pub [u8; 64]);

impl Ed25519Signature {
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Convert to fixed-size array.
    pub fn to_bytes(&self) -> [u8; 64] {
        self.0
    }

    /// Try to construct from a slice.
    pub fn try_from_slice(bytes: &[u8]) -> Result<Self, crate::AuraError> {
        let arr: [u8; 64] = bytes
            .try_into()
            .map_err(|_| crate::AuraError::crypto("Ed25519 signature must be exactly 64 bytes"))?;
        Ok(Self(arr))
    }
}

impl From<[u8; 64]> for Ed25519Signature {
    fn from(value: [u8; 64]) -> Self {
        Self(value)
    }
}

impl TryFrom<&[u8]> for Ed25519Signature {
    type Error = crate::AuraError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from_slice(value)
    }
}

impl TryFrom<Vec<u8>> for Ed25519Signature {
    type Error = crate::AuraError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from_slice(&value)
    }
}

/// Basic Ed25519 signing key wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ed25519SigningKey(pub [u8; 32]);

impl Ed25519SigningKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Convert to fixed-size array.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn try_from_slice(bytes: &[u8]) -> Result<Self, crate::AuraError> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| crate::AuraError::crypto("Ed25519 signing key must be exactly 32 bytes"))?;
        Ok(Self(arr))
    }

    pub fn verifying_key(&self) -> Result<Ed25519VerifyingKey, crate::AuraError> {
        let key = ed25519_dalek::SigningKey::from_bytes(&self.0);
        Ok(Ed25519VerifyingKey(key.verifying_key().to_bytes()))
    }

    pub fn sign(&self, message: &[u8]) -> Result<Ed25519Signature, crate::AuraError> {
        let key = ed25519_dalek::SigningKey::from_bytes(&self.0);
        let sig = key.sign(message);
        Ok(Ed25519Signature(sig.to_bytes()))
    }
}

impl TryFrom<&[u8]> for Ed25519SigningKey {
    type Error = crate::AuraError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from_slice(value)
    }
}

impl TryFrom<Vec<u8>> for Ed25519SigningKey {
    type Error = crate::AuraError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from_slice(&value)
    }
}

/// Basic Ed25519 verifying key wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ed25519VerifyingKey(pub [u8; 32]);

impl Ed25519VerifyingKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Result<Self, crate::AuraError> {
        ed25519_dalek::VerifyingKey::from_bytes(&bytes)
            .map(|_| Ed25519VerifyingKey(bytes))
            .map_err(|e| crate::AuraError::crypto(e.to_string()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Convert to fixed-size array.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    pub fn try_from_slice(bytes: &[u8]) -> Result<Self, crate::AuraError> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| crate::AuraError::crypto("invalid public key length"))?;
        Self::from_bytes(arr)
    }

    /// Convenience verification helper.
    pub fn verify(
        &self,
        message: &[u8],
        signature: &Ed25519Signature,
    ) -> Result<(), crate::AuraError> {
        if ed25519_verify(message, signature, self)? {
            Ok(())
        } else {
            Err(crate::AuraError::crypto("signature verification failed"))
        }
    }
}

/// Verify an Ed25519 signature using dalek.
pub fn ed25519_verify(
    message: &[u8],
    signature: &Ed25519Signature,
    public_key: &Ed25519VerifyingKey,
) -> Result<bool, crate::AuraError> {
    let pk = ed25519_dalek::VerifyingKey::from_bytes(&public_key.0)
        .map_err(|e| crate::AuraError::crypto(e.to_string()))?;
    let sig = ed25519_dalek::Signature::from_bytes(&signature.0);
    Ok(pk.verify_strict(message, &sig).is_ok())
}

/// Derive verifying key from signing key bytes.
pub fn ed25519_verifying_key(
    signing_key: &Ed25519SigningKey,
) -> Result<Ed25519VerifyingKey, crate::AuraError> {
    let key = ed25519_dalek::SigningKey::from_bytes(&signing_key.0);
    Ok(Ed25519VerifyingKey(key.verifying_key().to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::serialization;

    #[test]
    fn invalid_lengths_rejected() {
        assert!(Ed25519Signature::try_from_slice(&[0u8; 63]).is_err());
        assert!(Ed25519SigningKey::try_from_slice(&[0u8; 31]).is_err());
        assert!(Ed25519VerifyingKey::try_from_slice(&[0u8; 31]).is_err());
    }

    #[test]
    fn signature_roundtrip_cbor() {
        let signing_key = Ed25519SigningKey::from_bytes([7u8; 32]);
        let message = b"ed25519-test";
        let signature = signing_key.sign(message).expect("signing should succeed");

        let bytes = serialization::to_vec(&signature).expect("serialize signature");
        let decoded: Ed25519Signature =
            serialization::from_slice(&bytes).expect("deserialize signature");

        assert_eq!(signature, decoded);
        assert!(ed25519_verify(message, &decoded, &signing_key.verifying_key().unwrap()).unwrap());
    }

    #[test]
    fn verifying_key_roundtrip_cbor() {
        let signing_key = Ed25519SigningKey::from_bytes([11u8; 32]);
        let verifying_key = signing_key.verifying_key().expect("valid verifying key");

        let bytes = serialization::to_vec(&verifying_key).expect("serialize verifying key");
        let decoded: Ed25519VerifyingKey =
            serialization::from_slice(&bytes).expect("deserialize verifying key");

        assert_eq!(verifying_key, decoded);
    }
}
