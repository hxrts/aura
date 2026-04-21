use super::*;
use crate::core::physical_time_from_ms;
use aura_core::time::PhysicalTime;

/// Maintenance cleanup statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncMaintenanceStats {
    pub sessions_removed: u32,
    pub peer_states_pruned: u32,
}

impl SyncService {
    /// Run maintenance cleanup for long-lived state.
    pub async fn maintenance_cleanup(
        &self,
        now_ms: u64,
        peer_state_ttl_ms: u64,
        max_peer_states: usize, // usize ok: function parameter for collection sizing
    ) -> SyncResult<SyncMaintenanceStats> {
        let now = physical_time_from_ms(now_ms);

        let mut session_manager = self.session_manager.write();
        let sessions_removed = session_manager.cleanup_stale_sessions(&now)?;

        let mut journal_sync = self.journal_sync.write();
        let peer_states_pruned =
            journal_sync.prune_peer_states(now_ms, peer_state_ttl_ms, max_peer_states);

        Ok(SyncMaintenanceStats {
            sessions_removed: sessions_removed as u32,
            peer_states_pruned: peer_states_pruned as u32,
        })
    }

    /// Check rate limits for peer sync operations
    ///
    /// # Arguments
    /// - `peers`: List of peer device IDs to check
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub(super) async fn check_rate_limits(
        &self,
        peers: &[DeviceId],
        now_instant: MonotonicInstant,
    ) -> SyncResult<Vec<DeviceId>> {
        let mut allowed_peers = Vec::new();
        let mut rate_limiter = self.rate_limiter.write();

        for &peer in peers {
            let result = rate_limiter.check_rate_limit(peer, 1, now_instant);
            if result.is_allowed() {
                allowed_peers.push(peer);
            } else if let Some(retry_after) = result.retry_after() {
                tracing::debug!(
                    operation_id = JOURNAL_SYNC_OPERATION_ID,
                    peer_id = %peer,
                    retry_after_ms = retry_after.as_millis(),
                    "Rate limit exceeded for peer"
                );
            }
        }

        Ok(allowed_peers)
    }

    /// Update sync metrics based on sync results.
    pub(super) async fn update_sync_metrics(&self, results: &[(DeviceId, u64)]) -> SyncResult<()> {
        let now_ms = self
            .time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?
            .ts_ms;

        let metrics = self.metrics.write();
        for &(peer, synced_ops) in results {
            metrics.increment_sync_attempts(peer);
            metrics.increment_sync_successes(peer);
            metrics.update_last_sync(peer, now_ms);

            if synced_ops > 0 {
                metrics.add_synced_operations(peer, synced_ops);
            }
        }

        Ok(())
    }

    /// Clean up sync sessions after completion.
    pub(super) async fn cleanup_sync_sessions(&self, peers: &[DeviceId]) -> SyncResult<()> {
        let mut session_manager = self.session_manager.write();

        for &peer in peers {
            if let Err(e) = session_manager.close_session(peer) {
                tracing::warn!(
                    operation_id = JOURNAL_SYNC_OPERATION_ID,
                    peer_id = %peer,
                    error = %e,
                    "Failed to clean up session for peer"
                );
            } else {
                tracing::debug!(
                    operation_id = JOURNAL_SYNC_OPERATION_ID,
                    peer_id = %peer,
                    "Cleaned up sync session for peer"
                );
            }
        }

        Ok(())
    }

    /// Discover available peers via peer_manager.
    pub(super) async fn discover_available_peers(&self) -> SyncResult<Vec<DeviceId>> {
        let peer_manager = self.peer_manager.read();
        let mut available_peers = Vec::new();

        for peer in peer_manager.list_peers() {
            if peer_manager.is_peer_available(&peer) && peer_manager.get_peer_health(&peer) > 0.5 {
                available_peers.push(peer);
            }
        }

        tracing::debug!(
            operation_id = AUTO_SYNC_OPERATION_ID,
            available_peer_count = available_peers.len(),
            "Discovered available peers for sync"
        );
        Ok(available_peers)
    }

    /// Update peer states after sync operations.
    pub(super) async fn update_peer_states(&self, peers: &[DeviceId]) -> SyncResult<()> {
        let now = self
            .time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;

        let mut peer_manager = self.peer_manager.write();

        for &peer in peers {
            peer_manager.update_last_contact(peer, &now);

            let recent_success_rate = peer_manager.get_recent_sync_success_rate(&peer);
            if recent_success_rate < 0.3 {
                peer_manager.mark_peer_degraded(&peer, &now);
            } else if recent_success_rate > 0.8 {
                peer_manager.mark_peer_healthy(&peer, &now);
            }
        }

        Ok(())
    }

    /// Select best auto-sync peers based on health and priority.
    pub(super) async fn select_best_auto_sync_peers(
        peer_manager: &RwLock<PeerManager>,
        peers: &[DeviceId],
        max_peers: usize, // usize ok: function parameter for collection sizing
    ) -> SyncResult<Vec<DeviceId>> {
        let manager = peer_manager.read();
        let mut peer_scores = Vec::new();

        for &peer in peers {
            let health = manager.get_peer_health(&peer);
            let priority = manager.get_peer_priority(&peer);
            peer_scores.push((peer, health * priority));
        }

        peer_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(peer_scores
            .into_iter()
            .take(max_peers)
            .map(|(peer, _)| peer)
            .collect())
    }

    /// Create sync sessions for selected peers.
    pub(super) async fn create_sync_sessions<T: PhysicalTimeEffects>(
        session_manager: &RwLock<SessionManager<serde_json::Value>>,
        peers: &[DeviceId],
        time_effects: &T,
    ) -> SyncResult<Vec<DeviceId>> {
        let mut session_peers = Vec::new();
        let now = time_effects
            .physical_time()
            .await
            .map_err(time_error_to_aura)?;

        let mut manager = session_manager.write();
        for &peer in peers {
            match manager.create_session(vec![peer], &now) {
                Ok(session_id) => {
                    session_peers.push(peer);
                    tracing::debug!(
                        operation_id = JOURNAL_SYNC_OPERATION_ID,
                        session_id = %session_id,
                        peer_id = %peer,
                        "Created auto-sync session for peer"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        operation_id = JOURNAL_SYNC_OPERATION_ID,
                        peer_id = %peer,
                        error = %e,
                        "Failed to create auto-sync session for peer"
                    );
                }
            }
        }

        Ok(session_peers)
    }

    /// Update peer scores based on sync results.
    pub(super) async fn update_peer_scores_from_sync(
        peer_manager: &RwLock<PeerManager>,
        results: &[(DeviceId, bool)],
        now: &PhysicalTime,
    ) -> SyncResult<()> {
        let mut manager = peer_manager.write();

        for &(peer, success) in results {
            if success {
                manager.increment_sync_success(&peer, now);
                manager.update_last_successful_sync(&peer, now);
            } else {
                manager.increment_sync_failure(&peer);
            }
            manager.recalculate_peer_health(&peer);
        }

        Ok(())
    }

    /// Update auto-sync metrics.
    pub(super) async fn update_auto_sync_metrics(results: &[(DeviceId, bool)]) -> SyncResult<()> {
        let total_peers = results.len();
        let successful_syncs = results.iter().filter(|(_, success)| *success).count();
        let failed_syncs = total_peers - successful_syncs;

        tracing::info!(
            "Auto-sync metrics: {} total peers, {} successful, {} failed",
            total_peers,
            successful_syncs,
            failed_syncs
        );

        Ok(())
    }
}
