//! Effect-based Synchronization Service
//!
//! This module implements journal synchronization using the algebraic effects pattern.
//! Instead of choreographic programming, it uses effect composition to coordinate
//! distributed synchronization operations.

use aura_core::{AttestedOp, DeviceId, Hash32};
use aura_protocol::effects::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Synchronization session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSession {
    /// Session identifier
    pub session_id: Uuid,
    /// Target peer for synchronization
    pub peer_id: Uuid,
    /// Session start time
    pub started_at: u64,
    /// Current phase of synchronization
    pub phase: SyncPhase,
    /// Operations synchronized so far
    pub ops_synced: usize,
    /// Last error encountered (if any)
    pub last_error: Option<String>,
}

/// Phases of the synchronization protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncPhase {
    /// Exchanging digests with peer
    DigestExchange,
    /// Computing differences between OpLogs
    DifferenceComputation,
    /// Requesting missing operations
    OperationRequest,
    /// Merging received operations
    OperationMerge,
    /// Synchronization completed
    Completed,
    /// Synchronization failed
    Failed,
}

/// Statistics about synchronization operations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncStats {
    /// Total synchronization sessions initiated
    pub sessions_initiated: u64,
    /// Total sessions completed successfully
    pub sessions_completed: u64,
    /// Total sessions failed
    pub sessions_failed: u64,
    /// Total operations synchronized
    pub operations_synced: u64,
    /// Active synchronization sessions
    pub active_sessions: usize,
    /// Rate limiting violations
    pub rate_limit_violations: u64,
}

/// Main synchronization service implementing effect-based coordination
///
/// This service coordinates journal synchronization across devices using
/// the algebraic effects pattern. It composes different effect handlers
/// to implement anti-entropy synchronization.
pub struct SyncService<E> {
    /// Effect system for side-effect operations
    effects: Arc<E>,
    /// Configuration for anti-entropy protocol
    config: AntiEntropyConfig,
    /// Active synchronization sessions
    sessions: Arc<Mutex<HashMap<Uuid, SyncSession>>>,
    /// Rate limiting state (peer_id -> last_sync_time)
    rate_limits: Arc<RwLock<HashMap<Uuid, u64>>>,
    /// Service statistics
    stats: Arc<Mutex<SyncStats>>,
    /// Local device identifier
    #[allow(dead_code)]
    device_id: DeviceId,
}

