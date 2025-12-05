#![allow(missing_docs)]

//! Journal synchronization protocol
//!
//! Provides complete end-to-end journal synchronization using CRDT semantics
//! and anti-entropy algorithms. Consolidates all journal sync functionality
//! from scattered modules into a unified protocol implementation.
//!
//! # Architecture
//!
//! The journal sync protocol combines:
//! - Digest-based anti-entropy from `anti_entropy` module
//! - Operation log synchronization
//! - Peer state tracking
//! - Periodic and event-driven synchronization
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_sync::protocols::{JournalSyncProtocol, JournalSyncConfig};
//! use aura_core::effects::{JournalEffects, NetworkEffects};
//!
//! async fn sync_journal<E>(effects: &E, peers: Vec<DeviceId>) -> SyncResult<()>
//! where
//!     E: JournalEffects + NetworkEffects,
//! {
//!     let config = JournalSyncConfig::default();
//!     let protocol = JournalSyncProtocol::new(config);
//!
//!     let result = protocol.sync_with_peers(effects, peers).await?;
//!     println!("Synced {} operations", result.operations_synced);
//!     Ok(())
//! }
//! ```

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::{sync_session_error, SyncResult};
use crate::infrastructure::RetryPolicy;
use crate::protocols::anti_entropy::{AntiEntropyConfig, AntiEntropyProtocol, JournalDigest};
use aura_core::effects::{JournalEffects, NetworkEffects, PhysicalTimeEffects};
use aura_core::time::PhysicalTime;
use aura_core::{AccountId, AttestedOp, DeviceId};
use aura_protocol::guards::BiscuitGuardEvaluator;
use aura_wot::BiscuitTokenManager;
use futures;

// =============================================================================
// Types
// =============================================================================

/// Synchronization state for a peer
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncState {
    /// Never synchronized with this peer
    Idle,

    /// Synchronization in progress
    Syncing,

    /// Successfully synchronized
    Synced {
        /// Last sync timestamp (unified time system)
        last_sync: PhysicalTime,

        /// Operations synced in last round
        operations: usize,
    },

    /// Synchronization failed
    Failed {
        /// Error message
        error: String,

        /// Failed timestamp (unified time system)
        failed_at: PhysicalTime,
    },
}

impl SyncState {
    /// Get the last sync timestamp in milliseconds, if synced
    pub fn last_sync_ms(&self) -> Option<u64> {
        match self {
            SyncState::Synced { last_sync, .. } => Some(last_sync.ts_ms),
            _ => None,
        }
    }

    /// Get the failed timestamp in milliseconds, if failed
    pub fn failed_at_ms(&self) -> Option<u64> {
        match self {
            SyncState::Failed { failed_at, .. } => Some(failed_at.ts_ms),
            _ => None,
        }
    }

    /// Create a Synced state from milliseconds timestamp
    pub fn synced_from_ms(timestamp_ms: u64, operations: usize) -> Self {
        SyncState::Synced {
            last_sync: PhysicalTime {
                ts_ms: timestamp_ms,
                uncertainty: None,
            },
            operations,
        }
    }

    /// Create a Failed state from milliseconds timestamp
    pub fn failed_from_ms(error: String, timestamp_ms: u64) -> Self {
        SyncState::Failed {
            error,
            failed_at: PhysicalTime {
                ts_ms: timestamp_ms,
                uncertainty: None,
            },
        }
    }
}

/// Journal synchronization message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMessage {
    /// Request peer's journal digest
    DigestRequest,

    /// Response with journal digest
    DigestResponse { digest: JournalDigest },

    /// Request operations starting from index
    OperationsRequest { from_index: usize, max_ops: usize },

    /// Response with operations
    OperationsResponse {
        operations: Vec<AttestedOp>,
        has_more: bool,
    },

    /// Acknowledge successful sync
    SyncComplete { operations_applied: usize },
}

/// Journal synchronization result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSyncResult {
    /// Number of operations synchronized
    pub operations_synced: usize,

    /// Peers successfully synchronized
    pub peers_synced: Vec<DeviceId>,

    /// Peers that failed
    pub peers_failed: Vec<DeviceId>,

    /// Total synchronization time
    pub duration_ms: u64,

    /// Success indicator
    pub success: bool,
}

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for journal synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSyncConfig {
    /// Account to synchronize
    pub account_id: AccountId,

    /// Maximum operations per batch
    pub batch_size: usize,

    /// Maximum concurrent peer syncs
    pub max_concurrent_syncs: usize,

    /// Timeout for sync operations
    pub sync_timeout: Duration,

    /// Enable retry on failures
    pub retry_enabled: bool,

    /// Retry policy
    pub retry_policy: RetryPolicy,

    /// Anti-entropy configuration
    pub anti_entropy: AntiEntropyConfig,
}

impl Default for JournalSyncConfig {
    fn default() -> Self {
        Self {
            account_id: AccountId::new_from_entropy([0u8; 32]),
            batch_size: 128,
            max_concurrent_syncs: 5,
            sync_timeout: Duration::from_secs(30),
            retry_enabled: true,
            retry_policy: RetryPolicy::exponential().with_max_attempts(3),
            anti_entropy: AntiEntropyConfig::default(),
        }
    }
}

