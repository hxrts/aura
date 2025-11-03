//! Storage effects for file I/O operations

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Storage location wrapper
#[derive(Debug, Clone)]
pub struct StorageLocation {
    path: PathBuf,
}

impl StorageLocation {
    /// Create a new storage location
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Create from path
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self::new(path)
    }

    /// Get the path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get path as string
    pub fn as_str(&self) -> &str {
        self.path.to_str().unwrap_or("")
    }
}

/// Storage operation errors
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Failed to read file contents
    #[error("Failed to read file: {0}")]
    ReadFailed(String),
    /// Failed to write file contents
    #[error("Failed to write file: {0}")]
    WriteFailed(String),
    /// Failed to delete file
    #[error("Failed to delete file: {0}")]
    DeleteFailed(String),
    /// Failed to list files in directory
    #[error("Failed to list files: {0}")]
    ListFailed(String),
    /// File not found at the specified location
    #[error("File not found: {0}")]
    NotFound(String),
    /// Permission denied for storage operation
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

/// Storage effects interface for file I/O operations
pub trait StorageEffects {
    /// Read file contents from the specified location
    ///
    /// # Arguments
    /// * `location` - The storage location of the file to read
    fn read_file(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>, StorageError>> + Send + '_>>;

    /// Write data to file at the specified location
    ///
    /// Creates parent directories as needed.
    ///
    /// # Arguments
    /// * `location` - The storage location where the file should be written
    /// * `data` - The data to write to the file
    fn write_file(
        &self,
        location: StorageLocation,
        data: &[u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + '_>>;

    /// Delete file at the specified location
    ///
    /// # Arguments
    /// * `location` - The storage location of the file to delete
    fn delete_file(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + '_>>;

    /// List all files in a directory
    ///
    /// # Arguments
    /// * `location` - The storage location of the directory to list
    fn list_files(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = Result<Vec<StorageLocation>, StorageError>> + Send + '_>,
    >;
}

/// Production storage effects using real filesystem
///
/// Performs actual file I/O operations on the system's filesystem.
pub struct ProductionStorageEffects;

impl ProductionStorageEffects {
    /// Create a new production storage effects instance
    pub fn new() -> Self {
        Self
    }
}

impl StorageEffects for ProductionStorageEffects {
    fn read_file(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>, StorageError>> + Send + '_>> {
        Box::pin(async move {
            std::fs::read(location.path()).map_err(|e| StorageError::ReadFailed(e.to_string()))
        })
    }

    fn write_file(
        &self,
        location: StorageLocation,
        data: &[u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + '_>> {
        let data = data.to_vec(); // Clone for move
        Box::pin(async move {
            if let Some(parent) = location.path().parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
            }
            std::fs::write(location.path(), data)
                .map_err(|e| StorageError::WriteFailed(e.to_string()))
        })
    }

    fn delete_file(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + '_>> {
        Box::pin(async move {
            std::fs::remove_file(location.path())
                .map_err(|e| StorageError::DeleteFailed(e.to_string()))
        })
    }

    fn list_files(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = Result<Vec<StorageLocation>, StorageError>> + Send + '_>,
    > {
        Box::pin(async move {
            match std::fs::read_dir(location.path()) {
                Ok(entries) => {
                    let mut files = Vec::new();
                    for entry in entries {
                        match entry {
                            Ok(entry) => files.push(StorageLocation::from_path(entry.path())),
                            Err(e) => return Err(StorageError::ListFailed(e.to_string())),
                        }
                    }
                    Ok(files)
                }
                Err(e) => Err(StorageError::ListFailed(e.to_string())),
            }
        })
    }
}

/// Test storage effects using in-memory filesystem
///
/// Provides a mock filesystem implementation using a HashMap for testing
/// file I/O operations without accessing the actual filesystem.
pub struct TestStorageEffects {
    /// In-memory store of files mapped from path to contents
    files: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl TestStorageEffects {
    /// Create a new test storage effects instance with an empty filesystem
    pub fn new() -> Self {
        Self {
            files: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl StorageEffects for TestStorageEffects {
    fn read_file(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>, StorageError>> + Send + '_>> {
        let files = self.files.clone();
        Box::pin(async move {
            let files = files.read().unwrap();
            let path_str = location.path().to_string_lossy().to_string();
            files
                .get(&path_str)
                .cloned()
                .ok_or_else(|| StorageError::NotFound(path_str))
        })
    }

    fn write_file(
        &self,
        location: StorageLocation,
        data: &[u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + '_>> {
        let files = self.files.clone();
        let data = data.to_vec();
        Box::pin(async move {
            let mut files = files.write().unwrap();
            let path_str = location.path().to_string_lossy().to_string();
            files.insert(path_str, data);
            Ok(())
        })
    }

    fn delete_file(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + '_>> {
        let files = self.files.clone();
        Box::pin(async move {
            let mut files = files.write().unwrap();
            let path_str = location.path().to_string_lossy().to_string();
            files
                .remove(&path_str)
                .map(|_| ())
                .ok_or_else(|| StorageError::NotFound(path_str))
        })
    }

    fn list_files(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = Result<Vec<StorageLocation>, StorageError>> + Send + '_>,
    > {
        let files = self.files.clone();
        Box::pin(async move {
            let files = files.read().unwrap();
            let base_path = location.path().to_string_lossy();
            let mut result = Vec::new();

            for path in files.keys() {
                if path.starts_with(&*base_path) {
                    result.push(StorageLocation::from_path(path.clone()));
                }
            }
            Ok(result)
        })
    }
}
