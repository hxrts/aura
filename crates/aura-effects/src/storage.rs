//! Layer 3: Storage Effect Handlers - Production Only
//!
//! Stateless single-party implementations of StorageEffects from aura-core (Layer 1).
//! These handlers provide production storage operations delegating to filesystem or cloud APIs.
//!
//! **Layer Constraint**: NO mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production-grade stateless handlers.

use async_trait::async_trait;
use aura_core::effects::{StorageEffects, StorageError, StorageStats};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use tokio::fs::DirEntry;

/// Filesystem-based storage handler for production use
///
/// This handler stores data as files on the local filesystem.
/// It is stateless and delegates all storage operations to the filesystem.
#[derive(Debug, Clone)]
pub struct FilesystemStorageHandler {
    /// Base directory for storage files
    base_path: PathBuf,
}

impl FilesystemStorageHandler {
    /// Create a new filesystem storage handler
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Alias for clarity; avoids relying on `new` naming in higher layers.
    pub fn from_path(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Create a new filesystem storage handler with default path
    pub fn with_default_path() -> Self {
        Self::new(PathBuf::from("./storage"))
    }
}

#[async_trait]
impl StorageEffects for FilesystemStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        if key.is_empty() {
            return Err(StorageError::InvalidKey {
                reason: "Key cannot be empty".to_string(),
            });
        }

        let file_path = self.base_path.join(format!("{}.dat", key));
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                StorageError::WriteFailed(format!("Failed to create directory: {}", e))
            })?;
        }

        fs::write(&file_path, value)
            .await
            .map_err(|e| StorageError::WriteFailed(format!("Failed to write file: {}", e)))?;

        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let file_path = self.base_path.join(format!("{}.dat", key));

        if !file_path.exists() {
            return Ok(None);
        }

        let data = fs::read(&file_path)
            .await
            .map_err(|e| StorageError::ReadFailed(format!("Failed to read file: {}", e)))?;

        Ok(Some(data))
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let file_path = self.base_path.join(format!("{}.dat", key));

        if !file_path.exists() {
            return Ok(false);
        }

        fs::remove_file(&file_path)
            .await
            .map_err(|e| StorageError::DeleteFailed(format!("Failed to remove file: {}", e)))?;

        Ok(true)
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        // Keys may contain path separators (e.g. `journal/facts/...`), so we must
        // traverse the directory tree recursively and strip the `.dat` suffix
        // from persisted filenames.
        let mut keys = Vec::new();
        let mut stack: Vec<PathBuf> = vec![self.base_path.clone()];

        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(StorageError::ReadFailed(format!(
                        "Failed to read directory: {}",
                        e
                    )))
                }
            };

            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                StorageError::ReadFailed(format!("Failed to read directory entry: {}", e))
            })? {
                Self::visit_entry_for_keys(&self.base_path, entry, prefix, &mut stack, &mut keys)
                    .await?;
            }
        }

        keys.sort();
        Ok(keys)
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let file_path = self.base_path.join(format!("{}.dat", key));
        Ok(file_path.exists())
    }

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        for (k, v) in pairs {
            self.store(&k, v).await?;
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        let mut out = std::collections::HashMap::new();
        for key in keys {
            if let Some(val) = self.retrieve(key).await? {
                out.insert(key.clone(), val);
            }
        }
        Ok(out)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        match fs::remove_dir_all(&self.base_path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(StorageError::DeleteFailed(format!(
                    "Failed to remove storage directory: {}",
                    e
                )))
            }
        }

        fs::create_dir_all(&self.base_path).await.map_err(|e| {
            StorageError::WriteFailed(format!("Failed to recreate storage directory: {}", e))
        })?;
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let mut key_count: u64 = 0;
        let mut total_size: u64 = 0;

        let mut stack: Vec<PathBuf> = vec![self.base_path.clone()];
        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(StorageError::ReadFailed(format!(
                        "Failed to read directory: {}",
                        e
                    )))
                }
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let file_type = match entry.file_type().await {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };

                if file_type.is_dir() {
                    stack.push(entry.path());
                    continue;
                }

                if !file_type.is_file() {
                    continue;
                }

                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("dat") {
                    continue;
                }

                key_count += 1;
                if let Ok(metadata) = entry.metadata().await {
                    total_size = total_size.saturating_add(metadata.len());
                }
            }
        }

        Ok(StorageStats {
            key_count,
            total_size,
            available_space: None,
            backend_type: "filesystem".to_string(),
        })
    }
}

