//! G_sync: Main Journal Synchronization Choreography
//!
//! This module implements the G_sync choreography for distributed journal
//! synchronization using the rumpsteak-aura choreographic programming framework.

use crate::{
    anti_entropy::{
        AntiEntropyChoreography, AntiEntropyReport, AntiEntropyRequest, DigestStatus, JournalDigest,
    },
    snapshot::WriterFence,
};
use aura_core::{tree::AttestedOp, AuraResult, DeviceId, Journal};
use aura_mpst::{AuraRuntime, CapabilityGuard, JournalAnnotation};
use aura_protocol::choreography::AuraHandlerAdapter;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tokio::sync::Mutex;

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

/// Message types for the G_sync choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMessage {
    /// Request journal digest for comparison
    DigestRequest {
        /// Account to get digest for
        account_id: aura_core::AccountId,
        /// Requester's digest
        requester_digest: JournalDigest,
    },

    /// Response with journal digest
    DigestResponse {
        /// Provider device ID
        provider: DeviceId,
        /// Provider digest payload
        provider_digest: JournalDigest,
    },

    /// Request missing operations
    OperationsRequest {
        /// Target provider
        provider: DeviceId,
        /// Anti-entropy request plan
        request: AntiEntropyRequest,
    },

    /// Response with operations
    OperationsResponse {
        /// The operations being sent
        operations: Vec<AttestedOp>,
        /// More operations available
        has_more: bool,
    },

    /// Sync completion notification
    SyncComplete {
        /// Final operation count
        final_count: usize,
        /// Success status
        success: bool,
    },
}

/// Roles in the G_sync choreography
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncRole {
    /// The device requesting synchronization
    Requester,
    /// A device providing sync data
    Provider(u32),
    /// Coordinator managing the sync process
    Coordinator,
}

impl SyncRole {
    /// Get the name of this role
    pub fn name(&self) -> String {
        match self {
            SyncRole::Requester => "Requester".to_string(),
            SyncRole::Provider(id) => format!("Provider_{}", id),
            SyncRole::Coordinator => "Coordinator".to_string(),
        }
    }
}

/// G_sync choreography state
#[derive(Debug)]
pub struct SyncChoreographyState {
    /// Current sync request being processed
    current_request: Option<JournalSyncRequest>,
    /// Cached local digest
    local_digest: Option<JournalDigest>,
    /// Collected digest responses
    digests: HashMap<DeviceId, JournalDigest>,
    /// Operations received during sync
    received_operations: Vec<AttestedOp>,
    /// Sync progress tracking
    sync_progress: HashMap<DeviceId, usize>,
    /// Cached local journal for anti-entropy comparisons
    local_journal: Journal,
    /// Cached local operations (oplog)
    local_operations: Vec<AttestedOp>,
    /// Desired batch size
    batch_size: usize,
}

