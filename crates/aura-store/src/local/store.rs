//! Encrypted local store implementation
//!
//! Uses CryptoEffects for all cryptographic operations including:
//! - HKDF key derivation
//! - ChaCha20-Poly1305 encryption/decryption
//! - Nonce generation via RandomEffects
//!
//! Uses StorageEffects for all file I/O operations (no direct std::fs access).

use std::path::Path;

use aura_core::effects::{CryptoEffects, StorageEffects};

use super::errors::LocalStoreError;
use super::types::{LocalData, LocalStoreConfig};

/// Nonce size for ChaCha20-Poly1305 (96 bits = 12 bytes)
const NONCE_SIZE: usize = 12;

/// Key derivation info string
const KEY_INFO: &[u8] = b"aura-local-store-v1";

/// Encrypted local store for CLI/TUI preferences
///
/// Data is encrypted at rest using ChaCha20-Poly1305 with keys derived
/// via HKDF from the authority's cryptographic material.
///
/// All cryptographic operations use `CryptoEffects` for deterministic testing.
pub struct LocalStore {
    /// Configuration including path and salt
    config: LocalStoreConfig,

    /// Key material for deriving encryption key
    key_material: Vec<u8>,

    /// Current data (loaded or default)
    data: LocalData,
}

impl LocalStore {
    /// Create a new local store with the given key material
    ///
    /// The key material should be derived from the authority's secret key
    /// to ensure the store is bound to the identity.
    pub fn new(config: LocalStoreConfig, key_material: &[u8]) -> Self {
        Self {
            config,
            key_material: key_material.to_vec(),
            data: LocalData::default(),
        }
    }

    /// Load existing data from storage using CryptoEffects and StorageEffects
    ///
    /// If the storage key doesn't exist, returns a store with default data.
    pub async fn load<C: CryptoEffects, S: StorageEffects>(
        config: LocalStoreConfig,
        key_material: &[u8],
        crypto: &C,
        storage: &S,
    ) -> Result<Self, LocalStoreError> {
        let storage_key = config.storage_key();
        let data = if storage
            .exists(&storage_key)
            .await
            .map_err(|e| LocalStoreError::StorageError(e.to_string()))?
        {
            load_encrypted(&storage_key, key_material, &config.salt, crypto, storage).await?
        } else {
            LocalData::default()
        };
        Ok(Self {
            config,
            key_material: key_material.to_vec(),
            data,
        })
    }

    /// Save data to storage with fresh nonce
    ///
    /// Uses CryptoEffects for encryption and StorageEffects for persistence.
    pub async fn save<C: CryptoEffects, S: StorageEffects>(
        &self,
        crypto: &C,
        storage: &S,
    ) -> Result<(), LocalStoreError> {
        let storage_key = self.config.storage_key();
        save_encrypted(
            &storage_key,
            &self.key_material,
            &self.config.salt,
            &self.data,
            crypto,
            storage,
        )
        .await
    }

    /// Get a reference to the current data
    pub fn data(&self) -> &LocalData {
        &self.data
    }

    /// Get a mutable reference to the current data
    pub fn data_mut(&mut self) -> &mut LocalData {
        &mut self.data
    }

    /// Update data with a closure
    pub fn update<F>(&mut self, f: F)
    where
        F: FnOnce(&mut LocalData),
    {
        f(&mut self.data);
    }

    /// Get the store path
    pub fn path(&self) -> &Path {
        &self.config.path
    }
}

/// Load and decrypt data from storage using CryptoEffects and StorageEffects
async fn load_encrypted<C: CryptoEffects, S: StorageEffects>(
    storage_key: &str,
    key_material: &[u8],
    salt: &[u8; 32],
    crypto: &C,
    storage: &S,
) -> Result<LocalData, LocalStoreError> {
    let contents = storage
        .retrieve(storage_key)
        .await
        .map_err(|e| LocalStoreError::StorageError(e.to_string()))?
        .ok_or_else(|| LocalStoreError::StorageError("store not found".into()))?;

    // File format: nonce (12 bytes) || ciphertext
    if contents.len() < NONCE_SIZE {
        return Err(LocalStoreError::InvalidFormat(
            "file too short for nonce".into(),
        ));
    }

    let (nonce_bytes, ciphertext) = contents.split_at(NONCE_SIZE);
    let nonce: [u8; NONCE_SIZE] = nonce_bytes
        .try_into()
        .map_err(|_| LocalStoreError::InvalidFormat("invalid nonce length".into()))?;

    // Derive key using CryptoEffects
    let key_vec = crypto
        .hkdf_derive(key_material, salt, KEY_INFO, 32)
        .await
        .map_err(|e| LocalStoreError::KeyDerivationError(e.to_string()))?;

    let key: [u8; 32] = key_vec
        .try_into()
        .map_err(|_| LocalStoreError::KeyDerivationError("invalid key length".into()))?;

    // Decrypt using CryptoEffects
    let plaintext = crypto
        .chacha20_decrypt(ciphertext, &key, &nonce)
        .await
        .map_err(|e| LocalStoreError::DecryptionError(e.to_string()))?;

    let data: LocalData = serde_json::from_slice(&plaintext)?;
    Ok(data)
}

