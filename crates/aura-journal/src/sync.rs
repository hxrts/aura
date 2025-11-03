//! Journal synchronization interface
//!
//! This module provides a simple interface for journal synchronization
//! that delegates to the choreographic protocol implementation in aura-protocol.
//! The actual sync coordination is handled by choreographic protocols that
//! provide Byzantine fault tolerance, privacy preservation, and session type safety.

use crate::error::Result;
use crate::state::AccountState;
use aura_types::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

/// Simple sync message for compatibility with existing code
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncMessage {
    /// Source device sending the sync message
    pub from_device: DeviceId,
    /// Destination device receiving the sync message
    pub to_device: DeviceId,
    /// Serialized Automerge sync message
    pub automerge_message: Vec<u8>,
    /// Current vector clock heads for this device
    pub vector_clock: Vec<automerge::ChangeHash>,
    /// Current epoch number
    pub epoch: u64,
}

/// Simple sync result for compatibility with existing code
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncResult {
    /// Number of changes applied from sync message
    pub changes_applied: usize,
    /// Updated epoch number after sync
    pub new_epoch: u64,
    /// List of conflicts that were automatically resolved
    pub conflicts_resolved: Vec<String>,
}

/// Simple sync manager that delegates to choreographic protocols
pub struct SyncManager {
    /// Device ID for this sync manager
    device_id: DeviceId,

    /// Account state reference
    state: Arc<RwLock<AccountState>>,
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new(device_id: DeviceId, state: Arc<RwLock<AccountState>>) -> Self {
        Self { device_id, state }
    }

    /// Generate a sync message for a peer
    ///
    /// Note: This is a compatibility interface. The actual sync coordination
    /// is handled by choreographic protocols in aura-protocol.
    pub fn generate_sync_message(
        &self,
        peer: DeviceId,
        _account_id: AccountId,
    ) -> Result<SyncMessage> {
        let state = self.state.read().map_err(|_| {
            crate::error::Error::storage_failed("Failed to acquire read lock on account state")
        })?;

        // Simple implementation for compatibility
        // In production, this would delegate to the choreographic protocol
        Ok(SyncMessage {
            from_device: self.device_id,
            to_device: peer,
            automerge_message: vec![], // Placeholder
            vector_clock: state.get_heads(),
            epoch: state.get_epoch(),
        })
    }

    /// Receive and process a sync message
    ///
    /// Note: This is a compatibility interface. The actual sync coordination
    /// is handled by choreographic protocols in aura-protocol.
    pub fn receive_sync_message(
        &self,
        msg: SyncMessage,
        _account_id: AccountId,
    ) -> Result<SyncResult> {
        // Simple implementation for compatibility
        // In production, this would delegate to the choreographic protocol
        Ok(SyncResult {
            changes_applied: 0,
            new_epoch: msg.epoch,
            conflicts_resolved: vec![], // Automerge resolves conflicts automatically
        })
    }

    /// Generate a state snapshot
    pub fn generate_snapshot(&self, _account_id: AccountId) -> Result<Vec<u8>> {
        let state = self.state.read().map_err(|_| {
            crate::error::Error::storage_failed("Failed to acquire read lock on account state")
        })?;

        state.save().map_err(|e| {
            crate::error::Error::storage_failed(format!("Failed to generate snapshot: {}", e))
        })
    }

    /// Load state from a snapshot
    pub fn load_snapshot(
        &self,
        snapshot: &[u8],
        account_id: AccountId,
        group_public_key: &[u8],
    ) -> Result<()> {
        // Deserialize the public key
        let key_bytes: [u8; 32] = group_public_key.try_into().map_err(|_| {
            crate::error::Error::invalid_operation(format!(
                "Invalid group public key length: expected 32 bytes, got {}",
                group_public_key.len()
            ))
        })?;
        let public_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&key_bytes).map_err(|e| {
            crate::error::Error::invalid_operation(format!("Invalid group public key: {}", e))
        })?;

        // Load new state from snapshot
        let new_state = AccountState::load(snapshot, account_id, public_key).map_err(|e| {
            crate::error::Error::storage_failed(format!("Failed to load snapshot: {}", e))
        })?;

        // Replace current state
        let mut state = self.state.write().map_err(|_| {
            crate::error::Error::storage_failed("Failed to acquire write lock on account state")
        })?;