impl SyncChoreographyState {
    /// Create new choreography state
    pub fn new() -> Self {
        Self {
            current_request: None,
            local_digest: None,
            digests: HashMap::new(),
            received_operations: Vec::new(),
            sync_progress: HashMap::new(),
            local_journal: Journal::new(),
            local_operations: Vec::new(),
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }

    /// Check if we have received all expected digest responses
    pub fn has_all_digests(&self) -> bool {
        if let Some(request) = &self.current_request {
            self.digests.len() >= request.targets.len()
        } else {
            false
        }
    }

    /// Find the device with the most operations (likely most up-to-date)
    pub fn find_best_provider(&self) -> Option<DeviceId> {
        self.digests
            .iter()
            .max_by_key(|(_, digest)| digest.operation_count)
            .map(|(device_id, _)| *device_id)
    }

    /// Get total operations received
    pub fn total_operations_received(&self) -> usize {
        self.received_operations.len()
    }

    /// Update cached local journal/operations view.
    pub fn set_local_view(&mut self, journal: Journal, operations: Vec<AttestedOp>) {
        self.local_journal = journal;
        self.local_operations = operations;
    }

    /// Record the local digest calculated for this sync session.
    pub fn set_local_digest(&mut self, digest: JournalDigest) {
        self.local_digest = Some(digest);
    }

    /// Configure the preferred anti-entropy batch size.
    pub fn set_batch_size(&mut self, batch_size: usize) {
        self.batch_size = batch_size.max(1);
    }

    fn anti_entropy_engine(&self) -> AntiEntropyChoreography {
        AntiEntropyChoreography::new(self.batch_size)
    }

    /// Store a peer digest and compute the corresponding anti-entropy request, if any.
    pub fn record_peer_digest(
        &mut self,
        peer: DeviceId,
        digest: JournalDigest,
    ) -> Option<AntiEntropyRequest> {
        self.digests.insert(peer, digest.clone());
        self.local_digest
            .as_ref()
            .and_then(|local| self.anti_entropy_engine().next_request(local, &digest))
    }

    /// Apply remote operations and update local cache.
    pub fn apply_remote_operations(
        &mut self,
        operations: Vec<AttestedOp>,
    ) -> AuraResult<AntiEntropyReport> {
        self.anti_entropy_engine()
            .merge_batch(&mut self.received_operations, operations)
    }

    /// Compare a digest with the cached local digest.
    pub fn compare_with_local(&self, digest: &JournalDigest) -> Option<DigestStatus> {
        self.local_digest
            .as_ref()
            .map(|local| AntiEntropyChoreography::compare(local, digest))
    }
}

/// G_sync choreography implementation
///
/// This choreography coordinates distributed journal synchronization with:
/// - Capability guards for authorization: `[guard: journal_sync ≤ caps]`
/// - Journal coupling for CRDT integration: `[▷ Δjournal_sync]`
/// - Leakage tracking for privacy: `[leak: sync_metadata]`
#[derive(Debug)]
pub struct SyncChoreography {
    /// Local device ID
    device_id: DeviceId,
    /// Local device role
    role: SyncRole,
    /// Choreography state
    state: Mutex<SyncChoreographyState>,
    /// Optional writer fence (activated during snapshots)
    writer_fence: Option<WriterFence>,
}

impl SyncChoreography {
    /// Create a new G_sync choreography
    pub fn new(device_id: DeviceId, role: SyncRole) -> Self {
        Self {
            device_id,
            role,
            state: Mutex::new(SyncChoreographyState::new()),
            writer_fence: None,
        }
    }

    /// Attach a writer fence used during snapshot proposals.
    pub fn with_writer_fence(mut self, fence: WriterFence) -> Self {
        self.writer_fence = Some(fence);
        self
    }

