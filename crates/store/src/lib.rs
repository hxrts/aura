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

#![allow(warnings, clippy::all)]

/// Capability-based access control manager
pub mod capability_manager;
/// Capability-driven storage implementation with access control
pub mod capability_storage;
/// Proof-of-storage challenge system for data integrity verification
pub mod challenge;
/// Chunk storage and encryption
pub mod chunk_store;
/// Chunking strategy for large objects
pub mod chunking;
/// Encryption utilities for storage
pub mod encryption;
/// Erasure coding for reliable distributed storage (Phase 6.3)
pub mod erasure;
/// Comprehensive error handling for production
pub mod error_handling;
/// Content indexing - updated for Phase 3 manifest structure
pub mod indexer;
/// Object manifest structure with capability-based access control
pub mod manifest;
/// Proof-of-storage verification with challenge-response (Phase 6.2)
pub mod proof_of_storage;
/// Storage quota management and enforcement
pub mod quota;
/// Replication to static peers
pub mod replicator;
/// Social replica placement with trust-based selection (Phase 6.1)
pub mod social_placement;
/// Storage via rendezvous relationships
pub mod social_storage;
/// Storage type definitions and enums
pub mod types;

pub use capability_manager::*;
pub use capability_storage::*;
pub use challenge::*;
pub use chunk_store::*;
pub use chunking::*;
pub use encryption::*;
pub use erasure::*;
pub use error_handling::*;
// pub use indexer::*;
pub use manifest::*;
pub use proof_of_storage::*;
pub use quota::*;
pub use replicator::*;
pub use social_placement::*;
pub use social_storage::*;
pub use types::*;

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