impl FilesystemStorageHandler {
    async fn visit_entry_for_keys(
        base: &PathBuf,
        entry: DirEntry,
        prefix: Option<&str>,
        stack: &mut Vec<PathBuf>,
        keys: &mut Vec<String>,
    ) -> Result<(), StorageError> {
        let file_type = entry.file_type().await.map_err(|e| {
            StorageError::ReadFailed(format!("Failed to stat directory entry: {}", e))
        })?;
        let path = entry.path();

        if file_type.is_dir() {
            stack.push(path);
            return Ok(());
        }
        if !file_type.is_file() {
            return Ok(());
        }

        if path.extension().and_then(|e| e.to_str()) != Some("dat") {
            return Ok(());
        }

        let rel = path.strip_prefix(base).map_err(|e| {
            StorageError::ReadFailed(format!("Failed to compute relative key path: {}", e))
        })?;
        let rel = rel.with_extension("");
        let mut key = rel.to_string_lossy().to_string();
        if std::path::MAIN_SEPARATOR != '/' {
            key = key.replace(std::path::MAIN_SEPARATOR, "/");
        }

        if let Some(prefix) = prefix {
            if key.starts_with(prefix) {
                keys.push(key);
            }
        } else {
            keys.push(key);
        }

        Ok(())
    }
}

// =============================================================================
// Path-based filesystem storage (preserves filenames)
// =============================================================================

/// Filesystem storage handler that treats keys as relative paths under `base_path`.
///
/// This is useful for UI layers that already have stable filenames (e.g. `account.json`,
/// `journal.json`) and want to move I/O behind `StorageEffects` without changing on-disk layouts.
///
/// Security: keys are validated to be relative paths with no `..` components.
#[derive(Debug, Clone)]
pub struct PathFilesystemStorageHandler {
    base_path: PathBuf,
}

impl PathFilesystemStorageHandler {
    /// Create a new handler rooted at `base_path`.
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    fn resolve(&self, key: &str) -> Result<PathBuf, StorageError> {
        use std::path::{Component, Path};

        if key.is_empty() {
            return Err(StorageError::InvalidKey {
                reason: "Key cannot be empty".to_string(),
            });
        }

        let rel = Path::new(key);
        if rel.is_absolute() {
            return Err(StorageError::InvalidKey {
                reason: "Key must be a relative path".to_string(),
            });
        }

        for comp in rel.components() {
            match comp {
                Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                    return Err(StorageError::InvalidKey {
                        reason: "Key must not contain absolute or parent components".to_string(),
                    });
                }
                Component::CurDir | Component::Normal(_) => {}
            }
        }

        Ok(self.base_path.join(rel))
    }
}