        *state = new_state;
        Ok(())
    }

    /// Check if sync is needed with a peer
    pub fn needs_sync_with(&self, _peer: DeviceId, _account_id: AccountId) -> Result<bool> {
        let state = self.state.read().map_err(|_| {
            crate::error::Error::storage_failed("Failed to acquire read lock on account state")
        })?;

        // For now, always indicate sync is needed unless we have no changes
        let has_changes = !state.get_heads().is_empty();
        Ok(has_changes)
    }

    /// Reset sync state for a peer (no-op in choreographic approach)
    pub fn reset_sync_state(&self, _peer: DeviceId, _account_id: AccountId) -> Result<()> {
        // No-op in choreographic approach - state is managed by the protocol
        Ok(())
    }
}

/// Multi-device sync orchestrator (simplified for compatibility)
pub struct SyncOrchestrator {
    /// Underlying sync manager for message generation and processing
    sync_manager: SyncManager,
    /// List of peer devices to sync with
    peers: Arc<RwLock<Vec<DeviceId>>>,
    /// Account ID for this sync orchestrator
    account_id: AccountId,
}

impl SyncOrchestrator {
    /// Create a new sync orchestrator
    pub fn new(
        device_id: DeviceId,
        state: Arc<RwLock<AccountState>>,
        account_id: AccountId,
    ) -> Self {
        Self {
            sync_manager: SyncManager::new(device_id, state),
            peers: Arc::new(RwLock::new(Vec::new())),
            account_id,
        }
    }

    /// Add a peer to sync with
    pub fn add_peer(&self, peer: DeviceId) {
        let mut peers = self.peers.write().unwrap();
        if !peers.contains(&peer) {
            peers.push(peer);
        }
    }

    /// Remove a peer
    pub fn remove_peer(&self, peer: DeviceId) -> Result<()> {
        let mut peers = self.peers.write().unwrap();
        peers.retain(|p| p != &peer);
        self.sync_manager.reset_sync_state(peer, self.account_id)
    }

    /// Check sync status with all peers
    pub fn check_sync_status(&self) -> Result<HashMap<DeviceId, bool>> {
        let peers = self.peers.read().unwrap().clone();
        let mut results = HashMap::new();

        for peer in peers {
            let needs_sync = self.sync_manager.needs_sync_with(peer, self.account_id)?;
            results.insert(peer, needs_sync);
        }

        Ok(results)
    }

    /// Generate sync messages for all peers that need sync
    pub fn generate_sync_messages(&self) -> Result<Vec<(DeviceId, SyncMessage)>> {
        let peers = self.peers.read().unwrap().clone();
        let mut messages = Vec::new();

        for peer in peers {
            if self.sync_manager.needs_sync_with(peer, self.account_id)? {
                let message = self
                    .sync_manager
                    .generate_sync_message(peer, self.account_id)?;
                messages.push((peer, message));
            }
        }

        Ok(messages)
    }

    /// Process a received sync message
    pub fn receive_sync_message(&self, msg: SyncMessage) -> Result<SyncResult> {
        self.sync_manager.receive_sync_message(msg, self.account_id)
    }

    /// Get access to the sync manager for advanced operations
    pub fn sync_manager(&self) -> &SyncManager {
        &self.sync_manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_sync_manager_basic() {
        let effects = Effects::test(42);
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        let peer_id = DeviceId::new_with_effects(&effects);

        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();

        let state = Arc::new(RwLock::new(
            AccountState::new(account_id, group_public_key).unwrap(),
        ));

        let sync_manager = SyncManager::new(device_id, state);

        // Test sync message generation
        let result = sync_manager.generate_sync_message(peer_id, account_id);
        assert!(result.is_ok());

        let sync_msg = result.unwrap();
        assert_eq!(sync_msg.from_device, device_id);
        assert_eq!(sync_msg.to_device, peer_id);
    }

    #[test]
    fn test_sync_orchestrator() {
        let effects = Effects::test(42);
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        let peer_id = DeviceId::new_with_effects(&effects);

        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();

        let state = Arc::new(RwLock::new(
            AccountState::new(account_id, group_public_key).unwrap(),
        ));

        let orchestrator = SyncOrchestrator::new(device_id, state, account_id);

        // Add peer
        orchestrator.add_peer(peer_id);

        // Check sync status
        let status = orchestrator.check_sync_status().unwrap();
        assert_eq!(status.len(), 1);
        assert!(status.contains_key(&peer_id));
    }
}
