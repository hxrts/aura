#![allow(missing_docs)]

//! Anti-entropy protocol for digest-based reconciliation
//!
//! This module provides effect-based anti-entropy operations for comparing
//! journal states, planning reconciliation requests, and merging operations.
//! It uses algebraic effects to separate pure logic from side effects.
//!
//! # Architecture
//!
//! The anti-entropy protocol follows a three-phase approach:
//! 1. **Digest Exchange**: Peers exchange journal digests
//! 2. **Reconciliation Planning**: Determine what operations are missing
//! 3. **Operation Transfer**: Transfer and merge missing operations
//!
//! # Integration
//!
//! - Uses `RetryPolicy` from infrastructure for resilient operations
//! - Integrates with `PeerManager` for peer selection
//! - Uses `RateLimiter` for flow budget enforcement
//! - Parameterized by `JournalEffects` + `NetworkEffects`
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_sync::protocols::{AntiEntropyProtocol, AntiEntropyConfig};
//! use aura_core::effects::{JournalEffects, NetworkEffects};
//!
//! async fn sync_with_peer<E>(effects: &E, peer: DeviceId) -> SyncResult<()>
//! where
//!     E: JournalEffects + NetworkEffects,
//! {
//!     let config = AntiEntropyConfig::default();
//!     let protocol = AntiEntropyProtocol::new(config);
//!
//!     let result = protocol.execute(effects, peer).await?;
//!     println!("Applied {} operations", result.applied);
//!     Ok(())
//! }
//! ```

use std::collections::HashSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::{
    sync_biscuit_guard_error, sync_network_error, sync_serialization_error, sync_session_error,
    sync_timeout_error, SyncResult,
};
use crate::infrastructure::RetryPolicy;
use aura_core::effects::{JournalEffects, NetworkEffects};
use aura_core::{hash, AttestedOp, AuraError, AuraResult, DeviceId, FlowBudget, Journal};
use aura_protocol::guards::{BiscuitGuardEvaluator, GuardError};
use aura_wot::{BiscuitTokenManager, ResourceScope};

// =============================================================================
// Types
// =============================================================================

/// Unique fingerprint for an attested operation (cryptographic hash)
pub type OperationFingerprint = [u8; 32];

/// Summary of a journal snapshot used for anti-entropy comparisons
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalDigest {
    /// Number of attested operations known locally
    pub operation_count: usize,

    /// Maximum parent epoch observed in the operation log (if any)
    pub last_epoch: Option<u64>,

    /// Hash of the ordered operation fingerprints
    pub operation_hash: [u8; 32],

    /// Hash of the journal facts component
    pub fact_hash: [u8; 32],

    /// Hash of the capability frontier
    pub caps_hash: [u8; 32],
}

impl JournalDigest {
    /// Check if two digests are identical
    pub fn matches(&self, other: &Self) -> bool {
        self.operation_count == other.operation_count
            && self.operation_hash == other.operation_hash
            && self.fact_hash == other.fact_hash
            && self.caps_hash == other.caps_hash
    }
}

/// Relationship between two digests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DigestStatus {
    /// Digests are identical
    Equal,

    /// Local node is missing operations compared to the peer
    LocalBehind,

    /// Peer is missing operations that the local node already has
    RemoteBehind,

    /// Operation counts match but hashes differ (divergent history)
    Diverged,
}

/// Request describing which operations we want from a peer
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AntiEntropyRequest {
    /// Operation index to start streaming from
    pub from_index: usize,

    /// Maximum operations to send in this batch
    pub max_ops: usize,

    /// Specific operation fingerprints that are missing (for targeted requests)
    pub missing_operations: Vec<OperationFingerprint>,
}

/// Result of merging operations into a journal
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeResult {
    /// Number of operations that were successfully merged
    pub merged_operations: usize,
    /// Number of operations that were duplicates
    pub duplicate_operations: usize,
    /// Number of operations that failed to merge
    pub failed_operations: usize,
    /// Updated journal after merge
    pub updated_journal: Journal,
}