#[async_trait]
impl StorageEffects for PathFilesystemStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let file_path = self.resolve(key)?;
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                StorageError::WriteFailed(format!("Failed to create directory: {}", e))
            })?;
        }

        fs::write(&file_path, value)
            .await
            .map_err(|e| StorageError::WriteFailed(format!("Failed to write file: {}", e)))?;
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let file_path = self.resolve(key)?;

        match fs::read(&file_path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::ReadFailed(format!(
                "Failed to read file: {}",
                e
            ))),
        }
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let file_path = self.resolve(key)?;
        match fs::remove_file(&file_path).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(StorageError::DeleteFailed(format!(
                "Failed to remove file: {}",
                e
            ))),
        }
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let mut keys = Vec::new();
        let mut stack: Vec<PathBuf> = vec![self.base_path.clone()];

        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(StorageError::ReadFailed(format!(
                        "Failed to read directory: {}",
                        e
                    )))
                }
            };

            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                StorageError::ReadFailed(format!("Failed to read directory entry: {}", e))
            })? {
                let path = entry.path();
                let file_type = entry.file_type().await.map_err(|e| {
                    StorageError::ReadFailed(format!("Failed to stat entry: {}", e))
                })?;

                if file_type.is_dir() {
                    stack.push(path);
                } else if file_type.is_file() {
                    if let Ok(rel) = path.strip_prefix(&self.base_path) {
                        keys.push(rel.to_string_lossy().to_string());
                    }
                }
            }
        }

        if let Some(prefix) = prefix {
            keys.retain(|k| k.starts_with(prefix));
        }
        keys.sort();
        Ok(keys)
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let file_path = self.resolve(key)?;
        match fs::metadata(&file_path).await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(StorageError::ReadFailed(format!(
                "Failed to stat file: {}",
                e
            ))),
        }
    }

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        for (k, v) in pairs {
            self.store(&k, v).await?;
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        let mut out = std::collections::HashMap::new();
        for key in keys {
            if let Some(val) = self.retrieve(key).await? {
                out.insert(key.clone(), val);
            }
        }
        Ok(out)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        // Best-effort recursive clear.
        let keys = self.list_keys(None).await?;
        for k in keys {
            let _ = self.remove(&k).await;
        }
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let keys = self.list_keys(None).await?;
        let mut total_size: u64 = 0;
        for k in &keys {
            let p = self.resolve(k)?;
            if let Ok(m) = fs::metadata(&p).await {
                total_size = total_size.saturating_add(m.len());
            }
        }

        Ok(StorageStats {
            key_count: keys.len() as u64,
            total_size,
            available_space: None,
            backend_type: "path-filesystem".to_string(),
        })
    }
}

fn nonce_for(key: &[u8], key_str: &str) -> Result<chacha20poly1305::Nonce, StorageError> {
    use chacha20poly1305::Nonce;
    // Use aura_core's hash function (SHA-256) for nonce derivation
    let mut h = aura_core::hash::hasher();
    h.update(key);
    h.update(key_str.as_bytes());
    let derived = h.finalize();
    let mut nonce_bytes = [0u8; 12];
    nonce_bytes.copy_from_slice(&derived[..12]);
    Ok(Nonce::from(nonce_bytes))
}

fn encrypt_stream(data: &[u8], key: &[u8], key_str: &str) -> Result<Vec<u8>, StorageError> {
    use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, Key, KeyInit};
    if key.len() != 32 {
        return Err(StorageError::WriteFailed(
            "Encryption key must be 32 bytes".to_string(),
        ));
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = nonce_for(key, key_str)?;
    cipher
        .encrypt(&nonce, data)
        .map_err(|e| StorageError::WriteFailed(format!("Encryption failed: {}", e)))
}

fn decrypt_stream(data: &[u8], key: &[u8], key_str: &str) -> Result<Vec<u8>, StorageError> {
    use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, Key, KeyInit};
    if key.len() != 32 {
        return Err(StorageError::ReadFailed(
            "Encryption key must be 32 bytes".to_string(),
        ));
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = nonce_for(key, key_str)?;
    cipher
        .decrypt(&nonce, data)
        .map_err(|e| StorageError::ReadFailed(format!("Decryption failed: {}", e)))
}

/// Encrypted storage handler for production use
///
/// This handler provides encrypted storage by wrapping the filesystem handler
/// with encryption/decryption operations. It is stateless and delegates
/// storage operations to the filesystem while handling encryption in memory.
#[derive(Debug, Clone)]
pub struct EncryptedStorageHandler {
    /// Base filesystem handler for actual storage
    filesystem_handler: FilesystemStorageHandler,
    /// Optional symmetric key used for simple stream-style encryption
    encryption_key: Option<Vec<u8>>,
}

impl EncryptedStorageHandler {
    /// Create a new encrypted storage handler
    ///
    pub fn new(storage_path: PathBuf, encryption_key: Option<Vec<u8>>) -> Self {
        Self {
            filesystem_handler: FilesystemStorageHandler::new(storage_path),
            encryption_key,
        }
    }

