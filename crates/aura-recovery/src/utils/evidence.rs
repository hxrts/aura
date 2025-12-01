//! Evidence creation utilities for recovery operations

use crate::types::{RecoveryEvidence, RecoveryShare};
use aura_core::{AccountId, DeviceId};

/// Builder for creating recovery evidence with consistent patterns
pub struct EvidenceBuilder;

impl EvidenceBuilder {
    /// Create evidence for a successful recovery operation
    ///
    /// # Parameters
    /// - `account_id`: Account that was recovered
    /// - `recovering_device`: Device that initiated the recovery
    /// - `shares`: Guardian shares collected during recovery
    ///
    /// # Returns
    /// RecoveryEvidence with populated fields based on the operation
    pub fn create_success_evidence(
        account_id: AccountId,
        recovering_device: DeviceId,
        shares: &[RecoveryShare],
    ) -> RecoveryEvidence {
        Self::create_success_evidence_with_time(account_id, recovering_device, shares, 1234567890)
    }

    /// Create evidence for a successful recovery operation with explicit timestamp
    ///
    /// # Parameters
    /// - `account_id`: Account that was recovered
    /// - `recovering_device`: Device that initiated the recovery
    /// - `shares`: Guardian shares collected during recovery
    /// - `current_time`: Current timestamp in seconds since epoch
    ///
    /// # Returns
    /// RecoveryEvidence with populated fields based on the operation
    pub fn create_success_evidence_with_time(
        account_id: AccountId,
        recovering_device: DeviceId,
        shares: &[RecoveryShare],
        current_time: u64,
    ) -> RecoveryEvidence {
        let guardian_ids = shares
            .iter()
            .map(|share| share.guardian.guardian_id)
            .collect();

        let guardian_profiles = shares.iter().map(|share| share.guardian.clone()).collect();

        RecoveryEvidence {
            account_id,
            recovering_device,
            guardians: guardian_ids,
            issued_at: current_time,
            cooldown_expires_at: current_time + 900, // Default 15 minutes
            dispute_window_ends_at: current_time + 3600, // Default 1 hour
            guardian_profiles,
            disputes: Vec::new(),
            threshold_signature: None, // Will be set by caller if needed
        }
    }

    /// Create evidence for a failed recovery operation
    ///
    /// # Parameters
    /// - `account_id`: Account that recovery was attempted for
    /// - `recovering_device`: Device that attempted the recovery
    ///
    /// # Returns
    /// RecoveryEvidence indicating failure with minimal populated fields
    pub fn create_failed_evidence(
        account_id: AccountId,
        recovering_device: DeviceId,
    ) -> RecoveryEvidence {
        Self::create_failed_evidence_with_time(account_id, recovering_device, 1234567890)
    }

