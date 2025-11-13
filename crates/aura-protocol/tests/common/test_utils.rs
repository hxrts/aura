//! Test utilities for protocol testing
//!
//! This module provides test-specific implementations and utilities
//! that should not be used in production code.

use async_trait::async_trait;
use aura_core::effects::*;
use aura_core::hash;
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
            return Err(NetworkError::SendFailed {
                peer_id: Some(peer_id),
                reason: format!("Peer not connected: {}", peer_id),
            });
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
impl aura_core::RandomEffects for MockCrypto {
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

    async fn random_u64(&self) -> u64 {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        *counter
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        min + (max - min) / 2
    }
}

#[async_trait]
impl aura_core::CryptoEffects for MockCrypto {
    async fn hash(&self, data: &[u8]) -> [u8; 32] {
        hash::hash(data)
    }

    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32] {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
        mac.update(data);
        mac.finalize().into_bytes().into()
    }

    async fn hkdf_derive(
        &self,
        _ikm: &[u8],
        _salt: &[u8],
        _info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; output_len])
    }

    async fn derive_key(
        &self,
        _master_key: &[u8],
        _context: &aura_core::effects::crypto::KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 32])
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let seed = self.random_bytes_32().await;
        let private_key = seed.to_vec();
        let public_key = self.ed25519_public_key(&private_key).await?;
        Ok((private_key, public_key))
    }

    async fn ed25519_sign(
        &self,
        _message: &[u8],
        _private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 64])
    }

    async fn ed25519_verify(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        Ok(true)
    }

    async fn ed25519_public_key(&self, _private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 32])
    }

    async fn frost_generate_keys(
        &self,
        _threshold: u16,
        max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        Ok((0..max_signers).map(|_| vec![0u8; 32]).collect())
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 64])
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        _nonces: &[Vec<u8>],
        participants: &[u16],
    ) -> Result<aura_core::effects::crypto::FrostSigningPackage, CryptoError> {
        Ok(aura_core::effects::crypto::FrostSigningPackage {
            message: message.to_vec(),
            package: vec![0u8; 64],
            participants: participants.to_vec(),
        })
    }

    async fn frost_sign_share(
        &self,
        _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        _key_share: &[u8],
        _nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 64])
    }

    async fn frost_aggregate_signatures(
        &self,
        _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        _signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 64])
    }

    async fn frost_verify(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        Ok(true)
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(plaintext.to_vec())
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(ciphertext.to_vec())
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(plaintext.to_vec())
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(ciphertext.to_vec())
    }

    async fn frost_rotate_keys(
        &self,
        _old_shares: &[Vec<u8>],
        _old_threshold: u16,
        _new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        Ok((0..new_max_signers).map(|_| vec![0u8; 32]).collect())
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec!["ed25519".to_string(), "frost".to_string()]
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
        let pk = crypto.ed25519_public_key(&sk).await.unwrap();
        assert_eq!(vk, pk);
    }
}