// =============================================================================
// Journal Sync Protocol
// =============================================================================

/// Journal synchronization protocol
///
/// Provides complete journal synchronization using anti-entropy algorithms
/// and CRDT merge semantics. Integrates with infrastructure for peer
/// management, retry logic, and connection pooling.
///
/// Supports Biscuit token-based authorization for sync operations.
#[derive(Clone)]
pub struct JournalSyncProtocol {
    config: JournalSyncConfig,
    anti_entropy: AntiEntropyProtocol,
    peer_states: HashMap<DeviceId, SyncState>,
}

impl JournalSyncProtocol {
    /// Create a new journal sync protocol
    pub fn new(config: JournalSyncConfig) -> Self {
        let anti_entropy = AntiEntropyProtocol::new(config.anti_entropy.clone());

        Self {
            config,
            anti_entropy,
            peer_states: HashMap::new(),
        }
    }

    /// Create a new journal sync protocol with Biscuit authorization support
    pub fn with_biscuit_authorization(
        config: JournalSyncConfig,
        token_manager: BiscuitTokenManager,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        let anti_entropy = AntiEntropyProtocol::with_biscuit_authorization(
            config.anti_entropy.clone(),
            token_manager,
            guard_evaluator,
        );

        Self {
            config,
            anti_entropy,
            peer_states: HashMap::new(),
        }
    }

