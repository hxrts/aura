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
#[cfg(not(unix))]
use tokio::fs;
#[cfg(not(unix))]
use tokio::fs::DirEntry;

/// Filesystem-based storage handler for production use
///
/// This handler stores data as files on the local filesystem.
/// It is stateless and delegates all storage operations to the filesystem.
#[derive(Debug, Clone)]
pub struct FilesystemStorageHandler {
    /// Base directory for storage files
    base_path: PathBuf,
    #[cfg(unix)]
    directory: Option<crate::profile_directory::ProfileDirectory>,
    profile_owner: Option<std::sync::Arc<crate::profile_storage::OwnedProfileLease>>,
}

impl FilesystemStorageHandler {
    /// Create a new filesystem storage handler
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            base_path,
            profile_owner: None,
            #[cfg(unix)]
            directory: None,
        }
    }

    /// Bind the writer to the concrete selected profile's lifetime owner.
    pub fn retain_profile_owner(
        mut self,
        owner: std::sync::Arc<crate::profile_storage::OwnedProfileLease>,
    ) -> Result<Self, aura_core::effects::profile_storage::ProfileStorageError> {
        if !owner.matches_profile(&self.base_path)? {
            return Err(
                aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                    "filesystem writer differs from selected profile".into(),
                ),
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.base_path = std::fs::canonicalize(&self.base_path).map_err(|source| {
                aura_core::effects::profile_storage::ProfileStorageError::Io {
                    source: std::sync::Arc::new(source),
                }
            })?;
        }
        #[cfg(unix)]
        {
            self.directory = Some(owner.directory.clone());
        }
        self.profile_owner = Some(owner);

        Ok(self)
    }

    /// Alias for clarity; avoids relying on `new` naming in higher layers.
    pub fn from_path(base_path: PathBuf) -> Self {
        Self {
            base_path,
            profile_owner: None,
            #[cfg(unix)]
            directory: None,
        }
    }

    /// Create a new filesystem storage handler with default path
    pub fn with_default_path() -> Self {
        Self::new(PathBuf::from("./storage"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoragePublicationStage {
    Staged,
    Published,
    Durable,
}

impl FilesystemStorageHandler {
    fn descriptor_failure(operation: &str, source: std::io::Error) -> StorageError {
        StorageError::BackendFailure {
            operation: operation.into(),
            source: aura_core::AuraError::Storage {
                message: operation.into(),
                source: Some(std::sync::Arc::new(source)),
            },
        }
    }
    #[cfg(unix)]
    fn descriptor_directory(
        &self,
    ) -> Result<crate::profile_directory::ProfileDirectory, StorageError> {
        if let Some(directory) = &self.directory {
            return Ok(directory.clone());
        }
        std::fs::create_dir_all(&self.base_path)
            .map_err(|e| Self::descriptor_failure("create unowned storage base", e))?;
        crate::profile_directory::ProfileDirectory::open(&self.base_path)
            .map_err(|e| Self::descriptor_failure("open storage directory", e))
    }
    #[cfg(unix)]
    fn relative_file(&self, key: &str) -> Result<PathBuf, StorageError> {
        let path = self.path_for_key(key)?;
        path.strip_prefix(&self.base_path)
            .map(Path::to_path_buf)
            .map_err(|_| Self::invalid_key("profile key escapes base"))
    }
    async fn durable_store_with_checkpoint(
        &self,
        key: &str,
        value: Vec<u8>,
        checkpoint: impl Fn(StoragePublicationStage) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        #[cfg(unix)]
        {
            let directory = self.descriptor_directory()?;
            let relative = self.relative_file(key)?;
            let prepared = directory
                .prepare(&relative, &value)
                .map_err(|e| Self::descriptor_failure("prepare profile replacement", e))?;
            checkpoint(StoragePublicationStage::Staged)?;
            prepared
                .publish(false)
                .map_err(|e| Self::descriptor_failure("publish profile replacement", e))?;
            checkpoint(StoragePublicationStage::Published)?;
            prepared
                .acknowledge()
                .map_err(|e| Self::descriptor_failure("acknowledge profile replacement", e))?;
            checkpoint(StoragePublicationStage::Durable)?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            use tokio::io::AsyncWriteExt;
            let file_path = self.path_for_key(key)?;
            let parent = file_path
                .parent()
                .ok_or_else(|| StorageError::WriteFailed("missing storage parent".into()))?;
            let failure = |operation: &str, source: std::io::Error| StorageError::BackendFailure {
                operation: operation.into(),
                source: aura_core::AuraError::Storage {
                    message: operation.into(),
                    source: Some(std::sync::Arc::new(source)),
                },
            };
            if self.profile_owner.is_none() {
                fs::create_dir_all(&self.base_path)
                    .await
                    .map_err(|source| failure("create unowned storage base", source))?;
            }
            #[cfg(unix)]
            crate::profile_storage::create_contained_directory(
                &self.base_path,
                parent
                    .strip_prefix(&self.base_path)
                    .map_err(|_| Self::invalid_key("storage parent escapes profile"))?,
            )
            .map_err(|source| failure("create contained storage parent", source))?;
            #[cfg(not(unix))]
            fs::create_dir_all(parent)
                .await
                .map_err(|source| failure("create storage parent", source))?;
            let physical_base = fs::canonicalize(&self.base_path)
                .await
                .map_err(|source| failure("resolve physical storage base", source))?;
            let physical_parent = fs::canonicalize(parent)
                .await
                .map_err(|source| failure("resolve physical storage parent", source))?;
            if !physical_parent.starts_with(&physical_base) {
                return Err(StorageError::InvalidKey {
                    reason: "storage parent escapes the physical profile".into(),
                });
            }
            let file_path = physical_parent.join(file_path.file_name().ok_or_else(|| {
                StorageError::InvalidKey {
                    reason: "missing storage filename".into(),
                }
            })?);
            let parent = physical_parent.as_path();
            let durability_boundary = physical_base.parent().unwrap_or(&physical_base);

            // create_new rejects stale staging files and rare nonce collisions.
            // A live process never replaces another writer's staging bytes.
            let mut staging = None;
            for _ in 0..16 {
                use rand::RngCore;
                let mut nonce = [0u8; 16];
                rand::rngs::OsRng
                    .try_fill_bytes(&mut nonce)
                    .map_err(|source| StorageError::BackendFailure {
                        operation: "allocate storage staging nonce".into(),
                        source: aura_core::AuraError::Crypto {
                            message: "storage staging entropy failed".into(),
                            source: Some(std::sync::Arc::new(source)),
                        },
                    })?;
                let candidate = file_path.with_extension(format!("dat.tmp-{}", hex::encode(nonce)));
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                options.mode(0o600);
                match options.open(&candidate).await {
                    Ok(file) => {
                        staging = Some((candidate, file));
                        break;
                    }
                    Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(source) => return Err(failure("create exclusive storage staging", source)),
                }
            }
            let (staging_path, mut staging_file) = staging.ok_or_else(|| {
                failure(
                    "allocate storage staging",
                    std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "bounded staging attempts exhausted",
                    ),
                )
            })?;
            staging_file
                .write_all(&value)
                .await
                .map_err(|source| failure("write storage staging", source))?;
            staging_file
                .sync_all()
                .await
                .map_err(|source| failure("sync storage staging", source))?;
            drop(staging_file);
            checkpoint(StoragePublicationStage::Staged)?;
            // Never delete the original descriptor to repair a failed rename.
            // Cancelling an awaited OS operation can race its completion. Recovery
            // rereads the canonical record; an interrupted caller cannot infer absence.
            fs::rename(&staging_path, &file_path)
                .await
                .map_err(|source| failure("publish storage replacement", source))?;
            checkpoint(StoragePublicationStage::Published)?;
            // Persist the publication and newly created directory entries. A sync error
            // after rename is an uncertain outcome: the caller must reread and revalidate.
            let mut directory = Some(parent);
            while let Some(path) = directory {
                fs::File::open(path)
                    .await
                    .map_err(|source| failure("open storage durability directory", source))?
                    .sync_all()
                    .await
                    .map_err(|source| failure("sync storage durability directory", source))?;
                if path == durability_boundary {
                    break;
                }
                directory = path.parent();
            }
            checkpoint(StoragePublicationStage::Durable)?;
            Ok(())
        }
    }
}

#[async_trait]
impl StorageCoreEffects for FilesystemStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.durable_store_with_checkpoint(key, value, |_| Ok(()))
            .await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        #[cfg(unix)]
        {
            self.descriptor_directory()?
                .read(&self.relative_file(key)?, false)
                .map_err(|e| Self::descriptor_failure("read profile value", e))
        }
        #[cfg(not(unix))]
        {
            let file_path = self.path_for_key(key)?;

            if !file_path.exists() {
                return Ok(None);
            }

            let data = fs::read(&file_path)
                .await
                .map_err(|e| StorageError::ReadFailed(format!("Failed to read file: {e}")))?;

            Ok(Some(data))
        }
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        #[cfg(unix)]
        {
            self.descriptor_directory()?
                .remove(&self.relative_file(key)?)
                .map_err(|e| Self::descriptor_failure("remove profile value", e))
        }
        #[cfg(not(unix))]
        {
            let file_path = self.path_for_key(key)?;

            if !file_path.exists() {
                return Ok(false);
            }

            fs::remove_file(&file_path)
                .await
                .map_err(|e| StorageError::DeleteFailed(format!("Failed to remove file: {e}")))?;

            Ok(true)
        }
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        #[cfg(unix)]
        {
            if let Some(prefix) = prefix {
                Self::validate_key_prefix(prefix)?;
            }
            let mut keys = Vec::new();
            for relative in self
                .descriptor_directory()?
                .ordinary_files()
                .map_err(|e| Self::descriptor_failure("enumerate profile values", e))?
            {
                if relative.extension().and_then(|e| e.to_str()) != Some("dat") {
                    continue;
                }
                let relative = relative.with_extension("");
                let segments = relative
                    .components()
                    .map(|c| Self::decode_key_segment(&c.as_os_str().to_string_lossy()))
                    .collect::<Result<Vec<_>, _>>()?;
                let key = segments.join("/");
                if prefix.map_or(true, |prefix| key.starts_with(prefix)) {
                    keys.push(key);
                }
            }
            keys.sort();
            Ok(keys)
        }
        #[cfg(not(unix))]
        {
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
                    Self::visit_entry_for_keys(
                        &self.base_path,
                        entry,
                        prefix,
                        &mut stack,
                        &mut keys,
                    )
                    .await?;
                }
            }

            keys.sort();
            Ok(keys)
        }
    }
}

