//! Storage domain facts for journal integration
//!
//! This module defines fact types for storage state changes that integrate
//! with Aura's journal system. All storage operations are recorded as
//! immutable facts for audit trails and state derivation.
//!
//! **Architecture**: Layer 2 domain facts following the pattern from aura-wot.
//! These facts capture storage events and enable deterministic state reduction.
//!
//! **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.

use aura_core::identifiers::AuthorityId;
use aura_core::time::PhysicalTime;
use aura_core::{ChunkId, ContentId, ContextId};
use serde::{Deserialize, Serialize};

/// Unique type ID for storage facts in the journal system
pub const STORAGE_FACT_TYPE_ID: &str = "aura.store.v1";

/// Storage domain facts for journal integration
///
/// Each variant represents an atomic storage event that can be recorded
/// in a journal and used to derive current storage state.
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageFact {
    /// Content was added to storage
    ContentAdded {
        /// Authority that added the content
        authority_id: AuthorityId,
        /// Content identifier (hash of content)
        content_id: ContentId,
        /// Total size in bytes
        size_bytes: u64,
        /// Number of chunks
        chunk_count: u32,
        /// Context where content is stored (if any)
        context_id: Option<ContextId>,
        /// Timestamp when content was added (unified time system)
        added_at: PhysicalTime,
    },

    /// Content was removed from storage
    ContentRemoved {
        /// Authority that removed the content
        authority_id: AuthorityId,
        /// Content identifier
        content_id: ContentId,
        /// Reason for removal (optional)
        reason: Option<String>,
        /// Timestamp when content was removed (unified time system)
        removed_at: PhysicalTime,
    },

    /// A chunk was stored
    ChunkStored {
        /// Authority that stored the chunk
        authority_id: AuthorityId,
        /// Chunk identifier
        chunk_id: ChunkId,
        /// Parent content identifier
        content_id: ContentId,
        /// Chunk size in bytes
        size_bytes: u32,
        /// Chunk index within content
        chunk_index: u32,
        /// Whether this is a parity chunk
        is_parity: bool,
        /// Timestamp when chunk was stored (unified time system)
        stored_at: PhysicalTime,
    },

    /// A chunk was replicated to a node
    ChunkReplicated {
        /// Authority that initiated replication
        authority_id: AuthorityId,
        /// Chunk identifier
        chunk_id: ChunkId,
        /// Node identifier where chunk was replicated
        node_id: String,
        /// Timestamp when replication completed (unified time system)
        replicated_at: PhysicalTime,
    },

    /// A chunk was garbage collected
    ChunkCollected {
        /// Authority that performed garbage collection
        authority_id: AuthorityId,
        /// Chunk identifier
        chunk_id: ChunkId,
        /// Reason for collection
        reason: String,
        /// Timestamp when chunk was collected (unified time system)
        collected_at: PhysicalTime,
    },

    /// Search index was updated
    IndexUpdated {
        /// Authority that updated the index
        authority_id: AuthorityId,
        /// Content identifier that was indexed
        content_id: ContentId,
        /// Number of terms indexed
        term_count: u32,
        /// Timestamp when index was updated (unified time system)
        updated_at: PhysicalTime,
    },

    /// Search index entry was removed
    IndexEntryRemoved {
        /// Authority that removed the entry
        authority_id: AuthorityId,
        /// Content identifier
        content_id: ContentId,
        /// Timestamp when entry was removed (unified time system)
        removed_at: PhysicalTime,
    },

    /// Storage quota was updated for a context
    QuotaUpdated {
        /// Authority that updated the quota
        authority_id: AuthorityId,
        /// Context identifier
        context_id: ContextId,
        /// New quota limit in bytes
        quota_bytes: u64,
        /// Current usage in bytes
        used_bytes: u64,
        /// Timestamp when quota was updated (unified time system)
        updated_at: PhysicalTime,
    },
}

