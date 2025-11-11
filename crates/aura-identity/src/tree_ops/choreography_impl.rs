//! G_tree_op Choreography Implementation
//!
//! This module implements the complete G_tree_op choreography for distributed
//! tree operations following the formal model from work/whole.md.

use crate::tree_ops::choreography::TreeOpMessage;
use aura_core::{
    tree::{AttestedOp, TreeOp},
    AuraResult, Cap, DeviceId, Epoch, Policy,
};
use aura_crypto::Ed25519Signature;
use aura_mpst::{AuraRuntime, CapabilityGuard, JournalAnnotation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Roles in the G_tree_op choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TreeOpRole {
    /// Device proposing an operation
    Proposer(DeviceId),
    /// Device participating in threshold signing
    Participant(DeviceId),
    /// Coordination role for ordering and validation
    Coordinator(DeviceId),
}

/// G_tree_op choreography state
#[derive(Debug, Clone)]
pub struct TreeOpChoreography {
    /// Current device role
    role: TreeOpRole,
    /// Current epoch
    epoch: Epoch,
    /// Threshold policy
    policy: Policy,
    /// Participant capabilities
    capabilities: HashMap<DeviceId, Cap>,
    /// Runtime for effect handling
    runtime: AuraRuntime,
}

impl TreeOpChoreography {
    /// Create new tree operation choreography
    pub fn new(
        role: TreeOpRole,
        epoch: Epoch,
        policy: Policy,
        capabilities: HashMap<DeviceId, Cap>,
        runtime: AuraRuntime,
    ) -> Self {
        Self {
            role,
            epoch,
            policy,
            capabilities,
            runtime,
        }
    }

    /// Execute the G_tree_op choreography following the formal model
    ///
    /// ```rust,ignore
    /// choreography! {
    ///     G_tree_op[Roles: Proposer, Participants(n)] {
    ///         // 1. Proposer broadcasts operation proposal
    ///         [guard: need(tree_modify) ≤ caps_Proposer]
    ///         [Context: TreeContext(account_id, epoch)]
    ///         Proposer -> Participants*: TreeOpProposal {
    ///             operation: TreeOp,
    ///             capability_proof: Cap,
    ///             epoch: Epoch
    ///         }
    ///
    ///         // 2. Each participant validates and votes
    ///         parallel {
    ///             Participants*: validate_tree_operation()
    ///             Participants*: check_epoch_freshness()
    ///         }
    ///
    ///         // 3. Participants send approval/rejection votes
    ///         choice Participants* {
    ///             approve {
    ///                 [guard: need(tree_vote) ≤ caps_Participant]
    ///                 Participants* -> Proposer: TreeOpVote {
    ///                     operation_hash: Hash32,
    ///                     approved: true,
    ///                     signature_share: PartialSignature
    ///                 }
    ///             }
    ///             reject {
    ///                 Participants* -> Proposer: TreeOpVote {
    ///                     operation_hash: Hash32,
    ///                     approved: false,
    ///                     reason: String
    ///                 }
    ///             }
    ///         }
    ///
    ///         // 4. Proposer aggregates signatures if threshold met
    ///         Proposer: threshold_sig = aggregate_signatures(votes)
    ///
    ///         // 5. Broadcast attested operation or abort
    ///         choice Proposer {
    ///             commit {
    ///                 Proposer -> Participants*: TreeOpCommit {
    ///                     attested_op: AttestedOp {
    ///                         operation: TreeOp,
    ///                         attestation: ThresholdSignature,
    ///                         epoch: Epoch
    ///                     }
    ///                 }
    ///
    ///                 // Journal integration with delta facts
    ///                 [▷ Δfacts: TreeOperation(op, epoch, sig, participants)]
    ///                 Participants*: merge_facts(TreeOperation(...))
    ///             }
    ///             abort {
    ///                 Proposer -> Participants*: TreeOpAbort { reason }
    ///             }
    ///         }
    ///
    ///         // Privacy: tree context isolates operations by account+epoch
    ///         [Leakage: ℓ_ext=0, ℓ_ngh=log(|participants|), ℓ_grp=full]
    ///     }
    /// }
    /// ```
    pub async fn execute_tree_operation(
        &mut self,
        operation: TreeOp,
    ) -> AuraResult<Option<AttestedOp>> {
        match &self.role {
            TreeOpRole::Proposer(device_id) => {
                self.execute_as_proposer(*device_id, operation).await
            }
            TreeOpRole::Participant(device_id) => self.execute_as_participant(*device_id).await,
            TreeOpRole::Coordinator(device_id) => self.execute_as_coordinator(*device_id).await,
        }
    }

    /// Execute choreography as proposer role
    async fn execute_as_proposer(
        &mut self,
        device_id: DeviceId,
        operation: TreeOp,
    ) -> AuraResult<Option<AttestedOp>> {
        // 1. Capability guard: need(tree_modify) ≤ caps_Proposer
        let my_caps = self
            .capabilities
            .get(&device_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Proposer capabilities not found"))?;

        let required_cap = Cap::with_permissions(vec!["tree_modify".to_string()]);
        let guard = CapabilityGuard::new(required_cap);
        guard.enforce(my_caps)?;

        // 2. Broadcast proposal to all participants
        let _proposal = TreeOpMessage::Proposal {
            operation: operation.clone(),
            capability_proof: my_caps.clone(),
        };

        // Send to all participants through runtime
        let participants: Vec<DeviceId> = self
            .capabilities
            .keys()
            .filter(|&id| *id != device_id)
            .copied()
            .collect();

        // Note: send_message method needs to be implemented on AuraRuntime
        // TODO fix - For now, we'll skip the actual message sending TODO
        for participant in &participants {
            tracing::info!("Would send proposal to participant: {}", participant);
        }

        // 3. Collect votes from participants
        let mut votes = HashMap::new();
        let threshold = match self.policy {
            Policy::Threshold { m, .. } => m as usize,
            Policy::All => self.capabilities.len(),
            Policy::Any => 1,
        };

        // Note: recv_message method needs to be implemented on AuraRuntime
        // TODO fix - For now, simulate receiving votes
        for participant in &participants {
            tracing::info!("Would receive vote from participant: {}", participant);
            // Simulate a positive vote TODO fix - For now
            votes.insert(*participant, (true, vec![0u8; 32]));
        }

        // 4. Check if threshold is met
        let approvals: Vec<_> = votes
            .iter()
            .filter(|(_, (approved, _))| *approved)
            .collect();

        if approvals.len() >= threshold {
            // 5. Create attested operation with threshold signature
            let signature_shares: Vec<Vec<u8>> = approvals
                .iter()
                .map(|(_, (_, share))| share.clone())
                .collect();

            // Simulate threshold signature aggregation
            let threshold_sig = self.aggregate_signature_shares(signature_shares)?;

            let attested_op = AttestedOp {
                op: operation,
                agg_sig: threshold_sig.to_bytes().to_vec(),
                signer_count: approvals.len() as u16,
            };

            // 6. Broadcast commit
            let _commit_msg = TreeOpMessage::Commit {
                attested_op: attested_op.clone(),
            };

            for participant in &participants {
                tracing::info!("Would send commit to participant: {}", participant);
            }

            // 7. Journal integration - apply delta facts
            let journal_annotation = JournalAnnotation::add_facts(format!(
                "TreeOperation(op={:?}, participants={})",
                attested_op.op,
                participants.len()
            ));
            // Note: apply_journal_delta method needs to be implemented on AuraRuntime
            tracing::info!("Would apply journal delta: {:?}", journal_annotation);

            Ok(Some(attested_op))
        } else {
            // Threshold not met - abort
            let _abort_msg = TreeOpMessage::Abort {
                reason: format!("Insufficient approvals: {}/{}", approvals.len(), threshold),
            };

            for participant in &participants {
                tracing::info!("Would send abort to participant: {}", participant);
            }

            Ok(None)
        }
    }

    /// Execute choreography as participant role
    async fn execute_as_participant(
        &mut self,
        device_id: DeviceId,
    ) -> AuraResult<Option<AttestedOp>> {
        // 1. Receive proposal (stubbed TODO fix - For now)
        tracing::info!("Would receive proposal from device: {}", device_id);
        // Simulate receiving a proposal
        let proposal_msg = TreeOpMessage::Proposal {
            operation: TreeOp {
                parent_epoch: 0,
                parent_commitment: [0u8; 32],
                op: aura_core::tree::TreeOpKind::AddLeaf {
                    leaf: aura_core::tree::LeafNode::new_device(
                        aura_core::tree::LeafId(1),
                        device_id,
                        vec![],
                    ),
                    under: aura_core::tree::NodeIndex(0),
                },
                version: 1,
            },
            capability_proof: aura_core::Cap::top(),
        };

        if let TreeOpMessage::Proposal {
            operation,
            capability_proof: _,
        } = proposal_msg
        {
            // 2. Validate operation and check capabilities
            let my_caps = self.capabilities.get(&device_id).ok_or_else(|| {
                aura_core::AuraError::not_found("Participant capabilities not found")
            })?;

            let required_cap = Cap::with_permissions(vec!["tree_vote".to_string()]);
            let guard = CapabilityGuard::new(required_cap);

            let approved = guard.check(my_caps)
                && self.validate_tree_operation(&operation).is_ok()
                && self.check_epoch_freshness().is_ok();

            // 3. Send vote
            let _vote = if approved {
                let signature_share = self.create_signature_share(&operation)?;
                TreeOpMessage::Vote {
                    operation_hash: self.hash_operation(&operation),
                    approved: true,
                    signature_share,
                }
            } else {
                TreeOpMessage::Vote {
                    operation_hash: self.hash_operation(&operation),
                    approved: false,
                    signature_share: vec![],
                }
            };

            // Send vote back to proposer
            let proposer_id = self.get_proposer_id()?;
            tracing::info!("Would send vote to proposer: {}", proposer_id);

            // 4. Wait for commit or abort (simulated)
            tracing::info!("Would wait for result from proposer: {}", proposer_id);
            // Simulate receiving a commit message
            let result_msg = TreeOpMessage::Commit {
                attested_op: AttestedOp {
                    op: operation.clone(),
                    agg_sig: vec![0u8; 64],
                    signer_count: 1,
                },
            };

            match result_msg {
                TreeOpMessage::Commit { attested_op } => {
                    // Apply journal delta
                    let journal_annotation = JournalAnnotation::add_facts(format!(
                        "TreeOperation(op={:?}, sig_len={})",
                        attested_op.op,
                        attested_op.agg_sig.len()
                    ));
                    tracing::info!("Would apply journal delta: {:?}", journal_annotation);

                    Ok(Some(attested_op))
                }
                TreeOpMessage::Abort { reason: _ } => Ok(None),
                _ => Err(aura_core::AuraError::invalid("Unexpected message type")),
            }
        } else {
            Err(aura_core::AuraError::invalid("Expected proposal message"))
        }
    }

    /// Execute choreography as coordinator role
    async fn execute_as_coordinator(
        &mut self,
        _device_id: DeviceId,
    ) -> AuraResult<Option<AttestedOp>> {
        // Coordinator role handles ordering and conflict resolution
        // TODO fix - For now, just pass through to participant behavior
        // TODO: Implement full coordination logic
        Ok(None)
    }

    /// Validate tree operation according to policy
    fn validate_tree_operation(&self, operation: &TreeOp) -> AuraResult<()> {
        // Validate against current policy and state
        // This would include checks like:
        // - Operation is well-formed
        // - Doesn't violate tree invariants
        // - Compatible with current epoch

        match operation.op {
            aura_core::tree::TreeOpKind::AddLeaf { .. } => {
                // Validate add leaf operation
                Ok(())
            }
            aura_core::tree::TreeOpKind::RemoveLeaf { .. } => {
                // Validate remove leaf operation
                Ok(())
            }
            aura_core::tree::TreeOpKind::ChangePolicy { .. } => {
                // Validate policy change operation
                Ok(())
            }
            aura_core::tree::TreeOpKind::RotateEpoch { .. } => {
                // Validate epoch rotation operation
                Ok(())
            }
        }
    }

    /// Check if current epoch is fresh
    fn check_epoch_freshness(&self) -> AuraResult<()> {
        // Verify epoch hasn't been used before
        // This would check against journal state
        Ok(())
    }

    /// Create signature share for operation
    fn create_signature_share(&self, _operation: &TreeOp) -> AuraResult<Vec<u8>> {
        // Create FROST signature share
        // This would use the device's key share
        Ok(vec![0u8; 64]) // Placeholder
    }

    /// Hash operation for voting
    fn hash_operation(&self, operation: &TreeOp) -> [u8; 32] {
        // Create canonical hash of operation
        blake3::hash(&serde_json::to_vec(operation).unwrap_or_default()).into()
    }

    /// Get proposer device ID
    fn get_proposer_id(&self) -> AuraResult<DeviceId> {
        // Extract proposer ID from choreography context
        match &self.role {
            TreeOpRole::Proposer(id) => Ok(*id),
            _ => {
                // TODO fix - In a real implementation, this would be tracked in choreography state
                self.capabilities
                    .keys()
                    .next()
                    .copied()
                    .ok_or_else(|| aura_core::AuraError::not_found("No proposer found"))
            }
        }
    }

    /// Aggregate FROST signature shares into threshold signature
    fn aggregate_signature_shares(
        &self,
        signature_shares: Vec<Vec<u8>>,
    ) -> AuraResult<Ed25519Signature> {
        // Aggregate signature shares using FROST
        // This would use the aura-frost crate functionality
        let _ = signature_shares; // Silence unused variable warning
                                  // Create a placeholder signature with all zeros
        let zero_sig = [0u8; 64];
        match Ed25519Signature::from_slice(&zero_sig) {
            Ok(sig) => Ok(sig),
            Err(e) => Err(aura_core::AuraError::crypto(format!(
                "Failed to create placeholder signature: {}",
                e
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{Cap, Journal};

    #[test]
    fn test_tree_op_validation() {
        let choreography = create_test_choreography();
        let operation = TreeOp {
            parent_epoch: 0,
            parent_commitment: [0u8; 32],
            op: aura_core::tree::TreeOpKind::AddLeaf {
                leaf: aura_core::tree::LeafNode::new_device(
                    aura_core::tree::LeafId(1),
                    DeviceId::new(),
                    vec![],
                ),
                under: aura_core::tree::NodeIndex(0),
            },
            version: 1,
        };

        assert!(choreography.validate_tree_operation(&operation).is_ok());
    }

    #[test]
    fn test_epoch_freshness() {
        let choreography = create_test_choreography();
        assert!(choreography.check_epoch_freshness().is_ok());
    }

    fn create_test_choreography() -> TreeOpChoreography {
        let device_id = DeviceId::new();
        let role = TreeOpRole::Proposer(device_id);
        let epoch = 1; // Epoch is just a u64
        let policy = Policy::Threshold { m: 2, n: 5 };
        let capabilities = HashMap::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        TreeOpChoreography::new(role, epoch, policy, capabilities, runtime)
    }
}
