//! Aura Store: Capability-driven storage layer
//!
//! This crate provides encrypted, capability-protected data storage for the Aura platform.
//! It implements content-addressed storage with proof-of-storage challenges and capability-based
//! access control.
//!
//! ## Architecture
//!
//! The storage layer is organized into clear domains:
//!
//! - **content/**: Content processing (chunking, encryption, erasure coding)
//! - **manifest/**: Object metadata and capability definitions
//! - **access_control/**: Capability-based access control verification
//! - **storage/**: Core storage operations (chunks, quotas, indexing)
//! - **replication/**: Data durability through replication strategies
//!   - static_replication: Replication to static peers
//!   - social/: SSB-based trust and peer selection
//!   - verification/: Proof-of-storage challenges
//! - **error.rs**: Unified error types and recovery strategies
//!
//! ## Features
//!
//! - **Capability-based access control**: All storage operations require proper capabilities
//! - **Content encryption**: Data is encrypted using device-derived keys
//! - **Proof-of-storage**: Challenge-response system to verify data integrity
//! - **Quota management**: Per-device storage quotas with configurable limits
//!
//! ## Note
//!
//! Storage is out of scope for Phase 1-3 (DKD, Resharing, Recovery protocols).
//! This crate will be implemented in Phase 4 per 080_architecture_protocol_integration.md.

#![allow(warnings, clippy::all)]

// Unified error module (replaces error_handling.rs)
pub mod error;

// Core domains
pub mod access_control;
pub mod content;
pub mod manifest;
pub mod replication;
pub mod storage;

// Legacy modules (being refactored - keep for compatibility during transition)
// Temporarily disabled - uses old Permission enum API
// pub mod capability_manager;
pub mod challenge;
pub mod proof_of_storage;
pub mod replicator;
pub mod social_placement;
pub mod social_storage;
pub mod types;

// Re-export error types (new unified interface)
pub use error::{ErrorContext, ErrorSeverity, Result, StoreError, StoreErrorBuilder};

// Backward compatibility: old error type names
pub use error::StoreError as StorageError;

// Re-export commonly used types from domains
pub use access_control::{CapabilityChecker, CapabilityManager, CapabilityToken};
pub use content::{chunking, encryption, erasure};
pub use manifest::{AccessControl, ChunkingParams, KeyDerivationSpec, ObjectManifest};
pub use storage::{ChunkId, ChunkStore, Indexer, QuotaTracker};