impl StorageFact {
    /// Get the authority associated with this fact
    pub fn authority_id(&self) -> AuthorityId {
        match self {
            Self::ContentAdded { authority_id, .. } => *authority_id,
            Self::ContentRemoved { authority_id, .. } => *authority_id,
            Self::ChunkStored { authority_id, .. } => *authority_id,
            Self::ChunkReplicated { authority_id, .. } => *authority_id,
            Self::ChunkCollected { authority_id, .. } => *authority_id,
            Self::IndexUpdated { authority_id, .. } => *authority_id,
            Self::IndexEntryRemoved { authority_id, .. } => *authority_id,
            Self::QuotaUpdated { authority_id, .. } => *authority_id,
        }
    }

    /// Get the timestamp of this fact
    ///
    /// **Time System**: Returns `PhysicalTime` per the unified time architecture.
    pub fn timestamp(&self) -> PhysicalTime {
        match self {
            Self::ContentAdded { added_at, .. } => added_at.clone(),
            Self::ContentRemoved { removed_at, .. } => removed_at.clone(),
            Self::ChunkStored { stored_at, .. } => stored_at.clone(),
            Self::ChunkReplicated { replicated_at, .. } => replicated_at.clone(),
            Self::ChunkCollected { collected_at, .. } => collected_at.clone(),
            Self::IndexUpdated { updated_at, .. } => updated_at.clone(),
            Self::IndexEntryRemoved { removed_at, .. } => removed_at.clone(),
            Self::QuotaUpdated { updated_at, .. } => updated_at.clone(),
        }
    }

    /// Get the timestamp of this fact in milliseconds
    ///
    /// Convenience method for backward compatibility.
    pub fn timestamp_ms(&self) -> u64 {
        self.timestamp().ts_ms
    }

    /// Get the fact type ID for journal registration
    pub fn type_id() -> &'static str {
        STORAGE_FACT_TYPE_ID
    }
}

/// Delta tracking for storage facts during reduction
///
/// Tracks cumulative changes from processing storage facts,
/// useful for monitoring and debugging.
#[derive(Debug, Clone, Default)]
pub struct StorageFactDelta {
    /// Number of content additions
    pub content_additions: u64,
    /// Number of content removals
    pub content_removals: u64,
    /// Number of chunks stored
    pub chunks_stored: u64,
    /// Number of chunks replicated
    pub chunks_replicated: u64,
    /// Number of chunks collected
    pub chunks_collected: u64,
    /// Number of index updates
    pub index_updates: u64,
    /// Total bytes added
    pub bytes_added: u64,
    /// Total bytes removed (estimated)
    pub bytes_removed: u64,
}

impl StorageFactDelta {
    /// Create a new empty delta
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a storage fact to this delta
    pub fn apply(&mut self, fact: &StorageFact) {
        match fact {
            StorageFact::ContentAdded { size_bytes, .. } => {
                self.content_additions += 1;
                self.bytes_added += size_bytes;
            }
            StorageFact::ContentRemoved { .. } => {
                self.content_removals += 1;
            }
            StorageFact::ChunkStored { size_bytes, .. } => {
                self.chunks_stored += 1;
                self.bytes_added += *size_bytes as u64;
            }
            StorageFact::ChunkReplicated { .. } => {
                self.chunks_replicated += 1;
            }
            StorageFact::ChunkCollected { .. } => {
                self.chunks_collected += 1;
            }
            StorageFact::IndexUpdated { .. } => {
                self.index_updates += 1;
            }
            StorageFact::IndexEntryRemoved { .. } => {
                // Index removal doesn't directly affect byte counts
            }
            StorageFact::QuotaUpdated { .. } => {
                // Quota updates don't affect deltas
            }
        }
    }

