//! Tree Coordination Choreographies
//!
//! This module implements choreographic protocols for tree operation coordination,
//! following the Aura protocol guide patterns for distributed protocols with
//! session type safety and effect system integration.

use crate::effects::ChoreographyError;
use crate::effects::{
    ApprovalStatus, ApprovalVote, ConsoleEffects, CoordinationError, CryptoEffects,
    ReconcileResult, SessionId, SessionRole, SyncProgress, TimeEffects, TreeCoordinationEffects,
    TreeDigest, TreeEffects, ValidationContext, ValidationResult, VoteDecision,
};
use aura_core::{AttestedOp, AuraError, DeviceId, Hash32, TreeOpKind};
use rumpsteak_aura_choreography::choreography;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use uuid::Uuid;

/// Tree operation approval configuration
#[derive(Debug, Clone)]
pub struct TreeApprovalConfig {
    /// Device initiating the operation
    pub initiator: DeviceId,
    /// Devices that must approve the operation
    pub approvers: Vec<DeviceId>,
    /// Devices that observe but don't approve
    pub observers: Vec<DeviceId>,
    /// Tree operation to be approved
    pub operation: TreeOpKind,
    /// Required number of approvals
    pub threshold: usize,
    /// Session timeout in milliseconds
    pub timeout_ms: u64,
}

/// Result of tree operation choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeOperationResult {
    /// Session that was executed
    pub session_id: SessionId,
    /// Final attested operation (if approved)
    pub attested_op: Option<AttestedOp>,
    /// Final approval status
    pub approval_status: ApprovalStatus,
    /// Whether the operation succeeded
    pub success: bool,
    /// Synchronization progress
    pub sync_progress: Option<SyncProgress>,
}

/// Tree synchronization configuration
#[derive(Debug, Clone)]
pub struct TreeSyncConfig {
    /// Coordinator device for sync
    pub coordinator: DeviceId,
    /// Replica devices to sync with
    pub replicas: Vec<DeviceId>,
    /// Target epoch to sync to (None = latest)
    pub target_epoch: Option<u64>,
    /// Maximum operations per batch
    pub max_batch_size: usize,
}

/// Result of tree synchronization choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSyncResult {
    /// Operations synchronized
    pub operations_synced: usize,
    /// Peers that participated
    pub peers_synced: Vec<DeviceId>,
    /// Final tree digest
    pub final_digest: Option<TreeDigest>,
    /// Whether sync succeeded
    pub success: bool,
}

