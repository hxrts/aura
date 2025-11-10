//! G_gc Choreography Implementation
//!
//! This module implements the G_gc choreography for coordinated garbage collection
//! with snapshot safety following the formal model from work/whole.md.

use aura_core::{AuraResult, ChunkId, DeviceId, Hash32};
use aura_crypto::frost::ThresholdSignature;
use aura_mpst::{CapabilityGuard, JournalAnnotation};
use aura_protocol::effects::{AuraEffectSystem, NetworkEffects};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Messages for the G_gc choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GcMessage {
    /// Snapshot proposal from proposer
    SnapshotProposal {
        /// Proposing device
        proposer_id: DeviceId,
        /// Root commit hash for snapshot
        root_commit: Hash32,
        /// Watermarks per shard
        watermarks: HashMap<ShardId, EventId>,
        /// Current epoch
        epoch: u64,
        /// Proposal nonce
        proposal_nonce: [u8; 32],
    },

    /// Quorum approval of snapshot
    SnapshotApprove {
        /// Approving quorum member
        quorum_member_id: DeviceId,
        /// Partial signature over proposal
        partial_sig: Vec<u8>,
        /// Local watermark from this node
        local_watermark: EventId,
        /// Timestamp of approval
        timestamp: u64,
    },

    /// Quorum rejection of snapshot
    SnapshotReject {
        /// Rejecting quorum member
        quorum_member_id: DeviceId,
        /// Rejection reason
        reason: String,
        /// Timestamp of rejection
        timestamp: u64,
    },

    /// Snapshot commit notification
    SnapshotCommit {
        /// Committed snapshot
        snapshot: GcSnapshot,
        /// Threshold signature over snapshot
        threshold_signature: ThresholdSignature,
        /// Participating quorum members
        participating_members: Vec<DeviceId>,
    },
}

/// Garbage collection snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcSnapshot {
    /// Snapshot identifier
    pub snapshot_id: Hash32,
    /// Root commit hash
    pub root_commit: Hash32,
    /// Event watermarks per shard
    pub watermarks: HashMap<ShardId, EventId>,
    /// Snapshot epoch
    pub epoch: u64,
    /// Creation timestamp
    pub timestamp: u64,
    /// Chunks safe to collect
    pub collectible_chunks: HashSet<ChunkId>,
}

/// Shard identifier for watermark tracking
pub type ShardId = u32;

/// Event identifier for watermarks
pub type EventId = u64;

/// Garbage collection proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcProposal {
    /// Target root commit
    pub root_commit: Hash32,
    /// Proposed watermarks
    pub watermarks: HashMap<ShardId, EventId>,
    /// Safety constraints
    pub safety_constraints: Vec<SafetyConstraint>,
    /// Estimated space to reclaim
    pub estimated_reclaim: u64,
}

/// Safety constraints for garbage collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SafetyConstraint {
    /// Minimum retention time for chunks
    MinRetentionTime(u64),
    /// Preserve chunks referenced by active sessions
    PreserveActiveSessions,
    /// Preserve chunks needed for sync
    PreserveSyncState,
    /// Custom constraint predicate
    Custom(String),
}

/// Roles in the G_gc choreography
#[derive(Debug, Clone)]
pub enum GcRole {
    /// Device proposing garbage collection
    Proposer(DeviceId),
    /// Quorum member participating in approval
    QuorumMember(DeviceId),
    /// Garbage collection coordinator
    Coordinator(DeviceId),
}

/// G_gc choreography implementation
#[derive(Debug, Clone)]
pub struct GcChoreography {
    /// Current device role
    role: GcRole,
    /// Required quorum size
    quorum_size: usize,
    /// Effect system for handling operations
    effects: AuraEffectSystem,
    /// Local storage state
    local_storage: LocalStorageState,
}

/// Local storage state for GC decisions
#[derive(Debug, Clone)]
pub struct LocalStorageState {
    /// Local watermarks
    watermarks: HashMap<ShardId, EventId>,
    /// Active chunks
    active_chunks: HashSet<ChunkId>,
    /// Chunk reference counts
    reference_counts: HashMap<ChunkId, u32>,
    /// Last snapshot
    last_snapshot: Option<GcSnapshot>,
}

