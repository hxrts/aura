//! Layer 3: Unified Encrypted Storage Handler
//!
//! Provides transparent encryption for all storage operations by composing:
//! - `StorageEffects` for underlying data persistence
//! - `CryptoEffects` for ChaCha20-Poly1305 encryption
//! - `SecureStorageEffects` for master key storage in platform secure enclave
//!
//! **Architecture**: This is the SINGLE encryption layer for all persistent data.
//! All application data flows through this handler and is encrypted at rest.
//!
//! **Layer Constraint**: Stateless handler that composes three effect traits.
//! No multi-party coordination. No mock implementations (those belong in aura-testkit).

use async_trait::async_trait;
use aura_core::effects::{
    CryptoEffects, SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    StorageCoreEffects, StorageError, StorageExtendedEffects, StorageStats,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use zeroize::Zeroizing;

/// Nonce size for ChaCha20-Poly1305 (96 bits = 12 bytes)
const NONCE_SIZE: usize = 12;

/// Version byte for encrypted blob format
const BLOB_VERSION: u8 = 0x01;

/// Master key location in secure storage
const MASTER_KEY_NAMESPACE: &str = "aura-encryption";
const MASTER_KEY_ID: &str = "master-key";

type MasterKeyMaterial = Arc<Zeroizing<[u8; 32]>>;

/// Configuration for encrypted storage behavior
#[derive(Debug, Clone)]
pub struct EncryptedStorageConfig {
    /// Enable encryption (and master-key management) for all operations.
    ///
    /// This exists primarily for testing and bring-up. Production should keep this `true`.
    pub enabled: bool,
    /// Use opaque (hashed) file names instead of semantic names
    pub opaque_names: bool,
    /// Custom namespace for master key in secure storage
    pub key_namespace: Option<String>,
    /// Custom key identifier within namespace
    pub key_id: Option<String>,
    /// Allow reading legacy plaintext blobs when encryption is enabled
    pub allow_plaintext_read: bool,
    /// If true, re-encrypt legacy plaintext blobs on read
    pub migrate_on_read: bool,
}

impl Default for EncryptedStorageConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            opaque_names: false,
            key_namespace: None,
            key_id: None,
            allow_plaintext_read: false,
            migrate_on_read: false,
        }
    }
}

impl EncryptedStorageConfig {
    /// Create config with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable opaque file names (HKDF-derived from master key + semantic name)
    pub fn with_opaque_names(mut self) -> Self {
        self.opaque_names = true;
        self
    }

    /// Enable or disable encryption.
    pub fn with_encryption_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set custom key namespace
    pub fn with_key_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.key_namespace = Some(namespace.into());
        self
    }

    /// Set custom key identifier
    pub fn with_key_id(mut self, id: impl Into<String>) -> Self {
        self.key_id = Some(id.into());
        self
    }

    /// Allow legacy plaintext reads when encryption is enabled.
    pub fn with_plaintext_read(mut self, allowed: bool) -> Self {
        self.allow_plaintext_read = allowed;
        self
    }

    /// Enable migration of plaintext blobs on read.
    pub fn with_migrate_on_read(mut self, enabled: bool) -> Self {
        self.migrate_on_read = enabled;
        self
    }
}

/// Unified encrypted storage that wraps any StorageEffects implementation.
///
/// All data passing through this layer is encrypted using a master key
/// stored in the platform's secure enclave via SecureStorageEffects.
///
/// # Type Parameters
///
/// - `S`: Inner storage implementation (where encrypted blobs live)
/// - `C`: Crypto implementation (for ChaCha20-Poly1305 operations)
/// - `Sec`: Secure storage implementation (where master key lives)
///
/// # Example
///
/// ```rust,ignore
/// let encrypted = EncryptedStorage::new(
///     filesystem_handler,
///     crypto_handler,
///     secure_storage_handler,
///     EncryptedStorageConfig::default(),
/// ).await?;
///
/// // All operations are now transparently encrypted
/// encrypted.store("accounts", data).await?;
/// let data = encrypted.retrieve("accounts").await?;
/// ```
pub struct EncryptedStorage<S, C, Sec>
where
    S: StorageCoreEffects + StorageExtendedEffects,
    C: CryptoEffects,
    Sec: SecureStorageEffects,
{
    /// Underlying storage for encrypted blobs
    inner: S,
    /// Cryptographic operations
    crypto: Arc<C>,
    /// Secure storage for master key
    secure: Arc<Sec>,
    /// Cached master key (lazily loaded/created on first use).
    master_key: RwLock<Option<MasterKeyMaterial>>,
    /// Single-flight guard for master-key initialization.
    master_key_init: Mutex<()>,
    /// Configuration options
    config: EncryptedStorageConfig,
}

