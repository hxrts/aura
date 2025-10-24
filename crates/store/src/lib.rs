//! Aura Store: Capability-driven storage layer
//!
//! This crate provides encrypted, capability-protected data storage for the Aura platform.
//! It implements content-addressed storage with proof-of-storage challenges and capability-based
//! access control.
//!
//! ## Features
//!
//! - **Capability-based access control**: All storage operations require proper capabilities
//! - **Content encryption**: Data is encrypted using application secrets from CGKA
//! - **Proof-of-storage**: Challenge-response system to verify data integrity
//! - **Quota management**: Per-device storage quotas with configurable limits
//! - **Audit logging**: Complete access logs for security and compliance
//!
//! ## Architecture
//!
//! The storage layer is built around several key components:
//! - [`CapabilityStorage`]: Main storage interface with capability checking
//! - Challenge system for proof-of-storage verification
//! - Quota management for storage limits
//! - Content indexing and metadata management
//!
//! ## Note
//!
//! Storage is out of scope for Phase 1-3 (DKD, Resharing, Recovery protocols).
//! This crate will be implemented in Phase 4 per 080_architecture_protocol_integration.md.

/// Storage type definitions and enums
pub mod types;
// pub mod manifest;    // TODO Phase 4: Object manifests
// pub mod chunking;    // TODO Phase 4: Chunk management
// pub mod indexer;     // TODO Phase 4: Content indexing
/// Capability-driven storage implementation with access control
pub mod capability_storage;
/// Proof-of-storage challenge system for data integrity verification
pub mod challenge;
/// Storage quota management and enforcement
pub mod quota;

pub use types::*;
// pub use manifest::*;
// pub use indexer::*;
pub use capability_storage::*;
pub use challenge::*;
pub use quota::*;

use thiserror::Error;

/// Storage layer error types
#[derive(Error, Debug)]
pub enum StorageError {
    /// General storage operation error
    #[error("Storage error: {0}")]
    Storage(String),

    /// Requested object was not found in storage
    #[error("Object not found: {0}")]
    NotFound(String),

    /// Storage quota has been exceeded
    #[error("Quota exceeded")]
    QuotaExceeded,
}

/// Result type for storage operations
pub type Result<T> = std::result::Result<T, StorageError>;
