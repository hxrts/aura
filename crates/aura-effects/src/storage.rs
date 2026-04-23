//! Layer 3: Storage Effect Handlers - Production Only
//!
//! Stateless single-party implementations of StorageEffects from aura-core (Layer 1).
//! These handlers provide production storage operations delegating to filesystem or cloud APIs.
//!
//! **Layer Constraint**: NO mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production-grade stateless handlers.

use async_trait::async_trait;
use aura_core::effects::{StorageCoreEffects, StorageError, StorageExtendedEffects, StorageStats};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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

static NEXT_TEMP_WRITE_ID: AtomicU64 = AtomicU64::new(0);

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
impl StorageCoreEffects for FilesystemStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let file_path = self.path_for_key(key)?;
        let temp_file_path = file_path.with_extension(format!(
            "dat.tmp-{}",
            NEXT_TEMP_WRITE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                StorageError::WriteFailed(format!("Failed to create directory: {e}"))
            })?;
        }

        // Write via a sibling temp file so concurrent readers never observe
        // truncated ciphertext or partially-updated journal blobs.
        fs::write(&temp_file_path, value)
            .await
            .map_err(|e| StorageError::WriteFailed(format!("Failed to write temp file: {e}")))?;

        if let Err(err) = fs::rename(&temp_file_path, &file_path).await {
            if file_path.exists() {
                fs::remove_file(&file_path).await.map_err(|remove_err| {
                    StorageError::WriteFailed(format!(
                        "Failed to replace existing file after rename error ({err}): {remove_err}"
                    ))
                })?;
                fs::rename(&temp_file_path, &file_path)
                    .await
                    .map_err(|rename_err| {
                        StorageError::WriteFailed(format!(
                        "Failed to finalize atomic write after removing existing file: {rename_err}"
                    ))
                    })?;
            } else {
                return Err(StorageError::WriteFailed(format!(
                    "Failed to rename temp file into place: {err}"
                )));
            }
        }

        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let file_path = self.path_for_key(key)?;

        if !file_path.exists() {
            return Ok(None);
        }

        let data = fs::read(&file_path)
            .await
            .map_err(|e| StorageError::ReadFailed(format!("Failed to read file: {e}")))?;

        Ok(Some(data))
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let file_path = self.path_for_key(key)?;

        if !file_path.exists() {
            return Ok(false);
        }

        fs::remove_file(&file_path)
            .await
            .map_err(|e| StorageError::DeleteFailed(format!("Failed to remove file: {e}")))?;

        Ok(true)
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        if let Some(prefix) = prefix {
            Self::validate_key_prefix(prefix)?;
        }
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
                        "Failed to read directory: {e}"
                    )))
                }
            };

            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                StorageError::ReadFailed(format!("Failed to read directory entry: {e}"))
            })? {
                Self::visit_entry_for_keys(&self.base_path, entry, prefix, &mut stack, &mut keys)
                    .await?;
            }
        }

        keys.sort();
        Ok(keys)
    }
}

