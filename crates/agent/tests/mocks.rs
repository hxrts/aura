//! Mock implementations for testing UnifiedAgent
//!
//! This module provides mock Transport and Storage implementations
//! that can be used in tests without requiring real network or storage backends.

use async_trait::async_trait;
use aura_agent::{Result, Storage, StorageStats, Transport};
use aura_types::{AccountId, DeviceId};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Mock transport implementation for testing
#[derive(Debug)]
pub struct MockTransport {
    device_id: DeviceId,
    connected_peers: Arc<RwLock<Vec<DeviceId>>>,
    message_queue: Arc<RwLock<Vec<(DeviceId, Vec<u8>)>>>,
}

impl MockTransport {
    /// Create a new mock transport for the given device
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            connected_peers: Arc::new(RwLock::new(Vec::new())),
            message_queue: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Transport for MockTransport {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    async fn send_message(&self, peer_id: DeviceId, message: &[u8]) -> Result<()> {
        // In a real implementation, this would send over the network
        tracing::debug!(
            "Mock sending message to {}: {} bytes",
            peer_id,
            message.len()
        );
        Ok(())
    }

    async fn receive_messages(&self) -> Result<Vec<(DeviceId, Vec<u8>)>> {
        let mut queue = self.message_queue.write().await;
        let messages = queue.drain(..).collect();
        Ok(messages)
    }

    async fn connect(&self, peer_id: DeviceId) -> Result<()> {
        let mut peers = self.connected_peers.write().await;
        if !peers.contains(&peer_id) {
            peers.push(peer_id);
        }
        tracing::debug!("Mock connected to peer {}", peer_id);
        Ok(())
    }

    async fn disconnect(&self, peer_id: DeviceId) -> Result<()> {
        let mut peers = self.connected_peers.write().await;
        peers.retain(|&id| id != peer_id);
        tracing::debug!("Mock disconnected from peer {}", peer_id);
        Ok(())
    }

    async fn connected_peers(&self) -> Result<Vec<DeviceId>> {
        let peers = self.connected_peers.read().await;
        Ok(peers.clone())
    }

    async fn is_connected(&self, peer_id: DeviceId) -> Result<bool> {
        let peers = self.connected_peers.read().await;
        Ok(peers.contains(&peer_id))
    }
}

/// Mock storage implementation for testing
#[derive(Debug)]
pub struct MockStorage {
    account_id: AccountId,
    data: Arc<RwLock<std::collections::HashMap<String, Vec<u8>>>>,
}

impl MockStorage {
    /// Create a new mock storage for the given account
    pub fn new(account_id: AccountId) -> Self {
        Self {
            account_id,
            data: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }
}

#[async_trait]
impl Storage for MockStorage {
    fn account_id(&self) -> AccountId {
        self.account_id
    }

    async fn store(&self, key: &str, data: &[u8]) -> Result<()> {
        let mut storage = self.data.write().await;
        storage.insert(key.to_string(), data.to_vec());
        tracing::debug!("Mock stored {} bytes at key '{}'", data.len(), key);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let storage = self.data.read().await;
        let result = storage.get(key).cloned();
        tracing::debug!(
            "Mock retrieved {} from key '{}'",
            result.as_ref().map(|d| d.len()).unwrap_or(0),
            key
        );
        Ok(result)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut storage = self.data.write().await;
        storage.remove(key);
        tracing::debug!("Mock deleted key '{}'", key);
        Ok(())
    }

    async fn list_keys(&self) -> Result<Vec<String>> {
        let storage = self.data.read().await;
        Ok(storage.keys().cloned().collect())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let storage = self.data.read().await;
        Ok(storage.contains_key(key))
    }

    async fn stats(&self) -> Result<StorageStats> {
        let storage = self.data.read().await;
        let total_keys = storage.len() as u64;
        let total_size_bytes = storage.values().map(|v| v.len() as u64).sum();

        Ok(StorageStats {
            total_keys,
            total_size_bytes,
            available_space_bytes: Some(1_000_000_000), // 1GB mock limit
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_transport_mock() {
        let device_id1 = DeviceId(Uuid::new_v4());
        let device_id2 = DeviceId(Uuid::new_v4());
        let transport = MockTransport::new(device_id1);

        // Test device ID
        assert_eq!(transport.device_id(), device_id1);

        // Test connection management
        assert!(!transport.is_connected(device_id2).await.unwrap());
        transport.connect(device_id2).await.unwrap();
        assert!(transport.is_connected(device_id2).await.unwrap());

        let peers = transport.connected_peers().await.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0], device_id2);

        // Test disconnection
        transport.disconnect(device_id2).await.unwrap();
        assert!(!transport.is_connected(device_id2).await.unwrap());

        // Test messaging
        let message = b"test message";
        transport.send_message(device_id2, message).await.unwrap();

        let messages = transport.receive_messages().await.unwrap();
        assert_eq!(messages.len(), 0); // No messages in mock queue
    }

    #[tokio::test]
    async fn test_storage_mock() {
        let account_id = AccountId::new(Uuid::new_v4());
        let storage = MockStorage::new(account_id);

        // Test account ID
        assert_eq!(storage.account_id(), account_id);

        // Test storage operations
        let key = "test_key";
        let data = b"test data";

        // Initially empty
        assert!(!storage.exists(key).await.unwrap());
        assert_eq!(storage.retrieve(key).await.unwrap(), None);

        // Store data
        storage.store(key, data).await.unwrap();
        assert!(storage.exists(key).await.unwrap());

        // Retrieve data
        let retrieved = storage.retrieve(key).await.unwrap().unwrap();
        assert_eq!(retrieved, data);

        // List keys
        let keys = storage.list_keys().await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], key);

        // Get stats
        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.total_keys, 1);
        assert_eq!(stats.total_size_bytes, data.len() as u64);

        // Delete data
        storage.delete(key).await.unwrap();
        assert!(!storage.exists(key).await.unwrap());
        assert_eq!(storage.retrieve(key).await.unwrap(), None);
    }
}
