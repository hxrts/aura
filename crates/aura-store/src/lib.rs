//! Aura Storage Layer - Middleware-Based Architecture
//!
//! This crate implements the storage layer for Aura using the algebraic effect-style
//! middleware pattern. All storage functionality is provided through composable
//! middleware layers that can be stacked and configured for different use cases.
//!
//! ## Architecture
//!
//! The storage layer follows these principles:
//! - **Composable Middleware**: Stack storage concerns like encryption, compression, deduplication
//! - **Effects-Based**: All I/O operations go through the effects system for testability
//! - **Type-Safe Composition**: Middleware stacks are validated at compile time
//! - **Zero Legacy Code**: Clean implementation using foundation patterns from aura-types
//!
//! ## Example Usage
//!
//! ```rust
//! use aura_store::middleware::*;
//! use aura_types::effects::DefaultEffects;
//! use std::collections::HashMap;
//!
//! // Create a storage handler with middleware stack
//! let mut storage = BaseStorageHandler::new("/tmp/storage".to_string())
//!     .layer(EncryptionMiddleware::new().add_key("key1".to_string(), vec![0x42; 32]))
//!     .add_middleware(Box::new(CompressionMiddleware::new()))
//!     .add_middleware(Box::new(DeduplicationMiddleware::new()))
//!     .add_middleware(Box::new(QuotaMiddleware::new()));
//!
//! // Execute storage operations
//! let effects = DefaultEffects::new("storage-device".into());
//! let operation = StorageOperation::Store {
//!     chunk_id: "chunk-123".to_string(),
//!     data: b"Hello, World!".to_vec(),
//!     metadata: HashMap::new(),
//! };
//!
//! let result = storage.execute(operation, &effects)?;
//! ```
//!
//! ## Available Middleware
//!
//! - **EncryptionMiddleware**: Transparent encryption/decryption with multiple algorithms
//! - **CompressionMiddleware**: Data compression with configurable algorithms and thresholds
//! - **DeduplicationMiddleware**: Content-based deduplication to save storage space
//! - **QuotaMiddleware**: Storage quota enforcement and usage tracking
//! - **CachingMiddleware**: LRU caching for frequently accessed data
//! - **AccessControlMiddleware**: Permission-based access control
//! - **ReplicationMiddleware**: Data replication across multiple nodes
//! - **IntegrityMiddleware**: Data integrity verification with checksums

pub mod middleware;

// Re-export core types for convenience
pub use middleware::{
    // Core traits and types
    StorageHandler, StorageOperation, StorageResult, ChunkInfo,
    StorageMiddleware, StorageMiddlewareStack, StorageStackBuilder,
    
    // Base handler
    BaseStorageHandler,
    
    // Middleware implementations
    EncryptionMiddleware, CompressionMiddleware, DeduplicationMiddleware,
    QuotaMiddleware, CachingMiddleware, AccessControlMiddleware,
    ReplicationMiddleware, IntegrityMiddleware,
    
    // Extension trait for fluent composition
    StorageHandlerExt,
};

// Re-export foundation types
pub use aura_types::{MiddlewareContext, MiddlewareResult};
pub use aura_types::effects::AuraEffects;

/// Storage layer error type
pub use middleware::handler::StorageError;

/// Convenience function to create a standard storage stack
pub fn create_standard_storage_stack(
    storage_path: String,
    encryption_key: Option<Vec<u8>>,
) -> StorageMiddlewareStack {
    let base_handler = BaseStorageHandler::new(storage_path);
    let mut stack_builder = StorageStackBuilder::new();
    
    // Add standard middleware layers in order
    stack_builder = stack_builder.add_layer(Box::new(IntegrityMiddleware::new()));
    stack_builder = stack_builder.add_layer(Box::new(DeduplicationMiddleware::new()));
    stack_builder = stack_builder.add_layer(Box::new(CompressionMiddleware::new()));
    
    if let Some(key) = encryption_key {
        let encryption = EncryptionMiddleware::new()
            .add_key("default".to_string(), key);
        stack_builder = stack_builder.add_layer(Box::new(encryption));
    }
    
    stack_builder = stack_builder.add_layer(Box::new(QuotaMiddleware::new()));
    stack_builder = stack_builder.add_layer(Box::new(CachingMiddleware::new(1024 * 1024))); // 1MB cache
    
    stack_builder.build(Box::new(base_handler))
}

/// Convenience function to create a high-performance storage stack
pub fn create_performance_storage_stack(
    storage_path: String,
    cache_size: usize,
) -> StorageMiddlewareStack {
    let base_handler = BaseStorageHandler::new(storage_path);
    
    StorageStackBuilder::new()
        .add_layer(Box::new(CachingMiddleware::new(cache_size)))
        .add_layer(Box::new(DeduplicationMiddleware::new()))
        .add_layer(Box::new(CompressionMiddleware::new()))
        .build(Box::new(base_handler))
}

/// Convenience function to create a secure storage stack
pub fn create_secure_storage_stack(
    storage_path: String,
    encryption_key: Vec<u8>,
) -> StorageMiddlewareStack {
    let base_handler = BaseStorageHandler::new(storage_path);
    let encryption = EncryptionMiddleware::new()
        .add_key("secure".to_string(), encryption_key);
    
    StorageStackBuilder::new()
        .add_layer(Box::new(AccessControlMiddleware::new()))
        .add_layer(Box::new(IntegrityMiddleware::new()))
        .add_layer(Box::new(encryption))
        .add_layer(Box::new(ReplicationMiddleware::new(3)))
        .build(Box::new(base_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    
    #[test]
    fn test_storage_middleware_composition() {
        let storage = create_standard_storage_stack(
            "/tmp/test-storage".to_string(),
            Some(vec![0x42; 32])
        );
        
        let info = storage.stack_info();
        assert!(info.contains_key("middleware_count"));
        assert!(info.contains_key("middleware_layers"));
    }
    
    #[test]
    fn test_fluent_middleware_composition() {
        use middleware::handler::BaseStorageHandler;
        use middleware::StorageHandlerExt;
        
        let _storage = BaseStorageHandler::new("/tmp/test".to_string())
            .layer(CompressionMiddleware::new());
        
        // Test compiles successfully
    }
}