#[async_trait]
impl StorageExtendedEffects for FilesystemStorageHandler {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        #[cfg(unix)]
        {
            self.descriptor_directory()?
                .read(&self.relative_file(key)?, false)
                .map(|v| v.is_some())
                .map_err(|e| Self::descriptor_failure("inspect profile value", e))
        }
        #[cfg(not(unix))]
        {
            let file_path = self.path_for_key(key)?;
            Ok(file_path.exists())
        }
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
        #[cfg(unix)]
        {
            self.descriptor_directory()?
                .clear_data()
                .map_err(|e| Self::descriptor_failure("clear profile data", e))
        }
        #[cfg(not(unix))]
        {
            let io = |operation: &str, source: std::io::Error| StorageError::BackendFailure {
                operation: operation.into(),
                source: aura_core::AuraError::Storage {
                    message: operation.into(),
                    source: Some(std::sync::Arc::new(source)),
                },
            };
            // The profile lock inode is an infrastructure resource, not a data key.

            // Never remove/recreate its containing profile while a writer can live.
            let mut entries = match fs::read_dir(&self.base_path).await {
                Ok(entries) => entries,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(source) => return Err(io("read clear directory", source)),
            };
            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|source| io("read clear entry", source))?
            {
                if entry.file_name() == std::ffi::OsStr::new(".aura-profile-owner.lock")
                    || entry.file_name()
                        == std::ffi::OsStr::new(crate::profile_storage::SECURE_PROVIDER_DIRECTORY)
                {
                    continue;
                }
                let kind = entry
                    .file_type()
                    .await
                    .map_err(|source| io("inspect clear entry", source))?;
                if kind.is_dir() {
                    fs::remove_dir_all(entry.path()).await
                } else {
                    fs::remove_file(entry.path()).await
                }
                .map_err(|source| io("remove clear entry", source))?;
            }
            Ok(())
        }
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        #[cfg(unix)]
        {
            let directory = self.descriptor_directory()?;
            let mut key_count = 0;
            let mut total_size = 0u64;
            for path in directory
                .ordinary_files()
                .map_err(|e| Self::descriptor_failure("enumerate profile statistics", e))?
            {
                if path.extension().and_then(|e| e.to_str()) != Some("dat") {
                    continue;
                }
                let bytes = directory
                    .read(&path, false)
                    .map_err(|e| Self::descriptor_failure("read profile statistics", e))?
                    .ok_or_else(|| {
                        Self::invalid_key("profile value disappeared during statistics")
                    })?;
                key_count += 1;
                total_size = total_size.saturating_add(bytes.len() as u64);
            }
            Ok(StorageStats {
                key_count,
                total_size,
                available_space: None,
                backend_type: "filesystem".into(),
            })
        }
        #[cfg(not(unix))]
        {
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
                    if dir == self.base_path
                        && entry.file_name()
                            == std::ffi::OsStr::new(
                                crate::profile_storage::SECURE_PROVIDER_DIRECTORY,
                            )
                    {
                        continue;
                    }
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
        Self::require_ordinary_subtree(&segments)?;
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
        Self::validate_segments(&segments, true)?;
        Self::require_ordinary_subtree(&segments)
    }

    fn require_ordinary_subtree(segments: &[&str]) -> Result<(), StorageError> {
        if segments.first().copied() == Some(crate::profile_storage::SECURE_PROVIDER_DIRECTORY) {
            #[derive(Debug, thiserror::Error)]
            #[error("secure-provider subtree is outside ordinary storage scope")]
            struct SecureProviderSubtreeOutsideScope;
            return Err(StorageError::BackendFailure {
                operation: "ordinary storage physical scope".into(),
                source: aura_core::AuraError::PermissionDenied {
                    message: "ordinary storage cannot access the secure provider subtree".into(),
                    source: Some(std::sync::Arc::new(SecureProviderSubtreeOutsideScope)),
                },
            });
        }
        Ok(())
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
                    encoded.push(byte as char);
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

    #[cfg(not(unix))]
    async fn visit_entry_for_keys(
        base: &Path,
        entry: DirEntry,
        prefix: Option<&str>,
        stack: &mut Vec<PathBuf>,
        keys: &mut Vec<String>,
    ) -> Result<(), StorageError> {
        if entry.path().parent() == Some(base)
            && entry.file_name()
                == std::ffi::OsStr::new(crate::profile_storage::SECURE_PROVIDER_DIRECTORY)
        {
            return Ok(());
        }

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

#[cfg(all(test, unix))]
mod durable_publication_tests {
    use super::*;
    fn injected() -> StorageError {
        StorageError::BackendFailure {
            operation: "injected durability boundary failure".into(),
            source: aura_core::AuraError::Storage {
                message: "actual storage publication checkpoint".into(),
                source: Some(std::sync::Arc::new(std::io::Error::other(
                    "fixture checkpoint",
                ))),
            },
        }
    }
    #[tokio::test]
    async fn interruption_preserves_original_before_publication_and_complete_replacement_after() {
        let directory = tempfile::tempdir().unwrap();
        let storage = FilesystemStorageHandler::new(directory.path().into());
        storage
            .store("canonical/profile", b"original".to_vec())
            .await
            .unwrap();
        let failed = storage
            .durable_store_with_checkpoint("canonical/profile", b"prepared".to_vec(), |stage| {
                if stage == StoragePublicationStage::Staged {
                    Err(injected())
                } else {
                    Ok(())
                }
            })
            .await;
        assert!(failed.is_err());
        assert_eq!(
            storage
                .retrieve("canonical/profile")
                .await
                .unwrap()
                .unwrap(),
            b"original"
        );
        let failed = storage
            .durable_store_with_checkpoint("canonical/profile", b"replacement".to_vec(), |stage| {
                if stage == StoragePublicationStage::Published {
                    Err(injected())
                } else {
                    Ok(())
                }
            })
            .await;
        assert!(
            failed.is_err(),
            "publication failure must remain observable even if complete replacement exists"
        );
        drop(storage);
        let reopened = FilesystemStorageHandler::new(directory.path().into());
        assert_eq!(
            reopened
                .retrieve("canonical/profile")
                .await
                .unwrap()
                .unwrap(),
            b"replacement"
        );
        reopened
            .store("canonical/profile", b"durable".to_vec())
            .await
            .unwrap();
        assert_eq!(
            reopened
                .retrieve("canonical/profile")
                .await
                .unwrap()
                .unwrap(),
            b"durable"
        );
    }
    #[tokio::test]
    async fn relative_single_component_profile_uses_physical_durability_boundary() {
        let current = std::env::current_dir().unwrap();
        let directory = tempfile::Builder::new()
            .prefix("aura-relative-durable-")
            .tempdir_in(&current)
            .unwrap();
        let relative = directory
            .path()
            .strip_prefix(&current)
            .unwrap()
            .to_path_buf();
        assert_eq!(
            relative.components().count(),
            1,
            "fixture exercises an empty syntactic parent"
        );
        let storage = FilesystemStorageHandler::new(relative.clone());
        storage
            .store("nested/canonical-profile", b"durable".to_vec())
            .await
            .unwrap();
        drop(storage);
        let reopened = FilesystemStorageHandler::new(relative);
        assert_eq!(
            reopened
                .retrieve("nested/canonical-profile")
                .await
                .unwrap()
                .unwrap(),
            b"durable"
        );
    }

    #[tokio::test]
    async fn child_profile_publication() {
        use std::io::Write;
        let Some(path) = std::env::var_os("AURA_PROFILE_PUBLICATION_CHILD") else {
            return;
        };
        let requested = std::env::var("AURA_PROFILE_PUBLICATION_STAGE").unwrap();
        let _owner =
            crate::profile_storage::FilesystemProfileStorageHandler::new(path.clone().into())
                .acquire_owned_native()
                .unwrap();
        let storage = FilesystemStorageHandler::new(path.into());
        storage
            .durable_store_with_checkpoint("canonical/profile", b"replacement".to_vec(), |stage| {
                let stop = (requested == "staged" && stage == StoragePublicationStage::Staged)
                    || (requested == "published" && stage == StoragePublicationStage::Published);
                if stop {
                    println!("PROFILE_PUBLICATION_READY");
                    std::io::stdout().flush().unwrap();
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line).unwrap();
                }
                Ok(())
            })
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn actual_process_death_releases_owner_and_preserves_complete_descriptor() {
        use std::io::BufRead;
        for (stage, expected) in [
            ("staged", b"original".as_slice()),
            ("published", b"replacement".as_slice()),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let storage = FilesystemStorageHandler::new(directory.path().into());
            storage
                .store("canonical/profile", b"original".to_vec())
                .await
                .unwrap();
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage::durable_publication_tests::child_profile_publication",
                    "--nocapture",
                ])
                .env("AURA_PROFILE_PUBLICATION_CHILD", directory.path())
                .env("AURA_PROFILE_PUBLICATION_STAGE", stage)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let output = child.stdout.take().unwrap();
            let (sender, receiver) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in std::io::BufReader::new(output).lines() {
                    match line {
                        Ok(line) if line == "PROFILE_PUBLICATION_READY" => {
                            let _ = sender.send(());
                            return;
                        }
                        Err(_) => return,
                        _ => {}
                    }
                }
            });
            if receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .is_err()
            {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                panic!("child did not acknowledge exact durable boundary");
            }
            reader.join().unwrap();
            child.kill().unwrap();
            child.wait().unwrap();
            let _restarted = crate::profile_storage::FilesystemProfileStorageHandler::new(
                directory.path().into(),
            )
            .acquire_owned_native()
            .unwrap();
            let reopened = FilesystemStorageHandler::new(directory.path().into());
            assert_eq!(
                reopened
                    .retrieve("canonical/profile")
                    .await
                    .unwrap()
                    .unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn failed_publication_does_not_delete_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let storage = FilesystemStorageHandler::new(directory.path().into());
        let destination = storage.path_for_key("canonical/profile").unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(destination.join("sentinel"), b"keep").unwrap();
        let error = storage
            .store("canonical/profile", b"replacement".to_vec())
            .await
            .unwrap_err();
        assert!(matches!(error, StorageError::BackendFailure { .. }));
        assert_eq!(
            std::fs::read(destination.join("sentinel")).unwrap(),
            b"keep"
        );
    }
}
#[cfg(all(test, unix))]
mod descriptor_owner_tests {
    use super::*;
    use aura_core::effects::{
        SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    };
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    #[tokio::test]
    async fn child_keeps_original_profile_after_cwd_change() -> TestResult {
        let Some(foreign) = std::env::var_os("AURA_DESCRIPTOR_CWD_CHILD") else {
            return Ok(());
        };
        let path = PathBuf::from("profile");
        let owner = std::sync::Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(path.clone())
                .acquire_owned_native()?,
        );
        let storage =
            FilesystemStorageHandler::new(path.clone()).retain_profile_owner(owner.clone())?;
        let cloned = storage.clone();
        let secure =
            crate::secure::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner,
            )?;
        let location = SecureStorageLocation::new("original", "secret");
        let caps = [
            SecureStorageCapability::Write,
            SecureStorageCapability::Read,
            SecureStorageCapability::Delete,
        ];
        storage.store("canonical/value", b"before".to_vec()).await?;
        secure
            .secure_store(&location, b"original-secret", &caps)
            .await?;
        // Only this isolated child changes cwd. Its adapter clones retain the
        // original leased directory and provider's exact wrapping key.
        std::env::set_current_dir(foreign)?;
        assert_eq!(
            cloned.retrieve("canonical/value").await?,
            Some(b"before".to_vec())
        );
        cloned.store("canonical/value", b"after".to_vec()).await?;
        assert_eq!(
            secure.secure_retrieve(&location, &caps).await?,
            b"original-secret"
        );
        secure
            .secure_store(&location, b"after-secret", &caps)
            .await?;
        storage.store("remove/item", b"delete".to_vec()).await?;
        assert!(cloned.remove("remove/item").await?);
        assert!(!storage.exists("remove/item").await?);
        Ok(())
    }
    #[tokio::test]
    async fn actual_child_cwd_change_keeps_both_original_backends() -> TestResult {
        let origin = tempfile::tempdir()?;
        let foreign = tempfile::tempdir()?;
        std::fs::create_dir_all(foreign.path().join("profile/canonical"))?;
        std::fs::write(
            foreign.path().join("profile/canonical/value.dat"),
            b"foreign-sentinel",
        )?;
        let status = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "storage::descriptor_owner_tests::child_keeps_original_profile_after_cwd_change",
                "--nocapture",
            ])
            .current_dir(origin.path())
            .env("AURA_DESCRIPTOR_CWD_CHILD", foreign.path())
            .status()?;
        assert!(status.success());
        assert_eq!(
            std::fs::read(foreign.path().join("profile/canonical/value.dat"))?,
            b"foreign-sentinel"
        );
        assert!(!foreign.path().join("profile/secure_store").exists());
        let path = origin.path().join("profile");
        let owner = std::sync::Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(path.clone())
                .acquire_owned_native()?,
        );
        let storage =
            FilesystemStorageHandler::new(path.clone()).retain_profile_owner(owner.clone())?;
        assert_eq!(
            storage.retrieve("canonical/value").await?,
            Some(b"after".to_vec())
        );
        let secure =
            crate::secure::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner,
            )?;
        assert_eq!(
            secure
                .secure_retrieve(
                    &SecureStorageLocation::new("original", "secret"),
                    &[SecureStorageCapability::Read]
                )
                .await?,
            b"after-secret"
        );
        Ok(())
    }
    #[tokio::test]
    async fn replaced_profile_path_cannot_redirect_live_descriptors() -> TestResult {
        let temp = tempfile::tempdir()?;
        let foreign = tempfile::tempdir()?;
        let original = temp.path().join("profile");
        let moved = temp.path().join("retained");
        let owner = std::sync::Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(original.clone())
                .acquire_owned_native()?,
        );
        let storage =
            FilesystemStorageHandler::new(original.clone()).retain_profile_owner(owner.clone())?;
        let secure =
            crate::secure::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner,
            )?;
        std::fs::rename(&original, &moved)?;
        std::os::unix::fs::symlink(foreign.path(), &original)?;
        storage.store("actual/value", b"retained".to_vec()).await?;
        secure
            .secure_store(
                &SecureStorageLocation::new("actual", "secret"),
                b"retained-secret",
                &[SecureStorageCapability::Write],
            )
            .await?;
        assert_eq!(std::fs::read(moved.join("actual/value.dat"))?, b"retained");
        assert!(!foreign.path().join("actual").exists());
        assert!(!foreign.path().join("secure_store").exists());
        assert_eq!(
            secure
                .secure_retrieve(
                    &SecureStorageLocation::new("actual", "secret"),
                    &[SecureStorageCapability::Read]
                )
                .await?,
            b"retained-secret"
        );
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod secure_provider_scope_tests {
    use super::*;
    use aura_core::effects::{
        SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    };

    #[tokio::test]
    async fn ordinary_io_and_clear_cannot_cross_selected_secure_provider_subtree(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let owner = std::sync::Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(
                temporary.path().to_path_buf(),
            )
            .acquire_owned_native()?,
        );
        let ordinary = FilesystemStorageHandler::new(temporary.path().to_path_buf())
            .retain_profile_owner(owner.clone())?;
        let secure =
            crate::secure::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                owner,
            )?;
        // The exact physical filename previously collided with an ordinary
        // key's mandatory .dat suffix; both providers use their actual owner.
        let location = SecureStorageLocation::new("test_scope", "secret.dat");
        let caps = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        secure
            .secure_store_immutable(&location, b"first decision", &caps)
            .await?;
        ordinary
            .store("ordinary/value", b"ordinary".to_vec())
            .await?;
        assert!(ordinary
            .store("secure_store/test_scope/secret", b"replacement".to_vec())
            .await
            .is_err());
        assert!(ordinary
            .retrieve("secure_store/test_scope/secret")
            .await
            .is_err());
        assert!(ordinary
            .remove("secure_store/test_scope/secret")
            .await
            .is_err());
        assert!(ordinary
            .exists("secure_store/test_scope/secret")
            .await
            .is_err());
        assert!(ordinary.list_keys(Some("secure_store/")).await.is_err());
        assert_eq!(ordinary.list_keys(None).await?, vec!["ordinary/value"]);
        assert_eq!(ordinary.stats().await?.key_count, 1);
        ordinary.clear_all().await?;
        assert_eq!(ordinary.retrieve("ordinary/value").await?, None);
        assert_eq!(
            secure.secure_retrieve(&location, &caps).await?,
            b"first decision"
        );
        drop(ordinary);
        drop(secure);
        let reopened_owner = std::sync::Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(
                temporary.path().to_path_buf(),
            )
            .acquire_owned_native()?,
        );
        let reopened =
            crate::secure::ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                reopened_owner,
            )?;
        assert_eq!(
            reopened.secure_retrieve(&location, &caps).await?,
            b"first decision",
            "ordinary cleanup preserves the original wrapping key and record after reopen"
        );
        Ok(())
    }
}