    /// Create evidence for a failed recovery operation with explicit timestamp
    ///
    /// # Parameters
    /// - `account_id`: Account that recovery was attempted for
    /// - `recovering_device`: Device that attempted the recovery
    /// - `current_time`: Current timestamp in seconds since epoch
    ///
    /// # Returns
    /// RecoveryEvidence indicating failure with minimal populated fields
    pub fn create_failed_evidence_with_time(
        account_id: AccountId,
        recovering_device: DeviceId,
        current_time: u64,
    ) -> RecoveryEvidence {
        RecoveryEvidence {
            account_id,
            recovering_device,
            guardians: Vec::new(),
            issued_at: current_time,
            cooldown_expires_at: current_time,
            dispute_window_ends_at: current_time,
            guardian_profiles: Vec::new(),
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }

    /// Create minimal deterministic evidence for testing and compatibility
    ///
    /// # Returns
    /// RecoveryEvidence with default values for all fields
    pub fn create_default_evidence() -> RecoveryEvidence {
        RecoveryEvidence {
            account_id: AccountId::new_from_entropy([0u8; 32]),
            recovering_device: DeviceId::new_from_entropy([1u8; 32]),
            guardians: Vec::new(),
            issued_at: 0,
            cooldown_expires_at: 0,
            dispute_window_ends_at: 0,
            guardian_profiles: Vec::new(),
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }

    /// Update evidence with threshold signature after aggregation
    ///
    /// # Parameters
    /// - `evidence`: Mutable evidence to update
    /// - `signature`: Threshold signature to attach
    pub fn set_threshold_signature(
        evidence: &mut RecoveryEvidence,
        signature: aura_core::frost::ThresholdSignature,
    ) {
        evidence.threshold_signature = Some(signature);
    }

    /// Calculate appropriate cooldown and dispute window times
    ///
    /// # Parameters
    /// - `shares`: Guardian shares to analyze for cooldown requirements
    ///
    /// # Returns
    /// Tuple of (cooldown_expires_at, dispute_window_ends_at) in epoch seconds
    pub fn calculate_time_windows(shares: &[RecoveryShare]) -> (u64, u64) {
        Self::calculate_time_windows_with_time(shares, 1234567890)
    }

    /// Calculate appropriate cooldown and dispute window times with explicit timestamp
    ///
    /// # Parameters
    /// - `shares`: Guardian shares to analyze for cooldown requirements
    /// - `current_time`: Current timestamp in seconds since epoch
    ///
    /// # Returns
    /// Tuple of (cooldown_expires_at, dispute_window_ends_at) in epoch seconds
    pub fn calculate_time_windows_with_time(
        shares: &[RecoveryShare],
        current_time: u64,
    ) -> (u64, u64) {
        // Find the maximum cooldown among all participating guardians
        let max_cooldown = shares
            .iter()
            .map(|share| share.guardian.cooldown_secs)
            .max()
            .unwrap_or(900); // Default 15 minutes

        let cooldown_expires_at = current_time + max_cooldown;
        let dispute_window_ends_at = current_time + 3600; // 1 hour dispute window

        (cooldown_expires_at, dispute_window_ends_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GuardianProfile;
    use aura_core::{identifiers::GuardianId, TrustLevel};

    fn create_test_share(cooldown_secs: u64) -> RecoveryShare {
        RecoveryShare {
            guardian: GuardianProfile {
                guardian_id: GuardianId::new_from_entropy([cooldown_secs as u8; 32]),
                device_id: DeviceId::new_from_entropy([cooldown_secs as u8; 32]),
                label: "Test Guardian".to_string(),
                trust_level: TrustLevel::High,
                cooldown_secs,
            },
            share: vec![1, 2, 3],
            partial_signature: vec![4, 5, 6],
            issued_at: 1234567890,
        }
    }

    #[test]
    fn test_create_success_evidence() {
        let account_id = AccountId::new_from_entropy([1u8; 32]);
        let device_id = DeviceId::new_from_entropy([2u8; 32]);
        let shares = vec![create_test_share(900), create_test_share(1200)];

        let evidence = EvidenceBuilder::create_success_evidence(account_id, device_id, &shares);

        assert_eq!(evidence.account_id, account_id);
        assert_eq!(evidence.recovering_device, device_id);
        assert_eq!(evidence.guardians.len(), 2);
        assert_eq!(evidence.guardian_profiles.len(), 2);
        assert!(evidence.issued_at > 0);
        assert!(evidence.cooldown_expires_at > evidence.issued_at);
        assert!(evidence.dispute_window_ends_at > evidence.issued_at);
    }

    #[test]
    fn test_create_failed_evidence() {
        let account_id = AccountId::new_from_entropy([3u8; 32]);
        let device_id = DeviceId::new_from_entropy([3u8; 32]);

        let evidence = EvidenceBuilder::create_failed_evidence(account_id, device_id);

        assert_eq!(evidence.account_id, account_id);
        assert_eq!(evidence.recovering_device, device_id);
        assert!(evidence.guardians.is_empty());
        assert!(evidence.guardian_profiles.is_empty());
        assert_eq!(evidence.cooldown_expires_at, evidence.issued_at);
    }

    #[test]
    fn test_calculate_time_windows() {
        let shares = vec![
            create_test_share(600),  // 10 minutes
            create_test_share(1800), // 30 minutes
            create_test_share(900),  // 15 minutes
        ];

        let (cooldown, dispute) = EvidenceBuilder::calculate_time_windows(&shares);

        // Should use the maximum cooldown (1800 seconds = 30 minutes)
        // Cooldown should be equal to dispute window start time plus max cooldown
        assert_eq!(cooldown, dispute - 3600 + 1800);
        // Dispute window (3600 = 1 hour) should be longer than max cooldown (1800 = 30 minutes)
        assert!(dispute >= cooldown);
    }

    #[test]
    fn test_default_evidence() {
        let evidence = EvidenceBuilder::create_default_evidence();

        assert_eq!(evidence.issued_at, 0);
        assert_eq!(evidence.cooldown_expires_at, 0);
        assert_eq!(evidence.dispute_window_ends_at, 0);
        assert!(evidence.guardians.is_empty());
        assert!(evidence.disputes.is_empty());
        assert!(evidence.threshold_signature.is_none());
    }
}
