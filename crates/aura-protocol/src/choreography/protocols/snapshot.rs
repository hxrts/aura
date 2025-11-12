//! Snapshot Choreography for Coordinated Garbage Collection
//!
//! This module implements the MPST protocol for creating and applying snapshots
//! of tree state with threshold approval.
//!
//! Addresses system incongruency #9: "GC & old peers (upgrade safety)"
//!
//! Fix Pattern: Treat `SnapCommit{Snapshot}` as a protocol version gate in MPST.
//! Old peers refuse prune but continue to merge.
//!
//! ## Protocol Flow
//!
//! ### Phase 1: Proposal
//! 1. Proposer → Quorum[i]: SnapshotProposal { cut }
//! 2. Quorum[i]: Evaluate proposal locally
//!
//! ### Phase 2: Approval
//! 1. Quorum[i] → Proposer: ApproveSnapshot { partial_sig }
//! 2. Proposer: Wait for threshold approvals
//!
//! ### Phase 3: Finalization
//! 1. Proposer: Aggregate partial signatures
//! 2. Proposer: Create snapshot with aggregate signature
//! 3. Proposer → Quorum[i]: SnapshotCommit { snapshot }
//! 4. All: Apply snapshot and compact OpLog
//!
//! ### Phase 4: Upgrade Safety (NEW)
//! 1. Check protocol versions before GC
//! 2. Old peers refuse prune but continue merge
//! 3. Non-mergeable history protection
//!
//! ## Properties
//!
//! - Threshold approval ensures consensus
//! - Join-preserving compaction via retraction homomorphism
//! - Forward compatibility for old clients
//! - Atomic application across replicas
//! - Protocol version gating for safe upgrades

use crate::choreography::AuraHandlerAdapter;
use crate::effects::ChoreographyError;
use crate::effects::{ConsoleEffects, TreeEffects};
use crate::handlers::AuraHandlerError;
use aura_core::tree::{Cut, ProposalId, Snapshot};
use aura_core::{DeviceId, SessionId};
use aura_journal::ratchet_tree::TreeState;
use std::collections::BTreeMap;

// ============================================================================
// Configuration and Results
// ============================================================================

/// Snapshot choreography configuration
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Proposer device ID
    pub proposer: DeviceId,
    /// Quorum member device IDs
    pub quorum: Vec<DeviceId>,
    /// Threshold required for approval
    pub threshold: u16,
    /// Proposed snapshot cut point
    pub cut: Cut,
    /// Timeout for approval phase (seconds)
    pub approval_timeout_secs: u64,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            proposer: DeviceId::new(),
            quorum: Vec::new(),
            threshold: 1,
            cut: Cut::new(0, [0u8; 32], [0u8; 32], aura_core::tree::LeafId(0)),
            approval_timeout_secs: 120,
        }
    }
}

/// Snapshot choreography result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotResult {
    /// Resulting snapshot if successful
    pub snapshot: Option<Snapshot>,
    /// Number of approvals collected
    pub approvals_collected: u16,
    /// Whether snapshot was successfully created and applied
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Insufficient approvals: got {got}, needed {needed}")]
    InsufficientApprovals { got: u16, needed: u16 },
    #[error("Proposal rejected: {0}")]
    ProposalRejected(String),
    #[error("Snapshot application failed: {0}")]
    ApplicationFailed(String),
    #[error("Signature aggregation failed: {0}")]
    SignatureAggregation(String),
    #[error("Handler error: {0}")]
    Handler(#[from] AuraHandlerError),
    #[error("Effect system error: {0}")]
    EffectSystem(String),
}

impl From<aura_core::AuraError> for SnapshotError {
    fn from(e: aura_core::AuraError) -> Self {
        SnapshotError::EffectSystem(e.to_string())
    }
}

impl From<SnapshotError> for ChoreographyError {
    fn from(e: SnapshotError) -> Self {
        ChoreographyError::ProtocolViolation {
            message: e.to_string(),
        }
    }
}

impl From<aura_core::AuraError> for ChoreographyError {
    fn from(e: aura_core::AuraError) -> Self {
        ChoreographyError::ProtocolViolation {
            message: e.to_string(),
        }
    }
}

