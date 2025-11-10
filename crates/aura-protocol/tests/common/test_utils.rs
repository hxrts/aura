//! Test utilities for protocol testing
//!
//! This module provides test-specific implementations and utilities
//! that should not be used in production code.

use async_trait::async_trait;
use aura_protocol::effects::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Log levels for testing
#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Simple in-memory network transport for testing
#[derive(Debug, Default, Clone)]
pub struct MockNetworkTransport {
    messages: Arc<Mutex<Vec<(Uuid, Vec<u8>)>>>,
    connected_peers: Arc<Mutex<Vec<Uuid>>>,
}

impl MockNetworkTransport {
    /// Create a new memory transport
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all messages sent through this transport
    pub fn get_messages(&self) -> Vec<(Uuid, Vec<u8>)> {
        self.messages.lock().unwrap().clone()
    }

    /// Clear all messages from this transport
    pub fn clear_messages(&self) {
        self.messages.lock().unwrap().clear()
    }

    /// Add a connected peer
    pub fn add_peer(&self, peer_id: Uuid) {
        self.connected_peers.lock().unwrap().push(peer_id);
    }

    /// Remove a connected peer
    pub fn remove_peer(&self, peer_id: Uuid) {
        self.connected_peers
            .lock()
            .unwrap()
            .retain(|&id| id != peer_id);
    }
}

#[async_trait]
impl NetworkEffects for MockNetworkTransport {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        if !self.connected_peers.lock().unwrap().contains(&peer_id) {
            return Err(NetworkError::SendFailed(format!(
                "Peer not connected: {}",
                peer_id
            )));
        }

        self.messages.lock().unwrap().push((peer_id, message));
        Ok(())
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let broadcast_id = Uuid::from_u128(0); // Special ID for broadcasts
        self.messages.lock().unwrap().push((broadcast_id, message));
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        let messages = self.messages.lock().unwrap();
        if let Some((peer_id, message)) = messages.first() {
            Ok((*peer_id, message.clone()))
        } else {
            Err(NetworkError::NoMessage)
        }
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        let messages = self.messages.lock().unwrap();
        for (id, message) in messages.iter() {
            if *id == peer_id {
                return Ok(message.clone());
            }
        }
        Err(NetworkError::NoMessage)
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        self.connected_peers.lock().unwrap().clone()
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        self.connected_peers.lock().unwrap().contains(&peer_id)
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        // Return an empty stream for testing
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }
}

/// Simple in-memory storage for testing
#[derive(Debug, Default, Clone)]
pub struct MockStorage {
    data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_data(data: HashMap<String, Vec<u8>>) -> Self {
        Self {
            data: Arc::new(Mutex::new(data)),
        }
    }

    pub fn get_all_data(&self) -> HashMap<String, Vec<u8>> {
        self.data.lock().unwrap().clone()
    }
}

#[async_trait]
impl StorageEffects for MockStorage {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.data.lock().unwrap().insert(key.to_string(), value);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.data.lock().unwrap().get(key).cloned())
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.data.lock().unwrap().remove(key).is_some())
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let data = self.data.lock().unwrap();
        let keys: Vec<String> = if let Some(prefix) = prefix {
            data.keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect()
        } else {
            data.keys().cloned().collect()
        };
        Ok(keys)
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.data.lock().unwrap().contains_key(key))
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        let mut data = self.data.lock().unwrap();
        for (key, value) in pairs {
            data.insert(key, value);
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        let data = self.data.lock().unwrap();
        let mut result = HashMap::new();
        for key in keys {
            if let Some(value) = data.get(key) {
                result.insert(key.clone(), value.clone());
            }
        }
        Ok(result)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        self.data.lock().unwrap().clear();
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let data = self.data.lock().unwrap();
        Ok(StorageStats {
            key_count: data.len() as u64,
            total_size: data.values().map(|v| v.len() as u64).sum(),
            available_space: None,
            backend_type: "mock".to_string(),
        })
    }
}

/// Mock crypto implementation for testing
#[derive(Debug, Default)]
pub struct MockCrypto {
    counter: Arc<Mutex<u64>>,
}