    /// Alias for clarity; avoids relying on `new` naming in higher layers.
    pub fn from_path(storage_path: PathBuf, encryption_key: Option<Vec<u8>>) -> Self {
        Self::new(storage_path, encryption_key)
    }

    /// Create with default configuration
    pub fn with_default_config() -> Self {
        Self::new(PathBuf::from("./encrypted_storage"), None)
    }

    /// Get information about the storage configuration
    pub fn stack_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("type".to_string(), "encrypted_filesystem".to_string());
        info.insert(
            "encryption".to_string(),
            if self.encryption_key.is_some() {
                "xor-keystream".to_string()
            } else {
                "plaintext".to_string()
            },
        );
        info.insert(
            "base_path".to_string(),
            self.filesystem_handler.base_path.display().to_string(),
        );
        info
    }
}

#[async_trait]
impl StorageEffects for EncryptedStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let payload = if let Some(k) = &self.encryption_key {
            encrypt_stream(&value, k, key)?
        } else {
            value
        };
        self.filesystem_handler.store(key, payload).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let data = self.filesystem_handler.retrieve(key).await?;
        let decrypted = if let (Some(ciphertext), Some(k)) = (&data, &self.encryption_key) {
            Some(decrypt_stream(ciphertext, k, key)?)
        } else {
            data
        };
        Ok(decrypted)
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.filesystem_handler.remove(key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        self.filesystem_handler.list_keys(prefix).await
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.filesystem_handler.exists(key).await
    }

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        for (k, v) in pairs {
            self.store(&k, v).await?;
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        let mut out = std::collections::HashMap::new();
        for key in keys {
            if let Some(val) = self.retrieve(key).await? {
                out.insert(key.clone(), val);
            }
        }
        Ok(out)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        // Delegate to filesystem handler best-effort
        self.filesystem_handler.clear_all().await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        self.filesystem_handler.stats().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_filesystem_storage_handler() {
        let temp_dir = TempDir::new().unwrap();
        let handler = FilesystemStorageHandler::new(temp_dir.path().to_path_buf());

        // Test store and retrieve
        let key = "test_key";
        let value = b"test_value".to_vec();

        handler.store(key, value.clone()).await.unwrap();
        let retrieved = handler.retrieve(key).await.unwrap();
        assert_eq!(retrieved, Some(value));

        // Test exists
        assert!(handler.exists(key).await.unwrap());

        // Test remove
        assert!(handler.remove(key).await.unwrap());
        assert!(!handler.exists(key).await.unwrap());
    }

    #[tokio::test]
    async fn test_encrypted_storage_handler() {
        let temp_dir = TempDir::new().unwrap();
        let handler = EncryptedStorageHandler::new(temp_dir.path().to_path_buf(), None);

        // Test basic operations (currently delegates to filesystem handler)
        let key = "encrypted_test";
        let value = b"encrypted_value".to_vec();

        handler.store(key, value.clone()).await.unwrap();
        let retrieved = handler.retrieve(key).await.unwrap();
        assert_eq!(retrieved, Some(value));

        // Test stack info
        let info = handler.stack_info();
        assert!(info.contains_key("type"));
        assert_eq!(info.get("type"), Some(&"encrypted_filesystem".to_string()));
    }

    #[tokio::test]
    async fn test_delete_and_retrieve() {
        let temp_dir = TempDir::new().unwrap();
        let handler = FilesystemStorageHandler::new(temp_dir.path().to_path_buf());

        let key = "test_key";
        let data = b"test_data".to_vec();

        // Store data
        handler.store(key, data.clone()).await.unwrap();

        // Verify it exists
        let retrieved = handler.retrieve(key).await.unwrap();
        assert_eq!(retrieved, Some(data));

        // Delete it
        let was_deleted = handler.remove(key).await.unwrap();
        assert!(was_deleted);

        // Verify it's gone
        let retrieved_after = handler.retrieve(key).await.unwrap();
        assert_eq!(retrieved_after, None);
    }
}
