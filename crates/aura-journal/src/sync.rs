//! Sync protocol for distributed ledger

use crate::AccountState;
use crate::error::{Error, Result};
use automerge::sync::{Message, State as SyncState, SyncDoc};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Sync message between devices
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncMessage {
    pub from_device: DeviceId,
    pub to_device: DeviceId,
    pub automerge_message: Vec<u8>, // Serialized AutomergeSyncMessage
    pub vector_clock: Vec<automerge::ChangeHash>,
    pub epoch: u64,
}

/// Result of a sync operation
#[derive(Clone, Debug)]
pub struct SyncResult {
    pub changes_applied: usize,
    pub new_epoch: u64,
    pub conflicts_resolved: Vec<String>,
}

/// Automerge sync protocol handler
pub struct SyncProtocol {
    device_id: DeviceId,
    local_state: Arc<RwLock<AccountState>>,
    sync_states: Arc<RwLock<HashMap<DeviceId, SyncState>>>,
}

impl SyncProtocol {
    /// Create a new sync protocol handler
    pub fn new(device_id: DeviceId, local_state: Arc<RwLock<AccountState>>) -> Self {
        Self {
            device_id,
            local_state,
            sync_states: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Generate a sync message for a peer
    pub async fn generate_sync_message(&self, peer: DeviceId) -> Result<SyncMessage> {
        let state = self.local_state.read().await;
        let mut sync_states = self.sync_states.write().await;
        
        // Get or create sync state for this peer
        let sync_state = sync_states.entry(peer)
            .or_insert_with(SyncState::new);
        
        // Generate Automerge sync message
        let doc = state.automerge_doc();
        let automerge_msg = doc.generate_sync_message(sync_state);
        
        // Serialize the message
        let serialized = match automerge_msg {
            Some(msg) => msg.encode(),
            None => vec![], // No changes to sync
        };
        
        Ok(SyncMessage {
            from_device: self.device_id,
            to_device: peer,
            automerge_message: serialized,
            vector_clock: state.get_heads(),
            epoch: state.get_epoch(),
        })
    }
    
    /// Receive and process a sync message
    pub async fn receive_sync_message(&self, msg: SyncMessage) -> Result<SyncResult> {
        // Validate message is for us
        if msg.to_device != self.device_id {
            return Err(Error::coordination_failed(
                format!("Sync message not for this device: {:?} != {:?}", msg.to_device, self.device_id)
            ));
        }
        
        let mut state = self.local_state.write().await;
        let mut sync_states = self.sync_states.write().await;
        
        // Get or create sync state for sender
        let sync_state = sync_states.entry(msg.from_device)
            .or_insert_with(SyncState::new);
        
        // Decode Automerge sync message
        let automerge_msg = if !msg.automerge_message.is_empty() {
            Some(Message::decode(&msg.automerge_message)
                .map_err(|e| Error::storage_failed(format!("Failed to decode sync message: {}", e)))?)
        } else {
            None
        };
        
        // Count changes before sync
        let doc_before = state.automerge_doc();
        let changes_before = doc_before.get_changes(&[]).len();
        
        // Apply sync message  
        if let Some(sync_msg) = automerge_msg {
            // Apply sync message to state
            let mut doc = state.automerge_doc();
            doc.receive_sync_message(sync_state, sync_msg)
                .map_err(|e| Error::storage_failed(format!("Failed to receive sync message: {}", e)))?;
            // Apply changes back to state
            let heads_before = state.get_heads();
            let changes = doc.get_changes(&heads_before);
            state.apply_changes(changes.into_iter().cloned().collect())?;
        }
        
        // Update epoch if remote is higher (Max-Counter CRDT)
        if msg.epoch > state.get_epoch() {
            state.set_epoch_if_higher(msg.epoch)?;
        }
        
        // Count changes after sync
        let doc_after = state.automerge_doc();
        let changes_after = doc_after.get_changes(&[]).len();
        
        let changes_applied = changes_after.saturating_sub(changes_before);
        
        Ok(SyncResult {
            changes_applied,
            new_epoch: state.get_epoch(),
            conflicts_resolved: vec![], // Automerge resolves conflicts automatically
        })
    }
    
    /// Get sync state for a peer (for debugging/monitoring)
    pub async fn get_sync_state(&self, peer: DeviceId) -> Option<String> {
        let sync_states = self.sync_states.read().await;
        sync_states.get(&peer).map(|state| format!("{:?}", state))
    }
    
    /// Reset sync state for a peer (useful after network issues)
    pub async fn reset_sync_state(&self, peer: DeviceId) {
        let mut sync_states = self.sync_states.write().await;
        sync_states.remove(&peer);
    }
    
    /// Generate a full state snapshot for initial sync
    pub async fn generate_snapshot(&self) -> Result<Vec<u8>> {
        let state = self.local_state.read().await;
        state.save()
    }
    
    /// Load from a snapshot
    pub async fn load_snapshot(
        &self,
        snapshot: &[u8],
        account_id: aura_types::AccountId,
        group_public_key: aura_crypto::Ed25519VerifyingKey,
    ) -> Result<()> {
        let new_state = AccountState::load(snapshot, account_id, group_public_key)?;
        let mut state = self.local_state.write().await;
        *state = new_state;
        
        // Clear sync states as we have a new document
        self.sync_states.write().await.clear();
        
        Ok(())
    }
    
    /// Check if we need to sync with a peer
    pub async fn needs_sync_with(&self, peer: DeviceId) -> bool {
        let sync_states = self.sync_states.read().await;
        
        // If we don't have sync state for this peer, we need to sync
        if !sync_states.contains_key(&peer) {
            return true;
        }
        
        // TODO: Check if we have pending changes for this peer
        // For now, always allow sync attempts
        true
    }
}

/// Multi-device sync orchestrator
pub struct SyncOrchestrator {
    protocol: SyncProtocol,
    peers: Arc<RwLock<Vec<DeviceId>>>,
}

impl SyncOrchestrator {
    /// Create a new sync orchestrator
    pub fn new(device_id: DeviceId, local_state: Arc<RwLock<AccountState>>) -> Self {
        Self {
            protocol: SyncProtocol::new(device_id, local_state),
            peers: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    /// Add a peer to sync with
    pub async fn add_peer(&self, peer: DeviceId) {
        let mut peers = self.peers.write().await;
        if !peers.contains(&peer) {
            peers.push(peer);
        }
    }
    
    /// Remove a peer
    pub async fn remove_peer(&self, peer: DeviceId) {
        let mut peers = self.peers.write().await;
        peers.retain(|p| p != &peer);
        self.protocol.reset_sync_state(peer).await;
    }
    
    /// Sync with all known peers
    pub async fn sync_all(&self) -> HashMap<DeviceId, Result<SyncResult>> {
        let peers = self.peers.read().await.clone();
        let mut results = HashMap::new();
        
        for peer in peers {
            if self.protocol.needs_sync_with(peer).await {
                // Generate sync message
                match self.protocol.generate_sync_message(peer).await {
                    Ok(_msg) => {
                        // In a real implementation, send this message over the network
                        // For now, just record that we generated it
                        results.insert(peer, Ok(SyncResult {
                            changes_applied: 0,
                            new_epoch: 0,
                            conflicts_resolved: vec![],
                        }));
                    }
                    Err(e) => {
                        results.insert(peer, Err(e));
                    }
                }
            }
        }
        
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::AccountId;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};
    
    #[tokio::test]
    async fn test_sync_between_devices() {
        let effects = Effects::test(42);
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();
        
        let device1 = DeviceId::new_with_effects(&effects);
        let device2 = DeviceId::new_with_effects(&effects);
        
        // Create two states
        let state1 = Arc::new(RwLock::new(
            AccountState::new(account_id, group_public_key).unwrap()
        ));
        let state2 = Arc::new(RwLock::new(
            AccountState::new(account_id, group_public_key).unwrap()
        ));
        
        // Create sync protocols
        let sync1 = SyncProtocol::new(device1, state1.clone());
        let sync2 = SyncProtocol::new(device2, state2.clone());
        
        // Make changes on device1
        {
            let mut s1 = state1.write().await;
            s1.increment_epoch().unwrap();
        }
        
        // Generate sync message from device1 to device2
        let msg = sync1.generate_sync_message(device2).await.unwrap();
        assert!(!msg.automerge_message.is_empty());
        assert_eq!(msg.epoch, 1);
        
        // Apply sync message on device2
        let result = sync2.receive_sync_message(msg).await.unwrap();
        assert!(result.changes_applied > 0);
        assert_eq!(result.new_epoch, 1);
        
        // Verify states are synchronized
        {
            let s1 = state1.read().await;
            let s2 = state2.read().await;
            assert_eq!(s1.get_epoch(), s2.get_epoch());
            assert_eq!(s1.save().unwrap(), s2.save().unwrap());
        }
    }
    
    #[tokio::test]
    async fn test_bidirectional_sync() {
        let effects = Effects::test(42);
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();
        
        let device1 = DeviceId::new_with_effects(&effects);
        let device2 = DeviceId::new_with_effects(&effects);
        
        // Create two states
        let state1 = Arc::new(RwLock::new(
            AccountState::new(account_id, group_public_key).unwrap()
        ));
        let state2 = Arc::new(RwLock::new(
            AccountState::new(account_id, group_public_key).unwrap()
        ));
        
        // Create sync protocols
        let sync1 = SyncProtocol::new(device1, state1.clone());
        let sync2 = SyncProtocol::new(device2, state2.clone());
        
        // Make different changes on each device
        {
            let mut s1 = state1.write().await;
            s1.increment_epoch().unwrap();
        }
        
        {
            let mut s2 = state2.write().await;
            s2.increment_epoch().unwrap();
            s2.increment_epoch().unwrap(); // Device2 increments twice
        }
        
        // Sync device1 -> device2
        let msg1to2 = sync1.generate_sync_message(device2).await.unwrap();
        sync2.receive_sync_message(msg1to2).await.unwrap();
        
        // Sync device2 -> device1
        let msg2to1 = sync2.generate_sync_message(device1).await.unwrap();
        sync1.receive_sync_message(msg2to1).await.unwrap();
        
        // Both should have epoch 2 (max of both)
        {
            let s1 = state1.read().await;
            let s2 = state2.read().await;
            assert_eq!(s1.get_epoch(), 2);
            assert_eq!(s2.get_epoch(), 2);
        }
    }
}