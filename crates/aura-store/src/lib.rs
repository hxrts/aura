//! Aura Storage Domain Layer
//!
//! This crate provides storage domain types, semantics, and pure logic for the Aura platform.
//! It follows the 8-layer architecture as a Layer 2 (Specification/Domain) crate.
//!
//! ## Architecture Position
//!
//! - **Layer 2 - Domain/Specification**: Pure storage types and logic
//! - **Dependencies**: Only `aura-core` (foundation layer)
//! - **Responsibilities**: Storage domain concepts, no effect handlers or coordination
//!
//! ## Core Concepts
//!
//! - **Content Addressing**: Content-addressed storage with cryptographic chunk IDs
//! - **Capability-Based Access**: Storage permissions using meet-semilattice operations
//! - **Search Domain Types**: Query types and result filtering logic
//! - **Storage Semantics**: Pure functions for storage operations and access control
//!
//! ## What's NOT in this crate
//!
//! - Effect handlers (belong in `aura-effects`)
//! - Coordination logic (belongs in `aura-protocol`)
//! - Choreographic protocols (belong in `aura-storage`)
//! - Async execution (pure synchronous domain logic)

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Content addressing and chunk management types
pub mod chunk;

/// Storage capability types and access control logic
pub mod capabilities;

/// Search query types and result filtering logic
pub mod search;

/// Storage-specific CRDT types and operations
pub mod crdt;

/// Unified storage error types
pub mod errors;

/// Biscuit-based storage authorization
pub mod biscuit_authorization;

// Re-export core types from aura-core
pub use aura_core::{ChunkId, ContentId, ContentSize};

// Re-export main APIs
pub use capabilities::{
    AccessDecision, StorageCapability, StorageCapabilitySet, StoragePermission, StorageResource,
};
pub use chunk::{compute_chunk_layout, ChunkLayout, ChunkManifest, ContentManifest, ErasureConfig};
pub use crdt::{StorageIndex, StorageOpLog, StorageState};
pub use errors::StorageError;
pub use search::{FilteredResults, SearchIndexEntry, SearchQuery, SearchResults, SearchScope};

// Re-export Biscuit authorization APIs
pub use biscuit_authorization::{
    check_biscuit_access, evaluate_biscuit_access, BiscuitAccessRequest, BiscuitStorageError,
    BiscuitStorageEvaluator, PermissionMappings,
};
