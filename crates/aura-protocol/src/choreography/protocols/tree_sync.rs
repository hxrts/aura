//! Tree Synchronization Choreography
//!
//! This module implements choreographic protocols for tree synchronization operations.

use crate::effects::{CoordinationError, TreeDigest};
use aura_core::{AttestedOp, DeviceId, Hash32};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    #[error("Handler error: {0}")]
    Handler(#[from] crate::handlers::AuraHandlerError),
    #[error("Coordination error: {0}")]
    Coordination(#[from] CoordinationError),
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

// Tree synchronization choreography
choreography! {
    #[namespace = "tree_synchronization"]
    protocol TreeSynchronizationProtocol {
        roles: SyncCoordinator, Replica1, Replica2, Replica3;

        // Phase 1: Coordinator initiates sync with all replicas
        SyncCoordinator -> Replica1: SyncInitiate(SyncInitiation);
        SyncCoordinator -> Replica2: SyncInitiate(SyncInitiation);
        SyncCoordinator -> Replica3: SyncInitiate(SyncInitiation);

        // Phase 2: Coordinator requests digests from all replicas
        SyncCoordinator -> Replica1: SyncRequestDigest(DigestRequest);
        SyncCoordinator -> Replica2: SyncRequestDigest(DigestRequest);
        SyncCoordinator -> Replica3: SyncRequestDigest(DigestRequest);

        // Phase 3: Replicas send digests back
        Replica1 -> SyncCoordinator: SyncSendDigest(DigestResponse);
        Replica2 -> SyncCoordinator: SyncSendDigest(DigestResponse);
        Replica3 -> SyncCoordinator: SyncSendDigest(DigestResponse);

        // Phase 4: Coordinator requests missing operations
        SyncCoordinator -> Replica1: SyncRequestOperations(OperationRequest);
        SyncCoordinator -> Replica2: SyncRequestOperations(OperationRequest);
        SyncCoordinator -> Replica3: SyncRequestOperations(OperationRequest);

        // Phase 5: Replicas send operations back
        Replica1 -> SyncCoordinator: SyncSendOperations(OperationResponse);
        Replica2 -> SyncCoordinator: SyncSendOperations(OperationResponse);
        Replica3 -> SyncCoordinator: SyncSendOperations(OperationResponse);

        // Phase 6: Coordinator broadcasts sync completion
        SyncCoordinator -> Replica1: SyncComplete(SyncCompletion);
        SyncCoordinator -> Replica2: SyncComplete(SyncCompletion);
        SyncCoordinator -> Replica3: SyncComplete(SyncCompletion);
    }
}

/// Coordinator's role in tree synchronization
pub async fn coordinator_sync_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    config: &TreeSyncConfig,
) -> Result<TreeSyncResult, TreeChoreographyError> {
    #[allow(clippy::disallowed_methods)]
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
    let missing_ranges = vec![0..5]; // Simplified - would calculate from digest comparison
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
pub async fn replica_sync_session(
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
    effect_system: &crate::effects::AuraEffectSystem,
) -> Result<TreeSyncResult, TreeChoreographyError> {
    // Validate configuration
    if config.replicas.is_empty() {
        return Err(TreeChoreographyError::InvalidConfig(
            "No replicas provided for synchronization".to_string(),
        ));
    }

    let mut adapter =
        crate::choreography::AuraHandlerAdapter::new(device_id, effect_system.execution_mode());

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
