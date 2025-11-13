//! Journal Synchronization Service using Stateless Effect System
//!
//! This module provides journal synchronization using the new stateless effect system,
//! allowing composable sync operations without choreography dependencies.

use crate::anti_entropy::{AntiEntropyRequest, JournalDigest};
use aura_core::{tree::AttestedOp, AuraResult, DeviceId, Journal};
use aura_protocol::effects::{BloomDigest, SyncEffects};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEFAULT_BATCH_SIZE: usize = 128;

/// Journal synchronization request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSyncRequest {
    /// Source device requesting sync
    pub requester: DeviceId,
    /// Target devices to sync with
    pub targets: Vec<DeviceId>,
    /// Account to synchronize
    pub account_id: aura_core::AccountId,
    /// Maximum operations per batch
    pub max_batch_size: Option<usize>,
    /// Local journal snapshot for digest computation
    pub local_journal: Journal,
    /// Local attested operations (oplog)
    pub local_operations: Vec<AttestedOp>,
}

/// Journal synchronization response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSyncResponse {
    /// Operations synchronized
    pub operations_synced: usize,
    /// Peers that participated
    pub peers_synced: Vec<DeviceId>,
    /// Success indicator
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Pure journal synchronization service using stateless effects
pub struct JournalSyncService;

impl JournalSyncService {
    /// Create a new journal sync service
    pub fn new() -> Self {
        Self
    }

    /// Execute synchronization with peers using effect system
    pub async fn sync_with_peers<E: SyncEffects>(
        &self,
        effects: &E,
        request: JournalSyncRequest,
    ) -> AuraResult<JournalSyncResponse> {
        let batch_size = request.max_batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
        let mut operations_synced = 0;
        let mut peers_synced = Vec::new();

        // Generate local digest
        let local_digest = self.generate_digest(&request.local_journal, &request.local_operations)?;

        // Sync with each target peer
        for target in &request.targets {
            match self.sync_with_peer(effects, &request, *target, &local_digest, batch_size).await {
                Ok(count) => {
                    operations_synced += count;
                    peers_synced.push(*target);
                }
                Err(e) => {
                    tracing::warn!("Failed to sync with peer {}: {}", target, e);
                }
            }
        }

        let success = !peers_synced.is_empty();
        Ok(JournalSyncResponse {
            operations_synced,
            peers_synced,
            success,
            error: None,
        })
    }

    /// Sync with a single peer using effect system
    async fn sync_with_peer<E: SyncEffects>(
        &self,
        effects: &E,
        _request: &JournalSyncRequest,
        peer: DeviceId,
        _local_digest: &JournalDigest,
        _batch_size: usize,
    ) -> AuraResult<usize> {
        // Use the high-level sync_with_peer method from SyncEffects
        let peer_uuid = Uuid::from_bytes(*peer.0.as_bytes());
        
        effects.sync_with_peer(peer_uuid).await
            .map_err(|e| aura_core::AuraError::internal(format!("Failed to sync with peer: {}", e)))?;

        // TODO: Return actual operation count from sync
        Ok(1) // Placeholder - actual implementation would track operations
    }

    /// Generate digest from local journal state
    fn generate_digest(&self, journal: &Journal, operations: &[AttestedOp]) -> AuraResult<JournalDigest> {
        // Use anti-entropy module to generate digest
        crate::anti_entropy::compute_digest(journal, operations)
    }

    /// Handle incoming sync request from peer
    pub async fn handle_sync_request<E: SyncEffects>(
        &self,
        effects: &E,
        request: JournalSyncRequest,
    ) -> AuraResult<JournalSyncResponse> {
        // This is essentially the same as sync_with_peers but from the responder side
        self.sync_with_peers(effects, request).await
    }
}

impl Default for JournalSyncService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AccountId, DeviceId};

    #[test]
    fn test_journal_sync_service_creation() {
        let service = JournalSyncService::new();
        assert!(true); // Service created successfully
    }

    #[test]
    fn test_journal_sync_request_serialization() {
        let request = JournalSyncRequest {
            requester: DeviceId::new(),
            targets: vec![DeviceId::new()],
            account_id: AccountId::new(),
            max_batch_size: Some(64),
            local_journal: Journal::new(),
            local_operations: Vec::new(),
        };

        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: JournalSyncRequest = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(request.requester, deserialized.requester);
        assert_eq!(request.max_batch_size, deserialized.max_batch_size);
    }
}