    /// Merge another delta into this one
    pub fn merge(&mut self, other: &StorageFactDelta) {
        self.content_additions += other.content_additions;
        self.content_removals += other.content_removals;
        self.chunks_stored += other.chunks_stored;
        self.chunks_replicated += other.chunks_replicated;
        self.chunks_collected += other.chunks_collected;
        self.index_updates += other.index_updates;
        self.bytes_added += other.bytes_added;
        self.bytes_removed += other.bytes_removed;
    }
}

/// Reducer for deriving storage state from facts
///
/// Processes storage facts and updates derived state accordingly.
/// Used by the journal system for state derivation.
pub struct StorageFactReducer;

impl StorageFactReducer {
    /// Create a new reducer
    pub fn new() -> Self {
        Self
    }

    /// Process a single fact and return the delta
    pub fn reduce(&self, fact: &StorageFact) -> StorageFactDelta {
        let mut delta = StorageFactDelta::new();
        delta.apply(fact);
        delta
    }

    /// Process multiple facts and return the combined delta
    pub fn reduce_batch(&self, facts: &[StorageFact]) -> StorageFactDelta {
        let mut delta = StorageFactDelta::new();
        for fact in facts {
            delta.apply(fact);
        }
        delta
    }
}

impl Default for StorageFactReducer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_content_added_fact() {
        let fact = StorageFact::ContentAdded {
            authority_id: test_authority(1),
            content_id: ContentId::from_bytes(b"test-content"),
            size_bytes: 1024,
            chunk_count: 2,
            context_id: None,
            added_at: test_time(1000),
        };

        assert_eq!(fact.authority_id(), test_authority(1));
        assert_eq!(fact.timestamp_ms(), 1000);
        assert_eq!(fact.timestamp(), test_time(1000));
    }

    #[test]
    fn test_chunk_stored_fact() {
        let fact = StorageFact::ChunkStored {
            authority_id: test_authority(2),
            chunk_id: ChunkId::from_bytes(b"chunk1"),
            content_id: ContentId::from_bytes(b"content1"),
            size_bytes: 512,
            chunk_index: 0,
            is_parity: false,
            stored_at: test_time(2000),
        };

        assert_eq!(fact.authority_id(), test_authority(2));
        assert_eq!(fact.timestamp_ms(), 2000);
        assert_eq!(fact.timestamp(), test_time(2000));
    }

    #[test]
    fn test_storage_fact_delta() {
        let mut delta = StorageFactDelta::new();

        let fact1 = StorageFact::ContentAdded {
            authority_id: test_authority(1),
            content_id: ContentId::from_bytes(b"content1"),
            size_bytes: 1000,
            chunk_count: 1,
            context_id: None,
            added_at: test_time(1000),
        };

        let fact2 = StorageFact::ChunkStored {
            authority_id: test_authority(1),
            chunk_id: ChunkId::from_bytes(b"chunk1"),
            content_id: ContentId::from_bytes(b"content1"),
            size_bytes: 500,
            chunk_index: 0,
            is_parity: false,
            stored_at: test_time(1001),
        };

        delta.apply(&fact1);
        delta.apply(&fact2);

        assert_eq!(delta.content_additions, 1);
        assert_eq!(delta.chunks_stored, 1);
        assert_eq!(delta.bytes_added, 1500); // 1000 + 500
    }

    #[test]
    fn test_storage_fact_reducer() {
        let reducer = StorageFactReducer::new();

        let facts = vec![
            StorageFact::ContentAdded {
                authority_id: test_authority(1),
                content_id: ContentId::from_bytes(b"content1"),
                size_bytes: 1000,
                chunk_count: 2,
                context_id: None,
                added_at: test_time(1000),
            },
            StorageFact::ContentAdded {
                authority_id: test_authority(2),
                content_id: ContentId::from_bytes(b"content2"),
                size_bytes: 2000,
                chunk_count: 4,
                context_id: None,
                added_at: test_time(2000),
            },
        ];

        let delta = reducer.reduce_batch(&facts);
        assert_eq!(delta.content_additions, 2);
        assert_eq!(delta.bytes_added, 3000);
    }
}