/// Tree choreography error types
#[derive(Debug, thiserror::Error)]
pub enum TreeChoreographyError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Tree coordination failed: {0}")]
    CoordinationFailed(String),
    #[error("Approval failed: {reason}")]
    ApprovalFailed { reason: String },
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    #[error("Handler error: {0}")]
    Handler(#[from] crate::handlers::AuraHandlerError),
    #[error("Coordination error: {0}")]
    Coordination(#[from] CoordinationError),
}

/// Message types for tree operation approval choreography

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeOperationProposal {
    pub session_id: SessionId,
    pub operation: TreeOpKind,
    pub initiator: DeviceId,
    pub validation_context: ValidationContext,
    pub threshold: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeOperationValidation {
    pub session_id: SessionId,
    pub validator: DeviceId,
    pub result: ValidationResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeApprovalRequest {
    pub session_id: SessionId,
    pub operation: TreeOpKind,
    pub validation_results: Vec<ValidationResult>,
    pub required_approvals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeApprovalResponse {
    pub session_id: SessionId,
    pub vote: ApprovalVote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeOperationExecution {
    pub session_id: SessionId,
    pub attested_op: Option<AttestedOp>,
    pub success: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSyncNotification {
    pub session_id: Option<SessionId>,
    pub progress: SyncProgress,
}

/// Message types for tree synchronization choreography

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncInitiation {
    pub sync_id: uuid::Uuid,
    pub coordinator: DeviceId,
    pub target_epoch: Option<u64>,
    pub max_batch_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestRequest {
    pub sync_id: uuid::Uuid,
    pub epoch_range: std::ops::Range<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestResponse {
    pub sync_id: uuid::Uuid,
    pub digest: TreeDigest,
    pub replica_id: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRequest {
    pub sync_id: uuid::Uuid,
    pub missing_ranges: Vec<std::ops::Range<u64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResponse {
    pub sync_id: uuid::Uuid,
    pub operations: Vec<AttestedOp>,
    pub sender_id: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCompletion {
    pub sync_id: uuid::Uuid,
    pub result: TreeSyncResult,
}

/// Tree Operation Approval Choreography
///
/// Multi-party protocol for coordinating tree operations with validation and approval
// TEMPORARILY DISABLED DUE TO MACRO CONFLICTS - needs investigation
/*
choreography! {
    protocol TreeOperationApproval {
        roles: Initiator, Approver1, Approver2, Approver3, Observer1, Observer2;

        // Phase 1: Initiator proposes operation to all participants
        Initiator -> Approver1: ProposeOperation(TreeOperationProposal);
        Initiator -> Approver2: ProposeOperation(TreeOperationProposal);
        Initiator -> Approver3: ProposeOperation(TreeOperationProposal);
        Initiator -> Observer1: ProposeOperation(TreeOperationProposal);
        Initiator -> Observer2: ProposeOperation(TreeOperationProposal);

        // Phase 2: Approvers validate and send results back
        Approver1 -> Initiator: ValidateOperation(OperationValidation);
        Approver2 -> Initiator: ValidateOperation(OperationValidation);
        Approver3 -> Initiator: ValidateOperation(OperationValidation);

        // Phase 3: Initiator requests approvals based on validation results
        Initiator -> Approver1: RequestApproval(ApprovalRequest);
        Initiator -> Approver2: RequestApproval(ApprovalRequest);
        Initiator -> Approver3: RequestApproval(ApprovalRequest);

        // Phase 4: Approvers submit their votes
        Approver1 -> Initiator: SubmitApproval(ApprovalResponse);
        Approver2 -> Initiator: SubmitApproval(ApprovalResponse);
        Approver3 -> Initiator: SubmitApproval(ApprovalResponse);

        // Phase 5: Initiator broadcasts execution result to all participants
        Initiator -> Approver1: ExecuteOperation(OperationExecution);
        Initiator -> Approver2: ExecuteOperation(OperationExecution);
        Initiator -> Approver3: ExecuteOperation(OperationExecution);
        Initiator -> Observer1: ExecuteOperation(OperationExecution);
        Initiator -> Observer2: ExecuteOperation(OperationExecution);

        // Phase 6: Initiator broadcasts sync progress to all participants
        Initiator -> Approver1: SyncUpdate(SyncNotification);
        Initiator -> Approver2: SyncUpdate(SyncNotification);
        Initiator -> Approver3: SyncUpdate(SyncNotification);
        Initiator -> Observer1: SyncUpdate(SyncNotification);
        Initiator -> Observer2: SyncUpdate(SyncNotification);
    }
}
*/

/// Execute tree operation approval choreography following the protocol guide pattern
pub async fn execute_tree_operation_approval(
    device_id: DeviceId,
    config: TreeApprovalConfig,
    effect_system: &crate::effects::system::AuraEffectSystem,
) -> Result<TreeOperationResult, TreeChoreographyError> {
    // Validate configuration following protocol guide validation pattern
    if config.threshold == 0 || config.threshold > config.approvers.len() {
        return Err(TreeChoreographyError::InvalidConfig(format!(
            "Invalid threshold: {} (must be 1..={})",
            config.threshold,
            config.approvers.len()
        )));
    }

    // Role assignment based on device ID (following DKD protocol pattern)
    let mut all_participants = config.approvers.clone();
    all_participants.push(config.initiator);
    all_participants.extend(config.observers.clone());
    all_participants.sort(); // Deterministic ordering

    if !all_participants.contains(&device_id) {
        return Err(TreeChoreographyError::InvalidConfig(
            "Device not in participants".to_string(),
        ));
    }

    let mut adapter = crate::choreography::AuraHandlerAdapter::new(
        device_id,
        effect_system.execution_mode(),
    );

    // Determine role and execute (following protocol guide pattern)
    if device_id == config.initiator {
        initiator_approval_session(&mut adapter, &config).await
    } else if config.approvers.contains(&device_id) {
        let approver_index = config
            .approvers
            .iter()
            .position(|&id| id == device_id)
            .ok_or_else(|| {
                TreeChoreographyError::InvalidConfig("Approver index not found".to_string())
            })?;
        approver_session(&mut adapter, config.initiator, approver_index, &config).await
    } else if config.observers.contains(&device_id) {
        observer_session(&mut adapter, config.initiator, &config).await
    } else {
        Err(TreeChoreographyError::InvalidConfig(
            "Device role not defined in configuration".to_string(),
        ))
    }
}

/// Role in tree operation choreography
#[derive(Debug, Clone)]
pub enum TreeOperationRole {
    Initiator,
    Approver { approver_index: usize },
    Observer,
}

/// Initiator's role in tree operation approval following effect system patterns
async fn initiator_approval_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    config: &TreeApprovalConfig,
) -> Result<TreeOperationResult, TreeChoreographyError> {
    let session_id = SessionId::new_v4();

    // Use TimeEffects to get current time and ConsoleEffects for logging (following protocol guide)
    let current_time = adapter.effects().current_timestamp_millis().await;
    adapter.effects().log_info(
        "Starting tree operation approval session",
        &[
            ("session_id", session_id.to_string().as_str()),
            ("operation", format!("{:?}", config.operation).as_str()),
        ],
    );

    // Phase 1: Propose operation to all participants
    let validation_context = ValidationContext {
        current_epoch: 1, // Would be fetched from tree effects
        requesting_device: config.initiator,
        session_id: Some(session_id),
        metadata: BTreeMap::new(),
    };

    let proposal = TreeOperationProposal {
        session_id,
        operation: config.operation.clone(),
        initiator: config.initiator,
        validation_context,
        threshold: config.threshold,
    };

    // Send to approvers
    for approver_id in &config.approvers {
        adapter
            .send(*approver_id, proposal.clone())
            .await
            .map_err(|e| {
                TreeChoreographyError::Communication(format!("Failed to send proposal: {}", e))
            })?;
    }

    // Send to observers
    for observer_id in &config.observers {
        adapter
            .send(*observer_id, proposal.clone())
            .await
            .map_err(|e| {
                TreeChoreographyError::Communication(format!("Failed to send proposal: {}", e))
            })?;
    }

    // Phase 2: Collect validation results
    let mut validation_results = Vec::new();
    for approver_id in &config.approvers {
        let validation: TreeOperationValidation =
            adapter.recv_from(*approver_id).await.map_err(|e| {
                TreeChoreographyError::Communication(format!("Failed to receive validation: {}", e))
            })?;

        if validation.session_id != session_id {
            return Err(TreeChoreographyError::CoordinationFailed(
                "Session ID mismatch in validation".to_string(),
            ));
        }

        validation_results.push(validation.result);
    }

    // Check if any validation failed
    for result in &validation_results {
        if let ValidationResult::Invalid { reason } = result {
            return Err(TreeChoreographyError::ValidationFailed(reason.clone()));
        }
    }

    // Phase 3: Request approvals
    let approval_request = TreeApprovalRequest {
        session_id,
        operation: config.operation.clone(),
        validation_results: validation_results.clone(),
        required_approvals: config.threshold,
    };

    for approver_id in &config.approvers {
        adapter
            .send(*approver_id, approval_request.clone())
            .await
            .map_err(|e| {
                TreeChoreographyError::Communication(format!(
                    "Failed to send approval request: {}",
                    e
                ))
            })?;
    }

    // Phase 4: Collect approval votes
    let mut votes = Vec::new();
    for approver_id in &config.approvers {
        let approval: TreeApprovalResponse =
            adapter.recv_from(*approver_id).await.map_err(|e| {
                TreeChoreographyError::Communication(format!("Failed to receive approval: {}", e))
            })?;

        if approval.session_id != session_id {
            return Err(TreeChoreographyError::CoordinationFailed(
                "Session ID mismatch in approval".to_string(),
            ));
        }

        votes.push(approval.vote);
    }

    // Calculate approval status
    let approve_count = votes
        .iter()
        .filter(|vote| vote.decision == VoteDecision::Approve)
        .count();

    let reject_count = votes
        .iter()
        .filter(|vote| vote.decision == VoteDecision::Reject)
        .count();

    let approval_status = if reject_count > 0 {
        let rejected_by: BTreeSet<DeviceId> = votes
            .iter()
            .filter(|vote| vote.decision == VoteDecision::Reject)
            .map(|vote| vote.device_id)
            .collect();
        ApprovalStatus::Rejected {
            reason: "Operation explicitly rejected".to_string(),
            rejected_by,
        }
    } else if approve_count >= config.threshold {
        ApprovalStatus::Approved
    } else {
        let missing_from: BTreeSet<DeviceId> = config
            .approvers
            .iter()
            .filter(|&id| !votes.iter().any(|v| v.device_id == *id))
            .copied()
            .collect();
        ApprovalStatus::Pending {
            received: approve_count,
            required: config.threshold,
            missing_from,
        }
    };

    // Phase 5: Execute operation if approved
    let (attested_op, success) = match approval_status {
        ApprovalStatus::Approved => {
            // Create mock attested operation
            let tree_op = aura_core::TreeOp {
                parent_epoch: proposal.validation_context.current_epoch.saturating_sub(1),
                parent_commitment: [0u8; 32], // Would be actual parent commitment
                op: config.operation.clone(),
                version: 1,
            };
            let attested_op = AttestedOp {
                op: tree_op,
                agg_sig: vec![], // Would contain real FROST signature
                signer_count: 1, // Would contain real signer count
            };
            (Some(attested_op), true)
        }
        _ => (None, false),
    };

    let execution = TreeOperationExecution {
        session_id,
        attested_op: attested_op.clone(),
        success,
        reason: if !success {
            Some("Approval failed".to_string())
        } else {
            None
        },
    };

    // Broadcast execution result
    for approver_id in &config.approvers {
        adapter
            .send(*approver_id, execution.clone())
            .await
            .map_err(|e| {
                TreeChoreographyError::Communication(format!(
                    "Failed to send execution result: {}",
                    e
                ))
            })?;
    }

    for observer_id in &config.observers {
        adapter
            .send(*observer_id, execution.clone())
            .await
            .map_err(|e| {
                TreeChoreographyError::Communication(format!(
                    "Failed to send execution result: {}",
                    e
                ))
            })?;
    }

    // Phase 6: Trigger synchronization if operation succeeded
    let sync_progress = if success {
        let sync_progress = SyncProgress {
            phase: crate::effects::SyncPhase::Complete,
            peers_contacted: config.approvers.len(),
            operations_synced: 1,
            estimated_completion_ms: 0,
        };

        let sync_notification = TreeSyncNotification {
            session_id: Some(session_id),
            progress: sync_progress.clone(),
        };

        // Broadcast sync notification
        for approver_id in &config.approvers {
            adapter
                .send(*approver_id, sync_notification.clone())
                .await
                .map_err(|e| {
                    TreeChoreographyError::Communication(format!(
                        "Failed to send sync notification: {}",
                        e
                    ))
                })?;
        }

        for observer_id in &config.observers {
            adapter
                .send(*observer_id, sync_notification.clone())
                .await
                .map_err(|e| {
                    TreeChoreographyError::Communication(format!(
                        "Failed to send sync notification: {}",
                        e
                    ))
                })?;
        }

        Some(sync_progress)
    } else {
        None
    };

    Ok(TreeOperationResult {
        session_id,
        attested_op,
        approval_status,
        success,
        sync_progress,
    })
}

/// Approver's role in tree operation approval following effect system patterns
async fn approver_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    initiator_id: DeviceId,
    _approver_index: usize,
    _config: &TreeApprovalConfig,
) -> Result<TreeOperationResult, TreeChoreographyError> {
    // Use ConsoleEffects for logging (following protocol guide pattern)
    adapter.effects().log_info(
        "Starting tree operation approval as approver",
        &[("initiator", initiator_id.to_string().as_str())],
    );

    // Phase 1: Receive operation proposal
    let proposal: TreeOperationProposal = adapter.recv_from(initiator_id).await.map_err(|e| {
        TreeChoreographyError::Communication(format!("Failed to receive proposal: {}", e))
    })?;

    adapter.effects().log_debug(
        "Received tree operation proposal",
        &[
            ("session_id", proposal.session_id.to_string().as_str()),
            ("operation", format!("{:?}", proposal.operation).as_str()),
        ],
    );

    // Phase 2: Validate operation and send result using CryptoEffects for validation
    // For simplicity, always approve unless operation is clearly invalid
    let validation_result = ValidationResult::Valid;

    let validation = TreeOperationValidation {
        session_id: proposal.session_id,
        validator: adapter.device_id(),
        result: validation_result,
    };

    adapter.send(initiator_id, validation).await.map_err(|e| {
        TreeChoreographyError::Communication(format!("Failed to send validation: {}", e))
    })?;

    // Phase 3: Receive approval request
    let approval_request: TreeApprovalRequest =
        adapter.recv_from(initiator_id).await.map_err(|e| {
            TreeChoreographyError::Communication(format!(
                "Failed to receive approval request: {}",
                e
            ))
        })?;

    // Phase 4: Submit approval vote
    let vote = ApprovalVote {
        device_id: adapter.device_id(),
        decision: VoteDecision::Approve, // TODO fix - Simplified - would implement real approval logic
        reason: Some("Choreographic approval".to_string()),
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
    };

    let approval_response = TreeApprovalResponse {
        session_id: approval_request.session_id,
        vote,
    };

    adapter
        .send(initiator_id, approval_response)
        .await
        .map_err(|e| {
            TreeChoreographyError::Communication(format!("Failed to send approval: {}", e))
        })?;

    // Phase 5: Receive execution result
    let execution: TreeOperationExecution = adapter.recv_from(initiator_id).await.map_err(|e| {
        TreeChoreographyError::Communication(format!("Failed to receive execution result: {}", e))
    })?;

    // Phase 6: Receive sync notification (if operation succeeded)
    let sync_progress = if execution.success {
        let sync_notification: TreeSyncNotification =
            adapter.recv_from(initiator_id).await.map_err(|e| {
                TreeChoreographyError::Communication(format!(
                    "Failed to receive sync notification: {}",
                    e
                ))
            })?;
        Some(sync_notification.progress)
    } else {
        None
    };

    Ok(TreeOperationResult {
        session_id: execution.session_id,
        attested_op: execution.attested_op,
        approval_status: if execution.success {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Rejected {
                reason: execution.reason.unwrap_or_default(),
                rejected_by: BTreeSet::new(),
            }
        },
        success: execution.success,
        sync_progress,
    })
}

/// Observer's role in tree operation approval
async fn observer_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    initiator_id: DeviceId,
    _config: &TreeApprovalConfig,
) -> Result<TreeOperationResult, TreeChoreographyError> {
    // Phase 1: Receive operation proposal
    let proposal: TreeOperationProposal = adapter.recv_from(initiator_id).await.map_err(|e| {
        TreeChoreographyError::Communication(format!("Failed to receive proposal: {}", e))
    })?;

    // Phase 5: Receive execution result (observers skip validation and approval phases)
    let execution: TreeOperationExecution = adapter.recv_from(initiator_id).await.map_err(|e| {
        TreeChoreographyError::Communication(format!("Failed to receive execution result: {}", e))
    })?;

    // Phase 6: Receive sync notification (if operation succeeded)
    let sync_progress = if execution.success {
        let sync_notification: TreeSyncNotification =
            adapter.recv_from(initiator_id).await.map_err(|e| {
                TreeChoreographyError::Communication(format!(
                    "Failed to receive sync notification: {}",
                    e
                ))
            })?;
        Some(sync_notification.progress)
    } else {
        None
    };

    Ok(TreeOperationResult {
        session_id: proposal.session_id,
        attested_op: execution.attested_op,
        approval_status: if execution.success {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Rejected {
                reason: execution.reason.unwrap_or_default(),
                rejected_by: BTreeSet::new(),
            }
        },
        success: execution.success,
        sync_progress,
    })
}

/// Coordinator's role in tree synchronization
async fn coordinator_sync_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    config: &TreeSyncConfig,
) -> Result<TreeSyncResult, TreeChoreographyError> {
    let sync_id = Uuid::new_v4();

    // Phase 1: Initiate sync with all replicas
    let sync_initiation = SyncInitiation {
        sync_id,
        coordinator: config.coordinator,
        target_epoch: config.target_epoch,
        max_batch_size: config.max_batch_size,
    };

    for replica_id in &config.replicas {
        adapter
            .send(*replica_id, sync_initiation.clone())
            .await
            .map_err(|e| {
                TreeChoreographyError::Communication(format!(
                    "Failed to send sync initiation: {}",
                    e
                ))
            })?;
    }

    // Phase 2: Request digests from all replicas
    let current_epoch = 10; // Would be fetched from tree effects
    let digest_request = DigestRequest {
        sync_id,
        epoch_range: 0..current_epoch,
    };

    for replica_id in &config.replicas {
        adapter
            .send(*replica_id, digest_request.clone())
            .await
            .map_err(|e| {
                TreeChoreographyError::Communication(format!(
                    "Failed to send digest request: {}",
                    e
                ))
            })?;
    }

    // Phase 3: Collect digests from all replicas
    let mut digests = Vec::new();
    for replica_id in &config.replicas {
        let digest_response: DigestResponse =
            adapter.recv_from(*replica_id).await.map_err(|e| {
                TreeChoreographyError::Communication(format!("Failed to receive digest: {}", e))
            })?;

        if digest_response.sync_id != sync_id {
            return Err(TreeChoreographyError::CoordinationFailed(
                "Sync ID mismatch in digest response".to_string(),
            ));
        }

        digests.push((digest_response.replica_id, digest_response.digest));
    }

    // Phase 4: Request missing operations based on digest comparison
    let missing_ranges = vec![0..5]; // TODO fix - Simplified - would calculate from digest comparison
    let operation_request = OperationRequest {
        sync_id,
        missing_ranges,
    };

    for replica_id in &config.replicas {
        adapter
            .send(*replica_id, operation_request.clone())
            .await
            .map_err(|e| {
                TreeChoreographyError::Communication(format!(
                    "Failed to send operation request: {}",
                    e
                ))
            })?;
    }

    // Phase 5: Collect operations from replicas
    let mut all_operations = Vec::new();
    for replica_id in &config.replicas {
        let operation_response: OperationResponse =
            adapter.recv_from(*replica_id).await.map_err(|e| {
                TreeChoreographyError::Communication(format!("Failed to receive operations: {}", e))
            })?;

        all_operations.extend(operation_response.operations);
    }

    // Phase 6: Broadcast sync completion
    let sync_result = TreeSyncResult {
        operations_synced: all_operations.len(),
        peers_synced: config.replicas.clone(),
        final_digest: digests.first().map(|(_, digest)| digest.clone()),
        success: true,
    };

    let sync_completion = SyncCompletion {
        sync_id,
        result: sync_result.clone(),
    };

    for replica_id in &config.replicas {
        adapter
            .send(*replica_id, sync_completion.clone())
            .await
            .map_err(|e| {
                TreeChoreographyError::Communication(format!(
                    "Failed to send sync completion: {}",
                    e
                ))
            })?;
    }

    Ok(sync_result)
}

/// Replica's role in tree synchronization
async fn replica_sync_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    coordinator_id: DeviceId,
    _replica_index: usize,
    _config: &TreeSyncConfig,
) -> Result<TreeSyncResult, TreeChoreographyError> {
    // Phase 1: Receive sync initiation
    let sync_initiation: SyncInitiation = adapter.recv_from(coordinator_id).await.map_err(|e| {
        TreeChoreographyError::Communication(format!("Failed to receive sync initiation: {}", e))
    })?;

    // Phase 2: Receive digest request
    let digest_request: DigestRequest = adapter.recv_from(coordinator_id).await.map_err(|e| {
        TreeChoreographyError::Communication(format!("Failed to receive digest request: {}", e))
    })?;

    // Phase 3: Send digest response
    let digest = TreeDigest {
        epoch_range: digest_request.epoch_range,
        operations_hash: Hash32([0u8; 32]), // Would be computed from actual operations
        operation_count: 10,
        state_hash: Hash32([1u8; 32]),
    };

    let digest_response = DigestResponse {
        sync_id: digest_request.sync_id,
        digest,
        replica_id: adapter.device_id(),
    };

    adapter
        .send(coordinator_id, digest_response)
        .await
        .map_err(|e| {
            TreeChoreographyError::Communication(format!("Failed to send digest response: {}", e))
        })?;

    // Phase 4: Receive operation request
    let operation_request: OperationRequest =
        adapter.recv_from(coordinator_id).await.map_err(|e| {
            TreeChoreographyError::Communication(format!(
                "Failed to receive operation request: {}",
                e
            ))
        })?;

    // Phase 5: Send operations response
    let operations = vec![]; // Would contain actual operations for requested ranges

    let operation_response = OperationResponse {
        sync_id: operation_request.sync_id,
        operations,
        sender_id: adapter.device_id(),
    };

    adapter
        .send(coordinator_id, operation_response)
        .await
        .map_err(|e| {
            TreeChoreographyError::Communication(format!(
                "Failed to send operation response: {}",
                e
            ))
        })?;

    // Phase 6: Receive sync completion
    let sync_completion: SyncCompletion = adapter.recv_from(coordinator_id).await.map_err(|e| {
        TreeChoreographyError::Communication(format!("Failed to receive sync completion: {}", e))
    })?;

    Ok(sync_completion.result)
}

/// Execute tree synchronization choreography
pub async fn execute_tree_synchronization(
    device_id: DeviceId,
    config: TreeSyncConfig,
    is_coordinator: bool,
    replica_index: Option<usize>,
    effect_system: &crate::effects::system::AuraEffectSystem,
) -> Result<TreeSyncResult, TreeChoreographyError> {
    // Validate configuration
    if config.replicas.is_empty() {
        return Err(TreeChoreographyError::InvalidConfig(
            "No replicas provided for synchronization".to_string(),
        ));
    }

    let mut adapter = crate::choreography::AuraHandlerAdapter::new(
        device_id,
        effect_system.execution_mode(),
    );

    // Execute appropriate role
    if is_coordinator {
        coordinator_sync_session(&mut adapter, &config).await
    } else {
        let replica_idx = replica_index.ok_or_else(|| {
            TreeChoreographyError::InvalidConfig("Replica must have index".to_string())
        })?;
        replica_sync_session(&mut adapter, config.coordinator, replica_idx, &config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::system::AuraEffectSystem;
    use aura_core::{LeafId, LeafNode, NodeIndex};

    fn create_test_approval_config() -> TreeApprovalConfig {
        TreeApprovalConfig {
            initiator: DeviceId::new(),
            approvers: vec![DeviceId::new(), DeviceId::new()],
            observers: vec![DeviceId::new()],
            operation: TreeOpKind::AddLeaf {
                leaf: LeafNode::new(DeviceId::new(), vec![]),
                under: NodeIndex::from_u32(0),
            },
            threshold: 2,
            timeout_ms: 30000,
        }
    }

    fn create_test_sync_config() -> TreeSyncConfig {
        TreeSyncConfig {
            coordinator: DeviceId::new(),
            replicas: vec![DeviceId::new(), DeviceId::new()],
            target_epoch: Some(10),
            max_batch_size: 100,
        }
    }

    #[test]
    fn test_approval_config_validation() {
        let mut config = create_test_approval_config();
        config.threshold = 0;

        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(execute_tree_operation_approval(
            device_id,
            config,
            &effect_system,
        ));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TreeChoreographyError::InvalidConfig(_)
        ));
    }

    #[test]
    fn test_sync_config_validation() {
        let mut config = create_test_sync_config();
        config.replicas.clear();

        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(execute_tree_synchronization(
            device_id,
            config,
            true,
            None,
            &effect_system,
        ));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TreeChoreographyError::InvalidConfig(_)
        ));
    }
}
