//! Evidence creation utilities for recovery operations

use crate::types::{RecoveryEvidence, RecoveryShare};
use aura_core::identifiers::{AuthorityId, ContextId};

/// Builder for creating recovery evidence.
pub struct EvidenceBuilder;

impl EvidenceBuilder {
    /// Create evidence for a successful recovery operation.
    pub fn success(
        context_id: ContextId,
        account_id: AuthorityId,
        shares: &[RecoveryShare],
        completed_at_ms: u64,
    ) -> RecoveryEvidence {
        let approving_guardians = shares.iter().map(|s| s.guardian_id).collect();

        RecoveryEvidence {
            context_id,
            account_id,
            approving_guardians,
            completed_at_ms,
            dispute_window_ends_at_ms: completed_at_ms + 3_600_000, // 1 hour in ms
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }

    /// Create evidence for a failed recovery operation.
    pub fn failed(
        context_id: ContextId,
        account_id: AuthorityId,
        timestamp_ms: u64,
    ) -> RecoveryEvidence {
        RecoveryEvidence {
            context_id,
            account_id,
            approving_guardians: Vec::new(),
            completed_at_ms: timestamp_ms,
            dispute_window_ends_at_ms: timestamp_ms,
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_success_evidence() {
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let account_id = AuthorityId::new_from_entropy([2u8; 32]);
        let shares = vec![
            RecoveryShare {
                guardian_id: AuthorityId::new_from_entropy([3u8; 32]),
                guardian_label: Some("Guardian 1".to_string()),
                share: vec![1, 2, 3],
                partial_signature: vec![4, 5, 6],
                issued_at_ms: 1000,
            },
            RecoveryShare {
                guardian_id: AuthorityId::new_from_entropy([4u8; 32]),
                guardian_label: Some("Guardian 2".to_string()),
                share: vec![7, 8, 9],
                partial_signature: vec![10, 11, 12],
                issued_at_ms: 2000,
            },
        ];

        let evidence = EvidenceBuilder::success(context_id, account_id, &shares, 5000);

        assert_eq!(evidence.context_id, context_id);
        assert_eq!(evidence.account_id, account_id);
        assert_eq!(evidence.approving_guardians.len(), 2);
        assert_eq!(evidence.completed_at_ms, 5000);
        assert_eq!(evidence.dispute_window_ends_at_ms, 5000 + 3_600_000);
    }

    #[test]
    fn test_create_failed_evidence() {
        let context_id = ContextId::new_from_entropy([5u8; 32]);
        let account_id = AuthorityId::new_from_entropy([6u8; 32]);

        let evidence = EvidenceBuilder::failed(context_id, account_id, 1000);

        assert_eq!(evidence.context_id, context_id);
        assert_eq!(evidence.account_id, account_id);
        assert!(evidence.approving_guardians.is_empty());
        assert_eq!(evidence.completed_at_ms, 1000);
    }

    #[test]
    fn test_default_evidence() {
        let evidence = RecoveryEvidence::default();

        assert_eq!(evidence.completed_at_ms, 0);
        assert!(evidence.approving_guardians.is_empty());
        assert!(evidence.disputes.is_empty());
        assert!(evidence.threshold_signature.is_none());
    }
}
