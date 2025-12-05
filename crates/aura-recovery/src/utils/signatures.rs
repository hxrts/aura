//! Signature utilities for recovery operations
//!
//! # Guardian Signing Model
//!
//! Each guardian signs recovery approvals individually using their own authority's
//! FROST keys. The signatures are NOT aggregated across guardians - instead, each
//! guardian's signature is verified separately against their authority's public key.
//!
//! The recovery threshold is about counting unique valid guardian signatures,
//! not about FROST threshold aggregation.
//!
//! ## Flow
//! 1. Guardian receives recovery request
//! 2. Guardian signs using `ThresholdSigningEffects::sign()` with `ApprovalContext::RecoveryAssistance`
//! 3. The signature is stored in `RecoveryShare.partial_signature`
//! 4. Recovery coordinator verifies each guardian's signature individually
//! 5. Threshold is met when enough guardians have submitted valid signatures

use crate::types::RecoveryShare;
use aura_core::threshold::ThresholdSignature;

/// Utility functions for signature operations in recovery ceremonies.
///
/// Note: Guardian signatures are individual threshold signatures from each guardian's
/// authority. They are NOT aggregated across guardians since each guardian signs with
/// different keys.
pub struct SignatureUtils;

impl SignatureUtils {
    /// Get the signature from a recovery share as a ThresholdSignature.
    ///
    /// Each share contains an individual guardian's signature from their
    /// authority's FROST keys.
    pub fn share_signature(share: &RecoveryShare) -> ThresholdSignature {
        ThresholdSignature::new(
            share.partial_signature.clone(),
            1,          // Single signer (the guardian's authority)
            vec![1],    // Signer index
            Vec::new(), // Public key would need to be looked up
            0,          // Epoch
        )
    }

    /// Create an empty threshold signature for error cases.
    pub fn empty() -> ThresholdSignature {
        ThresholdSignature::new(vec![0u8; 64], 0, Vec::new(), Vec::new(), 0)
    }

    /// Validate that a recovery share has a properly-sized signature.
    ///
    /// A valid Ed25519 signature is exactly 64 bytes. FROST threshold signatures
    /// also produce 64-byte aggregated signatures.
    pub fn validate_share(share: &RecoveryShare) -> bool {
        // Ed25519/FROST signatures should be exactly 64 bytes
        share.partial_signature.len() == 64
    }

    /// Validate that a recovery share has a non-empty signature.
    ///
    /// Less strict validation - just checks that the share has some signature data.
    pub fn has_signature(share: &RecoveryShare) -> bool {
        !share.partial_signature.is_empty()
    }

    /// Count the number of shares with valid signatures.
    pub fn count_valid(shares: &[RecoveryShare]) -> usize {
        shares.iter().filter(|s| Self::validate_share(s)).count()
    }

    /// Count the number of shares with any signature (non-empty).
    pub fn count_with_signatures(shares: &[RecoveryShare]) -> usize {
        shares.iter().filter(|s| Self::has_signature(s)).count()
    }

    /// Collect signatures from shares for evidence.
    ///
    /// Returns all signature bytes for storage in recovery evidence.
    /// These are individual signatures, not aggregated.
    pub fn collect_signatures(shares: &[RecoveryShare]) -> Vec<Vec<u8>> {
        shares
            .iter()
            .filter(|s| Self::has_signature(s))
            .map(|s| s.partial_signature.clone())
            .collect()
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
    fn test_share_signature() {
        let share = create_test_share(vec![1; 64]);
        let sig = SignatureUtils::share_signature(&share);

        assert_eq!(sig.signature_bytes().len(), 64);
        assert!(sig.is_single_signer());
    }

    #[test]
    fn test_empty_signature() {
        let signature = SignatureUtils::empty();

        assert_eq!(signature.signature_bytes().len(), 64);
        assert!(signature.signers.is_empty());
    }

    #[test]
    fn test_validate_share_exact_size() {
        let valid_share = create_test_share(vec![1; 64]); // Exactly 64 bytes
        let short_share = create_test_share(vec![1; 32]); // Too short
        let long_share = create_test_share(vec![1; 128]); // Too long
        let empty_share = create_test_share(vec![]);

        assert!(SignatureUtils::validate_share(&valid_share));
        assert!(!SignatureUtils::validate_share(&short_share));
        assert!(!SignatureUtils::validate_share(&long_share));
        assert!(!SignatureUtils::validate_share(&empty_share));
    }

    #[test]
    fn test_has_signature() {
        let share_with_sig = create_test_share(vec![1; 32]);
        let empty_share = create_test_share(vec![]);

        assert!(SignatureUtils::has_signature(&share_with_sig));
        assert!(!SignatureUtils::has_signature(&empty_share));
    }

    #[test]
    fn test_count_valid_signatures() {
        let shares = vec![
            create_test_share(vec![1; 64]), // valid - exactly 64 bytes
            create_test_share(vec![]),      // invalid - empty
            create_test_share(vec![2; 64]), // valid - exactly 64 bytes
            create_test_share(vec![1; 32]), // invalid - wrong size
        ];

        assert_eq!(SignatureUtils::count_valid(&shares), 2);
    }

    #[test]
    fn test_collect_signatures() {
        let shares = vec![
            create_test_share(vec![1; 64]),
            create_test_share(vec![]),
            create_test_share(vec![2; 64]),
        ];

        let collected = SignatureUtils::collect_signatures(&shares);

        assert_eq!(collected.len(), 2); // Only non-empty
        assert_eq!(collected[0], vec![1; 64]);
        assert_eq!(collected[1], vec![2; 64]);
    }
}