impl<S, C, Sec> EncryptedStorage<S, C, Sec>
where
    S: StorageCoreEffects + StorageExtendedEffects,
    C: CryptoEffects,
    Sec: SecureStorageEffects,
{
    /// Create a new encrypted storage handler.
    ///
    /// Master key initialization is **lazy**: the key is loaded/created on the first
    /// `store/retrieve/...` call. This keeps runtime assembly synchronous and avoids
    /// turning effect-system constructors into async APIs.
    pub fn new(inner: S, crypto: Arc<C>, secure: Arc<Sec>, config: EncryptedStorageConfig) -> Self {
        Self {
            inner,
            crypto,
            secure,
            master_key: RwLock::new(None),
            master_key_init: Mutex::new(()),
            config,
        }
    }

    /// Get the secure storage location for the master key
    fn master_key_location(config: &EncryptedStorageConfig) -> SecureStorageLocation {
        SecureStorageLocation::new(
            config
                .key_namespace
                .as_deref()
                .unwrap_or(MASTER_KEY_NAMESPACE),
            config.key_id.as_deref().unwrap_or(MASTER_KEY_ID),
        )
    }

    async fn get_or_init_master_key(&self) -> Result<MasterKeyMaterial, StorageError> {
        if let Some(key) = self.master_key.read().await.clone() {
            return Ok(key);
        }

        // Single-flight to avoid concurrent "create key" races.
        let _guard = self.master_key_init.lock().await;
        if let Some(key) = self.master_key.read().await.clone() {
            return Ok(key);
        }

        let location = Self::master_key_location(&self.config);
        let read_caps = [SecureStorageCapability::Read];
        let write_caps = [SecureStorageCapability::Write];

        let mut key_bytes = if self.secure.secure_exists(&location).await.map_err(|e| {
            StorageError::ConfigurationError {
                reason: format!("Failed to check secure storage: {}", e),
            }
        })? {
            self.secure
                .secure_retrieve(&location, &read_caps)
                .await
                .map_err(|e| StorageError::ConfigurationError {
                    reason: format!("Failed to retrieve master key: {}", e),
                })?
        } else {
            let key_bytes = self.crypto.random_bytes(32).await;
            if key_bytes.len() != 32 {
                return Err(StorageError::ConfigurationError {
                    reason: "Failed to generate 32-byte key".to_string(),
                });
            }

            self.secure
                .secure_store(&location, &key_bytes, &write_caps)
                .await
                .map_err(|e| StorageError::ConfigurationError {
                    reason: format!("Failed to store master key: {}", e),
                })?;

            key_bytes
        };

        if key_bytes.len() != 32 {
            // If secure storage contains an invalid key length, treat it as corrupt and
            // re-generate a fresh master key. This preserves correctness because existing
            // encrypted data would be unreadable with a malformed key anyway.
            let delete_caps = [SecureStorageCapability::Delete];
            let _ = self.secure.secure_delete(&location, &delete_caps).await;
            let regenerated = self.crypto.random_bytes(32).await;
            if regenerated.len() != 32 {
                return Err(StorageError::ConfigurationError {
                    reason: "Failed to generate 32-byte key".to_string(),
                });
            }
            self.secure
                .secure_store(&location, &regenerated, &write_caps)
                .await
                .map_err(|e| StorageError::ConfigurationError {
                    reason: format!("Failed to store master key: {}", e),
                })?;
            key_bytes = regenerated;
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);
        let key = Arc::new(Zeroizing::new(key));
        *self.master_key.write().await = Some(key.clone());
        Ok(key)
    }

    /// Derive an opaque storage key from the semantic key.
    ///
    /// Uses HKDF to derive a deterministic but unpredictable key name
    /// from the master key and semantic key.
    async fn derive_opaque_key(&self, semantic_key: &str) -> Result<String, StorageError> {
        let master_key = self.get_or_init_master_key().await?;
        // Use HKDF to derive a 16-byte key name
        let derived = self
            .crypto
            .hkdf_derive(
                &**master_key,
                semantic_key.as_bytes(),
                b"aura-opaque-key-v1",
                16,
            )
            .await
            .map_err(|e| StorageError::EncryptionFailed {
                reason: format!("Opaque key derivation failed: {}", e),
            })?;

        // Encode as hex for filesystem-safe name
        Ok(hex::encode(&derived))
    }

    /// Get the storage key to use (opaque or semantic)
    async fn storage_key(&self, key: &str) -> Result<String, StorageError> {
        if !self.config.enabled {
            return Ok(key.to_string());
        }
        if self.config.opaque_names {
            self.derive_opaque_key(key).await
        } else {
            Ok(key.to_string())
        }
    }

    /// Derive a per-key encryption key using HKDF.
    ///
    /// This binds the encryption to the storage key, providing key separation
    /// and preventing cross-key ciphertext attacks without needing AAD.
    async fn derive_encryption_key(&self, storage_key: &str) -> Result<[u8; 32], StorageError> {
        let master_key = self.get_or_init_master_key().await?;
        let derived = self
            .crypto
            .hkdf_derive(
                &**master_key,
                storage_key.as_bytes(),
                b"aura-storage-encryption-v1",
                32,
            )
            .await
            .map_err(|e| StorageError::EncryptionFailed {
                reason: format!("Key derivation failed: {}", e),
            })?;

        let mut key = [0u8; 32];
        key.copy_from_slice(&derived);
        Ok(key)
    }

    /// Encrypt data with a key derived from master key + storage key.
    ///
    /// Format: version (1 byte) || nonce (12 bytes) || ciphertext
    async fn encrypt(&self, key: &str, data: &[u8]) -> Result<Vec<u8>, StorageError> {
        // Derive per-key encryption key
        let encryption_key = self.derive_encryption_key(key).await?;

        // Generate unique nonce
        let nonce_bytes = self.crypto.random_bytes(NONCE_SIZE).await;
        if nonce_bytes.len() != NONCE_SIZE {
            return Err(StorageError::EncryptionFailed {
                reason: "Failed to generate nonce".to_string(),
            });
        }

        let mut nonce = [0u8; NONCE_SIZE];
        nonce.copy_from_slice(&nonce_bytes);

        // Encrypt with ChaCha20-Poly1305
        let ciphertext = self
            .crypto
            .chacha20_encrypt(data, &encryption_key, &nonce)
            .await
            .map_err(|e| StorageError::EncryptionFailed {
                reason: e.to_string(),
            })?;

        // Build blob: version || nonce || ciphertext
        let mut blob = Vec::with_capacity(1 + NONCE_SIZE + ciphertext.len());
        blob.push(BLOB_VERSION);
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ciphertext);

        Ok(blob)
    }

    /// Decrypt data with a key derived from master key + storage key.
    ///
    /// Expects format: version (1 byte) || nonce (12 bytes) || ciphertext
    async fn decrypt(&self, key: &str, blob: &[u8]) -> Result<Vec<u8>, StorageError> {
        // Check minimum length
        if blob.len() < 1 + NONCE_SIZE {
            return Err(StorageError::DecryptionFailed {
                reason: "Blob too short".to_string(),
            });
        }

        // Check version
        let version = blob[0];
        if version != BLOB_VERSION {
            return Err(StorageError::DecryptionFailed {
                reason: format!("Unknown blob version: {}", version),
            });
        }

        // Derive per-key encryption key
        let encryption_key = self.derive_encryption_key(key).await?;

        // Extract nonce and ciphertext
        let nonce_bytes = &blob[1..1 + NONCE_SIZE];
        let ciphertext = &blob[1 + NONCE_SIZE..];

        let mut nonce = [0u8; NONCE_SIZE];
        nonce.copy_from_slice(nonce_bytes);

        // Decrypt with ChaCha20-Poly1305
        self.crypto
            .chacha20_decrypt(ciphertext, &encryption_key, &nonce)
            .await
            .map_err(|e| StorageError::DecryptionFailed {
                reason: e.to_string(),
            })
    }

    /// Check if a blob is encrypted (has our version header).
    ///
    /// Used for detecting unencrypted legacy data.
    pub fn is_encrypted(blob: &[u8]) -> bool {
        !blob.is_empty() && blob[0] == BLOB_VERSION
    }

    /// Get reference to inner storage (for debugging/testing)
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Get reference to secure storage (for key management)
    pub fn secure(&self) -> &Arc<Sec> {
        &self.secure
    }
}