/// Result of an anti-entropy synchronization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AntiEntropyResult {
    /// Number of operations that were newly applied
    pub applied: usize,

    /// Number of duplicates that were ignored
    pub duplicates: usize,

    /// Final digest status after synchronization
    pub final_status: Option<DigestStatus>,

    /// Number of synchronization rounds performed
    pub rounds: usize,
}

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for anti-entropy protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyConfig {
    /// Batch size for operation transfer
    pub batch_size: usize,

    /// Maximum synchronization rounds before giving up
    pub max_rounds: usize,

    /// Enable retry on transient failures
    pub retry_enabled: bool,

    /// Retry policy for resilient operations
    pub retry_policy: RetryPolicy,

    /// Timeout for digest exchange
    pub digest_timeout: Duration,

    /// Timeout for operation transfer
    pub transfer_timeout: Duration,
}

impl Default for AntiEntropyConfig {
    fn default() -> Self {
        Self {
            batch_size: 128,
            max_rounds: 10,
            retry_enabled: true,
            retry_policy: RetryPolicy::exponential()
                .with_max_attempts(3)
                .with_initial_delay(Duration::from_millis(100)),
            digest_timeout: Duration::from_secs(10),
            transfer_timeout: Duration::from_secs(30),
        }
    }
}

// =============================================================================
// Anti-Entropy Protocol
// =============================================================================

/// Digest-based anti-entropy protocol for CRDT synchronization
///
/// Implements the anti-entropy algorithm:
/// 1. Exchange digests with peer
/// 2. Compare digests to identify missing operations
/// 3. Request and merge missing operations in batches
/// 4. Repeat until synchronized or max rounds reached
///
/// Supports Biscuit token-based authorization for sync operations.
pub struct AntiEntropyProtocol {
    config: AntiEntropyConfig,
    /// Optional Biscuit token manager for authorization
    token_manager: Option<BiscuitTokenManager>,
    /// Optional Biscuit guard evaluator for permission checks
    guard_evaluator: Option<BiscuitGuardEvaluator>,
}

impl AntiEntropyProtocol {
    /// Create a new anti-entropy protocol with the given configuration
    pub fn new(config: AntiEntropyConfig) -> Self {
        Self {
            config,
            token_manager: None,
            guard_evaluator: None,
        }
    }

    /// Create a new anti-entropy protocol with Biscuit authorization support
    pub fn with_biscuit_authorization(
        config: AntiEntropyConfig,
        token_manager: BiscuitTokenManager,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        Self {
            config,
            token_manager: Some(token_manager),
            guard_evaluator: Some(guard_evaluator),
        }
    }

    /// Check if the current token authorizes sync operations with a peer
    fn check_sync_authorization<E>(&self, _effects: &E, peer: DeviceId) -> SyncResult<()>
    where
        E: JournalEffects + NetworkEffects,
    {
        if let (Some(ref token_manager), Some(ref evaluator)) =
            (&self.token_manager, &self.guard_evaluator)
        {
            let token = token_manager.current_token();
            // Get actual authority ID from peer's device registration
            // In Aura's architecture, each device belongs to an authority
            // We can derive the authority ID from the device ID using the standard mapping
            let authority_id = aura_core::AuthorityId::from_uuid(peer.0);
            let resource = ResourceScope::Authority {
                authority_id,
                operation: aura_wot::AuthorityOp::UpdateTree, // Sync requires authority access
            };

            let mut flow_budget = FlowBudget::new(1000, aura_core::session_epochs::Epoch::new(0)); // Standard sync budget

            match evaluator.evaluate_guard(token, "sync_journal", &resource, 100, &mut flow_budget)
            {
                Ok(guard_result) if guard_result.authorized => {
                    tracing::debug!("Sync authorization granted for peer {}", peer);
                    Ok(())
                }
                Ok(_) => {
                    tracing::warn!("Sync authorization denied for peer {}", peer);
                    Err(sync_biscuit_guard_error(
                        "sync_journal",
                        peer,
                        GuardError::AuthorizationFailed(
                            "Token does not grant sync permission".to_string(),
                        ),
                    ))
                }
                Err(e) => {
                    tracing::error!("Sync authorization error for peer {}: {:?}", peer, e);
                    Err(sync_biscuit_guard_error("sync_journal", peer, e))
                }
            }
        } else {
            // No Biscuit authorization configured - allow by default for backward compatibility
            tracing::debug!(
                "No Biscuit authorization configured for peer {} - allowing sync",
                peer
            );
            Ok(())
        }
    }