impl<E> SyncService<E>
where
    E: SyncEffects + Send + Sync + 'static,
{
    /// Create a new synchronization service
    pub fn new(effects: Arc<E>, config: AntiEntropyConfig, device_id: DeviceId) -> Self {
        Self {
            effects,
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(Mutex::new(SyncStats::default())),
            device_id,
        }
    }

    /// Initiate synchronization with a peer
    ///
    /// This is the main entry point for starting a sync session.
    /// It follows the anti-entropy protocol:
    /// 1. Check rate limits
    /// 2. Exchange digests
    /// 3. Compute differences
    /// 4. Request missing operations
    /// 5. Merge operations
    pub async fn sync_with_peer(&self, peer_id: Uuid) -> Result<SyncSession, SyncError> {
        // Check rate limits
        self.check_rate_limit(peer_id).await?;

        let session_id = Uuid::nil();
        let session = SyncSession {
            session_id,
            peer_id,
            started_at: self.current_time().await?,
            phase: SyncPhase::DigestExchange,
            ops_synced: 0,
            last_error: None,
        };

        // Track active session
        {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(session_id, session.clone());

            let mut stats = self.stats.lock().await;
            stats.sessions_initiated += 1;
            stats.active_sessions += 1;
        }

        info!(
            session_id = %session_id,
            peer_id = %peer_id,
            "Starting sync session"
        );

        match self.execute_sync_protocol(session).await {
            Ok(completed_session) => {
                self.complete_session(completed_session.clone()).await;
                Ok(completed_session)
            }
            Err(e) => {
                self.fail_session(session_id, e.to_string()).await;
                Err(e)
            }
        }
    }

    /// Execute the synchronization protocol
    async fn execute_sync_protocol(
        &self,
        mut session: SyncSession,
    ) -> Result<SyncSession, SyncError> {
        // Phase 1: Digest Exchange
        session.phase = SyncPhase::DigestExchange;
        self.update_session(session.clone()).await;

        debug!(session_id = %session.session_id, "Exchanging digests");
        let local_digest = self.get_local_digest().await?;
        let remote_digest = self
            .exchange_digest_with_peer(session.peer_id, &local_digest)
            .await?;

        // Phase 2: Difference Computation
        session.phase = SyncPhase::DifferenceComputation;
        self.update_session(session.clone()).await;

        debug!(session_id = %session.session_id, "Computing differences");
        let missing_cids = self
            .compute_missing_operations(&local_digest, &remote_digest)
            .await?;

        if missing_cids.is_empty() {
            debug!(session_id = %session.session_id, "No missing operations, sync complete");
            session.phase = SyncPhase::Completed;
            return Ok(session);
        }

        // Phase 3: Operation Request
        session.phase = SyncPhase::OperationRequest;
        self.update_session(session.clone()).await;

        debug!(
            session_id = %session.session_id,
            missing_ops = missing_cids.len(),
            "Requesting missing operations"
        );
        let missing_ops = self
            .request_operations_from_peer(session.peer_id, missing_cids)
            .await?;

        // Phase 4: Operation Merge
        session.phase = SyncPhase::OperationMerge;
        self.update_session(session.clone()).await;

        debug!(
            session_id = %session.session_id,
            ops_count = missing_ops.len(),
            "Merging operations"
        );
        self.merge_operations(missing_ops.clone()).await?;
        session.ops_synced = missing_ops.len();

        // Phase 5: Completion
        session.phase = SyncPhase::Completed;
        info!(
            session_id = %session.session_id,
            ops_synced = session.ops_synced,
            "Sync session completed successfully"
        );

        Ok(session)
    }

    /// Get local OpLog digest
    async fn get_local_digest(&self) -> Result<BloomDigest, SyncError> {
        self.effects.get_oplog_digest().await
    }

    /// Exchange digest with peer
    async fn exchange_digest_with_peer(
        &self,
        peer_id: Uuid,
        local_digest: &BloomDigest,
    ) -> Result<BloomDigest, SyncError> {
        // For now, just get the missing ops from the peer
        // In a full implementation, this would involve digest exchange protocol
        let remote_digest = BloomDigest::empty(); // Placeholder

        debug!(
            peer_id = %peer_id,
            local_ops = local_digest.len(),
            remote_ops = remote_digest.len(),
            "Exchanged digests (placeholder)"
        );

        Ok(remote_digest)
    }

    /// Compute operations missing from local OpLog
    async fn compute_missing_operations(
        &self,
        local_digest: &BloomDigest,
        remote_digest: &BloomDigest,
    ) -> Result<Vec<Hash32>, SyncError> {
        let missing: Vec<Hash32> = remote_digest
            .cids
            .iter()
            .filter(|cid| !local_digest.contains(cid))
            .copied()
            .collect();

        debug!(missing_count = missing.len(), "Computed missing operations");
        Ok(missing)
    }

    /// Request specific operations from peer
    async fn request_operations_from_peer(
        &self,
        peer_id: Uuid,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        // Use the SyncEffects trait method
        self.effects.request_ops_from_peer(peer_id, cids).await
    }

    /// Merge operations into local OpLog
    async fn merge_operations(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        // Use the SyncEffects trait method
        self.effects.merge_remote_ops(ops).await
    }

    /// Check rate limit for peer
    async fn check_rate_limit(&self, peer_id: Uuid) -> Result<(), SyncError> {
        let current_time = self.current_time().await?;
        let min_interval = self.config.min_sync_interval_ms;

        let rate_limits = self.rate_limits.read().await;
        if let Some(&last_sync) = rate_limits.get(&peer_id) {
            if current_time - last_sync < min_interval {
                let mut stats = self.stats.lock().await;
                stats.rate_limit_violations += 1;
                return Err(SyncError::RateLimitExceeded(peer_id));
            }
        }
        drop(rate_limits);

        // Update last sync time
        let mut rate_limits = self.rate_limits.write().await;
        rate_limits.insert(peer_id, current_time);

        Ok(())
    }

    /// Get current time from effects
    async fn current_time(&self) -> Result<u64, SyncError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        #[allow(clippy::disallowed_methods)]
        let now = SystemTime::now();
        now.duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .map_err(|_| SyncError::TimeError)
    }

    /// Update session state
    async fn update_session(&self, session: SyncSession) {
        let mut sessions = self.sessions.lock().await;
        sessions.insert(session.session_id, session);
    }

    /// Complete a synchronization session
    async fn complete_session(&self, session: SyncSession) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(&session.session_id);

        let mut stats = self.stats.lock().await;
        stats.sessions_completed += 1;
        stats.active_sessions -= 1;
        stats.operations_synced += session.ops_synced as u64;
    }

    /// Fail a synchronization session
    async fn fail_session(&self, session_id: Uuid, error: String) {
        warn!(session_id = %session_id, error = %error, "Sync session failed");

        let mut sessions = self.sessions.lock().await;
        if let Some(mut session) = sessions.remove(&session_id) {
            session.phase = SyncPhase::Failed;
            session.last_error = Some(error);
            sessions.insert(session_id, session);
        }

        let mut stats = self.stats.lock().await;
        stats.sessions_failed += 1;
        stats.active_sessions -= 1;
    }

    /// Get synchronization statistics
    pub async fn get_stats(&self) -> SyncStats {
        let stats = self.stats.lock().await;
        stats.clone()
    }

    /// Get active synchronization sessions
    pub async fn get_active_sessions(&self) -> Vec<SyncSession> {
        let sessions = self.sessions.lock().await;
        sessions.values().cloned().collect()
    }

    /// Get configuration
    pub fn get_config(&self) -> &AntiEntropyConfig {
        &self.config
    }

    /// Update configuration
    pub async fn update_config(&mut self, config: AntiEntropyConfig) {
        self.config = config;
        info!("Updated sync service configuration");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock effect system for testing
    struct MockSyncEffects;

    #[async_trait::async_trait]
    impl SyncEffects for MockSyncEffects {
        async fn sync_with_peer(&self, _peer_id: Uuid) -> Result<(), SyncError> {
            Ok(())
        }

        async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError> {
            Ok(BloomDigest::empty())
        }

        async fn get_missing_ops(
            &self,
            _remote_digest: &BloomDigest,
        ) -> Result<Vec<AttestedOp>, SyncError> {
            Ok(Vec::new())
        }

        async fn request_ops_from_peer(
            &self,
            _peer_id: Uuid,
            _cids: Vec<Hash32>,
        ) -> Result<Vec<AttestedOp>, SyncError> {
            Ok(Vec::new())
        }

        async fn merge_remote_ops(&self, _ops: Vec<AttestedOp>) -> Result<(), SyncError> {
            Ok(())
        }

        async fn announce_new_op(&self, _cid: Hash32) -> Result<(), SyncError> {
            Ok(())
        }

        async fn request_op(&self, _peer_id: Uuid, _cid: Hash32) -> Result<AttestedOp, SyncError> {
            Err(SyncError::OperationNotFound)
        }

        async fn push_op_to_peers(
            &self,
            _op: AttestedOp,
            _peers: Vec<Uuid>,
        ) -> Result<(), SyncError> {
            Ok(())
        }

        async fn get_connected_peers(&self) -> Result<Vec<Uuid>, SyncError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn test_sync_service_creation() {
        let effects = Arc::new(MockSyncEffects);
        let config = AntiEntropyConfig::default();
        let device_id = DeviceId::new();

        let service = SyncService::new(effects, config, device_id);
        let stats = service.get_stats().await;

        assert_eq!(stats.sessions_initiated, 0);
        assert_eq!(stats.active_sessions, 0);
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let effects = Arc::new(MockSyncEffects);
        let config = AntiEntropyConfig {
            min_sync_interval_ms: 1000, // 1 second
            ..Default::default()
        };
        let device_id = DeviceId::new();

        let service = SyncService::new(effects, config, device_id);
        let peer_id = DeviceId::new().into();

        // First sync should succeed
        assert!(service.check_rate_limit(peer_id).await.is_ok());

        // Immediate second sync should fail
        assert!(matches!(
            service.check_rate_limit(peer_id).await,
            Err(SyncError::RateLimitExceeded(_))
        ));
    }
}