#[async_trait]
impl<S, C, Sec> StorageCoreEffects for EncryptedStorage<S, C, Sec>
where
    S: StorageCoreEffects + StorageExtendedEffects + Send + Sync,
    C: CryptoEffects + Send + Sync,
    Sec: SecureStorageEffects + Send + Sync,
{
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        if !self.config.enabled {
            return self.inner.store(key, value).await;
        }
        let storage_key = self.storage_key(key).await?;
        let encrypted = self.encrypt(key, &value).await?;
        self.inner.store(&storage_key, encrypted).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        if !self.config.enabled {
            return self.inner.retrieve(key).await;
        }
        let storage_key = self.storage_key(key).await?;
        match self.inner.retrieve(&storage_key).await? {
            Some(blob) => {
                if Self::is_encrypted(&blob) {
                    let decrypted = self.decrypt(key, &blob).await?;
                    return Ok(Some(decrypted));
                }

                if !self.config.allow_plaintext_read {
                    return Err(StorageError::DecryptionFailed {
                        reason: "Plaintext blob detected while encryption is enabled".to_string(),
                    });
                }

                // Optionally migrate plaintext to encrypted form.
                if self.config.migrate_on_read {
                    let encrypted = self.encrypt(key, &blob).await?;
                    self.inner.store(&storage_key, encrypted).await?;
                }

                Ok(Some(blob))
            }
            None => Ok(None),
        }
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        if !self.config.enabled {
            return self.inner.remove(key).await;
        }
        let storage_key = self.storage_key(key).await?;
        self.inner.remove(&storage_key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        if !self.config.enabled {
            return self.inner.list_keys(prefix).await;
        }
        // If using opaque names, we can't filter by prefix effectively
        // Return all keys and let caller filter (or return empty if opaque)
        if self.config.opaque_names {
            // With opaque names, we can't meaningfully list by prefix
            // The caller would need to maintain their own index
            return self.inner.list_keys(None).await;
        }
        self.inner.list_keys(prefix).await
    }
}