    /// Execute anti-entropy synchronization with a peer
    ///
    /// This is the main entry point for the protocol. It performs digest
    /// exchange, reconciliation planning, and operation transfer.
    ///
    /// # Authorization
    /// - Checks Biscuit token permissions for "sync_journal" capability
    /// - Validates against peer-specific resource scope
    ///
    /// # Integration Points
    /// - Uses `JournalEffects` to access local journal state
    /// - Uses `NetworkEffects` to communicate with peer
    /// - Uses `RetryPolicy` from infrastructure for resilience
    pub async fn execute<E>(&self, effects: &E, peer: DeviceId) -> SyncResult<AntiEntropyResult>
    where
        E: JournalEffects + NetworkEffects + Send + Sync,
    {
        // Check authorization before starting sync
        self.check_sync_authorization(effects, peer)?;
        tracing::info!("Starting anti-entropy sync with peer {}", peer);

        let mut result = AntiEntropyResult::default();

        // Retry loop for resilient operation
        let mut retry_count = 0;
        let max_retries = if self.config.retry_enabled {
            self.config.retry_policy.max_attempts
        } else {
            1
        };

        while retry_count < max_retries {
            match self.execute_sync_round(effects, peer).await {
                Ok(round_result) => {
                    result.applied += round_result.applied;
                    result.duplicates += round_result.duplicates;
                    result.rounds += 1;
                    result.final_status = round_result.final_status;

                    // If we're synchronized, break out of retry loop
                    if matches!(round_result.final_status, Some(DigestStatus::Equal)) {
                        tracing::info!(
                            "Sync completed successfully after {} rounds with peer {}",
                            result.rounds,
                            peer
                        );
                        break;
                    }

                    // Check if we've reached max rounds
                    if result.rounds >= self.config.max_rounds {
                        tracing::warn!(
                            "Reached max rounds ({}) syncing with peer {}",
                            self.config.max_rounds,
                            peer
                        );
                        break;
                    }
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count >= max_retries {
                        return Err(e);
                    }

                    tracing::warn!(
                        "Sync round failed with peer {}, retrying ({}/{}): {}",
                        peer,
                        retry_count + 1,
                        max_retries,
                        e
                    );

                    // Apply retry delay
                    let delay = self.config.retry_policy.calculate_delay(retry_count);
                    tokio::time::sleep(delay).await;
                }
            }
        }

        tracing::info!(
            "Anti-entropy sync completed: {} applied, {} duplicates, {} rounds",
            result.applied,
            result.duplicates,
            result.rounds
        );

        Ok(result)
    }