impl MockCrypto {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl CryptoEffects for MockCrypto {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        // Deterministic for testing
        (0..len).map(|i| (i % 256) as u8).collect()
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&counter.to_le_bytes());
        bytes
    }

    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        range.start + (range.end - range.start) / 2
    }

    async fn hash(&self, data: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32] {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
        mac.update(data);
        mac.finalize().into_bytes().into()
    }

    async fn ed25519_sign(
        &self,
        data: &[u8],
        key: &ed25519_dalek::SigningKey,
    ) -> Result<ed25519_dalek::Signature, CryptoError> {
        use ed25519_dalek::Signer;
        Ok(key.sign(data))
    }

    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &ed25519_dalek::Signature,
        public_key: &ed25519_dalek::VerifyingKey,
    ) -> Result<bool, CryptoError> {
        match public_key.verify_strict(data, signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn ed25519_generate_keypair(
        &self,
    ) -> Result<(ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey), CryptoError> {
        // Use deterministic key generation for testing
        let seed = self.random_bytes_32().await;
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        Ok((signing_key, verifying_key))
    }

    async fn ed25519_public_key(
        &self,
        private_key: &ed25519_dalek::SigningKey,
    ) -> ed25519_dalek::VerifyingKey {
        private_key.verifying_key()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        if a.len() != b.len() {
            return false;
        }
        a.ct_eq(b).into()
    }

    fn secure_zero(&self, data: &mut [u8]) {
        use zeroize::Zeroize;
        data.zeroize();
    }
}

/// Generate a deterministic test UUID for non-production use
pub fn generate_test_uuid() -> Uuid {
    Uuid::from_bytes([
        0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd,
        0xef,
    ])
}

/// Generate deterministic test UUIDs with different seeds
pub fn generate_test_uuid_with_seed(seed: u8) -> Uuid {
    Uuid::from_bytes([
        seed, seed, seed, seed, seed, seed, seed, seed, seed, seed, seed, seed, seed, seed, seed,
        seed,
    ])
}

/// Create test keypair for deterministic testing
pub fn create_test_keypair() -> (ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey) {
    let seed = [0x42u8; 32];
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    (signing_key, verifying_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_network_transport() {
        let transport = MockNetworkTransport::new();
        let peer_id = generate_test_uuid();

        // Add peer and test connectivity
        transport.add_peer(peer_id);
        assert!(transport.is_peer_connected(peer_id).await);
        assert_eq!(transport.connected_peers().await, vec![peer_id]);

        // Test send
        transport
            .send_to_peer(peer_id, b"hello".to_vec())
            .await
            .unwrap();
        let messages = transport.get_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, peer_id);
        assert_eq!(messages[0].1, b"hello");

        // Test broadcast
        transport.broadcast(b"world".to_vec()).await.unwrap();
        let messages = transport.get_messages();
        assert_eq!(messages.len(), 2);

        // Test clear
        transport.clear_messages();
        assert!(transport.get_messages().is_empty());
    }

    #[tokio::test]
    async fn test_mock_storage() {
        let storage = MockStorage::new();

        // Test store and retrieve
        storage.store("key1", b"value1".to_vec()).await.unwrap();
        let value = storage.retrieve("key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Test exists
        assert!(storage.exists("key1").await.unwrap());
        assert!(!storage.exists("key2").await.unwrap());

        // Test list keys
        storage
            .store("prefix_key", b"value".to_vec())
            .await
            .unwrap();
        let keys = storage.list_keys(Some("prefix")).await.unwrap();
        assert_eq!(keys, vec!["prefix_key"]);

        // Test remove
        assert!(storage.remove("key1").await.unwrap());
        assert!(!storage.exists("key1").await.unwrap());
    }

    #[tokio::test]
    async fn test_mock_crypto() {
        let crypto = MockCrypto::new();

        // Test random bytes
        let bytes1 = crypto.random_bytes(10).await;
        let bytes2 = crypto.random_bytes(10).await;
        assert_eq!(bytes1.len(), 10);
        assert_eq!(bytes1, bytes2); // Deterministic

        // Test hash
        let data = b"test data";
        let hash = crypto.hash(data).await;
        assert_eq!(hash.len(), 32);

        // Test keypair generation
        let (sk, vk) = crypto.ed25519_generate_keypair().await.unwrap();
        let pk = crypto.ed25519_public_key(&sk).await;
        assert_eq!(vk, pk);
    }
}