#[async_trait]
impl StorageExtendedEffects for FilesystemStorageHandler {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let file_path = self.path_for_key(key)?;
        Ok(file_path.exists())
    }

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        for key in pairs.keys() {
            Self::validate_key_segments(key)?;
        }
        for (k, v) in pairs {
            self.store(&k, v).await?;
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        for key in keys {
            Self::validate_key_segments(key)?;
        }
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
                    "Failed to remove storage directory: {e}"
                )))
            }
        }

        fs::create_dir_all(&self.base_path).await.map_err(|e| {
            StorageError::WriteFailed(format!("Failed to recreate storage directory: {e}"))
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
                        "Failed to read directory: {e}"
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
    fn path_for_key(&self, key: &str) -> Result<PathBuf, StorageError> {
        let segments = Self::validate_key_segments(key)?;
        let mut path = self.base_path.clone();
        for segment in &segments[..segments.len().saturating_sub(1)] {
            path.push(Self::encode_key_segment(segment));
        }
        let last = segments
            .last()
            .ok_or_else(|| Self::invalid_key("key cannot be empty"))?;
        path.push(format!("{}.dat", Self::encode_key_segment(last)));
        Ok(path)
    }

    fn validate_key_segments(key: &str) -> Result<Vec<&str>, StorageError> {
        if key.is_empty() {
            return Err(Self::invalid_key("key cannot be empty"));
        }
        if key.starts_with('/') || key.starts_with('\\') {
            return Err(Self::invalid_key("key cannot be absolute"));
        }
        if key.contains('\0') {
            return Err(Self::invalid_key("key cannot contain NUL bytes"));
        }
        if key.contains('\\') {
            return Err(Self::invalid_key(
                "key cannot contain platform backslash separators",
            ));
        }

        let segments: Vec<&str> = key.split('/').collect();
        Self::validate_segments(&segments, false)?;
        Ok(segments)
    }

    fn validate_key_prefix(prefix: &str) -> Result<(), StorageError> {
        if prefix.is_empty() {
            return Ok(());
        }
        if prefix.starts_with('/') || prefix.starts_with('\\') {
            return Err(Self::invalid_key("key prefix cannot be absolute"));
        }
        if prefix.contains('\0') {
            return Err(Self::invalid_key("key prefix cannot contain NUL bytes"));
        }
        if prefix.contains('\\') {
            return Err(Self::invalid_key(
                "key prefix cannot contain platform backslash separators",
            ));
        }

        let segments: Vec<&str> = prefix.split('/').collect();
        Self::validate_segments(&segments, true)
    }

    fn validate_segments(
        segments: &[&str],
        allow_trailing_empty: bool,
    ) -> Result<(), StorageError> {
        for (index, segment) in segments.iter().enumerate() {
            let is_trailing_empty =
                allow_trailing_empty && index + 1 == segments.len() && segment.is_empty();
            if is_trailing_empty {
                continue;
            }
            if segment.is_empty() {
                return Err(Self::invalid_key("key cannot contain empty path segments"));
            }
            if *segment == "." || *segment == ".." {
                return Err(Self::invalid_key(
                    "key cannot contain current or parent directory segments",
                ));
            }
            if index == 0 && Self::is_windows_drive_prefix(segment) {
                return Err(Self::invalid_key(
                    "key cannot start with a Windows drive prefix",
                ));
            }
        }
        Ok(())
    }

    fn is_windows_drive_prefix(segment: &str) -> bool {
        let bytes = segment.as_bytes();
        bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
    }

    fn encode_key_segment(segment: &str) -> String {
        let mut encoded = String::with_capacity(segment.len());
        for byte in segment.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    encoded.push(byte as char)
                }
                _ => {
                    encoded.push('%');
                    encoded.push(Self::hex_digit(byte >> 4));
                    encoded.push(Self::hex_digit(byte & 0x0f));
                }
            }
        }
        encoded
    }

    fn decode_key_segment(segment: &str) -> Result<String, StorageError> {
        let bytes = segment.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'%' {
                decoded.push(bytes[index]);
                index += 1;
                continue;
            }
            if index + 2 >= bytes.len() {
                return Err(Self::invalid_key("stored key segment has invalid escape"));
            }
            let high = Self::hex_value(bytes[index + 1])
                .ok_or_else(|| Self::invalid_key("stored key segment has invalid escape"))?;
            let low = Self::hex_value(bytes[index + 2])
                .ok_or_else(|| Self::invalid_key("stored key segment has invalid escape"))?;
            decoded.push((high << 4) | low);
            index += 3;
        }
        String::from_utf8(decoded)
            .map_err(|_| Self::invalid_key("stored key segment is not valid UTF-8"))
    }

    fn hex_digit(value: u8) -> char {
        match value {
            0..=9 => (b'0' + value) as char,
            10..=15 => (b'A' + (value - 10)) as char,
            _ => '?',
        }
    }

    fn hex_value(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            b'A'..=b'F' => Some(value - b'A' + 10),
            _ => None,
        }
    }

    fn invalid_key(reason: impl Into<String>) -> StorageError {
        StorageError::InvalidKey {
            reason: reason.into(),
        }
    }

    async fn visit_entry_for_keys(
        base: &Path,
        entry: DirEntry,
        prefix: Option<&str>,
        stack: &mut Vec<PathBuf>,
        keys: &mut Vec<String>,
    ) -> Result<(), StorageError> {
        let file_type = entry.file_type().await.map_err(|e| {
            StorageError::ReadFailed(format!("Failed to stat directory entry: {e}"))
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
            StorageError::ReadFailed(format!("Failed to compute relative key path: {e}"))
        })?;
        let rel = rel.with_extension("");
        let decoded_segments = rel
            .components()
            .map(|component| {
                let segment = component.as_os_str().to_string_lossy();
                Self::decode_key_segment(&segment)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let key = decoded_segments.join("/");

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

    #[tokio::test]
    async fn filesystem_storage_rejects_path_traversal_keys() {
        let temp_dir = TempDir::new().unwrap();
        let handler = FilesystemStorageHandler::new(temp_dir.path().join("storage"));

        for key in [
            "",
            "../secret",
            "/tmp/x",
            "a/../../x",
            "C:/x",
            "C:\\x",
            "safe\\unsafe",
            "safe//unsafe",
            "safe/./unsafe",
            "safe/\0/unsafe",
        ] {
            let error = handler.store(key, b"blocked".to_vec()).await.unwrap_err();
            assert!(
                matches!(error, StorageError::InvalidKey { .. }),
                "expected InvalidKey for {key:?}, got {error:?}"
            );
        }

        assert!(!temp_dir.path().join("secret.dat").exists());
    }

    #[tokio::test]
    async fn filesystem_storage_safe_logical_prefixes_round_trip() {
        let temp_dir = TempDir::new().unwrap();
        let handler = FilesystemStorageHandler::new(temp_dir.path().join("storage"));

        handler
            .store("journal/facts/ota:proposal:1", b"proposal".to_vec())
            .await
            .unwrap();
        handler
            .store("journal/facts/ordinary", b"ordinary".to_vec())
            .await
            .unwrap();
        handler
            .store("other/facts/item", b"other".to_vec())
            .await
            .unwrap();

        assert_eq!(
            handler
                .retrieve("journal/facts/ota:proposal:1")
                .await
                .unwrap(),
            Some(b"proposal".to_vec())
        );
        assert!(handler.exists("journal/facts/ordinary").await.unwrap());

        let listed = handler.list_keys(Some("journal/facts/")).await.unwrap();
        assert_eq!(
            listed,
            vec![
                "journal/facts/ordinary".to_string(),
                "journal/facts/ota:proposal:1".to_string()
            ]
        );

        assert!(handler.remove("journal/facts/ordinary").await.unwrap());
        assert!(!handler.exists("journal/facts/ordinary").await.unwrap());
    }

    #[tokio::test]
    async fn filesystem_storage_batch_operations_validate_keys() {
        let temp_dir = TempDir::new().unwrap();
        let handler = FilesystemStorageHandler::new(temp_dir.path().join("storage"));
        let mut pairs = std::collections::HashMap::new();
        pairs.insert("valid/key".to_string(), b"ok".to_vec());
        pairs.insert("../escape".to_string(), b"bad".to_vec());

        let error = handler.store_batch(pairs).await.unwrap_err();
        assert!(matches!(error, StorageError::InvalidKey { .. }));

        let keys = vec!["valid/key".to_string(), "../escape".to_string()];
        let error = handler.retrieve_batch(&keys).await.unwrap_err();
        assert!(matches!(error, StorageError::InvalidKey { .. }));

        let error = handler.list_keys(Some("../")).await.unwrap_err();
        assert!(matches!(error, StorageError::InvalidKey { .. }));
    }
}
