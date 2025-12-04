//! Signature aggregation utilities for recovery operations

use crate::types::RecoveryShare;
use aura_core::frost::ThresholdSignature;

/// Utility functions for signature operations in recovery ceremonies.
pub struct SignatureUtils;

impl SignatureUtils {
    /// Aggregate partial signatures from recovery shares into a threshold signature.
    pub fn aggregate(shares: &[RecoveryShare]) -> ThresholdSignature {
        let mut combined_signature = Vec::new();
        for share in shares {
            combined_signature.extend_from_slice(&share.partial_signature);
        }

        // Pad or truncate to 64 bytes
        let signature_bytes = if combined_signature.len() >= 64 {
            combined_signature[..64].to_vec()
        } else {
            let mut padded = combined_signature;
            padded.resize(64, 0);
            padded
        };

        // Use indices (0, 1, 2, ...) for signers since we don't have participant mapping
        let signers: Vec<u16> = (0..shares.len() as u16).collect();

        ThresholdSignature::new(signature_bytes, signers)
    }

    /// Create an empty threshold signature for error cases.
    pub fn empty() -> ThresholdSignature {
        ThresholdSignature::new(vec![0u8; 64], Vec::new())
    }

    /// Validate that a recovery share has a proper signature.
    pub fn validate_share(share: &RecoveryShare) -> bool {
        !share.partial_signature.is_empty() && share.partial_signature.len() <= 128
    }

    /// Count the number of valid signatures in a collection of shares.
    pub fn count_valid(shares: &[RecoveryShare]) -> usize {
        shares.iter().filter(|s| Self::validate_share(s)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::AuthorityId;

    fn create_test_share(signature: Vec<u8>) -> RecoveryShare {
        RecoveryShare {
            guardian_id: AuthorityId::new_from_entropy([0u8; 32]),
            guardian_label: Some("Test Guardian".to_string()),
            share: vec![1, 2, 3],
            partial_signature: signature,
            issued_at_ms: 1234567890,
        }
    }

    #[test]
    fn test_aggregate_signature() {
        let shares = vec![
            create_test_share(vec![1; 32]),
            create_test_share(vec![2; 32]),
        ];

        let signature = SignatureUtils::aggregate(&shares);

        assert_eq!(signature.signature.len(), 64);
        assert_eq!(signature.signers.len(), 2);
    }

    #[test]
    fn test_aggregate_signature_padding() {
        let shares = vec![create_test_share(vec![1; 16])];

        let signature = SignatureUtils::aggregate(&shares);

        assert_eq!(signature.signature.len(), 64);
        assert_eq!(signature.signers.len(), 1);
    }

    #[test]
    fn test_empty_signature() {
        let signature = SignatureUtils::empty();

        assert_eq!(signature.signature.len(), 64);
        assert!(signature.signers.is_empty());
    }

    #[test]
    fn test_validate_share_signature() {
        let valid_share = create_test_share(vec![1; 64]);
        let empty_share = create_test_share(vec![]);
        let oversized_share = create_test_share(vec![1; 256]);

        assert!(SignatureUtils::validate_share(&valid_share));
        assert!(!SignatureUtils::validate_share(&empty_share));
        assert!(!SignatureUtils::validate_share(&oversized_share));
    }

    #[test]
    fn test_count_valid_signatures() {
        let shares = vec![
            create_test_share(vec![1; 64]),  // valid
            create_test_share(vec![]),       // invalid - empty
            create_test_share(vec![2; 32]),  // valid
            create_test_share(vec![1; 256]), // invalid - too large
        ];

        assert_eq!(SignatureUtils::count_valid(&shares), 2);
    }
}
