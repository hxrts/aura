//! # Aura Store - Layer 2: Specification (Domain Crate)
//!
//! **Purpose**: Define storage domain types, semantics, and capability-based access control.
//!
//! This crate provides storage domain types, semantics, and pure logic for the Aura platform.
//!
//! # Architecture Constraints
//!
//! **Layer 2 depends only on aura-core** (foundation).
//! - YES Storage domain types and semantics
//! - YES Capability-based access control logic
//! - YES Content-addressed storage abstraction
//! - YES Pure functions for storage operations
//! - NO effect handler implementations (use StorageEffects from aura-effects)
//! - NO handler composition (that's aura-composition)
//! - NO multi-party protocol logic (that's aura-protocol)
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

/// Encrypted local storage for CLI/TUI preferences
pub mod local;

// Biscuit-based storage authorization moved to aura-wot (proper domain)

// Re-export core types from aura-core
pub use aura_core::{ChunkId, ContentId, ContentSize};

// Re-export main APIs
pub use capabilities::{AccessDecision, StorageCapability, StoragePermission, StorageResource};
pub use chunk::{
    compute_chunk_layout, plan_chunk_layout_from_size, ChunkLayout, ChunkManifest, ContentManifest,
    ErasureConfig,
};
pub use crdt::{StorageIndex, StorageOpLog, StorageState};
pub use errors::StorageError;
pub use local::{
    ContactCache, LocalData, LocalStore, LocalStoreConfig, LocalStoreError, ThemePreference,
};
pub use search::{SearchIndexEntry, SearchQuery, SearchResults, SearchScope};

// Biscuit authorization APIs now available from aura-wot
// Import like: use aura_wot::{BiscuitStorageEvaluator, StoragePermission, etc.}
