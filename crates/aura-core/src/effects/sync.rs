//! Sync Effects - Anti-Entropy and Broadcast Operations
//!
//! This module defines effect traits for tree synchronization operations including
//! anti-entropy (digest-based reconciliation) and eager broadcast of new operations.
//!
//! # Effect Classification
//!
//! - **Category**: Protocol Coordination Effect
//! - **Implementation**: `aura-sync` or `aura-protocol` (Layer 4 or Layer 5)
//! - **Usage**: Anti-entropy synchronization, digest exchange, OpLog reconciliation
//!
//! This is a protocol coordination effect for multi-party synchronization. Implements
//! digest-based anti-entropy, pull-based operation reconciliation, and eager broadcast
//! for distributed commitment tree synchronization. Handlers in `aura-sync` or
//! `aura-protocol` coordinate sync protocols.
//!
//! ## Design Principles
//!
//! - **Digest Exchange**: Bloom filters or rolling hashes for efficient comparison
//! - **Bounded Leakage**: Rate limiting and batching for privacy
//! - **Pull-Based**: Requestor drives sync (no unsolicited pushes)
//! - **Verification**: All received operations verified before storage

use crate::{AttestedOp, Hash32};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Bloom filter digest for efficient OpLog comparison
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BloomDigest {
    /// Digest filter of operation CIDs
    #[serde(with = "serde_bytes")]
    pub filter: Vec<u8>,
    /// Approximate element count
    pub count: usize,
}

impl BloomDigest {
    /// Create an empty digest
    pub fn empty() -> Self {
        Self {
            filter: Vec::new(),
            count: 0,
        }
    }

    /// Check if digest is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Errors that can occur during sync operations
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Peer not found or unreachable
    #[error("Peer {0} not reachable")]
    PeerUnreachable(Uuid),

    /// Operation verification failed
    #[error("Operation verification failed: {0}")]
    VerificationFailed(String),

    /// Network error during sync
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded for peer {0}")]
    RateLimitExceeded(Uuid),

    /// Invalid digest received
    #[error("Invalid digest from peer {0}")]
    InvalidDigest(Uuid),

    /// Operation not found in local store
    #[error("Operation not found")]
    OperationNotFound,

    /// Back pressure - too many pending operations
    #[error("Back pressure active - too many pending operations")]
    BackPressure,

    /// Time-related error (e.g., system clock issues)
    #[error("Time error occurred")]
    TimeError,

    /// Authorization failed during guard chain evaluation
    #[error("Authorization failed: guard chain denied operation")]
    AuthorizationFailed,
}

/// Sync effect traits for anti-entropy and broadcast operations
///
/// This trait defines the operations needed for tree synchronization across
/// distributed replicas using digest exchange and operation reconciliation.
#[async_trait]
pub trait SyncEffects: Send + Sync {
    /// Perform anti-entropy sync with a peer
    ///
    /// This is the main entry point for digest-based reconciliation.
    /// It exchanges digests, computes differences, and reconciles OpLogs.
    ///
    /// ## Steps
    /// 1. Get local OpLog digest
    /// 2. Send digest to peer and receive peer's digest
    /// 3. Compute missing operations (what peer has that we don't)
    /// 4. Request and receive missing operations
    /// 5. Merge operations into local OpLog via join
    async fn sync_with_peer(&self, peer_id: Uuid) -> Result<(), SyncError>;

    /// Get digest of local OpLog
    ///
    /// Returns a compact representation of the local OpLog for comparison.
    /// This is typically a Bloom filter or rolling hash.
    async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError>;

    /// Get operations missing from remote digest
    ///
    /// Compares local OpLog with remote digest and returns operations
    /// that the remote peer is missing (what we have that they don't).
    ///
    /// **Note**: This is bounded by the leakage budget to prevent
    /// excessive metadata disclosure.
    async fn get_missing_ops(
        &self,
        remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError>;

    /// Request specific operations from a peer
    ///
    /// Pull-based operation retrieval. The requestor specifies which
    /// operations they want by CID.
    async fn request_ops_from_peer(
        &self,
        peer_id: Uuid,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError>;

    /// Merge remote operations into local OpLog
    ///
    /// Verifies and merges received operations using CRDT join semantics.
    /// All operations are verified before being added to the local OpLog.
    ///
    /// ## Verification Steps
    /// 1. Verify aggregate signature
    /// 2. Verify parent binding
    /// 3. Merge into OpLog via union (OR-set semantics)
    async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError>;

    /// Announce a newly created operation to immediate peers
    ///
    /// Eager push notification (just the CID, not the full operation).
    /// Peers can pull the operation if they're interested.
    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError>;

    /// Request a specific operation by CID
    ///
    /// Used in response to announcements or when filling OpLog gaps.
    async fn request_op(&self, peer_id: Uuid, cid: Hash32) -> Result<AttestedOp, SyncError>;

    /// Push an operation to specific peers
    ///
    /// Eager push of full operation (not just announcement).
    /// Used for immediate neighbors or high-priority operations.
    ///
    /// **Note**: This should be rate-limited to prevent flooding.
    async fn push_op_to_peers(&self, op: AttestedOp, peers: Vec<Uuid>) -> Result<(), SyncError>;

    /// Get list of currently connected peers
    ///
    /// Returns peers that are reachable for sync operations.
    async fn get_connected_peers(&self) -> Result<Vec<Uuid>, SyncError>;
}

/// Anti-entropy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyConfig {
    /// Minimum interval between sync attempts with same peer (milliseconds)
    pub min_sync_interval_ms: u64,

    /// Maximum number of operations to send in one batch
    pub max_ops_per_batch: usize,

    /// Maximum number of peers to sync with concurrently
    pub max_concurrent_syncs: usize,

    /// Timeout for sync operations (milliseconds)
    pub sync_timeout_ms: u64,
}

impl Default for AntiEntropyConfig {
    fn default() -> Self {
        Self {
            min_sync_interval_ms: 30_000, // 30 seconds
            max_ops_per_batch: 100,
            max_concurrent_syncs: 5,
            sync_timeout_ms: 10_000, // 10 seconds
        }
    }
}
