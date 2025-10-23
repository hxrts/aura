// Encrypted chunk store with inline metadata and proof-of-storage
//
// NOTE: Storage is out of scope for Phase 1-3 (DKD, Resharing, Recovery protocols).
// This crate will be implemented in Phase 4 per 080_architecture_protocol_integration.md.

pub mod types;
// pub mod manifest;    // TODO Phase 4: Object manifests
// pub mod chunking;    // TODO Phase 4: Chunk management
// pub mod indexer;     // TODO Phase 4: Content indexing
pub mod challenge;
pub mod quota;

pub use types::*;
// pub use manifest::*;
// pub use indexer::*;
pub use challenge::*;
pub use quota::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Storage error: {0}")]
    Storage(String),
    
    #[error("Object not found: {0}")]
    NotFound(String),
    
    #[error("Quota exceeded")]
    QuotaExceeded,
}

pub type Result<T> = std::result::Result<T, StorageError>;