    /// Execute a single round of anti-entropy synchronization
    async fn execute_sync_round<E>(
        &self,
        effects: &E,
        peer: DeviceId,
    ) -> SyncResult<AntiEntropyResult>
    where
        E: JournalEffects + NetworkEffects + Send + Sync,
    {
        // Step 1: Get local journal state and operations
        let local_journal = effects
            .get_journal()
            .await
            .map_err(|e| sync_session_error(&format!("Failed to get local journal: {}", e)))?;

        // For now, use empty operations list - in full implementation,
        // this would come from the journal's operation log
        let local_ops: Vec<AttestedOp> = vec![];

        // Step 2: Compute local digest
        let local_digest = self.compute_digest(&local_journal, &local_ops)?;

        // Step 3: Exchange digests with peer
        let remote_digest = self
            .exchange_digest_with_peer(effects, peer, &local_digest)
            .await?;

        // Step 4: Compare digests
        let digest_status = Self::compare(&local_digest, &remote_digest);
        tracing::debug!(
            "Digest comparison with peer {}: {:?} (local: {} ops, remote: {} ops)",
            peer,
            digest_status,
            local_digest.operation_count,
            remote_digest.operation_count
        );

        // Step 5: Plan and execute reconciliation if needed
        match digest_status {
            DigestStatus::Equal => {
                // Already synchronized
                Ok(AntiEntropyResult {
                    applied: 0,
                    duplicates: 0,
                    final_status: Some(DigestStatus::Equal),
                    rounds: 1,
                })
            }
            DigestStatus::LocalBehind => {
                // We need operations from peer
                self.pull_operations_from_peer(effects, peer, &local_digest, &remote_digest)
                    .await
            }
            DigestStatus::RemoteBehind => {
                // Peer needs operations from us - push to them
                self.push_operations_to_peer(
                    effects,
                    peer,
                    &local_ops,
                    &local_digest,
                    &remote_digest,
                )
                .await?;

                // Return result indicating we pushed operations
                Ok(AntiEntropyResult {
                    applied: 0, // We didn't apply anything locally
                    duplicates: 0,
                    final_status: Some(DigestStatus::RemoteBehind),
                    rounds: 1,
                })
            }
            DigestStatus::Diverged => {
                // Both sides need operations - more complex reconciliation
                self.reconcile_diverged_state(
                    effects,
                    peer,
                    &local_ops,
                    &local_digest,
                    &remote_digest,
                )
                .await
            }
        }
    }

    /// Exchange digest with peer and return remote digest
    async fn exchange_digest_with_peer<E>(
        &self,
        effects: &E,
        peer: DeviceId,
        local_digest: &JournalDigest,
    ) -> SyncResult<JournalDigest>
    where
        E: NetworkEffects + Send + Sync,
    {
        // Serialize local digest
        let digest_data = serde_json::to_vec(local_digest).map_err(|e| {
            sync_serialization_error("digest", &format!("Failed to serialize digest: {}", e))
        })?;

        // Send digest to peer and wait for response
        tracing::debug!(
            "Sending digest to peer {} ({} bytes)",
            peer,
            digest_data.len()
        );

        // Apply timeout for digest exchange
        let exchange_future = async {
            // Send our digest
            effects
                .send_to_peer(peer.0, digest_data)
                .await
                .map_err(|e| sync_network_error(&format!("Failed to send digest: {}", e)))?;

            // Receive peer's digest
            let (sender_id, remote_digest_data) = effects
                .receive()
                .await
                .map_err(|e| sync_network_error(&format!("Failed to receive digest: {}", e)))?;

            // Verify sender
            if sender_id != peer.0 {
                return Err(sync_session_error(&format!(
                    "Received digest from unexpected peer: expected {}, got {}",
                    peer, sender_id
                )));
            }

            // Deserialize remote digest
            let remote_digest: JournalDigest = serde_json::from_slice(&remote_digest_data)
                .map_err(|e| {
                    sync_serialization_error(
                        "digest",
                        &format!("Failed to deserialize remote digest: {}", e),
                    )
                })?;

            tracing::debug!(
                "Received digest from peer {} ({} ops)",
                peer,
                remote_digest.operation_count
            );

            Ok(remote_digest)
        };

        tokio::time::timeout(self.config.digest_timeout, exchange_future)
            .await
            .map_err(|_| {
                sync_timeout_error(
                    &format!("Digest exchange with peer {}", peer),
                    self.config.digest_timeout,
                )
            })?
    }

