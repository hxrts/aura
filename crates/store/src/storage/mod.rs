//! Storage Operations Domain
//!
//! This domain handles core storage operations for chunk persistence and retrieval:
//! - **Chunk Storage**: Low-level encrypted chunk persistence with deduplication
//! - **Quota Management**: Enforcing storage limits with LRU eviction strategies
//! - **Indexing**: Content indexing and manifest tracking for efficient lookup
//!
//! # Storage Architecture
//!
//! This domain manages **local storage** only. It does not handle:
//! - Access control (handled by `access_control` domain)
//! - Replication/durability (handled by `replication` domain)
//! - Content transformation (handled by `content` domain)
//! - Metadata management (handled by `manifest` domain)
//!
//! # Typical Operation Flow
//!
//! ```text
//! User Request
//!   ↓
//! Access Control Domain (capability verification)
//!   ↓
//! Quota Manager (check storage limit)
//!   ↓
//! Chunk Store (persist encrypted chunk locally)
//!   ↓
//! Indexer (update manifest mappings)
//!   ↓
//! Replication Domain (replicate to peers)
//! ```
//!
//! # Key Design Principles
//!
//! - **Immutable Chunks**: Once written, chunks are never modified
//! - **Content Addressed**: Chunks identified by Cid (content hash)
//! - **Deduplication**: Same content stored once (via Cid)
//! - **Quota Enforcement**: LRU eviction maintains storage limits
//! - **Indexing**: Fast lookup of chunks by manifest or Cid
//!
//! # Integration Points
//!
//! - **Upstream**: Content domain produces encrypted chunks, replication manager places replicas
//! - **Downstream**: Access control verifies capabilities before storage access
//! - **Manifest**: Tracks which chunks belong to which objects
//! - **Journal**: Storage operations logged as ledger events for auditability

pub mod chunk_store;
pub mod indexer;
pub mod quota;

pub use chunk_store::{
    ChunkError, ChunkId, ChunkMetadata, ChunkStore, EncryptedChunk, StorageStats,
};
pub use indexer::{Cid, Indexer, PutOpts};
pub use quota::{CacheEntry, LruEviction, QuotaConfig, QuotaTracker};