/// Encrypt and save data to storage using CryptoEffects and StorageEffects
async fn save_encrypted<C: CryptoEffects, S: StorageEffects>(
    storage_key: &str,
    key_material: &[u8],
    salt: &[u8; 32],
    data: &LocalData,
    crypto: &C,
    storage: &S,
) -> Result<(), LocalStoreError> {
    // Generate nonce using CryptoEffects (inherits RandomEffects)
    let nonce_vec = crypto.random_bytes(NONCE_SIZE).await;
    let nonce: [u8; NONCE_SIZE] = nonce_vec
        .try_into()
        .map_err(|_| LocalStoreError::EncryptionError("nonce generation failed".into()))?;

    // Derive key using CryptoEffects
    let key_vec = crypto
        .hkdf_derive(key_material, salt, KEY_INFO, 32)
        .await
        .map_err(|e| LocalStoreError::KeyDerivationError(e.to_string()))?;

    let key: [u8; 32] = key_vec
        .try_into()
        .map_err(|_| LocalStoreError::KeyDerivationError("invalid key length".into()))?;

    // Serialize and encrypt
    let plaintext = serde_json::to_vec(data)?;
    let ciphertext = crypto
        .chacha20_encrypt(&plaintext, &key, &nonce)
        .await
        .map_err(|e| LocalStoreError::EncryptionError(e.to_string()))?;

    // Data format: nonce || ciphertext
    let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    output.extend_from_slice(&nonce);
    output.extend_from_slice(&ciphertext);

    // Store using StorageEffects
    storage
        .store(storage_key, output)
        .await
        .map_err(|e| LocalStoreError::StorageError(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::effects::storage::{StorageError, StorageStats};
    use aura_testkit::stateful_effects::MockCryptoHandler;
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Test-only in-memory storage for testing LocalStore
    struct TestStorage {
        data: RwLock<HashMap<String, Vec<u8>>>,
    }

    impl TestStorage {
        fn new() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl StorageEffects for TestStorage {
        async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
            let mut data = self
                .data
                .write()
                .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
            data.insert(key.to_string(), value);
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ReadFailed(e.to_string()))?;
            Ok(data.get(key).cloned())
        }

        async fn remove(&self, key: &str) -> Result<bool, StorageError> {
            let mut data = self
                .data
                .write()
                .map_err(|e| StorageError::DeleteFailed(e.to_string()))?;
            Ok(data.remove(key).is_some())
        }

        async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ListFailed(e.to_string()))?;
            let keys: Vec<String> = match prefix {
                Some(p) => data.keys().filter(|k| k.starts_with(p)).cloned().collect(),
                None => data.keys().cloned().collect(),
            };
            Ok(keys)
        }

        async fn exists(&self, key: &str) -> Result<bool, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ReadFailed(e.to_string()))?;
            Ok(data.contains_key(key))
        }

        async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
            let mut data = self
                .data
                .write()
                .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
            data.extend(pairs);
            Ok(())
        }

        async fn retrieve_batch(
            &self,
            keys: &[String],
        ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ReadFailed(e.to_string()))?;
            let result: HashMap<String, Vec<u8>> = keys
                .iter()
                .filter_map(|k| data.get(k).map(|v| (k.clone(), v.clone())))
                .collect();
            Ok(result)
        }

        async fn clear_all(&self) -> Result<(), StorageError> {
            let mut data = self
                .data
                .write()
                .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
            data.clear();
            Ok(())
        }

        async fn stats(&self) -> Result<StorageStats, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ReadFailed(e.to_string()))?;
            let key_count = data.len() as u64;
            let total_size = data.values().map(|v| v.len() as u64).sum();
            Ok(StorageStats {
                key_count,
                total_size,
                available_space: None,
                backend_type: "memory".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let key_material = b"test-key-material-32-bytes-long!";
        let salt = [42u8; 32];

        let config = LocalStoreConfig::with_salt("/test/path.store", salt);
        let mut store = LocalStore::new(config.clone(), key_material);
        let crypto = MockCryptoHandler::with_seed(12345);
        let storage = TestStorage::new();

        // Modify data
        store.data_mut().theme = super::super::types::ThemePreference::Light;
        store.data_mut().set_setting("test_key", "test_value");

        // Save
        store.save(&crypto, &storage).await.unwrap();

        // Load and verify
        let loaded = LocalStore::load(config, key_material, &crypto, &storage)
            .await
            .unwrap();

        assert_eq!(
            loaded.data().theme,
            super::super::types::ThemePreference::Light
        );
        assert_eq!(
            loaded.data().get_setting("test_key"),
            Some(&"test_value".to_string())
        );
    }

    #[tokio::test]
    async fn test_wrong_key_fails() {
        let key_material = b"test-key-material-32-bytes-long!";
        let wrong_key = b"wrong-key-material-32bytes-long!";
        let salt = [42u8; 32];
        let crypto = MockCryptoHandler::with_seed(12345);
        let storage = TestStorage::new();

        let config = LocalStoreConfig::with_salt("/test/path2.store", salt);
        let store = LocalStore::new(config.clone(), key_material);
        store.save(&crypto, &storage).await.unwrap();

        // Try to load with wrong key - should fail or produce garbage
        let result = LocalStore::load(config, wrong_key, &crypto, &storage).await;

        // With our mock XOR cipher, wrong key produces different plaintext
        // which should fail JSON deserialization
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_new_store_has_defaults() {
        let config = LocalStoreConfig::new("/tmp/nonexistent.store");
        let key_material = b"test-key";

        let store = LocalStore::new(config, key_material);

        assert_eq!(
            store.data().theme,
            super::super::types::ThemePreference::Dark
        );
        assert!(store.data().contacts.is_empty());
    }
}
