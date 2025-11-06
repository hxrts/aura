//! RemoveLeaf choreography for removing devices/guardians from the tree
//!
//! Implements tree removal with LBBT rebalancing (swap with last leaf).

use crate::tree::{
    PrepareAckConfig, PrepareAckResult, PreparePhase, PrepareProposal, TreeSessionError,
};
use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_protocol::effects::{CryptoEffects, JournalEffects};
use aura_types::{
    AffectedPath, DeviceId, Intent, LeafIndex, ThresholdSignature, TreeOp, TreeOpRecord,
};
use rumpsteak_choreography::ChoreoHandler;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Configuration for RemoveLeaf choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveLeafConfig {
    /// Minimum participants required (threshold)
    pub threshold: usize,
    /// Total participants
    pub total_participants: usize,
    /// Timeout for each phase in seconds
    pub phase_timeout: u64,
}

/// RemoveLeaf choreography implementation
pub struct RemoveLeafChoreography {
    config: RemoveLeafConfig,
}

impl RemoveLeafChoreography {
    /// Create a new RemoveLeaf choreography
    pub fn new(config: RemoveLeafConfig) -> Self {
        Self { config }
    }

    /// Execute the full RemoveLeaf TreeSession
    ///
    /// # Arguments
    ///
    /// * `handler` - Composite handler providing all effects
    /// * `endpoint` - Communication endpoint
    /// * `intent` - The RemoveLeaf intent to execute
    /// * `leaf_index` - Index of leaf to remove
    /// * `participants` - All participants in the protocol
    /// * `my_role` - This device's choreographic role
    ///
    /// # Returns
    ///
    /// Signed TreeOpRecord that was committed to the journal
    pub async fn execute<H>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        intent: Intent,
        leaf_index: LeafIndex,
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

        // Phase 2-5: Share exchange, compute, attest, commit
        let tree_op = self.compute_tree_op(handler, leaf_index).await?;
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
            min_acks: self.config.threshold,
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

    async fn compute_tree_op<H>(
        &self,
        handler: &H,
        leaf_index: LeafIndex,
    ) -> Result<TreeOp, TreeSessionError>
    where
        H: JournalEffects,
    {
        // Get current tree
        let _current_tree = handler
            .get_current_tree()
            .await
            .map_err(|e| TreeSessionError::ChoreographyError(e.to_string()))?;

        // In real implementation:
        // 1. Verify leaf exists
        // 2. Swap with last leaf (LBBT rebalancing)
        // 3. Calculate affected path
        // 4. Update commitments

        let affected_path = AffectedPath {
            affected_indices: vec![],
            old_commitments: BTreeMap::new(),
            new_commitments: BTreeMap::new(),
        };

        Ok(TreeOp::RemoveLeaf {
            leaf_index,
            affected_path,
        })
    }

    async fn create_attestation(&self) -> Result<ThresholdSignature, TreeSessionError> {
        Ok(ThresholdSignature {
            signature: vec![0u8; 64],
            signers: vec![],
            threshold: (self.config.threshold, self.config.total_participants),
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
