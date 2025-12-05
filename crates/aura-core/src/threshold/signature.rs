//! Threshold Signature Types
//!
//! Result types for threshold signing operations.

use serde::{Deserialize, Serialize};

/// Result of a threshold signing operation.
///
/// This is the unified signature type returned by `ThresholdSigningService.sign()`.
/// It contains the aggregate FROST signature plus metadata about who signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdSignature {
    /// The aggregate FROST signature (64 bytes for Ed25519)
    pub signature: Vec<u8>,

    /// How many participants signed
    pub signer_count: u16,

    /// Which participants signed (by FROST index, 1-based)
    ///
    /// This reveals cardinality and which indices participated,
    /// but not the identity mapping (that's stored separately).
    pub signers: Vec<u16>,

    /// Public key package that verifies this signature
    ///
    /// This is the group public key established during DKG.
    pub public_key_package: Vec<u8>,

    /// Epoch when this signature was created
    pub epoch: u64,
}

impl ThresholdSignature {
    /// Create a new threshold signature
    pub fn new(
        signature: Vec<u8>,
        signer_count: u16,
        signers: Vec<u16>,
        public_key_package: Vec<u8>,
        epoch: u64,
    ) -> Self {
        Self {
            signature,
            signer_count,
            signers,
            public_key_package,
            epoch,
        }
    }

    /// Create a single-signer (1-of-1) signature
    ///
    /// Used for bootstrap scenarios and single-device accounts.
    pub fn single_signer(signature: Vec<u8>, public_key_package: Vec<u8>, epoch: u64) -> Self {
        Self {
            signature,
            signer_count: 1,
            signers: vec![1],
            public_key_package,
            epoch,
        }
    }

    /// Check if this is a single-signer signature
    pub fn is_single_signer(&self) -> bool {
        self.signer_count == 1
    }

    /// Get the signature bytes
    pub fn signature_bytes(&self) -> &[u8] {
        &self.signature
    }

    /// Get the public key package bytes
    pub fn public_key_bytes(&self) -> &[u8] {
        &self.public_key_package
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_signer_signature() {
        let sig = ThresholdSignature::single_signer(vec![1, 2, 3], vec![4, 5, 6], 0);

        assert!(sig.is_single_signer());
        assert_eq!(sig.signer_count, 1);
        assert_eq!(sig.signers, vec![1]);
        assert_eq!(sig.epoch, 0);
    }

    #[test]
    fn test_multi_signer_signature() {
        let sig = ThresholdSignature::new(
            vec![1, 2, 3],
            2,
            vec![1, 3], // signers 1 and 3 of a 2-of-3
            vec![4, 5, 6],
            5,
        );

        assert!(!sig.is_single_signer());
        assert_eq!(sig.signer_count, 2);
        assert_eq!(sig.signers, vec![1, 3]);
        assert_eq!(sig.epoch, 5);
    }

    #[test]
    fn test_signature_serialization() {
        let sig = ThresholdSignature::single_signer(vec![1, 2, 3], vec![4, 5, 6], 0);
        let json = serde_json::to_string(&sig).unwrap();
        let restored: ThresholdSignature = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.signature, vec![1, 2, 3]);
        assert_eq!(restored.public_key_package, vec![4, 5, 6]);
        assert!(restored.is_single_signer());
    }
}