impl GcChoreography {
    /// Create new garbage collection choreography
    pub fn new(role: GcRole, quorum_size: usize, effects: AuraEffectSystem) -> Self {
        Self {
            role,
            quorum_size,
            effects,
            local_storage: LocalStorageState::new(),
        }
    }

    /// Execute the G_gc choreography following the formal model
    ///
    /// ```rust,ignore
    /// choreography! {
    ///     G_gc[Roles: Proposer, Quorum(k)] {
    ///         // Proposer suggests snapshot point
    ///         [guard: need(gc_propose) ≤ caps_Proposer]
    ///         [Context: GID(gc_group, k)]
    ///         Proposer -> Quorum*: SnapshotProposal {
    ///             root_commit: Hash,
    ///             watermarks: Map<Shard, EventId>,
    ///             epoch
    ///         }
    ///
    ///         // Each quorum member validates safety
    ///         Quorum*: verify_snapshot_safe(watermarks)
    ///         Quorum*: check_local_invariants()
    ///
    ///         choice Quorum* {
    ///             approve {
    ///                 [guard: need(gc_approve) ≤ caps_Quorum]
    ///                 Quorum* -> Proposer: SnapshotApprove {
    ///                     partial_sig,
    ///                     local_watermark
    ///                 }
    ///             }
    ///             reject {
    ///                 Quorum* -> Proposer: SnapshotReject { reason }
    ///             }
    ///         }
    ///
    ///         // Combine signatures (FROST threshold)
    ///         Proposer: threshold_sig = combine_sigs(partial_sigs)
    ///         Proposer: snapshot = Snapshot { root_commit, watermarks, threshold_sig }
    ///
    ///         // Broadcast committed snapshot
    ///         Proposer -> Quorum*: SnapshotCommit { snapshot, threshold_sig }
    ///
    ///         // Journal integration - apply snapshot as delta fact
    ///         [▷ Δfacts: GcSnapshot(root_commit, watermarks, threshold_sig)]
    ///         All: merge_facts(GcSnapshot(...))
    ///
    ///         // Capability refinement (GC may affect access patterns)
    ///         [▷ Δcaps: gc_policy ⊓ current_caps]
    ///         All: refine_caps(gc_policy)
    ///
    ///         // Privacy: Group context for coordination, no external leakage
    ///         [Leakage: ℓ_ext=0, ℓ_ngh=log(k), ℓ_grp=full]
    ///     }
    /// }
    /// ```
    pub async fn execute_gc(&mut self, proposal: GcProposal) -> AuraResult<Option<GcSnapshot>> {
        match &self.role {
            GcRole::Proposer(device_id) => self.execute_as_proposer(*device_id, proposal).await,
            GcRole::QuorumMember(member_id) => self.execute_as_quorum_member(*member_id).await,
            GcRole::Coordinator(coordinator_id) => {
                self.execute_as_coordinator(*coordinator_id).await
            }
        }
    }

    /// Execute choreography as proposer
    async fn execute_as_proposer(
        &mut self,
        device_id: DeviceId,
        proposal: GcProposal,
    ) -> AuraResult<Option<GcSnapshot>> {
        // 1. Capability guard: need(gc_propose) ≤ caps_Proposer
        let guard = CapabilityGuard::new(
            "gc_propose".to_string(),
            aura_core::Cap::default(), // Would use actual proposer capabilities
        );
        let capabilities = aura_core::Cap::default(); // TODO: Use actual capabilities
        if !guard.check(&capabilities) {
            return Err(aura_core::AuraError::permission_denied(
                "Insufficient capabilities for GC proposal",
            ));
        }

        // 2. Generate proposal nonce and create proposal message
        let proposal_nonce = self.generate_proposal_nonce();
        let current_epoch = self.get_current_epoch();

        let proposal_msg = GcMessage::SnapshotProposal {
            proposer_id: device_id,
            root_commit: proposal.root_commit,
            watermarks: proposal.watermarks.clone(),
            epoch: current_epoch,
            proposal_nonce,
        };

        // 3. Send proposal to all quorum members
        let quorum_members = self.get_quorum_members().await?;
        for member_id in &quorum_members {
            let message_bytes = serde_json::to_vec(&proposal_msg)
                .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;
            self.effects.send_to_peer(member_id.0, message_bytes).await
                .map_err(|e| aura_core::AuraError::network(e.to_string()))?;
        }

        // 4. Collect responses from quorum members
        let mut approvals = Vec::new();
        let mut rejections = Vec::new();
        let mut partial_signatures = Vec::new();
        let mut local_watermarks = HashMap::new();

        // Wait for responses with timeout
        for member_id in &quorum_members {
            if let Ok(response) = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                async {
                    let bytes = self.effects.receive_from(member_id.0).await
                        .map_err(|e| aura_core::AuraError::network(e.to_string()))?;
                    serde_json::from_slice::<GcMessage>(&bytes)
                        .map_err(|e| aura_core::AuraError::serialization(e.to_string()))
                },
            )
            .await
            {
                match response? {
                    GcMessage::SnapshotApprove {
                        quorum_member_id,
                        partial_sig,
                        local_watermark,
                        timestamp: _,
                    } => {
                        approvals.push(quorum_member_id);
                        partial_signatures.push(partial_sig);
                        local_watermarks.insert(quorum_member_id, local_watermark);
                    }
                    GcMessage::SnapshotReject {
                        quorum_member_id,
                        reason,
                        timestamp: _,
                    } => {
                        rejections.push((quorum_member_id, reason));
                    }
                    _ => {} // Ignore other message types
                }
            }
        }

