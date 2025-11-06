//! AddLeaf choreography for adding new devices/guardians to the tree
//!
//! Implements the full TreeSession lifecycle:
//! 1. Prepare/ACK - Validate snapshot commitment
//! 2. Share Exchange - Collect threshold shares for affected path
//! 3. Compute - Calculate new leaf insertion and path update
//! 4. Attest - Create threshold signature on TreeOp
//! 5. Commit - Write TreeOpRecord to journal, tombstone intent

use crate::tree::{
    PrepareAckConfig, PrepareAckResult, PreparePhase, PrepareProposal, TreeSessionError,
};
use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_protocol::effects::{CryptoEffects, JournalEffects};
use aura_types::{
    AffectedPath, Commitment, DeviceId, Intent, LeafNode, NodeIndex, ThresholdSignature, TreeOp,
    TreeOpRecord,
};
use rumpsteak_choreography::ChoreoHandler;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Configuration for AddLeaf choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddLeafConfig {
    /// Minimum participants required (threshold)
    pub threshold: usize,
    /// Total participants
    pub total_participants: usize,
    /// Timeout for each phase in seconds
    pub phase_timeout: u64,
}

/// Share data for affected path nodes during AddLeaf
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathShare {
    /// Node index in the tree
    pub node_index: NodeIndex,
    /// Encrypted share data
    pub share_data: Vec<u8>,
    /// Commitment to this share
    pub share_commitment: Commitment,
    /// Device providing this share
    pub contributor: DeviceId,
}

/// Collection of shares for tree path update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathShareBundle {
    /// Shares for each affected node
    pub shares: BTreeMap<NodeIndex, Vec<PathShare>>,
    /// Epoch this bundle is valid for
    pub epoch: u64,
}

/// AddLeaf choreography implementation
pub struct AddLeafChoreography {
    config: AddLeafConfig,
}

impl AddLeafChoreography {
    /// Create a new AddLeaf choreography
    pub fn new(config: AddLeafConfig) -> Self {
        Self { config }
    }

    /// Execute the full AddLeaf TreeSession
    ///
    /// # Arguments
    ///
    /// * `handler` - Composite handler providing all effects
    /// * `endpoint` - Communication endpoint
    /// * `intent` - The AddLeaf intent to execute
    /// * `new_leaf` - The leaf node to add
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
        new_leaf: LeafNode,
        participants: Vec<ChoreographicRole>,
        my_role: ChoreographicRole,
    ) -> Result<TreeOpRecord, TreeSessionError>
    where
        H: ChoreoHandler<Role = ChoreographicRole> + JournalEffects + CryptoEffects + Clone,
    {
        // Phase 1: Prepare/ACK - Validate snapshot
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
            PrepareAckResult::Ack { .. } => {
                // Continue to share exchange
            }
        }

        // Phase 2: Share Exchange - Collect threshold shares for affected path
        let share_bundle = self
            .share_exchange_phase(handler, endpoint, &intent, &new_leaf, my_role)
            .await?;

        // Phase 3: Compute - Calculate tree operation locally
        let tree_op = self
            .compute_tree_op(handler, &new_leaf, &share_bundle)
            .await?;

        // Phase 4: Attest - Create threshold signature
        let attestation = self
            .attest_phase(handler, endpoint, &tree_op, my_role)
            .await?;

        // Phase 5: Commit - Write to journal and tombstone intent
        let tree_op_record = self
            .commit_phase(handler, tree_op, attestation, &intent)
            .await?;

        Ok(tree_op_record)
    }

    /// Phase 1: Prepare/ACK snapshot validation
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

    /// Phase 2: Share Exchange - Collect shares for affected path
    async fn share_exchange_phase<H: ChoreoHandler>(
        &self,
        _handler: &mut H,
        _endpoint: &mut H::Endpoint,
        intent: &Intent,
        _new_leaf: &LeafNode,
        _my_role: ChoreographicRole,
    ) -> Result<PathShareBundle, TreeSessionError> {
        // In a real implementation, this would:
        // 1. Use broadcast_and_gather with commit-reveal for shares
        // 2. Validate shares against commitments
        // 3. Verify threshold shares collected
        //
        // For now, return a stub bundle
        Ok(PathShareBundle {
            shares: BTreeMap::new(),
            epoch: intent.snapshot_commitment.as_bytes()[0] as u64, // Stub
        })
    }

    /// Phase 3: Compute tree operation
    async fn compute_tree_op<H>(
        &self,
        handler: &H,
        new_leaf: &LeafNode,
        _share_bundle: &PathShareBundle,
    ) -> Result<TreeOp, TreeSessionError>
    where
        H: JournalEffects,
    {
        // Get current tree to compute affected path
        let _current_tree = handler
            .get_current_tree()
            .await
            .map_err(|e| TreeSessionError::ChoreographyError(e.to_string()))?;

        // In real implementation, would:
        // 1. Find next available LBBT slot
        // 2. Calculate affected path from new leaf to root
        // 3. Use shares to compute new commitments
        //
        // For now, create a stub AffectedPath
        let affected_path = AffectedPath {
            affected_indices: vec![],
            old_commitments: BTreeMap::new(),
            new_commitments: BTreeMap::new(),
        };

        Ok(TreeOp::AddLeaf {
            leaf_node: new_leaf.clone(),
            affected_path,
        })
    }

    /// Phase 4: Create threshold attestation
    async fn attest_phase<H: ChoreoHandler>(
        &self,
        _handler: &H,
        _endpoint: &mut H::Endpoint,
        _tree_op: &TreeOp,
        _my_role: ChoreographicRole,
    ) -> Result<ThresholdSignature, TreeSessionError> {
        // In real implementation, would:
        // 1. Serialize TreeOp
        // 2. Use threshold_collect pattern to gather signature shares
        // 3. Aggregate into threshold signature
        // 4. Verify signature
        //
        // For now, return stub signature
        Ok(ThresholdSignature {
            signature: vec![0u8; 64], // Stub
            signers: vec![],
            threshold: (self.config.threshold, self.config.total_participants),
        })
    }

    /// Phase 5: Commit to journal and tombstone intent
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
        // Create TreeOpRecord
        let tree_op_record = TreeOpRecord {
            epoch: intent.snapshot_commitment.as_bytes()[0] as u64 + 1, // Stub: increment epoch
            op: tree_op,
            affected_indices: vec![],
            new_commitments: BTreeMap::new(),
            capability_refs: vec![],
            attestation,
            authored_at: 0, // Stub: would use TimeEffects
            author: intent.author,
        };

        // Write to journal
        handler
            .append_tree_op(tree_op_record.clone())
            .await
            .map_err(|e| TreeSessionError::ChoreographyError(e.to_string()))?;

        // Tombstone the intent
        handler
            .tombstone_intent(intent.intent_id)
            .await
            .map_err(|e| TreeSessionError::ChoreographyError(e.to_string()))?;

        Ok(tree_op_record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_leaf_config_creation() {
        let config = AddLeafConfig {
            threshold: 2,
            total_participants: 3,
            phase_timeout: 30,
        };

        assert_eq!(config.threshold, 2);
        assert_eq!(config.total_participants, 3);
        assert_eq!(config.phase_timeout, 30);
    }

    #[test]
    fn test_path_share_bundle_empty() {
        let bundle = PathShareBundle {
            shares: BTreeMap::new(),
            epoch: 1,
        };

        assert_eq!(bundle.shares.len(), 0);
        assert_eq!(bundle.epoch, 1);
    }
}