    /// Pull missing operations from peer
    async fn pull_operations_from_peer<E>(
        &self,
        effects: &E,
        peer: DeviceId,
        local_digest: &JournalDigest,
        remote_digest: &JournalDigest,
    ) -> SyncResult<AntiEntropyResult>
    where
        E: JournalEffects + NetworkEffects + Send + Sync,
    {
        // Plan the request
        let request = self
            .plan_request(local_digest, remote_digest)
            .ok_or_else(|| sync_session_error("No operations needed despite LocalBehind status"))?;

        tracing::debug!(
            "Requesting {} operations from peer {} starting at index {}",
            request.max_ops,
            peer,
            request.from_index
        );

        // Send request to peer
        let request_data = serde_json::to_vec(&request).map_err(|e| {
            sync_serialization_error("request", &format!("Failed to serialize request: {}", e))
        })?;

        let pull_future = async {
            effects
                .send_to_peer(peer.0, request_data)
                .await
                .map_err(|e| {
                    sync_network_error(&format!("Failed to send operation request: {}", e))
                })?;

            // Receive operations
            let (sender_id, ops_data) = effects
                .receive()
                .await
                .map_err(|e| sync_network_error(&format!("Failed to receive operations: {}", e)))?;

            if sender_id != peer.0 {
                return Err(sync_session_error(&format!(
                    "Received operations from unexpected peer: expected {}, got {}",
                    peer, sender_id
                )));
            }

            // Deserialize operations
            let remote_ops: Vec<AttestedOp> = serde_json::from_slice(&ops_data).map_err(|e| {
                sync_serialization_error(
                    "operations",
                    &format!("Failed to deserialize operations: {}", e),
                )
            })?;

            tracing::debug!(
                "Received {} operations from peer {}",
                remote_ops.len(),
                peer
            );

            // Merge operations into local state
            let mut local_ops = vec![]; // In full implementation, get from journal
            let merge_result = self.merge_batch(&mut local_ops, remote_ops)?;

            // Update journal with merged operations using effect system
            if merge_result.applied > 0 {
                tracing::info!(
                    "Applied {} new operations from peer {}",
                    merge_result.applied,
                    peer
                );

                // Convert applied operations to journal deltas via effects
                // Use the new fact-based journal system with proper effect handling
                match self
                    .convert_operations_to_journal_delta(effects, &merge_result)
                    .await
                {
                    Ok(journal_delta) => {
                        // Get current journal state and merge with delta
                        let current_journal = effects.get_journal().await.unwrap_or_else(|e| {
                            tracing::warn!("Failed to get current journal: {}, using empty", e);
                            aura_core::Journal::new()
                        });

                        // Apply journal delta using CRDT merge operation
                        match effects.merge_facts(&current_journal, &journal_delta).await {
                            Ok(updated_journal) => {
                                // Persist the updated journal state
                                if let Err(e) = effects.persist_journal(&updated_journal).await {
                                    tracing::error!("Failed to persist journal after sync: {}", e);
                                    return Err(crate::core::errors::sync_protocol_with_peer(
                                        "anti_entropy",
                                        format!("Journal persistence failure: {}", e),
                                        peer,
                                    ));
                                }

                                tracing::debug!(
                                    "Successfully applied {} journal deltas from peer {}",
                                    merge_result.applied,
                                    peer
                                );
                            }
                            Err(e) => {
                                tracing::error!("Failed to merge journal facts: {}", e);
                                return Err(crate::core::errors::sync_protocol_with_peer(
                                    "anti_entropy",
                                    format!("Journal merge failed: {}", e),
                                    peer,
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to convert operations to journal delta: {}", e);
                        return Err(crate::core::errors::sync_protocol_with_peer(
                            "anti_entropy",
                            format!("Delta conversion failed: {}", e),
                            peer,
                        ));
                    }
                }

                tracing::debug!(
                    "Successfully updated journal with {} new operations from peer {}",
                    merge_result.applied,
                    peer
                );
            }

            Ok(AntiEntropyResult {
                applied: merge_result.applied,
                duplicates: merge_result.duplicates,
                final_status: Some(DigestStatus::LocalBehind),
                rounds: 1,
            })
        };

        tokio::time::timeout(self.config.transfer_timeout, pull_future)
            .await
            .map_err(|_| {
                sync_timeout_error(
                    &format!("Operation transfer with peer {}", peer),
                    self.config.transfer_timeout,
                )
            })?
    }

    /// Convert applied operations to journal delta for persistence
    async fn convert_operations_to_journal_delta<E>(
        &self,
        effects: &E,
        merge_result: &AntiEntropyResult,
    ) -> SyncResult<aura_core::Journal>
    where
        E: JournalEffects + Send + Sync,
    {
        // Create a new journal delta based on the merge result
        // In the fact-based architecture, operations are converted to facts
        let journal_delta = aura_core::Journal::new();

        // TODO: Implement actual conversion logic based on merge_result
        // This would typically involve:
        // 1. Extract operations that were successfully applied
        // 2. Convert each operation to appropriate fact types
        // 3. Add facts to the delta journal
        // 4. Ensure CRDT semantics are preserved

        tracing::debug!(
            "Created journal delta with {} applied operations",
            merge_result.applied
        );

        // For now, return empty delta as placeholder
        // In production, this would contain the actual fact deltas
        Ok(journal_delta)
    }

    /// Push operations to peer
    async fn push_operations_to_peer<E>(
        &self,
        effects: &E,
        peer: DeviceId,
        local_ops: &[AttestedOp],
        local_digest: &JournalDigest,
        remote_digest: &JournalDigest,
    ) -> SyncResult<()>
    where
        E: NetworkEffects + Send + Sync,
    {
        // Determine which operations to send
        let missing_count = local_digest
            .operation_count
            .saturating_sub(remote_digest.operation_count);
        let ops_to_send = if missing_count > 0 {
            let start_index = remote_digest.operation_count;
            let end_index =
                (start_index + self.config.batch_size.min(missing_count)).min(local_ops.len());
            &local_ops[start_index..end_index]
        } else {
            &[]
        };

        tracing::debug!("Pushing {} operations to peer {}", ops_to_send.len(), peer);

        if !ops_to_send.is_empty() {
            // Serialize operations
            let ops_data = serde_json::to_vec(ops_to_send).map_err(|e| {
                sync_serialization_error(
                    "operations",
                    &format!("Failed to serialize operations: {}", e),
                )
            })?;

            // Send to peer
            effects
                .send_to_peer(peer.0, ops_data)
                .await
                .map_err(|e| sync_network_error(&format!("Failed to push operations: {}", e)))?;

            tracing::info!("Pushed {} operations to peer {}", ops_to_send.len(), peer);
        }

        Ok(())
    }

    /// Reconcile diverged state between peers
    async fn reconcile_diverged_state<E>(
        &self,
        effects: &E,
        peer: DeviceId,
        local_ops: &[AttestedOp],
        local_digest: &JournalDigest,
        remote_digest: &JournalDigest,
    ) -> SyncResult<AntiEntropyResult>
    where
        E: JournalEffects + NetworkEffects + Send + Sync,
    {
        tracing::warn!(
            "Diverged state detected with peer {} (local: {} ops, remote: {} ops)",
            peer,
            local_digest.operation_count,
            remote_digest.operation_count
        );

        // For diverged state, we do a full exchange:
        // 1. Send all our operations to peer
        // 2. Request all operations from peer
        // 3. Let CRDT merge semantics resolve conflicts

        // Push our operations first
        self.push_operations_to_peer(effects, peer, local_ops, local_digest, remote_digest)
            .await?;

        // Then pull their operations
        let pull_result = self
            .pull_operations_from_peer(effects, peer, local_digest, remote_digest)
            .await?;

        Ok(AntiEntropyResult {
            applied: pull_result.applied,
            duplicates: pull_result.duplicates,
            final_status: Some(DigestStatus::Diverged),
            rounds: 1,
        })
    }

    /// Compute a digest for the given journal state and operation log
    pub fn compute_digest(
        &self,
        journal: &Journal,
        operations: &[AttestedOp],
    ) -> SyncResult<JournalDigest> {
        let fact_hash = hash_serialized(&journal.facts)
            .map_err(|e| sync_session_error(&format!("Failed to hash facts: {}", e)))?;

        let caps_hash = hash_serialized(&journal.caps)
            .map_err(|e| sync_session_error(&format!("Failed to hash caps: {}", e)))?;

        let mut h = hash::hasher();
        let mut last_epoch: Option<u64> = None;

        for op in operations {
            let fp = fingerprint(op)
                .map_err(|e| sync_session_error(&format!("Failed to fingerprint op: {}", e)))?;
            h.update(&fp);

            let epoch = op.op.parent_epoch;
            last_epoch = Some(match last_epoch {
                Some(existing) => existing.max(epoch),
                None => epoch,
            });
        }

        let operation_hash = h.finalize();

        Ok(JournalDigest {
            operation_count: operations.len(),
            last_epoch,
            operation_hash,
            fact_hash,
            caps_hash,
        })
    }

    /// Compare two digests and classify their relationship
    pub fn compare(local: &JournalDigest, remote: &JournalDigest) -> DigestStatus {
        if local.matches(remote) {
            return DigestStatus::Equal;
        }

        match local.operation_count.cmp(&remote.operation_count) {
            std::cmp::Ordering::Less => DigestStatus::LocalBehind,
            std::cmp::Ordering::Greater => DigestStatus::RemoteBehind,
            std::cmp::Ordering::Equal => DigestStatus::Diverged,
        }
    }

    /// Plan the next anti-entropy request based on digest comparison
    pub fn plan_request(
        &self,
        local: &JournalDigest,
        remote: &JournalDigest,
    ) -> Option<AntiEntropyRequest> {
        match Self::compare(local, remote) {
            DigestStatus::LocalBehind => {
                let remaining = remote.operation_count.saturating_sub(local.operation_count);
                Some(AntiEntropyRequest {
                    from_index: local.operation_count,
                    max_ops: remaining.min(self.config.batch_size),
                    missing_operations: Vec::new(),
                })
            }
            DigestStatus::Diverged => Some(AntiEntropyRequest {
                from_index: 0,
                max_ops: self.config.batch_size,
                missing_operations: Vec::new(),
            }),
            DigestStatus::Equal | DigestStatus::RemoteBehind => None,
        }
    }

    /// Merge a batch of operations, deduplicating already-seen entries
    pub fn merge_batch(
        &self,
        local_ops: &mut Vec<AttestedOp>,
        incoming: Vec<AttestedOp>,
    ) -> SyncResult<AntiEntropyResult> {
        if incoming.is_empty() {
            return Ok(AntiEntropyResult::default());
        }

        let mut seen = HashSet::with_capacity(local_ops.len());
        for op in local_ops.iter() {
            let fp = fingerprint(op)
                .map_err(|e| sync_session_error(&format!("Failed to fingerprint: {}", e)))?;
            seen.insert(fp);
        }

        let mut applied = 0;
        let mut duplicates = 0;

        for op in incoming {
            let fp = fingerprint(&op)
                .map_err(|e| sync_session_error(&format!("Failed to fingerprint: {}", e)))?;
            if seen.insert(fp) {
                local_ops.push(op);
                applied += 1;
            } else {
                duplicates += 1;
            }
        }

        Ok(AntiEntropyResult {
            applied,
            duplicates,
            final_status: None,
            rounds: 1,
        })
    }
}

impl Default for AntiEntropyProtocol {
    fn default() -> Self {
        Self::new(AntiEntropyConfig::default())
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

fn hash_serialized<T: Serialize>(value: &T) -> AuraResult<[u8; 32]> {
    let bytes =
        bincode::serialize(value).map_err(|err| AuraError::serialization(err.to_string()))?;
    Ok(hash::hash(&bytes))
}

fn fingerprint(op: &AttestedOp) -> AuraResult<OperationFingerprint> {
    hash_serialized(op)
}

// =============================================================================
// Convenience Functions
// =============================================================================

/// Build reconciliation request by comparing local and peer digests
pub fn build_reconciliation_request(
    local: &JournalDigest,
    peer: &JournalDigest,
) -> SyncResult<AntiEntropyRequest> {
    let protocol = AntiEntropyProtocol::default();
    match protocol.plan_request(local, peer) {
        Some(request) => Ok(request),
        None => {
            // No sync needed - create empty request
            Ok(AntiEntropyRequest {
                from_index: 0,
                max_ops: 0,
                missing_operations: Vec::new(),
            })
        }
    }
}

/// Compute digest from journal state and operations
pub fn compute_digest(journal: &Journal, operations: &[AttestedOp]) -> SyncResult<JournalDigest> {
    let protocol = AntiEntropyProtocol::default();
    protocol.compute_digest(journal, operations)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{journal::FactValue, TreeOp, TreeOpKind};

    fn sample_journal() -> Journal {
        let mut journal = Journal::new();
        journal.facts.insert("counter", FactValue::Number(1));
        journal.caps.add_permission("sync");
        journal
    }

    fn sample_op(epoch: u64) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_epoch: epoch,
                parent_commitment: [0u8; 32],
                op: TreeOpKind::RotateEpoch { affected: vec![] },
                version: 1,
            },
            agg_sig: vec![],
            signer_count: 1,
        }
    }

    #[test]
    fn test_digest_computation() {
        let protocol = AntiEntropyProtocol::default();
        let journal = sample_journal();
        let ops = vec![sample_op(1), sample_op(2)];

        let digest = protocol.compute_digest(&journal, &ops).unwrap();

        assert_eq!(digest.operation_count, 2);
        assert_eq!(digest.last_epoch, Some(2));
    }

    #[test]
    fn test_digest_comparison_equal() {
        let protocol = AntiEntropyProtocol::default();
        let journal = sample_journal();
        let ops = vec![sample_op(1)];

        let digest1 = protocol.compute_digest(&journal, &ops).unwrap();
        let digest2 = protocol.compute_digest(&journal, &ops).unwrap();

        assert_eq!(
            AntiEntropyProtocol::compare(&digest1, &digest2),
            DigestStatus::Equal
        );
    }

    #[test]
    fn test_digest_comparison_local_behind() {
        let protocol = AntiEntropyProtocol::default();
        let journal = sample_journal();

        let ops1 = vec![sample_op(1)];
        let ops2 = vec![sample_op(1), sample_op(2)];

        let digest1 = protocol.compute_digest(&journal, &ops1).unwrap();
        let digest2 = protocol.compute_digest(&journal, &ops2).unwrap();

        assert_eq!(
            AntiEntropyProtocol::compare(&digest1, &digest2),
            DigestStatus::LocalBehind
        );
    }

    #[test]
    fn test_plan_request_local_behind() {
        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig {
            batch_size: 10,
            ..Default::default()
        });

        let journal = sample_journal();
        let ops1 = vec![sample_op(1)];
        let ops2 = vec![sample_op(1), sample_op(2), sample_op(3)];

        let digest1 = protocol.compute_digest(&journal, &ops1).unwrap();
        let digest2 = protocol.compute_digest(&journal, &ops2).unwrap();

        let request = protocol.plan_request(&digest1, &digest2).unwrap();

        assert_eq!(request.from_index, 1);
        assert_eq!(request.max_ops, 2);
    }

    #[test]
    fn test_merge_batch() {
        let protocol = AntiEntropyProtocol::default();
        let mut local_ops = vec![sample_op(1)];
        let incoming = vec![sample_op(1), sample_op(2), sample_op(3)];

        let result = protocol.merge_batch(&mut local_ops, incoming).unwrap();

        assert_eq!(result.applied, 2);
        assert_eq!(result.duplicates, 1);
        assert_eq!(local_ops.len(), 3);
    }
}
