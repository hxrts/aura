//! Recovery ceremony choreographies
//!
//! Implements guardian-based recovery with temporary recovery capabilities.
//! Recovery consists of two phases:
//! 1. Guardian Recovery Session - Guardians issue a time-limited RecoveryCapability
//! 2. Device Rekey Session - Device uses capability to rekey with guardian threshold

use crate::tree::{
    PrepareAckConfig, PrepareAckResult, PreparePhase, PrepareProposal, TreeSessionError,
};
use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_protocol::effects::{CryptoEffects, JournalEffects};
use aura_journal::ledger::capability::{CapabilitySignature, RecoveryCapability};
use aura_types::{
    AffectedPath, DeviceId, Intent, LeafIndex, NodeIndex, Policy, ThresholdSignature, TreeOp,
    TreeOpRecord,
};
use rumpsteak_choreography::ChoreoHandler;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Configuration for recovery choreographies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Guardian threshold required
    pub guardian_threshold: usize,
    /// Total guardians
    pub total_guardians: usize,
    /// Timeout for each phase in seconds
    pub phase_timeout: u64,
    /// Recovery capability TTL in seconds (e.g., 3600 for 1 hour)
    pub capability_ttl: u64,
}

/// RefreshPolicy choreography for activating guardian branch
///
/// This allows guardians to update policy for recovery operations.
pub struct RefreshPolicyChoreography {
    config: RecoveryConfig,
}

impl RefreshPolicyChoreography {
    /// Create a new RefreshPolicy choreography
    pub fn new(config: RecoveryConfig) -> Self {
        Self { config }
    }

    /// Execute policy refresh
    ///
    /// # Arguments
    ///
    /// * `handler` - Composite handler
    /// * `endpoint` - Communication endpoint
    /// * `intent` - RefreshPolicy intent
    /// * `guardian_branch` - Branch index to activate for recovery
    /// * `new_policy` - Policy to set (typically Threshold)
    /// * `participants` - All participants in the protocol
    /// * `my_role` - This device's role
    pub async fn execute<H>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        intent: Intent,
        guardian_branch: NodeIndex,
        new_policy: Policy,
        participants: Vec<ChoreographicRole>,
        my_role: ChoreographicRole,
    ) -> Result<TreeOpRecord, TreeSessionError>
    where
        H: ChoreoHandler<Role = ChoreographicRole> + JournalEffects + CryptoEffects + Clone,
    {
        // Phase 1: Prepare/ACK
        let prepare_result = self
            .prepare_phase::<H>(handler, endpoint, &intent, my_role, participants)
            .await?;

        match prepare_result {
            PrepareAckResult::Nack { nack_devices, .. } => {
                return Err(TreeSessionError::SnapshotMismatch {
                    expected: format!("{:?}", intent.snapshot_commitment),
                    actual: format!("Conflicting snapshots from {:?}", nack_devices),
                });
            }
            PrepareAckResult::Timeout => {
                return Err(TreeSessionError::Timeout {
                    phase: crate::tree::session::TreeSessionLifecycle::Prepare,
                });
            }
            PrepareAckResult::Ack { .. } => {}
        }

        // Phase 2-5: Compute, attest, commit
        // TODO: Compute affected_path from tree state
        let affected_path = AffectedPath::new();
        let tree_op = TreeOp::RefreshPolicy {
            node_index: guardian_branch,
            new_policy,
            affected_path,
        };

        let attestation = self.create_attestation().await?;
        let tree_op_record = self
            .commit_phase(handler, tree_op, attestation, &intent)
            .await?;

        Ok(tree_op_record)
    }

    async fn prepare_phase<H>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        intent: &Intent,
        my_role: ChoreographicRole,
        participants: Vec<ChoreographicRole>,
    ) -> Result<PrepareAckResult, TreeSessionError>
    where
        H: ChoreoHandler<Role = ChoreographicRole> + JournalEffects + CryptoEffects + Clone,
    {
        let prepare_config = PrepareAckConfig {
            timeout_seconds: self.config.phase_timeout,
            min_acks: self.config.guardian_threshold,
        };

        let prepare_phase = PreparePhase::<H>::new(prepare_config);

        let proposal = PrepareProposal {
            intent: intent.clone(),
            expected_snapshot: intent.snapshot_commitment,
            proposer: DeviceId(my_role.device_id),
        };

        // Handler implements both JournalEffects and CryptoEffects
        prepare_phase
            .execute(handler, endpoint, proposal, my_role, participants)
            .await
            .map_err(TreeSessionError::from)
    }

    async fn create_attestation(&self) -> Result<ThresholdSignature, TreeSessionError> {
        Ok(ThresholdSignature {
            signature: vec![0u8; 64],
            signers: vec![],
            threshold: (self.config.guardian_threshold, self.config.total_guardians),
        })
    }

    async fn commit_phase<H>(
        &self,
        handler: &H,
        tree_op: TreeOp,
        attestation: ThresholdSignature,
        intent: &Intent,
    ) -> Result<TreeOpRecord, TreeSessionError>
    where
        H: JournalEffects,
    {
        let tree_op_record = TreeOpRecord {
            epoch: intent.snapshot_commitment.as_bytes()[0] as u64 + 1,
            op: tree_op,
            affected_indices: vec![],
            new_commitments: BTreeMap::new(),
            capability_refs: vec![],
            attestation,
            authored_at: 0,
            author: intent.author,
        };

        handler
            .append_tree_op(tree_op_record.clone())
            .await
            .map_err(|e| TreeSessionError::ChoreographyError(e.to_string()))?;

        handler
            .tombstone_intent(intent.intent_id)
            .await
            .map_err(|e| TreeSessionError::ChoreographyError(e.to_string()))?;

        Ok(tree_op_record)
    }
}

