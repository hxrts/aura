//! Storage Middleware System
//!
//! This module implements the algebraic effect-style middleware pattern for storage operations.
//! All storage functionality is implemented as composable middleware layers that can be
//! stacked and configured for different use cases.

pub mod stack;
pub mod handler;
pub mod encryption;
pub mod compression;
pub mod deduplication;
pub mod quota_management;
pub mod caching;
pub mod access_control;
pub mod replication;
pub mod integrity;

// Re-export core middleware types
pub use stack::{StorageMiddlewareStack, StorageStackBuilder, StorageMiddleware, StorageHandlerExt};
pub use handler::{StorageHandler, StorageOperation, StorageResult, ChunkInfo, StorageError, BaseStorageHandler};
pub use aura_types::{MiddlewareContext, MiddlewareResult};

// Re-export middleware implementations
pub use encryption::EncryptionMiddleware;
pub use compression::CompressionMiddleware;
pub use deduplication::DeduplicationMiddleware;
pub use quota_management::QuotaMiddleware;
pub use caching::CachingMiddleware;
pub use access_control::AccessControlMiddleware;
pub use replication::ReplicationMiddleware;
pub use integrity::IntegrityMiddleware;