#[async_trait]
impl<S, C, Sec> StorageExtendedEffects for EncryptedStorage<S, C, Sec>
where
    S: StorageCoreEffects + StorageExtendedEffects + Send + Sync,
    C: CryptoEffects + Send + Sync,
    Sec: SecureStorageEffects + Send + Sync,
{
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        if !self.config.enabled {
            return self.inner.exists(key).await;
        }
        let storage_key = self.storage_key(key).await?;
        self.inner.exists(&storage_key).await
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        if !self.config.enabled {
            return self.inner.store_batch(pairs).await;
        }
        // Encrypt each value
        let mut encrypted_pairs = HashMap::with_capacity(pairs.len());
        for (key, value) in pairs {
            let storage_key = self.storage_key(&key).await?;
            let encrypted = self.encrypt(&key, &value).await?;
            encrypted_pairs.insert(storage_key, encrypted);
        }
        self.inner.store_batch(encrypted_pairs).await
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        if !self.config.enabled {
            return self.inner.retrieve_batch(keys).await;
        }
        // Map semantic keys to storage keys
        let mut storage_keys = Vec::with_capacity(keys.len());
        let mut key_map = HashMap::with_capacity(keys.len());
        for key in keys {
            let storage_key = self.storage_key(key).await?;
            key_map.insert(storage_key.clone(), key.clone());
            storage_keys.push(storage_key);
        }

        // Retrieve encrypted blobs
        let encrypted = self.inner.retrieve_batch(&storage_keys).await?;

        // Decrypt each value
        let mut decrypted = HashMap::with_capacity(encrypted.len());
        for (storage_key, blob) in encrypted {
            if let Some(semantic_key) = key_map.get(&storage_key) {
                let value = self.decrypt(semantic_key, &blob).await?;
                decrypted.insert(semantic_key.clone(), value);
            }
        }

        Ok(decrypted)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        if !self.config.enabled {
            return self.inner.clear_all().await;
        }
        self.inner.clear_all().await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        if !self.config.enabled {
            return self.inner.stats().await;
        }
        let mut stats = self.inner.stats().await?;
        // Update backend type to indicate encryption
        stats.backend_type = format!("encrypted({})", stats.backend_type);
        Ok(stats)
    }
}