/// Guardian Recovery Session - Issue recovery capability
///
/// Guardian-only TreeSession that produces a RecoveryCapability with short TTL.
pub struct GuardianRecoverySession {
    config: RecoveryConfig,
}

impl GuardianRecoverySession {
    /// Create a new guardian recovery session
    pub fn new(config: RecoveryConfig) -> Self {
        Self { config }
    }

    /// Execute guardian recovery session
    ///
    /// # Arguments
    ///
    /// * `handler` - Composite handler
    /// * `endpoint` - Communication endpoint
    /// * `target_device` - Device being recovered
    /// * `leaf_index` - Leaf index of recovering device
    /// * `guardians` - Participating guardians
    /// * `my_role` - This guardian's role
    ///
    /// # Returns
    ///
    /// RecoveryCapability with threshold guardian signatures
    pub async fn execute<H: ChoreoHandler>(
        &self,
        _handler: &mut H,
        _endpoint: &mut H::Endpoint,
        target_device: DeviceId,
        leaf_index: LeafIndex,
        guardians: Vec<DeviceId>,
        _my_role: ChoreographicRole,
    ) -> Result<RecoveryCapability, TreeSessionError> {
        // Verify guardian quorum
        if guardians.len() < self.config.guardian_threshold {
            return Err(TreeSessionError::InsufficientParticipants {
                threshold: self.config.guardian_threshold,
                available: guardians.len(),
            });
        }

        // In real implementation:
        // 1. Guardians propose recovery
        // 2. Collect threshold guardian signatures
        // 3. Create RecoveryCapability with TTL
        // 4. Write to journal (not as TreeOp, but as capability record)

        let current_time = 0; // Stub: would use TimeEffects
        let expires_at = current_time + self.config.capability_ttl;

        let signature = CapabilitySignature::new(vec![0u8; 64], guardians[0]);

        let recovery_cap = RecoveryCapability::new(
            target_device,
            guardians,
            self.config.guardian_threshold,
            expires_at,
            leaf_index.0, // Extract inner usize from LeafIndex
            1,            // Stub epoch
            signature,
        )
        .with_reason("Guardian-approved device recovery");

        Ok(recovery_cap)
    }
}

/// Device Rekey Session - Use recovery capability to rekey
///
/// Device presents RecoveryCapability to guardians and gets new keys via threshold.
pub struct DeviceRekeySession {
    config: RecoveryConfig,
}

impl DeviceRekeySession {
    /// Create a new device rekey session
    pub fn new(config: RecoveryConfig) -> Self {
        Self { config }
    }

