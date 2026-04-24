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
//! ```rust,ignore
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

use crate::capabilities::SyncCapability;
use crate::core::{
    binary_serialize, exchange_json_with_peer, json_serialize, send_bytes_to_peer,
    sync_biscuit_guard_error, sync_session_error, SyncResult,
};
use crate::infrastructure::RetryPolicy;
use crate::protocols::journal_apply::JournalApplyService;
use aura_authorization::{BiscuitTokenManager, VerifiedBiscuitToken};
use aura_core::effects::{JournalEffects, NetworkEffects, PhysicalTimeEffects};
use aura_core::types::scope::ResourceScope;
use aura_core::types::Epoch;
use aura_core::{
    hash, AttestedOp, AuraError, AuraResult, ContextId, DeviceId, FlowBudget, FlowCost, Hash32,
    Journal,
};
use aura_guards::{
    BiscuitGuardEvaluator, DecodedIngress, GuardContextProvider, GuardError, IngressSource,
    IngressVerificationError, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata, REQUIRED_INGRESS_VERIFICATION_CHECKS,
};
use aura_journal::commitment_tree::apply_verified_sync;
use aura_protocol::effects::TreeEffects;

const ANTI_ENTROPY_OPERATION_ID: &str = "anti_entropy";
const ANTI_ENTROPY_AUTHZ_OPERATION_ID: &str = "anti_entropy.authorize";
const ANTI_ENTROPY_PROGRESS_OPERATION_ID: &str = "anti_entropy.progress";
const ANTI_ENTROPY_SCHEMA_VERSION: u16 = 1;

// =============================================================================
// Types
// =============================================================================

/// Unique fingerprint for an attested operation (cryptographic hash)
pub type OperationFingerprint = [u8; 32];

fn peer_sync_context(peer: DeviceId) -> ContextId {
    let entropy = peer
        .to_bytes()
        .unwrap_or_else(|_| hash::hash(peer.to_string().as_bytes()));
    ContextId::new_from_entropy(entropy)
}

/// Summary of a journal snapshot used for anti-entropy comparisons
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalDigest {
    /// Number of attested operations known locally
    pub operation_count: u64,

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
    pub from_index: u64,

    /// Maximum operations to send in this batch
    pub max_ops: u32,

    /// Specific operation fingerprints that are missing (for targeted requests)
    pub missing_operations: Vec<OperationFingerprint>,
}

/// Remote anti-entropy operations that have passed batch-level verification and
/// are eligible for the canonical apply boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedRemoteOpsBatch {
    ops: Vec<AttestedOp>,
}

impl VerifiedRemoteOpsBatch {
    #[must_use]
    pub fn new(ops: Vec<AttestedOp>) -> Self {
        Self { ops }
    }

    #[must_use]
    pub fn ops(&self) -> &[AttestedOp] {
        &self.ops
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// Result of merging operations into a journal
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeResult {
    /// Number of operations that were successfully merged
    pub merged_operations: u64,
    /// Number of operations that were duplicates
    pub duplicate_operations: u64,
    /// Number of operations that failed to merge
    pub failed_operations: u64,
    /// Updated journal after merge
    pub updated_journal: Journal,
}

/// Result of an anti-entropy synchronization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AntiEntropyResult {
    /// Number of operations that were newly applied
    pub applied: u64,

    /// Number of duplicates that were ignored
    pub duplicates: u64,

    /// Operations that were newly applied (for journal conversion/persistence)
    pub applied_ops: Vec<AttestedOp>,

    /// Final digest status after synchronization
    pub final_status: Option<DigestStatus>,

    /// Number of synchronization rounds performed
    pub rounds: u32,
}

// =============================================================================
// Progress Tracking Types
// =============================================================================

/// Progress event emitted during anti-entropy synchronization
///
/// These events allow UI to track sync status in real-time for optimistic updates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncProgressEvent {
    /// Sync started with a peer
    Started {
        /// Peer being synchronized with
        peer_id: uuid::Uuid,
        /// Total peers to sync with in this batch
        total_peers: u32,
    },

    /// Digest exchange completed
    DigestExchanged {
        /// Peer
        peer_id: uuid::Uuid,
        /// Result of digest comparison
        status: DigestStatus,
        /// Operations we have locally
        local_ops: u64,
        /// Operations peer has
        remote_ops: u64,
    },

    /// Operations being pulled from peer
    Pulling {
        /// Peer
        peer_id: uuid::Uuid,
        /// Operations pulled so far
        pulled: u64,
        /// Total operations to pull
        total: u64,
    },

    /// Operations being pushed to peer
    Pushing {
        /// Peer
        peer_id: uuid::Uuid,
        /// Operations pushed so far
        pushed: u64,
        /// Total operations to push
        total: u64,
    },

    /// Single peer sync completed
    PeerCompleted {
        /// Peer that was synced
        peer_id: uuid::Uuid,
        /// Operations applied from this peer
        applied: u64,
        /// Whether sync was successful
        success: bool,
        /// Peers remaining
        peers_remaining: u32,
    },

    /// All peers synced
    AllCompleted {
        /// Total operations applied across all peers
        total_applied: u64,
        /// Total duplicates across all peers
        total_duplicates: u64,
        /// Peers that synced successfully
        successful_peers: u32,
        /// Peers that failed
        failed_peers: u32,
    },

    /// Sync failed for a peer
    PeerFailed {
        /// Peer that failed
        peer_id: uuid::Uuid,
        /// Error message
        error: String,
        /// Will retry
        will_retry: bool,
        /// Retry attempt number
        retry_attempt: u32,
    },
}

/// Callback trait for receiving sync progress events
///
/// Implement this trait to receive real-time progress updates during sync.
pub trait SyncProgressCallback: Send + Sync {
    /// Called when a progress event occurs
    fn on_progress(&self, event: SyncProgressEvent);
}

/// A no-op progress callback for when progress tracking isn't needed
pub struct NoOpProgressCallback;

impl SyncProgressCallback for NoOpProgressCallback {
    fn on_progress(&self, _event: SyncProgressEvent) {
        // Intentionally empty
    }
}

/// A progress callback that logs events
pub struct LoggingProgressCallback;