// Debug impl that doesn't expose the master key
impl<S, C, Sec> std::fmt::Debug for EncryptedStorage<S, C, Sec>
where
    S: StorageCoreEffects + StorageExtendedEffects + std::fmt::Debug,
    C: CryptoEffects,
    Sec: SecureStorageEffects,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedStorage")
            .field("inner", &self.inner)
            .field("config", &self.config)
            .field("master_key", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::storage::StorageStats;
    use aura_core::effects::{
        CryptoCoreEffects, CryptoExtendedEffects, StorageCoreEffects, StorageExtendedEffects,
    };
    use aura_core::time::PhysicalTime;
    use std::sync::RwLock;

    // Simple mock implementations for testing

    #[derive(Default)]
    struct MockStorage {
        data: RwLock<HashMap<String, Vec<u8>>>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl StorageCoreEffects for MockStorage {
        async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
            self.data.write().unwrap().insert(key.to_string(), value);
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(self.data.read().unwrap().get(key).cloned())
        }

        async fn remove(&self, key: &str) -> Result<bool, StorageError> {
            Ok(self.data.write().unwrap().remove(key).is_some())
        }

        async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
            let data = self.data.read().unwrap();
            let keys: Vec<String> = match prefix {
                Some(p) => data.keys().filter(|k| k.starts_with(p)).cloned().collect(),
                None => data.keys().cloned().collect(),
            };
            Ok(keys)
        }
    }

    #[async_trait]
    impl StorageExtendedEffects for MockStorage {
        async fn exists(&self, key: &str) -> Result<bool, StorageError> {
            Ok(self.data.read().unwrap().contains_key(key))
        }

        async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
            self.data.write().unwrap().extend(pairs);
            Ok(())
        }

        async fn retrieve_batch(
            &self,
            keys: &[String],
        ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
            let data = self.data.read().unwrap();
            Ok(keys
                .iter()
                .filter_map(|k| data.get(k).map(|v| (k.clone(), v.clone())))
                .collect())
        }

        async fn clear_all(&self) -> Result<(), StorageError> {
            self.data.write().unwrap().clear();
            Ok(())
        }

        async fn stats(&self) -> Result<StorageStats, StorageError> {
            let data = self.data.read().unwrap();
            Ok(StorageStats {
                key_count: data.len() as u64,
                total_size: data.values().map(|v| v.len() as u64).sum(),
                available_space: None,
                backend_type: "mock".to_string(),
            })
        }
    }

    struct MockCrypto;

    #[async_trait]
    impl aura_core::effects::random::RandomCoreEffects for MockCrypto {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            vec![42u8; len] // Deterministic for testing
        }

        async fn random_bytes_32(&self) -> [u8; 32] {
            [42u8; 32]
        }

        async fn random_u64(&self) -> u64 {
            42
        }
    }

    #[async_trait]
    impl CryptoCoreEffects for MockCrypto {
        async fn hkdf_derive(
            &self,
            ikm: &[u8],
            salt: &[u8],
            info: &[u8],
            len: usize,
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            let mut result = vec![0u8; len];
            for (i, byte) in ikm.iter().chain(salt.iter()).chain(info.iter()).enumerate() {
                result[i % len] ^= byte;
            }
            Ok(result)
        }

        async fn derive_key(
            &self,
            master_key: &[u8],
            context: &aura_core::effects::crypto::KeyDerivationContext,
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            self.hkdf_derive(master_key, context.context.as_bytes(), b"derive", 32)
                .await
        }

        async fn ed25519_generate_keypair(
            &self,
        ) -> Result<(Vec<u8>, Vec<u8>), aura_core::AuraError> {
            Ok((vec![1u8; 32], vec![2u8; 32]))
        }

        async fn ed25519_sign(
            &self,
            message: &[u8],
            _private_key: &[u8],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(message.to_vec())
        }

        async fn ed25519_verify(
            &self,
            _message: &[u8],
            _signature: &[u8],
            _public_key: &[u8],
        ) -> Result<bool, aura_core::AuraError> {
            Ok(true)
        }

        fn is_simulated(&self) -> bool {
            true
        }

        fn crypto_capabilities(&self) -> Vec<String> {
            vec!["mock".to_string()]
        }

        fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
            a == b
        }

        fn secure_zero(&self, data: &mut [u8]) {
            for byte in data.iter_mut() {
                *byte = 0;
            }
        }
    }

    #[async_trait]
    impl CryptoExtendedEffects for MockCrypto {
        async fn generate_signing_keys(
            &self,
            _threshold: u16,
            _max_signers: u16,
        ) -> Result<aura_core::effects::crypto::SigningKeyGenResult, aura_core::AuraError> {
            Ok(aura_core::effects::crypto::SigningKeyGenResult {
                key_packages: vec![vec![0u8; 32]],
                public_key_package: vec![0u8; 32],
                mode: aura_core::crypto::single_signer::SigningMode::SingleSigner,
            })
        }

        async fn sign_with_key(
            &self,
            message: &[u8],
            _key_package: &[u8],
            _mode: aura_core::crypto::single_signer::SigningMode,
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(message.to_vec())
        }

        async fn verify_signature(
            &self,
            _message: &[u8],
            _signature: &[u8],
            _public_key_package: &[u8],
            _mode: aura_core::crypto::single_signer::SigningMode,
        ) -> Result<bool, aura_core::AuraError> {
            Ok(true)
        }

        async fn frost_generate_keys(
            &self,
            _threshold: u16,
            max_signers: u16,
        ) -> Result<aura_core::effects::crypto::FrostKeyGenResult, aura_core::AuraError> {
            Ok(aura_core::effects::crypto::FrostKeyGenResult {
                key_packages: vec![vec![0u8; 32]; max_signers as usize],
                public_key_package: vec![0u8; 32],
            })
        }

        async fn frost_generate_nonces(
            &self,
            _key_package: &[u8],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![0u8; 64])
        }

        async fn frost_create_signing_package(
            &self,
            message: &[u8],
            _nonces: &[Vec<u8>],
            participants: &[u16],
            public_key_package: &[u8],
        ) -> Result<aura_core::effects::crypto::FrostSigningPackage, aura_core::AuraError> {
            Ok(aura_core::effects::crypto::FrostSigningPackage {
                message: message.to_vec(),
                package: vec![0u8; 64],
                participants: participants.to_vec(),
                public_key_package: public_key_package.to_vec(),
            })
        }

        async fn frost_sign_share(
            &self,
            _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
            _key_share: &[u8],
            _nonces: &[u8],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![0u8; 64])
        }

        async fn frost_aggregate_signatures(
            &self,
            _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
            _signature_shares: &[Vec<u8>],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![0u8; 64])
        }

        async fn frost_verify(
            &self,
            _message: &[u8],
            _signature: &[u8],
            _group_public_key: &[u8],
        ) -> Result<bool, aura_core::AuraError> {
            Ok(true)
        }

        async fn ed25519_public_key(
            &self,
            _private_key: &[u8],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![2u8; 32])
        }

        async fn chacha20_encrypt(
            &self,
            plaintext: &[u8],
            key: &[u8; 32],
            nonce: &[u8; 12],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            // Simple XOR cipher for testing
            let mut result = plaintext.to_vec();
            for (i, byte) in result.iter_mut().enumerate() {
                *byte ^= key[i % 32] ^ nonce[i % 12];
            }
            Ok(result)
        }

        async fn chacha20_decrypt(
            &self,
            ciphertext: &[u8],
            key: &[u8; 32],
            nonce: &[u8; 12],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            // XOR is symmetric
            self.chacha20_encrypt(ciphertext, key, nonce).await
        }

        async fn aes_gcm_encrypt(
            &self,
            plaintext: &[u8],
            key: &[u8; 32],
            nonce: &[u8; 12],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            self.chacha20_encrypt(plaintext, key, nonce).await
        }

        async fn aes_gcm_decrypt(
            &self,
            ciphertext: &[u8],
            key: &[u8; 32],
            nonce: &[u8; 12],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            self.chacha20_decrypt(ciphertext, key, nonce).await
        }

        async fn frost_rotate_keys(
            &self,
            _old_shares: &[Vec<u8>],
            _old_threshold: u16,
            new_threshold: u16,
            new_max_signers: u16,
        ) -> Result<aura_core::effects::crypto::FrostKeyGenResult, aura_core::AuraError> {
            self.frost_generate_keys(new_threshold, new_max_signers)
                .await
        }
    }

    struct MockSecureStorage {
        data: RwLock<HashMap<String, Vec<u8>>>,
    }

    impl MockSecureStorage {
        fn new() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl SecureStorageEffects for MockSecureStorage {
        async fn secure_store(
            &self,
            location: &SecureStorageLocation,
            data: &[u8],
            _caps: &[SecureStorageCapability],
        ) -> Result<(), aura_core::AuraError> {
            self.data
                .write()
                .unwrap()
                .insert(location.full_path(), data.to_vec());
            Ok(())
        }

        async fn secure_retrieve(
            &self,
            location: &SecureStorageLocation,
            _caps: &[SecureStorageCapability],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            self.data
                .read()
                .unwrap()
                .get(&location.full_path())
                .cloned()
                .ok_or_else(|| aura_core::AuraError::storage("not found"))
        }

        async fn secure_delete(
            &self,
            location: &SecureStorageLocation,
            _caps: &[SecureStorageCapability],
        ) -> Result<(), aura_core::AuraError> {
            self.data.write().unwrap().remove(&location.full_path());
            Ok(())
        }

        async fn secure_exists(
            &self,
            location: &SecureStorageLocation,
        ) -> Result<bool, aura_core::AuraError> {
            Ok(self
                .data
                .read()
                .unwrap()
                .contains_key(&location.full_path()))
        }

        async fn secure_list_keys(
            &self,
            namespace: &str,
            _caps: &[SecureStorageCapability],
        ) -> Result<Vec<String>, aura_core::AuraError> {
            let prefix = format!("{}/", namespace);
            Ok(self
                .data
                .read()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .cloned()
                .collect())
        }

        async fn secure_generate_key(
            &self,
            location: &SecureStorageLocation,
            _key_type: &str,
            caps: &[SecureStorageCapability],
        ) -> Result<Option<Vec<u8>>, aura_core::AuraError> {
            let key = vec![0u8; 32];
            self.secure_store(location, &key, caps).await?;
            Ok(Some(key))
        }

        async fn secure_create_time_bound_token(
            &self,
            _location: &SecureStorageLocation,
            _caps: &[SecureStorageCapability],
            _expires_at: &PhysicalTime,
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![])
        }

        async fn secure_access_with_token(
            &self,
            _token: &[u8],
            _location: &SecureStorageLocation,
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![])
        }

        async fn get_device_attestation(&self) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(b"mock-attestation".to_vec())
        }

        async fn is_secure_storage_available(&self) -> bool {
            true
        }

        fn get_secure_storage_capabilities(&self) -> Vec<String> {
            vec!["mock".to_string()]
        }
    }

    #[tokio::test]
    async fn test_encrypted_storage_round_trip() {
        let storage = MockStorage::new();
        let crypto = Arc::new(MockCrypto);
        let secure = Arc::new(MockSecureStorage::new());

        let encrypted =
            EncryptedStorage::new(storage, crypto, secure, EncryptedStorageConfig::default());

        // Store data
        let key = "test-key";
        let value = b"hello world".to_vec();
        encrypted.store(key, value.clone()).await.unwrap();

        // Retrieve and verify
        let retrieved = encrypted.retrieve(key).await.unwrap().unwrap();
        assert_eq!(retrieved, value);
    }

    #[tokio::test]
    async fn test_encrypted_storage_master_key_generated() {
        let storage = MockStorage::new();
        let crypto = Arc::new(MockCrypto);
        let secure = Arc::new(MockSecureStorage::new());

        // Create encrypted storage and trigger key init via a write.
        let encrypted = EncryptedStorage::new(
            storage,
            crypto,
            secure.clone(),
            EncryptedStorageConfig::default(),
        );
        encrypted.store("probe", b"probe".to_vec()).await.unwrap();

        // Verify master key was stored in secure storage
        let location = SecureStorageLocation::new(MASTER_KEY_NAMESPACE, MASTER_KEY_ID);
        assert!(secure.secure_exists(&location).await.unwrap());
    }

    #[tokio::test]
    async fn test_encrypted_storage_master_key_reused() {
        let secure = Arc::new(MockSecureStorage::new());

        // Pre-store a master key
        let location = SecureStorageLocation::new(MASTER_KEY_NAMESPACE, MASTER_KEY_ID);
        let key = vec![1u8; 32];
        secure
            .secure_store(&location, &key, &[SecureStorageCapability::Write])
            .await
            .unwrap();

        // Create encrypted storage (should reuse existing key)
        let storage = MockStorage::new();
        let crypto = Arc::new(MockCrypto);

        let encrypted = EncryptedStorage::new(
            storage,
            crypto,
            secure.clone(),
            EncryptedStorageConfig::default(),
        );

        // Store and retrieve to verify encryption works
        encrypted.store("test", b"data".to_vec()).await.unwrap();
        let retrieved = encrypted.retrieve("test").await.unwrap().unwrap();
        assert_eq!(retrieved, b"data");
    }

    #[tokio::test]
    async fn test_encrypted_storage_blob_format() {
        let storage = MockStorage::new();
        let crypto = Arc::new(MockCrypto);
        let secure = Arc::new(MockSecureStorage::new());

        let encrypted =
            EncryptedStorage::new(storage, crypto, secure, EncryptedStorageConfig::default());

        // Store data
        encrypted.store("test", b"data".to_vec()).await.unwrap();

        // Check raw blob format
        let raw = encrypted.inner().retrieve("test").await.unwrap().unwrap();
        assert_eq!(raw[0], BLOB_VERSION); // Version byte
        assert!(raw.len() > NONCE_SIZE); // At least version + nonce
    }

    #[tokio::test]
    async fn test_is_encrypted_detection() {
        // Valid encrypted blob
        let mut blob = vec![BLOB_VERSION];
        blob.extend_from_slice(&[0u8; NONCE_SIZE]);
        blob.extend_from_slice(b"ciphertext");
        assert!(
            EncryptedStorage::<MockStorage, MockCrypto, MockSecureStorage>::is_encrypted(&blob)
        );

        // Plaintext (wrong version)
        let plaintext = b"just plain text";
        assert!(!EncryptedStorage::<
            MockStorage,
            MockCrypto,
            MockSecureStorage,
        >::is_encrypted(plaintext));

        // Empty
        assert!(!EncryptedStorage::<
            MockStorage,
            MockCrypto,
            MockSecureStorage,
        >::is_encrypted(&[]));
    }

    #[tokio::test]
    async fn test_stats_shows_encrypted() {
        let storage = MockStorage::new();
        let crypto = Arc::new(MockCrypto);
        let secure = Arc::new(MockSecureStorage::new());

        let encrypted =
            EncryptedStorage::new(storage, crypto, secure, EncryptedStorageConfig::default());

        let stats = encrypted.stats().await.unwrap();
        assert!(stats.backend_type.starts_with("encrypted("));
    }

    #[tokio::test]
    async fn test_plaintext_read_allowed() {
        let storage = MockStorage::new();
        let crypto = Arc::new(MockCrypto);
        let secure = Arc::new(MockSecureStorage::new());

        let config = EncryptedStorageConfig::default()
            .with_plaintext_read(true)
            .with_migrate_on_read(false);

        let encrypted = EncryptedStorage::new(storage, crypto, secure, config);

        // Write plaintext directly to inner storage.
        encrypted
            .inner()
            .store("legacy", b"legacy-data".to_vec())
            .await
            .unwrap();

        let retrieved = encrypted.retrieve("legacy").await.unwrap().unwrap();
        assert_eq!(retrieved, b"legacy-data");
    }

    #[tokio::test]
    async fn test_plaintext_migrated_on_read() {
        let storage = MockStorage::new();
        let crypto = Arc::new(MockCrypto);
        let secure = Arc::new(MockSecureStorage::new());

        let config = EncryptedStorageConfig::default()
            .with_plaintext_read(true)
            .with_migrate_on_read(true);

        let encrypted = EncryptedStorage::new(storage, crypto, secure, config);

        encrypted
            .inner()
            .store("legacy", b"legacy-data".to_vec())
            .await
            .unwrap();

        let retrieved = encrypted.retrieve("legacy").await.unwrap().unwrap();
        assert_eq!(retrieved, b"legacy-data");

        let raw = encrypted.inner().retrieve("legacy").await.unwrap().unwrap();
        assert!(EncryptedStorage::<MockStorage, MockCrypto, MockSecureStorage>::is_encrypted(&raw));
    }
}
