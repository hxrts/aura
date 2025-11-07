//! Storage Middleware System
//!
//! This module implements the algebraic effect-style middleware pattern for storage operations.
//! All storage functionality is implemented as composable middleware layers that can be
//! stacked and configured for different use cases.

pub mod access_control;
pub mod caching;
pub mod compression;
pub mod deduplication;
pub mod encryption;
pub mod handler;
pub mod integrity;
pub mod quota_management;
pub mod replication;
pub mod stack;

// Re-export core middleware types
pub use aura_protocol::middleware::{MiddlewareContext, MiddlewareResult};
pub use handler::{
    BaseStorageHandler, ChunkInfo, StorageError, StorageHandler, StorageOperation, StorageResult,
};
pub use stack::{
    StorageHandlerExt, StorageMiddleware, StorageMiddlewareStack, StorageStackBuilder,
};

// Re-export middleware implementations
pub use access_control::AccessControlMiddleware;
pub use caching::CachingMiddleware;
pub use compression::CompressionMiddleware;
pub use deduplication::DeduplicationMiddleware;
pub use encryption::EncryptionMiddleware;
pub use integrity::IntegrityMiddleware;
pub use quota_management::QuotaMiddleware;
pub use replication::ReplicationMiddleware;