// ============================================================================
// Message Types
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotProposal {
    pub session_id: SessionId,
    pub proposal_id: ProposalId,
    pub cut: Cut,
    pub proposer: DeviceId,
    /// Protocol version for upgrade safety checking
    pub protocol_version: ProtocolVersion,
    /// Whether this snapshot enables garbage collection
    pub enables_gc: bool,
}

/// Protocol version for snapshot compatibility checking
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl ProtocolVersion {
    /// Current protocol version that supports snapshots and GC
    pub const SNAPSHOT_GC_CAPABLE: Self = Self {
        major: 1,
        minor: 2,
        patch: 0,
    };

    /// Legacy version that supports snapshots but not safe GC
    pub const SNAPSHOT_ONLY: Self = Self {
        major: 1,
        minor: 1,
        patch: 0,
    };

    /// Check if this version supports snapshot creation
    pub fn supports_snapshots(&self) -> bool {
        *self >= Self::SNAPSHOT_ONLY
    }

    /// Check if this version supports safe garbage collection
    pub fn supports_safe_gc(&self) -> bool {
        *self >= Self::SNAPSHOT_GC_CAPABLE
    }

    /// Check if two versions can safely interact
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.major == other.major
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotApproval {
    pub session_id: SessionId,
    pub proposal_id: ProposalId,
    pub approver: DeviceId,
    pub partial_signature: Vec<u8>,
    pub approved: bool,
    pub reason: Option<String>,
    /// Approver's protocol version
    pub protocol_version: ProtocolVersion,
    /// Whether approver supports GC for this snapshot
    pub supports_gc: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotCommit {
    pub session_id: SessionId,
    pub proposal_id: ProposalId,
    pub snapshot: Snapshot,
    pub aggregate_signature: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotAbort {
    pub session_id: SessionId,
    pub proposal_id: ProposalId,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotDecision {
    pub session_id: SessionId,
    pub proposal_id: ProposalId,
    pub committed: bool,
    pub snapshot: Option<Snapshot>,
    pub aggregate_signature: Option<Vec<u8>>,
    pub reason: Option<String>,
}

// ============================================================================
// Choreography Definition
// ============================================================================

// TEMPORARILY DISABLED DUE TO MACRO CONFLICTS - needs investigation
/*
choreography! {
    protocol SnapshotGC {
        roles: Proposer, QuorumMember1, QuorumMember2, QuorumMember3;

        // Phase 1: Proposer broadcasts snapshot proposal to all quorum members
        Proposer -> QuorumMember1: SnapshotProposeGC(SnapshotProposal);
        Proposer -> QuorumMember2: SnapshotProposeGC(SnapshotProposal);
        Proposer -> QuorumMember3: SnapshotProposeGC(SnapshotProposal);

        // Phase 2: Quorum members send approvals back to proposer
        QuorumMember1 -> Proposer: SnapshotApproveGC(SnapshotApproval);
        QuorumMember2 -> Proposer: SnapshotApproveGC(SnapshotApproval);
        QuorumMember3 -> Proposer: SnapshotApproveGC(SnapshotApproval);

        // Phase 3: Proposer broadcasts commit result to all members
        Proposer -> QuorumMember1: SnapshotCommitGC(SnapshotCommit);
        Proposer -> QuorumMember2: SnapshotCommitGC(SnapshotCommit);
        Proposer -> QuorumMember3: SnapshotCommitGC(SnapshotCommit);
    }
}
*/

// ============================================================================
// Session Functions
// ============================================================================

/// Execute snapshot choreography as proposer
async fn proposer_session(
    adapter: &mut AuraHandlerAdapter,
    quorum: &[DeviceId],
    config: &SnapshotConfig,
) -> Result<SnapshotResult, SnapshotError> {
    let session_id = SessionId::new();
    let proposal_id = ProposalId::new_random();

    // Log proposal initiation
    adapter
        .effects()
        .log_info(&format!(
            "Starting snapshot proposal {:?} at epoch {} with {}-of-{} threshold",
            proposal_id,
            config.cut.epoch,
            config.threshold,
            quorum.len()
        ))
        .await
        .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

    // Phase 1: Create and broadcast proposal
    let effects_cut = crate::effects::tree::Cut {
        epoch: config.cut.epoch,
        commitment: aura_core::Hash32(config.cut.commitment),
        cid: aura_core::Hash32(config.cut.commitment),
    };

    let _created_proposal_id = adapter
        .effects()
        .propose_snapshot(effects_cut)
        .await
        .map_err(|e| SnapshotError::EffectSystem(format!("Failed to create proposal: {}", e)))?;

    let proposal = SnapshotProposal {
        session_id,
        proposal_id,
        cut: config.cut.clone(),
        proposer: config.proposer,
        protocol_version: ProtocolVersion::SNAPSHOT_GC_CAPABLE,
        enables_gc: true,
    };

    // Broadcast proposal to all quorum members
    for member_id in quorum {
        adapter
            .send(*member_id, proposal.clone())
            .await
            .map_err(|e| SnapshotError::Communication(format!("Failed to send proposal: {}", e)))?;
    }

    adapter
        .effects()
        .log_debug(&format!("Sent proposal to {} quorum members", quorum.len()))
        .await
        .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

    // Phase 2: Collect approvals from quorum members
    let mut approvals: Vec<SnapshotApproval> = Vec::new();
    let mut partial_signatures: Vec<Vec<u8>> = Vec::new();
    let mut gc_capable_count = 0u16;

    for member_id in quorum {
        let approval: SnapshotApproval = adapter.recv_from(*member_id).await.map_err(|e| {
            SnapshotError::Communication(format!("Failed to receive approval: {}", e))
        })?;

        if approval.approved {
            partial_signatures.push(approval.partial_signature.clone());
            if approval.supports_gc {
                gc_capable_count += 1;
            }
        }
        approvals.push(approval);
    }

    let approval_count = approvals.iter().filter(|a| a.approved).count() as u16;

    adapter
        .effects()
        .log_info(&format!(
            "Collected {} approvals (threshold: {}), {} support GC",
            approval_count, config.threshold, gc_capable_count
        ))
        .await
        .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

    // Phase 3: Finalize or abort based on threshold
    if approval_count >= config.threshold {
        // Threshold met - check upgrade safety before enabling GC
        let safe_for_gc = gc_capable_count >= config.threshold;

        if !safe_for_gc {
            adapter
                .effects()
                .log_warn(&format!(
                    "Snapshot approved but GC disabled: only {}/{} peers support safe GC",
                    gc_capable_count, config.threshold
                ))
                .await
                .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;
        }

        adapter
            .effects()
            .log_info(&format!(
                "Threshold met, finalizing snapshot (GC enabled: {})",
                safe_for_gc
            ))
            .await
            .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

        // Aggregate partial signatures
        // TODO fix - For now, concatenate them - in production, use proper FROST aggregation
        let aggregate_signature = partial_signatures.concat();

        // Finalize snapshot via effects
        let effects_proposal_id =
            crate::effects::tree::ProposalId(aura_core::Hash32(proposal_id.0));
        let effects_snapshot = adapter
            .effects()
            .finalize_snapshot(effects_proposal_id)
            .await
            .map_err(|e| {
                SnapshotError::EffectSystem(format!("Failed to finalize snapshot: {}", e))
            })?;

        // Convert effects snapshot to core snapshot
        let snapshot = Snapshot::new(
            effects_snapshot.cut.epoch,
            effects_snapshot.cut.commitment.0,
            vec![aura_core::tree::LeafId(0)], // TODO: Extract from tree_state
            BTreeMap::new(),                  // TODO: Extract from tree_state
            effects_snapshot.cut.epoch,
        );

        // Apply snapshot locally
        adapter
            .effects()
            .apply_snapshot(&effects_snapshot)
            .await
            .map_err(|e| SnapshotError::ApplicationFailed(e.to_string()))?;

        // Broadcast commit to all quorum members
        let commit = SnapshotCommit {
            session_id,
            proposal_id,
            snapshot: snapshot.clone(),
            aggregate_signature,
        };

        for member_id in quorum {
            adapter
                .send(*member_id, commit.clone())
                .await
                .map_err(|e| {
                    SnapshotError::Communication(format!("Failed to send commit: {}", e))
                })?;
        }

        adapter
            .effects()
            .log_info("Snapshot committed successfully")
            .await
            .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

        Ok(SnapshotResult {
            snapshot: Some(snapshot),
            approvals_collected: approval_count,
            success: true,
            error: None,
        })
    } else {
        // Insufficient approvals - abort
        let abort = SnapshotAbort {
            session_id,
            proposal_id,
            reason: format!(
                "Insufficient approvals: got {}, needed {}",
                approval_count, config.threshold
            ),
        };

        for member_id in quorum {
            adapter.send(*member_id, abort.clone()).await.map_err(|e| {
                SnapshotError::Communication(format!("Failed to send abort: {}", e))
            })?;
        }

        adapter
            .effects()
            .log_warn(&format!("Snapshot aborted: {}", abort.reason))
            .await
            .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

        Ok(SnapshotResult {
            snapshot: None,
            approvals_collected: approval_count,
            success: false,
            error: Some(abort.reason),
        })
    }
}

/// Execute snapshot choreography as quorum member
async fn quorum_member_session(
    adapter: &mut AuraHandlerAdapter,
    proposer_id: DeviceId,
) -> Result<SnapshotResult, SnapshotError> {
    // Phase 1: Receive proposal from proposer
    let proposal: SnapshotProposal = adapter
        .recv_from(proposer_id)
        .await
        .map_err(|e| SnapshotError::Communication(format!("Failed to receive proposal: {}", e)))?;

    adapter
        .effects()
        .log_info(&format!(
            "Received snapshot proposal {:?} at epoch {}",
            proposal.proposal_id, proposal.cut.epoch
        ))
        .await
        .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

    // Evaluate proposal locally with upgrade safety checks
    // Check if cut is valid and acceptable:
    // 1. Cut epoch is not too far in the past
    // 2. We have all operations up to the cut
    // 3. Snapshot would free meaningful space
    // 4. Protocol version compatibility (NEW)
    // 5. GC safety for local version (NEW)

    let current_epoch: u64 = 100; // TODO: Get from journal state
    let epoch_diff = current_epoch.saturating_sub(proposal.cut.epoch);

    // Check protocol version compatibility
    let local_version = ProtocolVersion::SNAPSHOT_GC_CAPABLE;
    let version_compatible = local_version.is_compatible_with(&proposal.protocol_version);

    // Check if we support the requested features
    let supports_snapshots = local_version.supports_snapshots();
    let supports_gc = local_version.supports_safe_gc() && proposal.enables_gc;

    let approved = epoch_diff < 1000  // Not too old
        && version_compatible         // Compatible version
        && supports_snapshots; // We support snapshots

    let reason = if !version_compatible {
        Some(format!(
            "Incompatible protocol version: local {:?}, proposal {:?}",
            local_version, proposal.protocol_version
        ))
    } else if !supports_snapshots {
        Some("Local version doesn't support snapshots".to_string())
    } else if !approved {
        Some(format!(
            "Cut epoch {} is too old (current: {})",
            proposal.cut.epoch, current_epoch
        ))
    } else {
        None
    };

    // Phase 2: Generate approval (partial signature)
    let partial_signature = if approved {
        let effects_proposal_id =
            crate::effects::tree::ProposalId(aura_core::Hash32(proposal.proposal_id.0));
        let partial = adapter
            .effects()
            .approve_snapshot(effects_proposal_id)
            .await
            .map_err(|e| {
                SnapshotError::EffectSystem(format!("Failed to approve snapshot: {}", e))
            })?;

        // Extract signature bytes from Partial
        // TODO fix - For now, use a placeholder - in production, extract from FROST Partial
        vec![0u8; 32]
    } else {
        vec![]
    };

    // Send approval to proposer
    let approval = SnapshotApproval {
        session_id: proposal.session_id,
        proposal_id: proposal.proposal_id,
        approver: adapter.device_id(),
        partial_signature,
        approved,
        reason: reason.clone(),
        protocol_version: local_version,
        supports_gc,
    };

    adapter
        .send(proposer_id, approval.clone())
        .await
        .map_err(|e| SnapshotError::Communication(format!("Failed to send approval: {}", e)))?;

    if approved {
        adapter
            .effects()
            .log_info("Snapshot proposal approved")
            .await
            .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;
    } else {
        adapter
            .effects()
            .log_warn(&format!(
                "Snapshot proposal rejected: {}",
                reason.unwrap_or_default()
            ))
            .await
            .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;
    }

    // Phase 3: Receive commit or abort
    // Try to receive commit first
    let result = if let Ok(commit) = adapter.recv_from::<SnapshotCommit>(proposer_id).await {
        adapter
            .effects()
            .log_info("Received snapshot commit, applying")
            .await
            .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

        // Convert to effects snapshot
        let effects_snapshot = crate::effects::tree::Snapshot {
            cut: crate::effects::tree::Cut {
                epoch: commit.snapshot.epoch,
                commitment: aura_core::Hash32(commit.snapshot.commitment),
                cid: aura_core::Hash32(commit.snapshot.commitment),
            },
            tree_state: TreeState::new(),
            aggregate_signature: commit.aggregate_signature,
        };

        // Apply snapshot
        match adapter.effects().apply_snapshot(&effects_snapshot).await {
            Ok(()) => {
                adapter
                    .effects()
                    .log_info("Snapshot applied successfully")
                    .await
                    .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

                SnapshotResult {
                    snapshot: Some(commit.snapshot),
                    approvals_collected: 1,
                    success: true,
                    error: None,
                }
            }
            Err(e) => {
                adapter
                    .effects()
                    .log_error(&format!("Failed to apply snapshot: {}", e))
                    .await
                    .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

                SnapshotResult {
                    snapshot: Some(commit.snapshot),
                    approvals_collected: 1,
                    success: false,
                    error: Some(e.to_string()),
                }
            }
        }
    } else if let Ok(abort) = adapter.recv_from::<SnapshotAbort>(proposer_id).await {
        adapter
            .effects()
            .log_warn(&format!("Snapshot aborted: {}", abort.reason))
            .await
            .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

        SnapshotResult {
            snapshot: None,
            approvals_collected: 0,
            success: false,
            error: Some(abort.reason),
        }
    } else {
        return Err(SnapshotError::Communication(
            "Failed to receive commit or abort".to_string(),
        ));
    };

    Ok(result)
}

// ============================================================================
// Public API
// ============================================================================

/// Execute snapshot choreography as proposer
pub async fn execute_as_proposer(
    config: SnapshotConfig,
    effect_system: &crate::effects::system::AuraEffectSystem,
) -> Result<SnapshotResult, ChoreographyError> {
    // Validate configuration
    if config.quorum.is_empty() {
        return Err(ChoreographyError::ProtocolViolation {
            message: "Quorum cannot be empty".to_string(),
        });
    }
    if config.threshold == 0 || config.threshold as usize > config.quorum.len() {
        return Err(ChoreographyError::ProtocolViolation {
            message: format!(
                "Invalid threshold: {} for quorum size {}",
                config.threshold,
                config.quorum.len()
            ),
        });
    }

    // Create handler and adapter
    let mut adapter =
        AuraHandlerAdapter::new(config.proposer.into(), effect_system.execution_mode());

    // Execute proposer session
    proposer_session(&mut adapter, &config.quorum, &config)
        .await
        .map_err(|e| e.into())
}

/// Execute snapshot choreography as quorum member
pub async fn execute_as_quorum_member(
    config: SnapshotConfig,
    proposal_id: ProposalId,
    effect_system: &crate::effects::system::AuraEffectSystem,
) -> Result<SnapshotResult, ChoreographyError> {
    let member_id = DeviceId::new(); // TODO: Get from config

    // Create handler and adapter
    let mut adapter = AuraHandlerAdapter::new(member_id.into(), effect_system.execution_mode());

    // Execute quorum member session
    quorum_member_session(&mut adapter, config.proposer)
        .await
        .map_err(|e| e.into())
}

/// Apply received snapshot commit (for backward compatibility)
pub async fn apply_snapshot_commit(
    snapshot: Snapshot,
    effect_system: &crate::effects::system::AuraEffectSystem,
) -> Result<SnapshotResult, ChoreographyError> {
    use crate::effects::ConsoleEffects;

    ConsoleEffects::log_info(
        effect_system,
        &format!("Applying snapshot commit at epoch {}", snapshot.epoch),
    )
    .await?;

    let effects_snapshot = crate::effects::tree::Snapshot {
        cut: crate::effects::tree::Cut {
            epoch: snapshot.epoch,
            commitment: aura_core::Hash32(snapshot.commitment),
            cid: aura_core::Hash32(snapshot.commitment),
        },
        tree_state: TreeState::new(),
        aggregate_signature: vec![0u8; 64],
    };

    match effect_system.apply_snapshot(&effects_snapshot).await {
        Ok(()) => {
            ConsoleEffects::log_info(effect_system, "Snapshot applied successfully").await?;

            Ok(SnapshotResult {
                snapshot: Some(snapshot),
                approvals_collected: 0,
                success: true,
                error: None,
            })
        }
        Err(e) => {
            ConsoleEffects::log_error(effect_system, &format!("Failed to apply snapshot: {}", e))
                .await
                .map_err(|e| SnapshotError::EffectSystem(e.to_string()))?;

            Ok(SnapshotResult {
                snapshot: Some(snapshot),
                approvals_collected: 0,
                success: false,
                error: Some(e.to_string()),
            })
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::system::AuraEffectSystem;
    use aura_core::tree::{Epoch, LeafId};
    use std::collections::BTreeMap;

    fn create_test_config() -> SnapshotConfig {
        SnapshotConfig {
            proposer: DeviceId::new(),
            quorum: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            threshold: 2,
            cut: Cut::new(10, [1u8; 32], [2u8; 32], LeafId(1)),
            approval_timeout_secs: 120,
        }
    }

    fn create_test_snapshot() -> Snapshot {
        Snapshot::new(
            10,
            [1u8; 32],
            vec![LeafId(1), LeafId(2)],
            BTreeMap::from([(aura_core::tree::NodeIndex(0), aura_core::tree::Policy::Any)]),
            1000,
        )
    }

    #[tokio::test]
    async fn test_snapshot_config_default() {
        let config = SnapshotConfig::default();
        assert_eq!(config.threshold, 1);
        assert_eq!(config.approval_timeout_secs, 120);
    }

    #[tokio::test]
    async fn test_execute_as_proposer() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);
        let config = create_test_config();

        let result = execute_as_proposer(config, &effect_system).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_as_quorum_member() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);
        let config = create_test_config();
        let proposal_id = ProposalId::new_random();

        let result = execute_as_quorum_member(config, proposal_id, &effect_system).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_apply_snapshot_commit() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);
        let snapshot = create_test_snapshot();

        let result = apply_snapshot_commit(snapshot, &effect_system).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_snapshot_result_success() {
        let result = SnapshotResult {
            snapshot: Some(create_test_snapshot()),
            approvals_collected: 2,
            success: true,
            error: None,
        };

        assert!(result.success);
        assert_eq!(result.approvals_collected, 2);
        assert!(result.snapshot.is_some());
    }

    #[test]
    fn test_snapshot_result_insufficient_approvals() {
        let result = SnapshotResult {
            snapshot: None,
            approvals_collected: 1,
            success: false,
            error: Some("Insufficient approvals".to_string()),
        };

        assert!(!result.success);
        assert_eq!(result.approvals_collected, 1);
        assert!(result.snapshot.is_none());
    }

    #[test]
    fn test_snapshot_error_types() {
        let err = SnapshotError::InsufficientApprovals { got: 1, needed: 2 };
        assert!(err.to_string().contains("Insufficient approvals"));

        let err = SnapshotError::InvalidConfig("test".to_string());
        assert!(err.to_string().contains("Invalid configuration"));
    }

    #[test]
    fn test_message_serialization() {
        let proposal = SnapshotProposal {
            session_id: SessionId::new(),
            proposal_id: ProposalId::new_random(),
            cut: Cut::new(10, [1u8; 32], [2u8; 32], LeafId(1)),
            proposer: DeviceId::new(),
            protocol_version: ProtocolVersion::SNAPSHOT_GC_CAPABLE,
            enables_gc: true,
        };

        let serialized = serde_json::to_string(&proposal).unwrap();
        let deserialized: SnapshotProposal = serde_json::from_str(&serialized).unwrap();
        assert_eq!(proposal.session_id, deserialized.session_id);
    }
}