    /// Execute device rekey session
    ///
    /// # Arguments
    ///
    /// * `handler` - Composite handler
    /// * `endpoint` - Communication endpoint
    /// * `recovery_capability` - Valid recovery capability
    /// * `my_role` - This device's role
    ///
    /// # Returns
    ///
    /// TreeOpRecord for the rekey operation (RotatePath with epoch bump)
    pub async fn execute<H>(
        &self,
        handler: &mut H,
        _endpoint: &mut H::Endpoint,
        recovery_capability: RecoveryCapability,
        my_role: ChoreographicRole,
    ) -> Result<TreeOpRecord, TreeSessionError>
    where
        H: ChoreoHandler + JournalEffects,
    {
        // Verify capability validity
        let current_time = 0; // Stub: would use TimeEffects
        if !recovery_capability.is_valid(current_time) {
            return Err(TreeSessionError::ChoreographyError(
                "Recovery capability expired or invalid".to_string(),
            ));
        }

        // Verify target device matches
        if recovery_capability.target_device != DeviceId(my_role.device_id) {
            return Err(TreeSessionError::ChoreographyError(
                "Recovery capability target mismatch".to_string(),
            ));
        }

        // In real implementation:
        // 1. Present capability to guardians
        // 2. Guardians verify capability signature and TTL
        // 3. Generate new device keys via threshold
        // 4. Create RotatePath op with epoch bump
        // 5. Threshold attest the rekey
        // 6. Tombstone the recovery capability

        // Create rekey TreeOp (RotatePath for post-compromise security)
        let tree_op = TreeOp::RotatePath {
            leaf_index: LeafIndex(0), // Stub: would get from capability
            affected_path: aura_types::AffectedPath {
                affected_indices: vec![],
                old_commitments: BTreeMap::new(),
                new_commitments: BTreeMap::new(),
            },
        };

        let attestation = ThresholdSignature {
            signature: vec![0u8; 64],
            signers: recovery_capability.issuing_guardians.clone(),
            threshold: (
                recovery_capability.guardian_threshold,
                recovery_capability.issuing_guardians.len(),
            ),
        };

        let tree_op_record = TreeOpRecord {
            epoch: 2, // Stub: epoch bump for post-compromise security
            op: tree_op,
            affected_indices: vec![],
            new_commitments: BTreeMap::new(),
            capability_refs: vec![recovery_capability.capability],
            attestation,
            authored_at: current_time,
            author: DeviceId(my_role.device_id),
        };

        // Write to journal
        handler
            .append_tree_op(tree_op_record.clone())
            .await
            .map_err(|e| TreeSessionError::ChoreographyError(e.to_string()))?;

        // Tombstone the recovery capability (prevent replay)
        // In real implementation, would tombstone via capability ID

        Ok(tree_op_record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_config_creation() {
        let config = RecoveryConfig {
            guardian_threshold: 2,
            total_guardians: 3,
            phase_timeout: 30,
            capability_ttl: 3600,
        };

        assert_eq!(config.guardian_threshold, 2);
        assert_eq!(config.capability_ttl, 3600);
    }

    #[test]
    fn test_refresh_policy_choreography_creation() {
        let config = RecoveryConfig {
            guardian_threshold: 2,
            total_guardians: 3,
            phase_timeout: 30,
            capability_ttl: 3600,
        };

        let choreography = RefreshPolicyChoreography::new(config);
        assert_eq!(choreography.config.guardian_threshold, 2);
    }

    #[test]
    fn test_guardian_recovery_session_creation() {
        let config = RecoveryConfig {
            guardian_threshold: 2,
            total_guardians: 3,
            phase_timeout: 30,
            capability_ttl: 3600,
        };

        let session = GuardianRecoverySession::new(config);
        assert_eq!(session.config.guardian_threshold, 2);
    }

    #[test]
    fn test_device_rekey_session_creation() {
        let config = RecoveryConfig {
            guardian_threshold: 2,
            total_guardians: 3,
            phase_timeout: 30,
            capability_ttl: 3600,
        };

        let session = DeviceRekeySession::new(config);
        assert_eq!(session.config.guardian_threshold, 2);
    }
}