        // 5. Check if we have enough approvals for threshold
        if approvals.len() >= self.quorum_size {
            // 6. Combine partial signatures using FROST
            let threshold_signature = self.combine_signatures(partial_signatures)?;

            // 7. Create committed snapshot
            let snapshot = GcSnapshot {
                snapshot_id: self.compute_snapshot_id(&proposal, &approvals)?,
                root_commit: proposal.root_commit,
                watermarks: proposal.watermarks,
                epoch: current_epoch,
                timestamp: self.get_current_timestamp(),
                collectible_chunks: self.compute_collectible_chunks(&proposal).await?,
            };

            // 8. Broadcast snapshot commit
            let commit_msg = GcMessage::SnapshotCommit {
                snapshot: snapshot.clone(),
                threshold_signature: threshold_signature.clone(),
                participating_members: approvals.clone(),
            };

            for member_id in &quorum_members {
                let message_bytes = serde_json::to_vec(&commit_msg)
                    .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;
                self.effects.send_to_peer(member_id.0, message_bytes).await
                    .map_err(|e| aura_core::AuraError::network(e.to_string()))?;
            }

            // 9. Journal integration - apply delta facts
            let journal_annotation = JournalAnnotation::add_facts(format!(
                "GcSnapshot(root={:?}, watermarks={:?}, sig={:?})",
                snapshot.root_commit, snapshot.watermarks, threshold_signature
            ));
            // TODO: Apply journal delta through effect system
            // self.effects.apply_journal_delta(&journal_annotation).await?;
            // For now, skip journal application

            // 10. Capability refinement based on GC policy
            let capability_annotation =
                JournalAnnotation::add_facts("CapabilityUpdate(gc_policy_applied)".into());
            // TODO: Apply journal delta through effect system  
            // self.effects.apply_journal_delta(&capability_annotation).await?;
            // For now, skip journal application

            Ok(Some(snapshot))
        } else {
            // Insufficient approvals
            Err(aura_core::AuraError::internal(format!(
                "GC proposal failed: only {}/{} quorum members approved (need {})",
                approvals.len(),
                quorum_members.len(),
                self.quorum_size
            )))
        }
    }

    /// Execute choreography as quorum member
    async fn execute_as_quorum_member(
        &mut self,
        member_id: DeviceId,
    ) -> AuraResult<Option<GcSnapshot>> {
        // 1. Receive snapshot proposal
        let bytes = self.effects.receive_from(member_id.0).await
            .map_err(|e| aura_core::AuraError::network(e.to_string()))?;
        let proposal_msg = serde_json::from_slice::<GcMessage>(&bytes)
            .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;

        if let GcMessage::SnapshotProposal {
            proposer_id,
            root_commit,
            watermarks,
            epoch,
            proposal_nonce,
        } = proposal_msg
        {
            // 2. Verify snapshot safety
            let safety_check = self.verify_snapshot_safe(&watermarks, epoch).await;
            let invariants_check = self.check_local_invariants(&root_commit).await;

            // 3. Make approval decision
            if safety_check.is_ok() && invariants_check.is_ok() {
                // Check approval capability guard
                let guard = CapabilityGuard::new(
                    "gc_approve".to_string(),
                    aura_core::Cap::default(), // Would use actual quorum capabilities
                );

                let capabilities = aura_core::Cap::default(); // TODO: Use actual capabilities
                if guard.check(&capabilities) {
                    // Approve - create partial signature
                    let partial_sig =
                        self.create_partial_signature(&root_commit, &watermarks, proposal_nonce)?;
                    let local_watermark = self.get_local_watermark().await?;

                    let approval_msg = GcMessage::SnapshotApprove {
                        quorum_member_id: member_id,
                        partial_sig,
                        local_watermark,
                        timestamp: self.get_current_timestamp(),
                    };

                    let message_bytes = serde_json::to_vec(&approval_msg)
                        .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;
                    self.effects.send_to_peer(proposer_id.0, message_bytes).await
                        .map_err(|e| aura_core::AuraError::network(e.to_string()))?;
                } else {
                    // Insufficient capabilities
                    let reject_msg = GcMessage::SnapshotReject {
                        quorum_member_id: member_id,
                        reason: "Insufficient quorum capabilities".into(),
                        timestamp: self.get_current_timestamp(),
                    };

                    let message_bytes = serde_json::to_vec(&reject_msg)
                        .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;
                    self.effects.send_to_peer(proposer_id.0, message_bytes).await
                        .map_err(|e| aura_core::AuraError::network(e.to_string()))?;
                }
            } else {
                // Safety or invariant check failed
                let reason = if safety_check.is_err() {
                    format!(
                        "Snapshot safety check failed: {:?}",
                        safety_check.unwrap_err()
                    )
                } else {
                    format!(
                        "Local invariants check failed: {:?}",
                        invariants_check.unwrap_err()
                    )
                };

                let reject_msg = GcMessage::SnapshotReject {
                    quorum_member_id: member_id,
                    reason,
                    timestamp: self.get_current_timestamp(),
                };

                let message_bytes = serde_json::to_vec(&reject_msg)
                        .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;
                    self.effects.send_to_peer(proposer_id.0, message_bytes).await
                        .map_err(|e| aura_core::AuraError::network(e.to_string()))?;
            }

            // 4. Wait for commit notification
            if let Ok(commit_msg) = tokio::time::timeout(
                std::time::Duration::from_secs(120),
                async {
                    let bytes = self.effects.receive_from(member_id.0).await
                        .map_err(|e| aura_core::AuraError::network(e.to_string()))?;
                    serde_json::from_slice::<GcMessage>(&bytes)
                        .map_err(|e| aura_core::AuraError::serialization(e.to_string()))
                },
            )
            .await
            {
                if let GcMessage::SnapshotCommit {
                    snapshot,
                    threshold_signature: _,
                    participating_members: _,
                } = commit_msg?
                {
                    // Apply journal delta locally
                    let journal_annotation = JournalAnnotation::add_facts(format!(
                        "GcSnapshot(root={:?}, watermarks={:?})",
                        snapshot.root_commit, snapshot.watermarks
                    ));
                    // TODO: Apply journal delta through effect system
                    // self.effects.apply_journal_delta(&journal_annotation).await?;
                    // For now, skip journal application

                    return Ok(Some(snapshot));
                }
            }
        }

        Ok(None) // Quorum members don't return snapshots by default
    }

    /// Execute choreography as coordinator
    async fn execute_as_coordinator(
        &mut self,
        _coordinator_id: DeviceId,
    ) -> AuraResult<Option<GcSnapshot>> {
        // Coordinator manages GC scheduling and policy enforcement
        // TODO fix - For now, pass through
        Ok(None)
    }

    /// Verify snapshot safety constraints
    async fn verify_snapshot_safe(
        &self,
        watermarks: &HashMap<ShardId, EventId>,
        epoch: u64,
    ) -> AuraResult<()> {
        // Check that watermarks are safe for garbage collection
        // This would verify that no active sessions depend on data before watermarks
        for (shard_id, watermark) in watermarks {
            let local_watermark = self
                .local_storage
                .watermarks
                .get(shard_id)
                .copied()
                .unwrap_or(0);

            if *watermark > local_watermark {
                return Err(aura_core::AuraError::invalid(format!(
                    "Watermark for shard {} too high: {} > local {}",
                    shard_id, watermark, local_watermark
                )));
            }
        }

        // Check epoch consistency
        let current_epoch = self.get_current_epoch();
        if epoch > current_epoch {
            return Err(aura_core::AuraError::invalid(format!(
                "Future epoch in proposal: {} > current {}",
                epoch, current_epoch
            )));
        }

        Ok(())
    }

    /// Check local storage invariants
    async fn check_local_invariants(&self, root_commit: &Hash32) -> AuraResult<()> {
        // Verify that root_commit is reachable and valid
        // This would check that the proposed root is a valid commit in our view
        Ok(())
    }

    /// Generate proposal nonce
    fn generate_proposal_nonce(&self) -> [u8; 32] {
        // Generate cryptographically secure random nonce
        [0u8; 32] // Placeholder
    }

    /// Get current epoch
    fn get_current_epoch(&self) -> u64 {
        // Get current epoch from time effects
        1 // Placeholder
    }

    /// Get current timestamp
    fn get_current_timestamp(&self) -> u64 {
        // Get current timestamp from time effects
        1234567890 // Placeholder
    }

    /// Get quorum members
    async fn get_quorum_members(&self) -> AuraResult<Vec<DeviceId>> {
        // Query network for available quorum members
        Ok(vec![DeviceId::new(), DeviceId::new(), DeviceId::new()]) // Placeholder
    }

    /// Create partial signature over proposal
    fn create_partial_signature(
        &self,
        root_commit: &Hash32,
        watermarks: &HashMap<ShardId, EventId>,
        proposal_nonce: [u8; 32],
    ) -> AuraResult<Vec<u8>> {
        // Create FROST partial signature over proposal data
        Ok(vec![0u8; 64]) // Placeholder
    }

    /// Combine partial signatures into threshold signature
    fn combine_signatures(&self, partial_sigs: Vec<Vec<u8>>) -> AuraResult<ThresholdSignature> {
        // Use FROST to aggregate partial signatures
        Ok(ThresholdSignature::new(b"placeholder_signature".to_vec(), vec![1, 2, 3])) // Placeholder
    }

    /// Get local watermark for this node
    async fn get_local_watermark(&self) -> AuraResult<EventId> {
        // Get current local watermark
        Ok(12345) // Placeholder
    }

    /// Compute snapshot identifier
    fn compute_snapshot_id(
        &self,
        proposal: &GcProposal,
        approvals: &[DeviceId],
    ) -> AuraResult<Hash32> {
        // Compute deterministic snapshot ID from proposal and approvals
        Ok(Hash32::from_bytes(&[0u8; 32])) // Placeholder
    }

    /// Compute chunks safe for collection
    async fn compute_collectible_chunks(
        &self,
        proposal: &GcProposal,
    ) -> AuraResult<HashSet<ChunkId>> {
        // Determine which chunks can safely be collected based on watermarks
        Ok(HashSet::new()) // Placeholder
    }
}

impl LocalStorageState {
    /// Create new local storage state
    pub fn new() -> Self {
        Self {
            watermarks: HashMap::new(),
            active_chunks: HashSet::new(),
            reference_counts: HashMap::new(),
            last_snapshot: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_proposal_creation() {
        let proposal = GcProposal {
            root_commit: Hash32::default(),
            watermarks: HashMap::new(),
            safety_constraints: vec![
                SafetyConstraint::MinRetentionTime(3600),
                SafetyConstraint::PreserveActiveSessions,
            ],
            estimated_reclaim: 1024 * 1024 * 100, // 100MB
        };

        assert_eq!(proposal.safety_constraints.len(), 2);
        assert_eq!(proposal.estimated_reclaim, 1024 * 1024 * 100);
    }

    #[test]
    fn test_snapshot_creation() {
        let snapshot = GcSnapshot {
            snapshot_id: Hash32::default(),
            root_commit: Hash32::default(),
            watermarks: HashMap::new(),
            epoch: 1,
            timestamp: 1234567890,
            collectible_chunks: HashSet::new(),
        };

        assert_eq!(snapshot.epoch, 1);
        assert_eq!(snapshot.collectible_chunks.len(), 0);
    }
}