impl SyncProgressCallback for LoggingProgressCallback {
    fn on_progress(&self, event: SyncProgressEvent) {
        match &event {
            SyncProgressEvent::Started {
                peer_id,
                total_peers,
            } => {
                tracing::info!(
                    operation_id = ANTI_ENTROPY_PROGRESS_OPERATION_ID,
                    peer_id = %peer_id,
                    total_peers,
                    "Sync started with peer"
                );
            }
            SyncProgressEvent::DigestExchanged {
                peer_id,
                status,
                local_ops,
                remote_ops,
            } => {
                tracing::debug!(
                    operation_id = ANTI_ENTROPY_PROGRESS_OPERATION_ID,
                    peer_id = %peer_id,
                    digest_status = ?status,
                    local_ops,
                    remote_ops,
                    "Digest exchanged with peer"
                );
            }
            SyncProgressEvent::Pulling {
                peer_id,
                pulled,
                total,
            } => {
                tracing::debug!(
                    operation_id = ANTI_ENTROPY_PROGRESS_OPERATION_ID,
                    peer_id = %peer_id,
                    pulled,
                    total,
                    "Pulling operations from peer"
                );
            }
            SyncProgressEvent::Pushing {
                peer_id,
                pushed,
                total,
            } => {
                tracing::debug!(
                    operation_id = ANTI_ENTROPY_PROGRESS_OPERATION_ID,
                    peer_id = %peer_id,
                    pushed,
                    total,
                    "Pushing operations to peer"
                );
            }
            SyncProgressEvent::PeerCompleted {
                peer_id,
                applied,
                success,
                peers_remaining,
            } => {
                tracing::info!(
                    operation_id = ANTI_ENTROPY_PROGRESS_OPERATION_ID,
                    peer_id = %peer_id,
                    applied,
                    success,
                    peers_remaining,
                    "Peer sync completed"
                );
            }
            SyncProgressEvent::AllCompleted {
                total_applied,
                total_duplicates,
                successful_peers,
                failed_peers,
            } => {
                tracing::info!(
                    operation_id = ANTI_ENTROPY_PROGRESS_OPERATION_ID,
                    total_applied,
                    total_duplicates,
                    successful_peers,
                    failed_peers,
                    "All peer syncs completed"
                );
            }
            SyncProgressEvent::PeerFailed {
                peer_id,
                error,
                will_retry,
                retry_attempt,
            } => {
                tracing::warn!(
                    operation_id = ANTI_ENTROPY_PROGRESS_OPERATION_ID,
                    peer_id = %peer_id,
                    error,
                    will_retry,
                    retry_attempt,
                    "Peer sync failed"
                );
            }
        }
    }
}

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for anti-entropy protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyConfig {
    /// Batch size for operation transfer
    pub batch_size: u32,

    /// Maximum synchronization rounds before giving up
    pub max_rounds: u32,

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
#[derive(Clone)]
pub struct AntiEntropyProtocol {
    config: AntiEntropyConfig,
    /// Optional Biscuit token manager for authorization
    token_manager: Option<BiscuitTokenManager>,
    /// Optional Biscuit guard evaluator for permission checks
    guard_evaluator: Option<std::sync::Arc<BiscuitGuardEvaluator>>,
}

impl AntiEntropyProtocol {
    fn status_result(final_status: DigestStatus) -> AntiEntropyResult {
        AntiEntropyResult {
            final_status: Some(final_status),
            rounds: 1,
            ..AntiEntropyResult::default()
        }
    }

    fn with_status(final_status: DigestStatus, result: AntiEntropyResult) -> AntiEntropyResult {
        AntiEntropyResult {
            final_status: Some(final_status),
            rounds: 1,
            ..result
        }
    }

    fn operations_to_push<'a>(
        &self,
        local_ops: &'a [AttestedOp],
        local_digest: &JournalDigest,
        remote_digest: &JournalDigest,
    ) -> &'a [AttestedOp] {
        let missing_count = local_digest
            .operation_count
            .saturating_sub(remote_digest.operation_count);
        if missing_count == 0 {
            return &[];
        }

        let start_index = remote_digest.operation_count as usize;
        let batch = (self.config.batch_size as u64).min(missing_count) as usize;
        let end_index = (start_index + batch).min(local_ops.len());
        &local_ops[start_index..end_index]
    }

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
            guard_evaluator: Some(std::sync::Arc::new(guard_evaluator)),
        }
    }

    /// Check if the current token authorizes sync operations with a peer
    async fn check_sync_authorization<E>(&self, effects: &E, peer: DeviceId) -> SyncResult<()>
    where
        E: JournalEffects + NetworkEffects + GuardContextProvider + PhysicalTimeEffects,
    {
        if let (Some(ref token_manager), Some(ref evaluator)) =
            (&self.token_manager, &self.guard_evaluator)
        {
            let token = VerifiedBiscuitToken::from_token(
                token_manager.current_token(),
                evaluator.root_public_key(),
            )
            .map_err(|error| {
                AuraError::permission_denied(format!(
                    "sync Biscuit token verification failed: {error}"
                ))
            })?;
            let resource = ResourceScope::Authority {
                authority_id: effects.authority_id(),
                operation: aura_core::types::scope::AuthorityOp::UpdateTree, // Sync requires authority access
            };

            let mut flow_budget = FlowBudget::new(1000, Epoch::new(0)); // Standard sync budget

            let capability = SyncCapability::RequestDigest.as_name();
            let current_time_seconds = effects
                .physical_time()
                .await
                .map_err(|error| {
                    AuraError::internal(format!("sync auth time unavailable: {error}"))
                })?
                .ts_ms
                / 1000;
            match evaluator.evaluate_guard(
                &token,
                &capability,
                &resource,
                FlowCost::new(100),
                &mut flow_budget,
                current_time_seconds,
            ) {
                Ok(guard_result) if guard_result.authorized => {
                    tracing::debug!(
                        operation_id = ANTI_ENTROPY_AUTHZ_OPERATION_ID,
                        authority_id = %effects.authority_id(),
                        peer_id = %peer,
                        capability = capability.as_str(),
                        "Sync authorization granted for peer"
                    );
                    Ok(())
                }
                Ok(_) => {
                    tracing::warn!(
                        operation_id = ANTI_ENTROPY_AUTHZ_OPERATION_ID,
                        authority_id = %effects.authority_id(),
                        peer_id = %peer,
                        capability = capability.as_str(),
                        "Sync authorization denied for peer"
                    );
                    Err(sync_biscuit_guard_error(
                        capability.as_str(),
                        peer,
                        GuardError::MissingCapability {
                            capability: capability.to_string(),
                        },
                    ))
                }
                Err(e) => {
                    tracing::error!(
                        operation_id = ANTI_ENTROPY_AUTHZ_OPERATION_ID,
                        authority_id = %effects.authority_id(),
                        peer_id = %peer,
                        capability = capability.as_str(),
                        error = ?e,
                        "Sync authorization error for peer"
                    );
                    Err(sync_biscuit_guard_error(capability.as_str(), peer, e))
                }
            }
        } else {
            // No Biscuit authorization configured - deny access (authorization is required)
            tracing::error!(
                operation_id = ANTI_ENTROPY_AUTHZ_OPERATION_ID,
                authority_id = %effects.authority_id(),
                peer_id = %peer,
                "Sync denied: no authorization configured for peer"
            );
            Err(AuraError::permission_denied(format!(
                "Authorization required for sync with peer {peer}. Configure Biscuit token manager and guard evaluator."
            )))
        }
    }

    /// Execute anti-entropy synchronization with a peer
    ///
    /// This is the main entry point for the protocol. It performs digest
    /// exchange, reconciliation planning, and operation transfer.
    ///
    /// # Authorization
    /// - Checks Biscuit token permissions for `sync:request_digest`
    /// - Validates against peer-specific resource scope
    ///
    /// # Integration Points
    /// - Uses `JournalEffects` to access local journal state
    /// - Uses `NetworkEffects` to communicate with peer
    /// - Uses `RetryPolicy` from infrastructure for resilience
    pub async fn execute<E>(&self, effects: &E, peer: DeviceId) -> SyncResult<AntiEntropyResult>
    where
        E: JournalEffects
            + NetworkEffects
            + Send
            + Sync
            + PhysicalTimeEffects
            + GuardContextProvider
            + TreeEffects,
    {
        let authority_id = effects.authority_id();
        // Check authorization before starting sync
        self.check_sync_authorization(effects, peer).await?;
        tracing::info!(
            operation_id = ANTI_ENTROPY_OPERATION_ID,
            authority_id = %authority_id,
            peer_id = %peer,
            "Starting anti-entropy sync with peer"
        );

        let mut result = AntiEntropyResult::default();

        loop {
            let round_result = if self.config.retry_enabled {
                let retry_policy = self
                    .config
                    .retry_policy
                    .clone()
                    .with_max_attempts(self.config.retry_policy.max_attempts.saturating_sub(1));
                retry_policy
                    .execute_with_effects(effects, || self.execute_sync_round(effects, peer))
                    .await?
            } else {
                self.execute_sync_round(effects, peer).await?
            };

            result.applied += round_result.applied;
            result.duplicates += round_result.duplicates;
            result.rounds += 1;
            result.final_status = round_result.final_status;

            if matches!(round_result.final_status, Some(DigestStatus::Equal)) {
                tracing::info!(
                    operation_id = ANTI_ENTROPY_OPERATION_ID,
                    authority_id = %authority_id,
                    peer_id = %peer,
                    rounds = result.rounds,
                    "Sync completed successfully with peer"
                );
                break;
            }

            if result.rounds >= self.config.max_rounds {
                tracing::warn!(
                    operation_id = ANTI_ENTROPY_OPERATION_ID,
                    authority_id = %authority_id,
                    peer_id = %peer,
                    max_rounds = self.config.max_rounds,
                    "Reached max rounds syncing with peer"
                );
                break;
            }
        }

        tracing::info!(
            operation_id = ANTI_ENTROPY_OPERATION_ID,
            authority_id = %authority_id,
            peer_id = %peer,
            applied = result.applied,
            duplicates = result.duplicates,
            rounds = result.rounds,
            final_status = ?result.final_status,
            "Anti-entropy sync completed"
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
        E: JournalEffects + NetworkEffects + TreeEffects + Send + Sync,
    {
        // Step 1: Get local journal state and operations
        let local_journal = effects
            .get_journal()
            .await
            .map_err(|e| sync_session_error(format!("Failed to get local journal: {e}")))?;

        // Currently uses empty operations list; transport-level sync fills in ops
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
            operation_id = ANTI_ENTROPY_OPERATION_ID,
            peer_id = %peer,
            digest_status = ?digest_status,
            local_ops = local_digest.operation_count,
            remote_ops = remote_digest.operation_count,
            "Digest comparison with peer"
        );

        // Step 5: Plan and execute reconciliation if needed
        match digest_status {
            DigestStatus::Equal => {
                // Already synchronized
                Ok(Self::status_result(DigestStatus::Equal))
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
                Ok(Self::status_result(DigestStatus::RemoteBehind))
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
        // Send digest to peer and wait for response
        tracing::debug!(
            operation_id = ANTI_ENTROPY_OPERATION_ID,
            peer_id = %peer,
            payload_bytes = json_serialize("digest", "digest", local_digest)?.len(),
            "Sending digest to peer"
        );

        // Apply timeout for digest exchange
        let exchange_future = async {
            let remote_digest: JournalDigest = exchange_json_with_peer(
                effects,
                peer.0,
                &peer,
                "digest",
                "digest",
                local_digest,
                "digest",
                "digest",
            )
            .await?;
            tracing::debug!(
                operation_id = ANTI_ENTROPY_OPERATION_ID,
                peer_id = %peer,
                remote_ops = remote_digest.operation_count,
                "Received digest from peer"
            );

            Ok(remote_digest)
        };

        // Execute without runtime-specific timeout; callers should enforce via PhysicalTimeEffects if needed.
        exchange_future.await
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
        E: JournalEffects + NetworkEffects + TreeEffects + Send + Sync,
    {
        // Plan the request
        let request = self
            .plan_request(local_digest, remote_digest)
            .ok_or_else(|| sync_session_error("No operations needed despite LocalBehind status"))?;

        tracing::debug!(
            operation_id = ANTI_ENTROPY_OPERATION_ID,
            peer_id = %peer,
            max_ops = request.max_ops,
            from_index = request.from_index,
            "Requesting operations from peer"
        );

        let pull_future = async {
            let remote_ops: Vec<AttestedOp> = exchange_json_with_peer(
                effects,
                peer.0,
                &peer,
                "request",
                "operation request",
                &request,
                "operations",
                "operations",
            )
            .await?;

            let remote_ops = crate::protocols::ingress::verified_device_payload(
                peer,
                peer_sync_context(peer),
                ANTI_ENTROPY_SCHEMA_VERSION,
                remote_ops,
            )?;
            let remote_ops = self
                .verify_remote_operation_batch(effects, peer, remote_ops)
                .await?;
            let remote_ops =
                JournalApplyService::new().accept_verified_relational_facts(remote_ops)?;

            tracing::debug!(
                operation_id = ANTI_ENTROPY_OPERATION_ID,
                peer_id = %peer,
                remote_ops = remote_ops.payload().len(),
                "Received operations from peer"
            );

            // Merge operations into local state
            let mut local_ops = vec![]; // In full implementation, get from journal
            let merge_chunk_size = self.merge_chunk_size();
            let merge_batch_count =
                Self::merge_batch_count(remote_ops.payload().len(), merge_chunk_size);
            if remote_ops.payload().len() > request.max_ops as usize {
                tracing::warn!(
                    operation_id = ANTI_ENTROPY_OPERATION_ID,
                    peer_id = %peer,
                    requested_max_ops = request.max_ops,
                    received_ops = remote_ops.payload().len(),
                    merge_chunk_size,
                    merge_batch_count,
                    "Peer returned more operations than requested; applying in bounded chunks"
                );
            } else {
                tracing::debug!(
                    operation_id = ANTI_ENTROPY_OPERATION_ID,
                    peer_id = %peer,
                    received_ops = remote_ops.payload().len(),
                    merge_chunk_size,
                    merge_batch_count,
                    "Applying received operations in bounded merge batches"
                );
            }

            let mut total_result = AntiEntropyResult::default();
            for (chunk_index, chunk) in remote_ops
                .payload()
                .ops()
                .chunks(merge_chunk_size)
                .enumerate()
            {
                let verified_chunk =
                    verified_remote_ops_batch(remote_ops.evidence().metadata(), chunk.to_vec())?;
                let merge_result = self.merge_batch(&mut local_ops, verified_chunk)?;

                tracing::debug!(
                    operation_id = ANTI_ENTROPY_PROGRESS_OPERATION_ID,
                    peer_id = %peer,
                    merge_batch_index = chunk_index + 1,
                    merge_batch_count,
                    merge_batch_size = chunk.len(),
                    applied = merge_result.applied,
                    duplicates = merge_result.duplicates,
                    "Processed anti-entropy merge batch"
                );

                if merge_result.applied > 0 {
                    tracing::info!(
                        operation_id = ANTI_ENTROPY_OPERATION_ID,
                        peer_id = %peer,
                        merge_batch_index = chunk_index + 1,
                        merge_batch_count,
                        merge_batch_size = chunk.len(),
                        applied = merge_result.applied,
                        "Applied new operations from peer"
                    );

                    self.persist_applied_operations_chunk(effects, peer, &merge_result.applied_ops)
                        .await?;
                }

                total_result.applied += merge_result.applied;
                total_result.duplicates += merge_result.duplicates;
            }

            total_result.final_status = Some(DigestStatus::LocalBehind);
            total_result.rounds = 1;
            Ok(total_result)
        };

        // Execute without runtime-specific timeout; callers should enforce via PhysicalTimeEffects if needed.
        pull_future.await
    }

    async fn verify_remote_operation_batch<E>(
        &self,
        effects: &E,
        peer: DeviceId,
        incoming: VerifiedIngress<Vec<AttestedOp>>,
    ) -> SyncResult<VerifiedIngress<VerifiedRemoteOpsBatch>>
    where
        E: JournalEffects + TreeEffects + Send + Sync,
    {
        let (incoming, evidence) = incoming.into_parts();
        let metadata = evidence.metadata().clone();
        let expected_context = peer_sync_context(peer);

        if metadata.source() != IngressSource::Device(peer) {
            return Err(crate::core::errors::sync_protocol_with_peer(
                "anti_entropy",
                format!(
                    "verified ingress source {:?} does not match authenticated peer {}",
                    metadata.source(),
                    peer
                ),
                peer,
            ));
        }

        if metadata.context_id() != expected_context {
            return Err(crate::core::errors::sync_protocol_with_peer(
                "anti_entropy",
                format!(
                    "verified ingress context {} does not match expected sync namespace {}",
                    metadata.context_id(),
                    expected_context
                ),
                peer,
            ));
        }

        if metadata.schema_version() != ANTI_ENTROPY_SCHEMA_VERSION {
            return Err(crate::core::errors::sync_protocol_with_peer(
                "anti_entropy",
                format!(
                    "unsupported anti-entropy schema version {}; expected {}",
                    metadata.schema_version(),
                    ANTI_ENTROPY_SCHEMA_VERSION
                ),
                peer,
            ));
        }

        effects.get_journal().await.map_err(|error| {
            crate::core::errors::sync_protocol_with_peer(
                "anti_entropy",
                format!("journal load failed before remote batch verification: {error}"),
                peer,
            )
        })?;

        let mut shadow_state = effects.get_current_state().await.map_err(|error| {
            crate::core::errors::sync_protocol_with_peer(
                "anti_entropy",
                format!("tree state load failed before remote batch verification: {error}"),
                peer,
            )
        })?;

        let mut seen_fingerprints = HashSet::with_capacity(incoming.len());
        for (index, op) in incoming.iter().enumerate() {
            let fingerprint = fingerprint(op).map_err(|error| {
                crate::core::errors::sync_protocol_with_peer(
                    "anti_entropy",
                    format!("fingerprint remote operation {index} failed: {error}"),
                    peer,
                )
            })?;

            if !seen_fingerprints.insert(fingerprint) {
                return Err(crate::core::errors::sync_protocol_with_peer(
                    "anti_entropy",
                    format!(
                        "remote operation batch replayed fingerprint {} at index {}",
                        hex::encode(fingerprint),
                        index
                    ),
                    peer,
                ));
            }

            let signature_valid = effects
                .verify_aggregate_sig(op, &shadow_state)
                .await
                .map_err(|error| {
                    crate::core::errors::sync_protocol_with_peer(
                        "anti_entropy",
                        format!("remote operation {index} signature verification failed: {error}"),
                        peer,
                    )
                })?;
            if !signature_valid {
                return Err(crate::core::errors::sync_protocol_with_peer(
                    "anti_entropy",
                    format!("remote operation {index} failed signature verification"),
                    peer,
                ));
            }

            apply_verified_sync(&mut shadow_state, op).map_err(|error| {
                crate::core::errors::sync_protocol_with_peer(
                    "anti_entropy",
                    format!(
                        "remote operation {index} failed causal or parent verification: {error}"
                    ),
                    peer,
                )
            })?;
        }

        verified_remote_ops_batch(&metadata, incoming)
    }

    async fn verify_and_apply_remote_operation<E>(
        &self,
        effects: &E,
        peer: DeviceId,
        op: &AttestedOp,
    ) -> SyncResult<bool>
    where
        E: TreeEffects + Send + Sync,
    {
        let current_state = effects.get_current_state().await.map_err(|error| {
            crate::core::errors::sync_protocol_with_peer(
                "anti_entropy",
                format!("Tree state load failed before remote op verification: {error}"),
                peer,
            )
        })?;
        let current_commitment = Hash32(current_state.current_commitment());

        if op.op.parent_epoch != current_state.current_epoch() {
            return Err(crate::core::errors::sync_protocol_with_peer(
                "anti_entropy",
                format!(
                    "Remote operation epoch {} does not match current epoch {}",
                    op.op.parent_epoch,
                    current_state.current_epoch()
                ),
                peer,
            ));
        }
        if Hash32(op.op.parent_commitment) != current_commitment {
            return Err(crate::core::errors::sync_protocol_with_peer(
                "anti_entropy",
                format!(
                    "Remote operation parent commitment {:?} does not match current commitment {:?}",
                    Hash32(op.op.parent_commitment),
                    current_commitment
                ),
                peer,
            ));
        }

        let signature_valid = effects
            .verify_aggregate_sig(op, &current_state)
            .await
            .map_err(|error| {
                crate::core::errors::sync_protocol_with_peer(
                    "anti_entropy",
                    format!("Remote operation signature verification failed: {error}"),
                    peer,
                )
            })?;
        if !signature_valid {
            return Err(crate::core::errors::sync_protocol_with_peer(
                "anti_entropy",
                "Remote operation aggregate signature is invalid".to_string(),
                peer,
            ));
        }

        let updated_commitment = effects
            .apply_attested_op(op.clone())
            .await
            .map_err(|error| {
                crate::core::errors::sync_protocol_with_peer(
                    "anti_entropy",
                    format!("Canonical remote op application failed: {error}"),
                    peer,
                )
            })?;

        Ok(updated_commitment != current_commitment)
    }

    async fn persist_applied_operations_chunk<E>(
        &self,
        effects: &E,
        peer: DeviceId,
        applied_ops: &[AttestedOp],
    ) -> SyncResult<()>
    where
        E: TreeEffects + Send + Sync,
    {
        let mut newly_applied = 0usize;
        let mut duplicates = 0usize;
        for op in applied_ops {
            match self
                .verify_and_apply_remote_operation(effects, peer, op)
                .await
            {
                Ok(true) => newly_applied += 1,
                Ok(false) => duplicates += 1,
                Err(error) => {
                    tracing::error!(
                        operation_id = ANTI_ENTROPY_OPERATION_ID,
                        peer_id = %peer,
                        error = %error,
                        "Failed to verify or apply remote operation through canonical tree path"
                    );
                    return Err(error);
                }
            }
        }

        tracing::debug!(
            operation_id = ANTI_ENTROPY_OPERATION_ID,
            peer_id = %peer,
            applied_ops = applied_ops.len(),
            newly_applied,
            duplicates,
            "Successfully applied bounded anti-entropy merge batch through canonical tree path"
        );

        Ok(())
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
        let ops_to_send = self.operations_to_push(local_ops, local_digest, remote_digest);

        tracing::debug!(
            operation_id = ANTI_ENTROPY_OPERATION_ID,
            peer_id = %peer,
            operation_count = ops_to_send.len(),
            "Pushing operations to peer"
        );

        if !ops_to_send.is_empty() {
            // Serialize operations
            let ops_data = json_serialize("operations", "operations", ops_to_send)?;
            send_bytes_to_peer(effects, peer.0, &peer, "operations", ops_data).await?;

            tracing::info!(
                operation_id = ANTI_ENTROPY_OPERATION_ID,
                peer_id = %peer,
                operation_count = ops_to_send.len(),
                "Pushed operations to peer"
            );
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
        E: JournalEffects + NetworkEffects + TreeEffects + Send + Sync,
    {
        tracing::warn!(
            operation_id = ANTI_ENTROPY_OPERATION_ID,
            peer_id = %peer,
            local_ops = local_digest.operation_count,
            remote_ops = remote_digest.operation_count,
            "Diverged state detected with peer"
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

        Ok(Self::with_status(DigestStatus::Diverged, pull_result))
    }

    /// Compute a digest for the given journal state and operation log
    pub fn compute_digest(
        &self,
        journal: &Journal,
        operations: &[AttestedOp],
    ) -> SyncResult<JournalDigest> {
        let fact_hash = hash_serialized(&journal.facts)
            .map_err(|e| sync_session_error(format!("Failed to hash facts: {e}")))?;

        let caps_hash = hash_serialized(&journal.caps)
            .map_err(|e| sync_session_error(format!("Failed to hash caps: {e}")))?;

        let mut h = hash::hasher();
        let mut last_epoch: Option<u64> = None;

        for op in operations {
            let fp = fingerprint(op)
                .map_err(|e| sync_session_error(format!("Failed to fingerprint op: {e}")))?;
            h.update(&fp);

            let epoch = u64::from(op.op.parent_epoch);
            last_epoch = Some(match last_epoch {
                Some(existing) => existing.max(epoch),
                None => epoch,
            });
        }

        let operation_hash = h.finalize();

        Ok(JournalDigest {
            operation_count: operations.len() as u64,
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
                    max_ops: remaining.min(self.config.batch_size as u64) as u32,
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
        incoming: VerifiedIngress<VerifiedRemoteOpsBatch>,
    ) -> SyncResult<AntiEntropyResult> {
        let (incoming, _) = incoming.into_parts();
        if incoming.ops.is_empty() {
            return Ok(AntiEntropyResult::default());
        }

        let mut seen = HashSet::with_capacity(local_ops.len());
        for op in local_ops.iter() {
            let fp = fingerprint(op)
                .map_err(|e| sync_session_error(format!("Failed to fingerprint: {e}")))?;
            seen.insert(fp);
        }

        let mut applied = 0;
        let mut duplicates = 0;
        let mut applied_ops = Vec::new();

        for op in incoming.ops {
            let fp = fingerprint(&op)
                .map_err(|e| sync_session_error(format!("Failed to fingerprint: {e}")))?;
            if seen.insert(fp) {
                applied_ops.push(op.clone());
                local_ops.push(op);
                applied += 1;
            } else {
                duplicates += 1;
            }
        }

        Ok(AntiEntropyResult {
            applied,
            duplicates,
            applied_ops,
            final_status: None,
            rounds: 1,
        })
    }

    fn merge_chunk_size(&self) -> usize {
        self.config.batch_size.max(1) as usize
    }

    fn merge_batch_count(incoming_len: usize, merge_chunk_size: usize) -> usize {
        if incoming_len == 0 {
            0
        } else {
            incoming_len.div_ceil(merge_chunk_size)
        }
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
    let bytes = binary_serialize("hash_input", "hash input", value)?;
    Ok(hash::hash(&bytes))
}

fn fingerprint(op: &AttestedOp) -> AuraResult<OperationFingerprint> {
    hash_serialized(op)
}

fn verified_remote_ops_batch(
    metadata: &VerifiedIngressMetadata,
    ops: Vec<AttestedOp>,
) -> SyncResult<VerifiedIngress<VerifiedRemoteOpsBatch>> {
    let batch = VerifiedRemoteOpsBatch::new(ops);
    let payload_hash = Hash32::from_value(&batch).map_err(|error| {
        sync_session_error(format!("hash verified anti-entropy batch payload: {error}"))
    })?;
    let metadata = VerifiedIngressMetadata::new(
        metadata.source(),
        metadata.context_id(),
        metadata.session_id(),
        payload_hash,
        metadata.schema_version(),
    );
    let evidence =
        IngressVerificationEvidence::new(metadata.clone(), REQUIRED_INGRESS_VERIFICATION_CHECKS)
            .map_err(|error: IngressVerificationError| {
                sync_session_error(format!(
                    "build verified anti-entropy batch ingress evidence: {error}"
                ))
            })?;

    DecodedIngress::new(batch, metadata)
        .verify(evidence)
        .map_err(|error| {
            sync_session_error(format!(
                "promote verified anti-entropy batch ingress: {error}"
            ))
        })
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
    use async_trait::async_trait;
    use aura_core::{AuthorityId, Epoch, FlowBudget, FlowCost, TreeOp, TreeOpKind};
    use aura_journal::commitment_tree::TreeState;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct VerificationTestEffects {
        state: Arc<Mutex<TreeState>>,
        journal_result: Result<Journal, AuraError>,
        applied: Arc<Mutex<Vec<AttestedOp>>>,
    }

    impl VerificationTestEffects {
        fn healthy() -> Self {
            Self {
                state: Arc::new(Mutex::new(TreeState::new())),
                journal_result: Ok(Journal::default()),
                applied: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn failing_journal_load(error: AuraError) -> Self {
            Self {
                state: Arc::new(Mutex::new(TreeState::new())),
                journal_result: Err(error),
                applied: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl JournalEffects for VerificationTestEffects {
        async fn merge_facts(
            &self,
            target: Journal,
            _delta: Journal,
        ) -> Result<Journal, AuraError> {
            Ok(target)
        }

        async fn refine_caps(
            &self,
            target: Journal,
            _refinement: Journal,
        ) -> Result<Journal, AuraError> {
            Ok(target)
        }

        async fn get_journal(&self) -> Result<Journal, AuraError> {
            self.journal_result.clone()
        }

        async fn persist_journal(&self, _journal: &Journal) -> Result<(), AuraError> {
            Ok(())
        }

        async fn get_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &AuthorityId,
        ) -> Result<FlowBudget, AuraError> {
            Ok(FlowBudget::new(100, Epoch::initial()))
        }

        async fn update_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &AuthorityId,
            budget: &FlowBudget,
        ) -> Result<FlowBudget, AuraError> {
            Ok(budget.clone())
        }

        async fn charge_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &AuthorityId,
            _cost: FlowCost,
        ) -> Result<FlowBudget, AuraError> {
            Ok(FlowBudget::new(100, Epoch::initial()))
        }
    }

    #[async_trait]
    impl TreeEffects for VerificationTestEffects {
        async fn get_current_state(&self) -> Result<TreeState, AuraError> {
            Ok(self.state.lock().unwrap().clone())
        }

        async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
            Ok(Hash32::new(self.state.lock().unwrap().current_commitment()))
        }

        async fn get_current_epoch(&self) -> Result<Epoch, AuraError> {
            Ok(self.state.lock().unwrap().current_epoch())
        }

        async fn apply_attested_op(&self, op: AttestedOp) -> Result<Hash32, AuraError> {
            let mut state = self.state.lock().unwrap();
            apply_verified_sync(&mut state, &op)
                .map_err(|error| AuraError::invalid(format!("apply attested op: {error}")))?;
            let commitment = Hash32(state.current_commitment());
            self.applied.lock().unwrap().push(op);
            Ok(commitment)
        }

        async fn verify_aggregate_sig(
            &self,
            op: &AttestedOp,
            _state: &TreeState,
        ) -> Result<bool, AuraError> {
            Ok(!op.agg_sig.is_empty())
        }

        async fn add_leaf(
            &self,
            _leaf: aura_core::LeafNode,
            _under: aura_core::NodeIndex,
        ) -> Result<aura_core::TreeOpKind, AuraError> {
            Err(AuraError::internal("test-only add_leaf"))
        }

        async fn remove_leaf(
            &self,
            _leaf_id: aura_core::LeafId,
            _reason: u8,
        ) -> Result<aura_core::TreeOpKind, AuraError> {
            Err(AuraError::internal("test-only remove_leaf"))
        }

        async fn change_policy(
            &self,
            _node: aura_core::NodeIndex,
            _new_policy: aura_core::Policy,
        ) -> Result<aura_core::TreeOpKind, AuraError> {
            Err(AuraError::internal("test-only change_policy"))
        }

        async fn rotate_epoch(
            &self,
            _affected: Vec<aura_core::NodeIndex>,
        ) -> Result<aura_core::TreeOpKind, AuraError> {
            Err(AuraError::internal("test-only rotate_epoch"))
        }

        async fn propose_snapshot(
            &self,
            _cut: aura_protocol::effects::tree::Cut,
        ) -> Result<aura_core::tree::ProposalId, AuraError> {
            Err(AuraError::internal("test-only propose_snapshot"))
        }

        async fn apply_snapshot(
            &self,
            _snapshot: &aura_protocol::effects::tree::Snapshot,
        ) -> Result<(), AuraError> {
            Err(AuraError::internal("test-only apply_snapshot"))
        }
    }

    fn sample_journal() -> Journal {
        // Minimal journal for digest tests; facts/caps remain default
        Journal::default()
    }

    fn sample_op(epoch: u64) -> AttestedOp {
        AttestedOp {
            op: TreeOp {
                parent_epoch: Epoch::new(epoch),
                parent_commitment: [0u8; 32],
                op: TreeOpKind::RotateEpoch { affected: vec![] },
                version: 1,
            },
            agg_sig: vec![],
            signer_count: 1,
        }
    }

    fn valid_verified_batch(peer: DeviceId, count: usize) -> VerifiedIngress<Vec<AttestedOp>> {
        let mut state = TreeState::new();
        let mut ops = Vec::with_capacity(count);
        for ordinal in 0..count {
            let op = AttestedOp {
                op: TreeOp {
                    parent_epoch: state.current_epoch(),
                    parent_commitment: state.current_commitment(),
                    op: TreeOpKind::RotateEpoch {
                        affected: vec![aura_core::NodeIndex(0)],
                    },
                    version: ANTI_ENTROPY_SCHEMA_VERSION,
                },
                agg_sig: vec![1, ordinal as u8],
                signer_count: 1,
            };
            apply_verified_sync(&mut state, &op).expect("test op should reduce");
            ops.push(op);
        }

        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Device(peer),
            peer_sync_context(peer),
            None,
            Hash32::from_value(&ops).expect("hash remote ops"),
            ANTI_ENTROPY_SCHEMA_VERSION,
        );
        let evidence = IngressVerificationEvidence::new(
            metadata.clone(),
            REQUIRED_INGRESS_VERIFICATION_CHECKS,
        )
        .expect("complete ingress evidence");
        DecodedIngress::new(ops, metadata)
            .verify(evidence)
            .expect("verified remote ops")
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
        let peer = DeviceId::new_from_entropy([7u8; 32]);
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Device(peer),
            peer_sync_context(peer),
            None,
            Hash32::from_value(&incoming).expect("hash incoming ops"),
            ANTI_ENTROPY_SCHEMA_VERSION,
        );
        let incoming = verified_remote_ops_batch(&metadata, incoming).unwrap();

        let result = protocol.merge_batch(&mut local_ops, incoming).unwrap();

        assert_eq!(result.applied, 2);
        assert_eq!(result.duplicates, 1);
        assert_eq!(local_ops.len(), 3);
    }

    #[tokio::test]
    async fn verify_remote_operation_batch_accepts_valid_verified_sync_batch() {
        let protocol = AntiEntropyProtocol::default();
        let peer = DeviceId::new_from_entropy([41u8; 32]);
        let effects = VerificationTestEffects::healthy();

        let verified = protocol
            .verify_remote_operation_batch(&effects, peer, valid_verified_batch(peer, 2))
            .await
            .expect("valid batch should verify");

        assert_eq!(verified.payload().len(), 2);
    }

    #[tokio::test]
    async fn verify_remote_operation_batch_rejects_forged_remote_operations() {
        let protocol = AntiEntropyProtocol::default();
        let peer = DeviceId::new_from_entropy([42u8; 32]);
        let effects = VerificationTestEffects::healthy();
        let mut incoming = valid_verified_batch(peer, 1);
        let (mut ops, metadata) = incoming.into_parts();
        ops[0].agg_sig.clear();
        incoming = DecodedIngress::new(ops, metadata.metadata().clone())
            .verify(metadata)
            .expect("re-wrapped batch");

        let error = protocol
            .verify_remote_operation_batch(&effects, peer, incoming)
            .await
            .expect_err("forged signature should fail");

        assert!(error.to_string().contains("signature verification"));
    }

    #[tokio::test]
    async fn verify_remote_operation_batch_rejects_unauthorized_namespace_context() {
        let protocol = AntiEntropyProtocol::default();
        let peer = DeviceId::new_from_entropy([43u8; 32]);
        let effects = VerificationTestEffects::healthy();
        let incoming = valid_verified_batch(peer, 1);
        let (ops, _) = incoming.into_parts();
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Device(peer),
            ContextId::new_from_entropy([99u8; 32]),
            None,
            Hash32::from_value(&ops).expect("hash incoming ops"),
            ANTI_ENTROPY_SCHEMA_VERSION,
        );
        let evidence = IngressVerificationEvidence::new(
            metadata.clone(),
            REQUIRED_INGRESS_VERIFICATION_CHECKS,
        )
        .expect("complete ingress evidence");
        let wrong_namespace = DecodedIngress::new(ops, metadata)
            .verify(evidence)
            .expect("verified wrong-namespace batch");

        let error = protocol
            .verify_remote_operation_batch(&effects, peer, wrong_namespace)
            .await
            .expect_err("wrong namespace should fail");

        assert!(error.to_string().contains("sync namespace"));
    }

    #[tokio::test]
    async fn verify_remote_operation_batch_rejects_replayed_fingerprints() {
        let protocol = AntiEntropyProtocol::default();
        let peer = DeviceId::new_from_entropy([44u8; 32]);
        let effects = VerificationTestEffects::healthy();
        let incoming = valid_verified_batch(peer, 1);
        let (ops, _) = incoming.into_parts();
        let replayed = verified_remote_ops_batch(
            &VerifiedIngressMetadata::new(
                IngressSource::Device(peer),
                peer_sync_context(peer),
                None,
                Hash32::from_value(&ops).expect("hash replayed ops"),
                ANTI_ENTROPY_SCHEMA_VERSION,
            ),
            vec![ops[0].clone(), ops[0].clone()],
        )
        .expect("replayed batch ingress");

        let error = protocol
            .verify_remote_operation_batch(&effects, peer, {
                let (batch, metadata) = replayed.into_parts();
                let ops = batch.ops().to_vec();
                let metadata = VerifiedIngressMetadata::new(
                    metadata.metadata().source(),
                    metadata.metadata().context_id(),
                    metadata.metadata().session_id(),
                    Hash32::from_value(&ops).expect("hash ops"),
                    metadata.metadata().schema_version(),
                );
                let evidence = IngressVerificationEvidence::new(
                    metadata.clone(),
                    REQUIRED_INGRESS_VERIFICATION_CHECKS,
                )
                .expect("complete ingress evidence");
                DecodedIngress::new(ops, metadata)
                    .verify(evidence)
                    .expect("verified replayed ops")
            })
            .await
            .expect_err("replayed fingerprint should fail");

        assert!(error.to_string().contains("replayed fingerprint"));
    }

    #[tokio::test]
    async fn verify_remote_operation_batch_fails_closed_on_journal_load_error() {
        let protocol = AntiEntropyProtocol::default();
        let peer = DeviceId::new_from_entropy([45u8; 32]);
        let effects =
            VerificationTestEffects::failing_journal_load(AuraError::storage("boom".to_string()));

        let error = protocol
            .verify_remote_operation_batch(&effects, peer, valid_verified_batch(peer, 1))
            .await
            .expect_err("journal load failure should fail closed");

        assert!(error.to_string().contains("journal load failed"));
    }

    #[test]
    fn malformed_remote_operations_json_is_rejected() {
        let error = crate::core::json_deserialize::<Vec<AttestedOp>>(
            "operations",
            "anti-entropy operations",
            br#"{"not":"a-batch"}"#,
        )
        .expect_err("malformed payload should fail");

        assert!(error.to_string().contains("Failed to deserialize"));
    }

    #[test]
    fn test_merge_batch_count_uses_configured_chunk_size() {
        assert_eq!(AntiEntropyProtocol::merge_batch_count(0, 4), 0);
        assert_eq!(AntiEntropyProtocol::merge_batch_count(1, 4), 1);
        assert_eq!(AntiEntropyProtocol::merge_batch_count(4, 4), 1);
        assert_eq!(AntiEntropyProtocol::merge_batch_count(5, 4), 2);
        assert_eq!(AntiEntropyProtocol::merge_batch_count(9, 4), 3);
    }

    #[test]
    fn test_merge_chunk_size_never_returns_zero() {
        let protocol = AntiEntropyProtocol::new(AntiEntropyConfig {
            batch_size: 0,
            ..Default::default()
        });

        assert_eq!(protocol.merge_chunk_size(), 1);
    }

    // -------------------------------------------------------------------------
    // Progress Tracking Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_sync_progress_event_serialization() {
        // Use deterministic UUIDs for test reproducibility
        let events = vec![
            SyncProgressEvent::Started {
                peer_id: uuid::Uuid::from_bytes([1u8; 16]),
                total_peers: 3,
            },
            SyncProgressEvent::DigestExchanged {
                peer_id: uuid::Uuid::from_bytes([2u8; 16]),
                status: DigestStatus::LocalBehind,
                local_ops: 5,
                remote_ops: 10,
            },
            SyncProgressEvent::Pulling {
                peer_id: uuid::Uuid::from_bytes([3u8; 16]),
                pulled: 3,
                total: 10,
            },
            SyncProgressEvent::Pushing {
                peer_id: uuid::Uuid::from_bytes([4u8; 16]),
                pushed: 2,
                total: 5,
            },
            SyncProgressEvent::PeerCompleted {
                peer_id: uuid::Uuid::from_bytes([5u8; 16]),
                applied: 5,
                success: true,
                peers_remaining: 2,
            },
            SyncProgressEvent::AllCompleted {
                total_applied: 15,
                total_duplicates: 3,
                successful_peers: 3,
                failed_peers: 0,
            },
            SyncProgressEvent::PeerFailed {
                peer_id: uuid::Uuid::from_bytes([6u8; 16]),
                error: "connection timeout".to_string(),
                will_retry: true,
                retry_attempt: 1,
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let deserialized: SyncProgressEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, deserialized);
        }
    }

    #[test]
    fn test_no_op_progress_callback() {
        let callback = NoOpProgressCallback;
        // Should not panic (use deterministic UUID)
        callback.on_progress(SyncProgressEvent::Started {
            peer_id: uuid::Uuid::from_bytes([7u8; 16]),
            total_peers: 1,
        });
    }

    #[test]
    fn test_logging_progress_callback() {
        let callback = LoggingProgressCallback;
        // Should not panic (logging happens internally)
        callback.on_progress(SyncProgressEvent::AllCompleted {
            total_applied: 10,
            total_duplicates: 2,
            successful_peers: 3,
            failed_peers: 1,
        });
    }

    /// Test custom callback implementation
    struct TestProgressCallback {
        events: std::sync::Mutex<Vec<SyncProgressEvent>>,
    }

    impl TestProgressCallback {
        fn new() -> Self {
            Self {
                events: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn events(&self) -> Vec<SyncProgressEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    impl SyncProgressCallback for TestProgressCallback {
        fn on_progress(&self, event: SyncProgressEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn test_custom_progress_callback() {
        let callback = TestProgressCallback::new();
        let peer_id = uuid::Uuid::from_bytes([8u8; 16]);

        callback.on_progress(SyncProgressEvent::Started {
            peer_id,
            total_peers: 2,
        });
        callback.on_progress(SyncProgressEvent::DigestExchanged {
            peer_id,
            status: DigestStatus::Equal,
            local_ops: 10,
            remote_ops: 10,
        });
        callback.on_progress(SyncProgressEvent::PeerCompleted {
            peer_id,
            applied: 0,
            success: true,
            peers_remaining: 1,
        });

        let events = callback.events();
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], SyncProgressEvent::Started { .. }));
        assert!(matches!(
            events[1],
            SyncProgressEvent::DigestExchanged { .. }
        ));
        assert!(matches!(events[2], SyncProgressEvent::PeerCompleted { .. }));
    }
}
