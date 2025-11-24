//! Signature aggregation utilities for recovery operations

use crate::types::RecoveryShare;
use aura_core::frost::ThresholdSignature;

/// Utility functions for signature operations in recovery ceremonies
pub struct SignatureUtils;

impl SignatureUtils {
    /// Aggregate partial signatures from recovery shares into a threshold signature
    ///
    /// This combines signature bytes from all shares and creates a ThresholdSignature
    /// with the appropriate signer list.
    ///
    /// # Parameters
    /// - `shares`: Collection of recovery shares containing partial signatures
    ///
    /// # Returns
    /// A ThresholdSignature combining all partial signatures
    pub fn aggregate_signature(shares: &[RecoveryShare]) -> ThresholdSignature {
        let mut combined_signature = Vec::new();
        for share in shares {
            combined_signature.extend_from_slice(&share.partial_signature);
        }

        let signature_bytes = if combined_signature.len() >= 64 {
            combined_signature[..64].to_vec()
        } else {
            let mut padded = combined_signature;
            padded.resize(64, 0);
            padded
        };

        let signers: Vec<u16> = shares
            .iter()
            .enumerate()
            .map(|(idx, _)| idx as u16)
            .collect();

        ThresholdSignature::new(signature_bytes, signers)
    }

    /// Create an empty threshold signature for error cases
    ///
    /// # Returns
    /// An empty ThresholdSignature with 64 zero bytes and no signers
    pub fn create_empty_signature() -> ThresholdSignature {
        ThresholdSignature::new(vec![0; 64], vec![])
    }

    /// Validate that a recovery share has a proper signature length
    ///
    /// # Parameters
    /// - `share`: Recovery share to validate
    ///
    /// # Returns
    /// `true` if the signature is a reasonable length, `false` otherwise
    pub fn validate_share_signature(share: &RecoveryShare) -> bool {
        !share.partial_signature.is_empty() && share.partial_signature.len() <= 128
    }

    /// Count the number of valid signatures in a collection of shares
    ///
    /// # Parameters
    /// - `shares`: Collection of recovery shares to count
    ///
    /// # Returns
    /// Number of shares with valid signatures
    pub fn count_valid_signatures(shares: &[RecoveryShare]) -> usize {
        shares
            .iter()
            .filter(|share| Self::validate_share_signature(share))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GuardianProfile, RecoveryShare};
    use aura_core::{identifiers::GuardianId, DeviceId, TrustLevel};

    fn create_test_share(signature: Vec<u8>) -> RecoveryShare {
        RecoveryShare {
            guardian: GuardianProfile {
                guardian_id: GuardianId::new(),
                device_id: DeviceId::new(),
                label: "Test Guardian".to_string(),
                trust_level: TrustLevel::High,
                cooldown_secs: 900,
            },
            share: vec![1, 2, 3],
            partial_signature: signature,
            issued_at: 1234567890,
        }
    }

    #[test]
    fn test_aggregate_signature() {
        let shares = vec![
            create_test_share(vec![1; 32]),
            create_test_share(vec![2; 32]),
        ];

        let signature = SignatureUtils::aggregate_signature(&shares);

        // Should have combined the signatures
        assert_eq!(signature.signature.len(), 64);
        assert_eq!(signature.signers.len(), 2);
        assert_eq!(signature.signers, vec![0, 1]);
    }

    #[test]
    fn test_aggregate_signature_padding() {
        let shares = vec![create_test_share(vec![1; 16])];

        let signature = SignatureUtils::aggregate_signature(&shares);

        // Should pad to 64 bytes
        assert_eq!(signature.signature.len(), 64);
        assert_eq!(signature.signers.len(), 1);
    }

    #[test]
    fn test_empty_signature() {
        let signature = SignatureUtils::create_empty_signature();

        assert_eq!(signature.signature.len(), 64);
        assert!(signature.signature.iter().all(|&b| b == 0));
        assert!(signature.signers.is_empty());
    }

    #[test]
    fn test_validate_share_signature() {
        let valid_share = create_test_share(vec![1; 64]);
        let empty_share = create_test_share(vec![]);
        let oversized_share = create_test_share(vec![1; 256]);

        assert!(SignatureUtils::validate_share_signature(&valid_share));
        assert!(!SignatureUtils::validate_share_signature(&empty_share));
        assert!(!SignatureUtils::validate_share_signature(&oversized_share));
    }

    #[test]
    fn test_count_valid_signatures() {
        let shares = vec![
            create_test_share(vec![1; 64]),  // valid
            create_test_share(vec![]),       // invalid - empty
            create_test_share(vec![2; 32]),  // valid
            create_test_share(vec![1; 256]), // invalid - too large
        ];

        assert_eq!(SignatureUtils::count_valid_signatures(&shares), 2);
    }
}
