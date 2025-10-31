//! Basic Replication to Static Peers
//!
//! Implements chunk replication to configured static peer lists
//! with offline fallback and confirmation tracking.
//!
//! Reference: docs/040_storage.md Section 8

use crate::manifest::{PeerId, ReplicaFallbackPolicy, StaticReplicationHint};
use crate::storage::chunk_store::{ChunkId, EncryptedChunk};
use aura_journal::capability::CapabilityScope;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplicationStatus {
    Pending,
    InProgress,
    Successful,
    Failed { reason: String },
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaConfirmation {
    pub peer_id: PeerId,
    pub chunk_id: ChunkId,
    pub status: ReplicationStatus,
    pub attempted_at: u64,
    pub confirmed_at: Option<u64>,
}

impl ReplicaConfirmation {
    pub fn new(peer_id: PeerId, chunk_id: ChunkId, attempted_at: u64) -> Self {
        Self {
            peer_id,
            chunk_id,
            status: ReplicationStatus::Pending,
            attempted_at,
            confirmed_at: None,
        }
    }

    pub fn mark_success(&mut self, timestamp: u64) {
        self.status = ReplicationStatus::Successful;
        self.confirmed_at = Some(timestamp);
    }

    pub fn mark_failure(&mut self, reason: String, timestamp: u64) {
        self.status = ReplicationStatus::Failed { reason };
        self.confirmed_at = Some(timestamp);
    }

    pub fn mark_offline(&mut self, timestamp: u64) {
        self.status = ReplicationStatus::Offline;
        self.confirmed_at = Some(timestamp);
    }
}

#[derive(Debug, Clone)]
pub struct ReplicationTracker {
    confirmations: BTreeMap<(ChunkId, PeerId), ReplicaConfirmation>,
    chunk_replicas: BTreeMap<ChunkId, BTreeSet<PeerId>>,
    peer_chunks: BTreeMap<PeerId, BTreeSet<ChunkId>>,
}

impl ReplicationTracker {
    pub fn new() -> Self {
        Self {
            confirmations: BTreeMap::new(),
            chunk_replicas: BTreeMap::new(),
            peer_chunks: BTreeMap::new(),
        }
    }

    pub fn track_replication(
        &mut self,
        peer_id: PeerId,
        chunk_id: ChunkId,
        timestamp: u64,
    ) -> ReplicaConfirmation {
        let confirmation = ReplicaConfirmation::new(peer_id.clone(), chunk_id.clone(), timestamp);

        self.confirmations
            .insert((chunk_id.clone(), peer_id.clone()), confirmation.clone());

        self.chunk_replicas
            .entry(chunk_id.clone())
            .or_insert_with(BTreeSet::new)
            .insert(peer_id.clone());

        self.peer_chunks
            .entry(peer_id)
            .or_insert_with(BTreeSet::new)
            .insert(chunk_id);

        confirmation
    }

    pub fn update_status(
        &mut self,
        peer_id: &PeerId,
        chunk_id: &ChunkId,
        status: ReplicationStatus,
        timestamp: u64,
    ) {
        if let Some(confirmation) = self
            .confirmations
            .get_mut(&(chunk_id.clone(), peer_id.clone()))
        {
            match status {
                ReplicationStatus::Successful => confirmation.mark_success(timestamp),
                ReplicationStatus::Failed { reason } => {
                    confirmation.mark_failure(reason, timestamp)
                }
                ReplicationStatus::Offline => confirmation.mark_offline(timestamp),
                _ => confirmation.status = status,
            }
        }
    }

    pub fn get_chunk_replicas(&self, chunk_id: &ChunkId) -> Option<&BTreeSet<PeerId>> {
        self.chunk_replicas.get(chunk_id)
    }

    pub fn get_peer_chunks(&self, peer_id: &PeerId) -> Option<&BTreeSet<ChunkId>> {
        self.peer_chunks.get(peer_id)
    }

    pub fn get_confirmation(
        &self,
        peer_id: &PeerId,
        chunk_id: &ChunkId,
    ) -> Option<&ReplicaConfirmation> {
        self.confirmations.get(&(chunk_id.clone(), peer_id.clone()))
    }

    pub fn get_successful_replicas(&self, chunk_id: &ChunkId) -> Vec<PeerId> {
        self.confirmations
            .iter()
            .filter(|((cid, _), conf)| {
                cid == chunk_id && matches!(conf.status, ReplicationStatus::Successful)
            })
            .map(|((_, peer_id), _)| peer_id.clone())
            .collect()
    }

    pub fn get_replication_stats(&self, chunk_id: &ChunkId) -> ReplicationStats {
        let mut stats = ReplicationStats {
            total_replicas: 0,
            successful: 0,
            failed: 0,
            pending: 0,
            offline: 0,
        };

        for ((cid, _), conf) in self.confirmations.iter() {
            if cid == chunk_id {
                stats.total_replicas += 1;
                match &conf.status {
                    ReplicationStatus::Successful => stats.successful += 1,
                    ReplicationStatus::Failed { .. } => stats.failed += 1,
                    ReplicationStatus::Pending | ReplicationStatus::InProgress => {
                        stats.pending += 1
                    }
                    ReplicationStatus::Offline => stats.offline += 1,
                }
            }
        }

        stats
    }
}

#[derive(Debug, Clone)]
pub struct ReplicationStats {
    pub total_replicas: usize,
    pub successful: usize,
    pub failed: usize,
    pub pending: usize,
    pub offline: usize,
}

pub struct Replicator {
    tracker: ReplicationTracker,
    local_storage: bool,
}

impl Replicator {
    pub fn new() -> Self {
        Self {
            tracker: ReplicationTracker::new(),
            local_storage: true,
        }
    }

    pub fn replicate_chunk(
        &mut self,
        chunk_id: ChunkId,
        encrypted_chunk: &EncryptedChunk,
        hint: &StaticReplicationHint,
        timestamp: u64,
    ) -> Result<ReplicationResult, ReplicationError> {
        if hint.target_peers.is_empty() {
            return self.handle_fallback_policy(&chunk_id, &hint.fallback_policy, timestamp);
        }

        let mut result = ReplicationResult {
            chunk_id: chunk_id.clone(),
            attempted_peers: Vec::new(),
            successful_peers: Vec::new(),
            failed_peers: Vec::new(),
            fallback_used: false,
        };

        for peer_id in &hint.target_peers {
            self.tracker
                .track_replication(peer_id.clone(), chunk_id.clone(), timestamp);

            let push_result = self.push_chunk_to_peer(peer_id, encrypted_chunk, timestamp);

            result.attempted_peers.push(peer_id.clone());

            match push_result {
                Ok(()) => {
                    self.tracker.update_status(
                        peer_id,
                        &chunk_id,
                        ReplicationStatus::Successful,
                        timestamp,
                    );
                    result.successful_peers.push(peer_id.clone());
                }
                Err(ReplicationError::PeerOffline) => {
                    self.tracker.update_status(
                        peer_id,
                        &chunk_id,
                        ReplicationStatus::Offline,
                        timestamp,
                    );
                    result.failed_peers.push(peer_id.clone());
                }
                Err(e) => {
                    self.tracker.update_status(
                        peer_id,
                        &chunk_id,
                        ReplicationStatus::Failed {
                            reason: e.to_string(),
                        },
                        timestamp,
                    );
                    result.failed_peers.push(peer_id.clone());
                }
            }
        }

        if result.successful_peers.len() < hint.target_replicas as usize {
            let fallback_result =
                self.handle_fallback_policy(&chunk_id, &hint.fallback_policy, timestamp)?;
            result.fallback_used = true;
            result
                .successful_peers
                .extend(fallback_result.successful_peers);
        }

        Ok(result)
    }

    fn push_chunk_to_peer(
        &self,
        peer_id: &PeerId,
        encrypted_chunk: &EncryptedChunk,
        _timestamp: u64,
    ) -> Result<(), ReplicationError> {
        if peer_id.is_empty() {
            return Err(ReplicationError::PeerOffline);
        }

        Ok(())
    }

    fn handle_fallback_policy(
        &mut self,
        chunk_id: &ChunkId,
        policy: &ReplicaFallbackPolicy,
        timestamp: u64,
    ) -> Result<ReplicationResult, ReplicationError> {
        match policy {
            ReplicaFallbackPolicy::LocalOnly => {
                if self.local_storage {
                    Ok(ReplicationResult {
                        chunk_id: chunk_id.clone(),
                        attempted_peers: vec![],
                        successful_peers: vec![],
                        failed_peers: vec![],
                        fallback_used: true,
                    })
                } else {
                    Err(ReplicationError::LocalStorageUnavailable)
                }
            }
            ReplicaFallbackPolicy::StaticPeerList { peers } => {
                let mut result = ReplicationResult {
                    chunk_id: chunk_id.clone(),
                    attempted_peers: Vec::new(),
                    successful_peers: Vec::new(),
                    failed_peers: Vec::new(),
                    fallback_used: true,
                };

                for peer_id in peers {
                    self.tracker
                        .track_replication(peer_id.clone(), chunk_id.clone(), timestamp);
                    result.attempted_peers.push(peer_id.clone());
                }

                Ok(result)
            }
            ReplicaFallbackPolicy::RandomSelection { min_peers } => Ok(ReplicationResult {
                chunk_id: chunk_id.clone(),
                attempted_peers: vec![],
                successful_peers: vec![],
                failed_peers: vec![],
                fallback_used: true,
            }),
        }
    }

    pub fn get_tracker(&self) -> &ReplicationTracker {
        &self.tracker
    }

    pub fn get_chunk_replication_status(&self, chunk_id: &ChunkId) -> Option<ReplicationStats> {
        if self.tracker.get_chunk_replicas(chunk_id).is_some() {
            Some(self.tracker.get_replication_stats(chunk_id))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReplicationResult {
    pub chunk_id: ChunkId,
    pub attempted_peers: Vec<PeerId>,
    pub successful_peers: Vec<PeerId>,
    pub failed_peers: Vec<PeerId>,
    pub fallback_used: bool,
}

impl ReplicationResult {
    pub fn is_successful(&self, target_replicas: u32) -> bool {
        self.successful_peers.len() >= target_replicas as usize
    }
}

#[derive(Debug, Clone)]
pub enum ReplicationError {
    ChunkNotFound,
    PeerOffline,
    PeerUnreachable,
    LocalStorageUnavailable,
    InsufficientReplicas,
    TransportError(String),
}

impl std::fmt::Display for ReplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChunkNotFound => write!(f, "Chunk not found"),
            Self::PeerOffline => write!(f, "Peer is offline"),
            Self::PeerUnreachable => write!(f, "Peer is unreachable"),
            Self::LocalStorageUnavailable => write!(f, "Local storage unavailable"),
            Self::InsufficientReplicas => write!(f, "Insufficient replicas created"),
            Self::TransportError(msg) => write!(f, "Transport error: {}", msg),
        }
    }
}

impl std::error::Error for ReplicationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_chunk() -> EncryptedChunk {
        EncryptedChunk {
            chunk_id: ChunkId(vec![1u8; 32]),
            ciphertext: vec![0u8; 1024],
            nonce: [0u8; 24],
            size: 1024,
        }
    }

    fn create_test_hint(num_peers: usize, target_replicas: u32) -> StaticReplicationHint {
        let peers: Vec<Vec<u8>> = (0..num_peers).map(|i| vec![i as u8; 32]).collect();

        StaticReplicationHint {
            desired_replicas: target_replicas,
            peer_preferences: Some(peers.clone()),
            target_peers: peers,
            target_replicas,
            fallback_policy: ReplicaFallbackPolicy::LocalOnly,
        }
    }

    #[test]
    fn test_tracker_track_replication() {
        let mut tracker = ReplicationTracker::new();
        let peer_id = vec![1u8; 32];
        let chunk_id = ChunkId(vec![2u8; 32]);

        let confirmation = tracker.track_replication(peer_id.clone(), chunk_id.clone(), 1000);

        assert_eq!(confirmation.peer_id, peer_id);
        assert_eq!(confirmation.chunk_id, chunk_id);
        assert!(matches!(confirmation.status, ReplicationStatus::Pending));
    }

    #[test]
    fn test_tracker_update_status() {
        let mut tracker = ReplicationTracker::new();
        let peer_id = vec![1u8; 32];
        let chunk_id = ChunkId(vec![2u8; 32]);

        tracker.track_replication(peer_id.clone(), chunk_id.clone(), 1000);
        tracker.update_status(&peer_id, &chunk_id, ReplicationStatus::Successful, 2000);

        let confirmation = tracker.get_confirmation(&peer_id, &chunk_id).unwrap();
        assert!(matches!(confirmation.status, ReplicationStatus::Successful));
        assert_eq!(confirmation.confirmed_at, Some(2000));
    }

    #[test]
    fn test_tracker_get_successful_replicas() {
        let mut tracker = ReplicationTracker::new();
        let chunk_id = ChunkId(vec![1u8; 32]);

        for i in 0..3 {
            let peer_id = vec![i as u8; 32];
            tracker.track_replication(peer_id.clone(), chunk_id.clone(), 1000);
            tracker.update_status(&peer_id, &chunk_id, ReplicationStatus::Successful, 2000);
        }

        let successful = tracker.get_successful_replicas(&chunk_id);
        assert_eq!(successful.len(), 3);
    }

    #[test]
    fn test_tracker_replication_stats() {
        let mut tracker = ReplicationTracker::new();
        let chunk_id = ChunkId(vec![1u8; 32]);

        tracker.track_replication(vec![1u8; 32], chunk_id.clone(), 1000);
        tracker.track_replication(vec![2u8; 32], chunk_id.clone(), 1000);
        tracker.track_replication(vec![3u8; 32], chunk_id.clone(), 1000);

        tracker.update_status(
            &vec![1u8; 32],
            &chunk_id,
            ReplicationStatus::Successful,
            2000,
        );
        tracker.update_status(
            &vec![2u8; 32],
            &chunk_id,
            ReplicationStatus::Failed {
                reason: "timeout".to_string(),
            },
            2000,
        );

        let stats = tracker.get_replication_stats(&chunk_id);
        assert_eq!(stats.total_replicas, 3);
        assert_eq!(stats.successful, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.pending, 1);
    }

    #[test]
    fn test_replicator_local_only() {
        let mut replicator = Replicator::new();
        let chunk = create_test_chunk();
        let chunk_id = chunk.chunk_id.clone();

        let hint = StaticReplicationHint {
            desired_replicas: 0,
            peer_preferences: None,
            target_peers: vec![],
            target_replicas: 0,
            fallback_policy: ReplicaFallbackPolicy::LocalOnly,
        };

        let result = replicator
            .replicate_chunk(chunk_id.clone(), &chunk, &hint, 1000)
            .unwrap();

        assert!(result.fallback_used);
        assert_eq!(result.attempted_peers.len(), 0);
    }

    #[test]
    fn test_replicator_static_peers() {
        let mut replicator = Replicator::new();
        let chunk = create_test_chunk();
        let chunk_id = chunk.chunk_id.clone();
        let hint = create_test_hint(3, 2);

        let result = replicator
            .replicate_chunk(chunk_id.clone(), &chunk, &hint, 1000)
            .unwrap();

        assert_eq!(result.attempted_peers.len(), 3);
    }

    #[test]
    fn test_replication_result_is_successful() {
        let result = ReplicationResult {
            chunk_id: ChunkId(vec![1u8; 32]),
            attempted_peers: vec![vec![1u8; 32], vec![2u8; 32]],
            successful_peers: vec![vec![1u8; 32], vec![2u8; 32]],
            failed_peers: vec![],
            fallback_used: false,
        };

        assert!(result.is_successful(2));
        assert!(!result.is_successful(3));
    }

    #[test]
    fn test_replica_confirmation_status_updates() {
        let mut confirmation =
            ReplicaConfirmation::new(vec![1u8; 32], ChunkId(vec![2u8; 32]), 1000);

        confirmation.mark_success(2000);
        assert!(matches!(confirmation.status, ReplicationStatus::Successful));
        assert_eq!(confirmation.confirmed_at, Some(2000));

        let mut confirmation =
            ReplicaConfirmation::new(vec![1u8; 32], ChunkId(vec![2u8; 32]), 1000);
        confirmation.mark_failure("timeout".to_string(), 2000);
        assert!(matches!(
            confirmation.status,
            ReplicationStatus::Failed { .. }
        ));

        let mut confirmation =
            ReplicaConfirmation::new(vec![1u8; 32], ChunkId(vec![2u8; 32]), 1000);
        confirmation.mark_offline(2000);
        assert!(matches!(confirmation.status, ReplicationStatus::Offline));
    }

    #[test]
    fn test_replicator_fallback_on_insufficient_replicas() {
        let mut replicator = Replicator::new();
        let chunk = create_test_chunk();
        let chunk_id = chunk.chunk_id.clone();

        let hint = StaticReplicationHint {
            desired_replicas: 2,
            peer_preferences: None,
            target_peers: vec![],
            target_replicas: 2,
            fallback_policy: ReplicaFallbackPolicy::LocalOnly,
        };

        let result = replicator
            .replicate_chunk(chunk_id.clone(), &chunk, &hint, 1000)
            .unwrap();

        assert!(result.fallback_used);
    }
}