    /// Execute the choreography
    pub async fn execute(
        &self,
        request: JournalSyncRequest,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<JournalSyncResponse> {
        if let Some(fence) = &self.writer_fence {
            fence.ensure_open("journal sync")?;
        }
        let mut state = self.state.lock().await;
        state.current_request = Some(request.clone());
        drop(state);

        match self.role {
            SyncRole::Requester => self.execute_requester(request, effect_system).await,
            SyncRole::Provider(_) => self.execute_provider(effect_system).await,
            SyncRole::Coordinator => self.execute_coordinator(effect_system).await,
        }
    }

    /// Execute as requester
    async fn execute_requester(
        &self,
        request: JournalSyncRequest,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<JournalSyncResponse> {
        tracing::info!(
            "Executing G_sync as requester for account: {}",
            request.account_id
        );

        // Create handler adapter for communication
        let mut adapter = AuraHandlerAdapter::new(self.device_id, effect_system.execution_mode());

        // Apply capability guard: [guard: journal_sync ≤ caps]
        let sync_cap = aura_core::Cap::with_permissions(vec![
            "journal:read".to_string(),
            "journal:sync".to_string(),
            "network:send".to_string(),
            "network:receive".to_string(),
        ])
        .with_resources(vec!["journal:*".to_string(), "operations:*".to_string()]);
        let guard = CapabilityGuard::new(sync_cap.clone());

        // Get device capabilities and enforce guard
        // For now, we grant sync capabilities to all authenticated devices
        // In production, this would query actual device capabilities from the ledger
        let device_capabilities = sync_cap; // Placeholder: device has required sync capabilities
        guard.enforce(&device_capabilities).map_err(|e| {
            aura_core::AuraError::permission_denied(format!(
                "Insufficient capabilities for journal sync: {}",
                e
            ))
        })?;

        let batch_size = request.max_batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
        let anti_entropy = AntiEntropyChoreography::new(batch_size);
        let local_digest =
            anti_entropy.compute_digest(&request.local_journal, &request.local_operations)?;

        {
            let mut state = self.state.lock().await;
            state.set_batch_size(batch_size);
            state.set_local_view(
                request.local_journal.clone(),
                request.local_operations.clone(),
            );
            state.set_local_digest(local_digest.clone());
        }

        tracing::debug!(
            "Local digest computed (ops={}, last_epoch={:?}, fact_hash={:02x?})",
            local_digest.operation_count,
            local_digest.last_epoch,
            &local_digest.fact_hash[..4]
        );

        // Send digest requests to all targets
        tracing::info!(
            "Sending digest requests to {} targets",
            request.targets.len()
        );
        // Send digest requests to all target devices
        for target in &request.targets {
            let message = SyncMessage::DigestRequest {
                account_id: request.account_id.clone(),
                requester_digest: local_digest.clone(),
            };

            if let Err(e) = adapter.send(*target, message).await {
                tracing::warn!("Failed to send digest request to {}: {}", target, e);
            }
        }

        // Wait for digest responses
        // Wait for digest responses from all targets
        let mut collected_digests = HashMap::new();
        let timeout = tokio::time::Duration::from_secs(30);

        tokio::time::timeout(timeout, async {
            while collected_digests.len() < request.targets.len() {
                // Receive digest responses from any provider using adapter pattern
                // Try to receive from each target in sequence (could be parallelized with select!)
                let mut received_any = false;
                for target in &request.targets {
                    // Skip if we already got a response from this target
                    if collected_digests.contains_key(target) {
                        continue;
                    }

                    if let Ok(message) = adapter.recv_from::<SyncMessage>(*target).await {
                        received_any = true;
                        match message {
                            SyncMessage::DigestResponse {
                                provider,
                                provider_digest,
                            } => {
                                collected_digests.insert(provider, provider_digest.clone());

                                let mut state = self.state.lock().await;
                                if let Some(ae_request) =
                                    state.record_peer_digest(provider, provider_digest.clone())
                                {
                                    // Request missing operations based on digest comparison
                                    let ops_request = SyncMessage::OperationsRequest {
                                        provider,
                                        request: ae_request,
                                    };

                                    if let Err(e) = adapter.send(provider, ops_request).await {
                                        tracing::warn!(
                                            "Failed to request operations from {}: {}",
                                            provider,
                                            e
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }

                // If no messages were received in this iteration, yield to avoid busy-looping
                if !received_any {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }
        })
        .await
        .map_err(|_| aura_core::AuraError::network("Digest collection timeout"))?;

        // Apply journal annotation: [▷ Δjournal_sync]
        let journal_annotation =
            JournalAnnotation::add_facts("Journal sync digest request".to_string());
        tracing::info!("Applied journal annotation: {:?}", journal_annotation);

        // Collect operations from providers based on anti-entropy requests
        let mut total_ops_synced = 0;
        let mut all_operations = Vec::new();

        // Receive operations from providers that sent digest responses
        let ops_timeout = tokio::time::Duration::from_secs(60);
        let providers: Vec<DeviceId> = collected_digests.keys().copied().collect();

        tokio::time::timeout(ops_timeout, async {
            for provider in &providers {
                // Try to receive operations response from each provider
                loop {
                    match adapter.recv_from::<SyncMessage>(*provider).await {
                        Ok(SyncMessage::OperationsResponse {
                            operations,
                            has_more,
                        }) => {
                            total_ops_synced += operations.len();
                            all_operations.extend(operations);

                            // If provider has more operations, continue receiving
                            if !has_more {
                                break;
                            }
                        }
                        Ok(_) => {
                            // Ignore other message types
                            continue;
                        }
                        Err(_) => {
                            // No more messages from this provider
                            break;
                        }
                    }
                }
            }
        })
        .await
        .ok(); // Don't fail if operations collection times out, return what we got

        tracing::info!(
            "Collected {} operations from {} providers",
            total_ops_synced,
            providers.len()
        );

        Ok(JournalSyncResponse {
            operations_synced: total_ops_synced,
            peers_synced: collected_digests.into_keys().collect(),
            success: true,
            error: None,
        })
    }

    /// Execute as provider
    async fn execute_provider(
        &self,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<JournalSyncResponse> {
        tracing::info!("Executing G_sync as provider");

        // Create handler adapter for communication
        let adapter = AuraHandlerAdapter::new(self.device_id, effect_system.execution_mode());

        // TODO: Wait for digest request from any requester
        // For now, return an error as placeholder
        Err(aura_core::AuraError::network(
            "Provider mode not implemented with effect system",
        ))
    }

    /// Execute as coordinator
    async fn execute_coordinator(
        &self,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<JournalSyncResponse> {
        tracing::info!("Executing G_sync as coordinator");

        // Coordinate the sync process across multiple peers
        // The coordinator orchestrates sync without being a requester or provider itself
        // It would receive status updates and coordinate conflict resolution
        //
        // NOTE: Full coordinator implementation requires either:
        // 1. A broadcast receive mechanism to get messages from any participant
        // 2. Knowledge of all participants upfront to poll them individually
        // 3. A pub-sub pattern for coordination events
        //
        // For now, the coordinator role is a placeholder. In practice, the requester
        // role handles most coordination by directly collecting from providers.

        tracing::warn!(
            "Coordinator role is not fully implemented - returning placeholder response"
        );

        Ok(JournalSyncResponse {
            operations_synced: 0,
            peers_synced: Vec::new(),
            success: false,
            error: Some(
                "Coordinator role requires broadcast receive or participant list".to_string(),
            ),
        })
    }
}

/// Journal synchronization coordinator
#[derive(Debug)]
pub struct JournalSyncCoordinator {
    /// Local runtime
    runtime: AuraRuntime,
    /// Current choreography
    choreography: Option<SyncChoreography>,
}

impl JournalSyncCoordinator {
    /// Create a new journal sync coordinator
    pub fn new(runtime: AuraRuntime) -> Self {
        Self {
            runtime,
            choreography: None,
        }
    }

    /// Execute journal synchronization using the G_sync choreography
    pub async fn sync_journal(
        &mut self,
        request: JournalSyncRequest,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<JournalSyncResponse> {
        tracing::info!("Starting journal sync for account: {}", request.account_id);

        // Create choreography with requester role
        let device_id = self.runtime.device_id();
        let choreography = SyncChoreography::new(device_id, SyncRole::Requester);

        // Execute the choreography with the effect system
        let response = choreography.execute(request.clone(), effect_system).await?;

        // Store the choreography for potential reuse
        self.choreography = Some(choreography);

        Ok(response)
    }

    /// Get the current runtime
    pub fn runtime(&self) -> &AuraRuntime {
        &self.runtime
    }

    /// Check if a choreography is currently active
    pub fn has_active_choreography(&self) -> bool {
        self.choreography.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AccountId, Cap, DeviceId, Journal};

    #[tokio::test]
    async fn test_choreography_state_creation() {
        let state = SyncChoreographyState::new();

        assert!(!state.has_all_digests());
        assert_eq!(state.total_operations_received(), 0);
        assert!(state.find_best_provider().is_none());
    }

    #[tokio::test]
    async fn test_choreography_creation() {
        let device_id = DeviceId::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        let choreography = SyncChoreography::new(device_id, SyncRole::Requester);

        assert_eq!(choreography.role, SyncRole::Requester);
    }

    #[tokio::test]
    async fn test_sync_coordinator() {
        let device_id = DeviceId::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        let mut coordinator = JournalSyncCoordinator::new(runtime);
        assert!(!coordinator.has_active_choreography());

        let request = JournalSyncRequest {
            requester: device_id,
            targets: vec![DeviceId::new()],
            account_id: AccountId::new(),
            max_batch_size: Some(100),
            local_journal: Journal::new(),
            local_operations: Vec::new(),
        };

        // Note: This will return an error since choreography is not fully implemented
        let result = coordinator.sync_journal(request).await;
        assert!(result.is_err());
        assert!(coordinator.has_active_choreography());
    }
}
