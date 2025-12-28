//! # Aura Store - Layer 2: Specification (Domain Crate)
//!
//! **Purpose**: Define storage domain types, semantics, and fact-based state management.
//!
//! This crate provides storage domain types, semantics, and pure logic for the Aura platform.
//! Storage operations are recorded as immutable facts for journal integration.
//!
//! # Architecture Constraints
//!
//! **Layer 2 depends only on aura-core** (foundation).
//! - ✅ Storage domain types and semantics
//! - ✅ Fact types for journal integration (`StorageFact`)
//! - ✅ Content-addressed storage abstraction
//! - ✅ CRDT types for distributed storage state
//! - ✅ Capability metadata types (not authorization logic)
//! - ❌ NO effect handler implementations (use StorageEffects from aura-effects)
//! - ❌ NO handler composition (that's aura-composition)
//! - ❌ NO multi-party protocol logic (that's aura-protocol)
//!
//! ## Core Concepts
//!
//! - **Content Addressing**: Content-addressed storage with cryptographic chunk IDs
//! - **Fact-Based State**: Storage changes recorded as `StorageFact` for journals
//! - **CRDT Storage State**: Distributed storage state with join-semilattice merge
//! - **Authority Model**: Operations attributed to `AuthorityId`, not devices
//! - **Search Domain Types**: Query types and result filtering logic
//!
//! ## Authorization
//!
//! Storage capability types (`StorageCapability`, `StorageResource`) are **metadata**
//! describing required access levels. Actual authorization is performed via Biscuit
//! tokens - see `aura-authorization` for the authorization implementation.
//!
//! ## What's NOT in this crate
//!
//! - Effect handlers (belong in `aura-effects`)
//! - Coordination logic (belongs in `aura-protocol`)
//! - Choreographic protocols (belong in feature crates)
//! - Async execution (pure synchronous domain logic)

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Content addressing and chunk management types
pub mod chunk;

/// Storage capability metadata types
pub mod capabilities;

/// Storage domain facts for journal integration
pub mod facts;

/// Search query types and result filtering logic
pub mod search;

/// Storage-specific CRDT types and operations
pub mod crdt;

/// Unified storage error types
pub mod errors;

// Biscuit-based storage authorization moved to aura-authorization (proper domain)

// Re-export core types from aura-core
pub use aura_core::{ChunkId, ContentId, ContentSize};

// Re-export main APIs
pub use capabilities::{AccessDecision, StorageCapability, StoragePermission, StorageResource};
pub use chunk::{
    compute_chunk_layout, plan_chunk_layout_from_size, ChunkLayout, ChunkManifest, ContentManifest,
    ErasureConfig,
};
pub use crdt::{StorageIndex, StorageOpLog, StorageOpType, StorageOperation, StorageState};
pub use errors::StorageError;
pub use facts::{StorageFact, StorageFactDelta, StorageFactReducer, STORAGE_FACT_TYPE_ID};
pub use search::{SearchIndexEntry, SearchQuery, SearchResults, SearchScope};

// Biscuit authorization APIs now available from aura-authorization
// Import like: use aura_authorization::{BiscuitStorageEvaluator, StoragePermission, etc.}