    /// Synchronize with multiple peers
    ///
    /// # Integration Points
    /// - Uses `JournalEffects` to access local journal
    /// - Uses `NetworkEffects` to communicate with peers
    /// - Uses `PeerManager` from infrastructure for peer selection
    /// - Uses `RetryPolicy` for resilient operations
    ///
    /// Note: Callers should obtain `start` via their time provider (e.g., `PhysicalTimeEffects`) and pass it to this method
    pub async fn sync_with_peers<E>(
        &mut self,
        effects: &E,
        peers: Vec<DeviceId>,
        start: std::time::Instant,
    ) -> SyncResult<JournalSyncResult>
    where
        E: JournalEffects + NetworkEffects + Send + Sync + PhysicalTimeEffects,
    {
        let mut operations_synced = 0;
        let mut peers_synced = Vec::new();
        let mut peers_failed = Vec::new();

        tracing::info!(
            "Starting journal synchronization with {} peers",
            peers.len()
        );

        // Mark all peers as syncing
        for peer in &peers {
            self.peer_states.insert(*peer, SyncState::Syncing);
        }

        // Limit concurrent syncs to prevent overwhelming the system
        let chunks = peers.chunks(self.config.max_concurrent_syncs);

        for chunk in chunks {
            // Create futures for this batch of peers
            let sync_futures: Vec<_> = chunk
                .iter()
                .map(|&peer| Box::pin(self.sync_with_peer_impl(effects, peer)))
                .collect();

            // Wait for all syncs in this batch to complete
            let results = futures::future::join_all(sync_futures).await;

            // Process results
            for (i, result) in results.into_iter().enumerate() {
                let peer = chunk[i];
                match result {
                    Ok(ops_count) => {
                        operations_synced += ops_count;
                        peers_synced.push(peer);

                        // Update peer state to synced with current wall-clock time
                        let sync_time = effects.physical_time().await.unwrap_or(PhysicalTime {
                            ts_ms: 0,
                            uncertainty: None,
                        });
                        self.peer_states.insert(
                            peer,
                            SyncState::Synced {
                                last_sync: sync_time,
                                operations: ops_count,
                            },
                        );

                        tracing::info!(
                            "Successfully synced {} operations with peer {}",
                            ops_count,
                            peer
                        );
                    }
                    Err(e) => {
                        peers_failed.push(peer);

                        // Update peer state to failed with current wall-clock time
                        let failed_time = effects.physical_time().await.unwrap_or(PhysicalTime {
                            ts_ms: 0,
                            uncertainty: None,
                        });
                        self.peer_states.insert(
                            peer,
                            SyncState::Failed {
                                error: e.to_string(),
                                failed_at: failed_time,
                            },
                        );

                        tracing::warn!("Failed to sync with peer {}: {}", peer, e);
                    }
                }
            }

            // Small delay between batches to prevent overwhelming network
            if chunk.len() == self.config.max_concurrent_syncs {
                let _ = effects.sleep_ms(100).await;
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        let success = !peers_synced.is_empty();

        tracing::info!(
            "Journal synchronization completed: {} operations synced across {} peers ({} failed) in {}ms",
            operations_synced, peers_synced.len(), peers_failed.len(), duration_ms
        );

        Ok(JournalSyncResult {
            operations_synced,
            peers_synced,
            peers_failed,
            duration_ms,
            success,
        })
    }

    /// Synchronize with a single peer
    pub async fn sync_with_peer<E>(&mut self, effects: &E, peer: DeviceId) -> SyncResult<usize>
    where
        E: JournalEffects + NetworkEffects + Send + Sync + PhysicalTimeEffects,
    {
        self.peer_states.insert(peer, SyncState::Syncing);
        self.sync_with_peer_impl(effects, peer).await
    }

    /// Internal implementation of peer synchronization
    async fn sync_with_peer_impl<E>(&self, effects: &E, peer: DeviceId) -> SyncResult<usize>
    where
        E: JournalEffects + NetworkEffects + Send + Sync + PhysicalTimeEffects,
    {
        tracing::debug!("Starting synchronization with peer {}", peer);

        // Apply timeout to the entire sync operation
        let sync_future =
            async {
                // Use anti-entropy protocol for the actual synchronization
                let result = self
                    .anti_entropy
                    .execute(effects, peer)
                    .await
                    .map_err(|e| sync_session_error(format!("Anti-entropy sync failed: {}", e)))?;

                tracing::debug!(
                "Anti-entropy sync with peer {} completed: {} applied, {} duplicates, {} rounds",
                peer, result.applied, result.duplicates, result.rounds
            );

                // Apply additional journal-specific processing if needed
                if result.applied > 0 {
                    // Update local journal state after successful sync
                    // In a full implementation, this would trigger journal rebuilding
                    // or other post-sync processing

                    tracing::info!(
                        "Journal sync with peer {} applied {} new operations",
                        peer,
                        result.applied
                    );
                }

                Ok(result.applied)
            };

        // Execute without runtime-specific timeout; external callers should enforce via PhysicalTimeEffects.
        sync_future.await
    }

    /// Get synchronization state for a peer
    pub fn get_peer_state(&self, peer: &DeviceId) -> Option<&SyncState> {
        self.peer_states.get(peer)
    }

    /// Update peer state after sync
    pub fn update_peer_state(&mut self, peer: DeviceId, state: SyncState) {
        self.peer_states.insert(peer, state);
    }

    /// Clear all peer states
    pub fn clear_states(&mut self) {
        self.peer_states.clear();
    }

    /// Get statistics about synchronization
    pub fn statistics(&self) -> JournalSyncStatistics {
        let total = self.peer_states.len();
        let syncing = self
            .peer_states
            .values()
            .filter(|s| matches!(s, SyncState::Syncing))
            .count();
        let synced = self
            .peer_states
            .values()
            .filter(|s| matches!(s, SyncState::Synced { .. }))
            .count();
        let failed = self
            .peer_states
            .values()
            .filter(|s| matches!(s, SyncState::Failed { .. }))
            .count();

        JournalSyncStatistics {
            total_peers: total,
            syncing_peers: syncing,
            synced_peers: synced,
            failed_peers: failed,
        }
    }
}

impl Default for JournalSyncProtocol {
    fn default() -> Self {
        Self::new(JournalSyncConfig::default())
    }
}

/// Journal synchronization statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSyncStatistics {
    /// Total tracked peers
    pub total_peers: usize,

    /// Peers currently syncing
    pub syncing_peers: usize,

    /// Successfully synced peers
    pub synced_peers: usize,

    /// Failed peers
    pub failed_peers: usize,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_journal_sync_protocol_creation() {
        let config = JournalSyncConfig::default();
        let protocol = JournalSyncProtocol::new(config);

        let stats = protocol.statistics();
        assert_eq!(stats.total_peers, 0);
    }

    #[test]
    fn test_peer_state_tracking() {
        let mut protocol = JournalSyncProtocol::default();
        let peer = DeviceId::from_bytes([1; 32]);

        assert!(protocol.get_peer_state(&peer).is_none());

        protocol.update_peer_state(peer, SyncState::Syncing);
        assert!(matches!(
            protocol.get_peer_state(&peer),
            Some(SyncState::Syncing)
        ));

        protocol.update_peer_state(
            peer,
            SyncState::Synced {
                last_sync: test_time(100),
                operations: 42,
            },
        );
        assert!(matches!(
            protocol.get_peer_state(&peer),
            Some(SyncState::Synced { operations: 42, .. })
        ));
    }

    #[test]
    fn test_statistics() {
        let mut protocol = JournalSyncProtocol::default();

        let peer1 = DeviceId::from_bytes([1; 32]);
        let peer2 = DeviceId::from_bytes([2; 32]);
        let peer3 = DeviceId::from_bytes([3; 32]);

        protocol.update_peer_state(peer1, SyncState::Syncing);
        protocol.update_peer_state(
            peer2,
            SyncState::Synced {
                last_sync: test_time(100),
                operations: 10,
            },
        );
        protocol.update_peer_state(
            peer3,
            SyncState::Failed {
                error: "timeout".to_string(),
                failed_at: test_time(200),
            },
        );

        let stats = protocol.statistics();
        assert_eq!(stats.total_peers, 3);
        assert_eq!(stats.syncing_peers, 1);
        assert_eq!(stats.synced_peers, 1);
        assert_eq!(stats.failed_peers, 1);
    }
}
