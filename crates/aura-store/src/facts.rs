//! Storage domain facts for journal integration
//!
//! This module defines fact types for storage state changes that integrate
//! with Aura's journal system. All storage operations are recorded as
//! immutable facts for audit trails and state derivation.
//!
//! **Architecture**: Layer 2 domain facts following the pattern from aura-authorization.
//! These facts capture storage events and enable deterministic state reduction.
//!
//! **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.

use aura_core::identifiers::AuthorityId;
use aura_core::time::PhysicalTime;
use aura_core::types::facts::{
    FactDelta, FactDeltaReducer, FactEncoding, FactEnvelope, FactError, FactTypeId,
    MAX_FACT_PAYLOAD_BYTES,
};
use aura_core::util::serialization::{from_slice, to_vec, SerializationError};
use aura_core::{ChunkId, ContentId, ContextId};
use crate::types::{ByteSize, ChunkCount, ChunkIndex, NodeId};
use serde::{Deserialize, Serialize};

/// Unique type ID for storage facts in the journal system
pub static STORAGE_FACT_TYPE_ID: FactTypeId = FactTypeId::new("aura.store.v1");
/// Schema version for storage fact encoding
pub const STORAGE_FACT_SCHEMA_VERSION: u16 = 1;

/// Get the typed fact ID for storage facts
pub fn storage_fact_type_id() -> &'static FactTypeId {
    &STORAGE_FACT_TYPE_ID
}

/// Storage domain facts for journal integration
///
/// Each variant represents an atomic storage event that can be recorded
/// in a journal and used to derive current storage state.
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StorageFact {
    /// Content was added to storage
    ContentAdded {
        /// Authority that added the content
        authority_id: AuthorityId,
        /// Content identifier (hash of content)
        content_id: ContentId,
        /// Total size in bytes
        size_bytes: ByteSize,
        /// Number of chunks
        chunk_count: ChunkCount,
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
        size_bytes: ByteSize,
        /// Chunk index within content
        chunk_index: ChunkIndex,
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
        node_id: NodeId,
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
        quota_bytes: ByteSize,
        /// Current usage in bytes
        used_bytes: ByteSize,
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

    /// Validate quota invariants for quota update facts.
    pub fn quota_is_valid(&self) -> bool {
        match self {
            StorageFact::QuotaUpdated {
                quota_bytes,
                used_bytes,
                ..
            } => used_bytes.value() <= quota_bytes.value(),
            _ => true,
        }
    }

    /// Get the fact type ID for journal registration
    pub fn type_id() -> &'static str {
        STORAGE_FACT_TYPE_ID.as_str()
    }

    /// Encode this fact with a canonical envelope.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if serialization fails.
    pub fn try_encode(&self) -> Result<Vec<u8>, FactError> {
        let payload = to_vec(self)?;
        if payload.len() > MAX_FACT_PAYLOAD_BYTES {
            return Err(FactError::PayloadTooLarge {
                size: payload.len() as u64,
                max: MAX_FACT_PAYLOAD_BYTES as u64,
            });
        }
        let envelope = FactEnvelope {
            type_id: storage_fact_type_id().clone(),
            schema_version: STORAGE_FACT_SCHEMA_VERSION,
            encoding: FactEncoding::DagCbor,
            payload,
        };
        let bytes = to_vec(&envelope)?;
        Ok(bytes)
    }

    /// Decode a fact from a canonical envelope.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if deserialization fails or version/type mismatches.
    pub fn try_decode(bytes: &[u8]) -> Result<Self, FactError> {
        let envelope: FactEnvelope = from_slice(bytes)?;

        if envelope.type_id.as_str() != storage_fact_type_id().as_str() {
            return Err(FactError::TypeMismatch {
                expected: storage_fact_type_id().to_string(),
                actual: envelope.type_id.to_string(),
            });
        }

        if envelope.schema_version != STORAGE_FACT_SCHEMA_VERSION {
            return Err(FactError::VersionMismatch {
                expected: STORAGE_FACT_SCHEMA_VERSION,
                actual: envelope.schema_version,
            });
        }

        let fact = match envelope.encoding {
            FactEncoding::DagCbor => from_slice::<Self>(&envelope.payload)?,
            FactEncoding::Json => serde_json::from_slice::<Self>(&envelope.payload).map_err(|err| {
                FactError::Serialization(SerializationError::InvalidFormat(format!(
                    "JSON decode failed: {err}"
                )))
            })?,
        };
        Ok(fact)
    }

    /// Encode this fact with proper error handling.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if serialization fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, FactError> {
        self.try_encode()
    }

    /// Decode a fact with proper error handling.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if deserialization fails or version/type mismatches.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FactError> {
        Self::try_decode(bytes)
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

impl FactDelta for StorageFactDelta {
    fn merge(&mut self, other: &Self) {
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
                self.bytes_added += size_bytes.value();
            }
            StorageFact::ContentRemoved { .. } => {
                self.content_removals += 1;
            }
            StorageFact::ChunkStored { size_bytes, .. } => {
                self.chunks_stored += 1;
                self.bytes_added += size_bytes.value();
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
}

impl Default for StorageFactReducer {
    fn default() -> Self {
        Self::new()
    }
}

impl FactDeltaReducer<StorageFact, StorageFactDelta> for StorageFactReducer {
    fn apply(&self, fact: &StorageFact) -> StorageFactDelta {
        let mut delta = StorageFactDelta::new();
        delta.apply(fact);
        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::facts::FactDeltaReducer;

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
            size_bytes: ByteSize::new(1024),
            chunk_count: ChunkCount::new(2),
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
            size_bytes: ByteSize::new(512),
            chunk_index: ChunkIndex::new(0),
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
            size_bytes: ByteSize::new(1000),
            chunk_count: ChunkCount::new(1),
            context_id: None,
            added_at: test_time(1000),
        };

        let fact2 = StorageFact::ChunkStored {
            authority_id: test_authority(1),
            chunk_id: ChunkId::from_bytes(b"chunk1"),
            content_id: ContentId::from_bytes(b"content1"),
            size_bytes: ByteSize::new(500),
            chunk_index: ChunkIndex::new(0),
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
                size_bytes: ByteSize::new(1000),
                chunk_count: ChunkCount::new(2),
                context_id: None,
                added_at: test_time(1000),
            },
            StorageFact::ContentAdded {
                authority_id: test_authority(2),
                content_id: ContentId::from_bytes(b"content2"),
                size_bytes: ByteSize::new(2000),
                chunk_count: ChunkCount::new(4),
                context_id: None,
                added_at: test_time(2000),
            },
        ];

        let delta = reducer.reduce_batch(&facts);
        assert_eq!(delta.content_additions, 2);
        assert_eq!(delta.bytes_added, 3000);
    }
}

/// Property tests for semilattice laws on StorageFactDelta
#[cfg(test)]
mod proptest_semilattice {
    use super::*;
    use aura_core::types::facts::FactDelta;
    use proptest::prelude::*;

    /// Strategy for generating arbitrary StorageFactDelta values
    fn arb_delta() -> impl Strategy<Value = StorageFactDelta> {
        (
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1_000_000,
            0u64..1_000_000,
        )
            .prop_map(
                |(
                    content_additions,
                    content_removals,
                    chunks_stored,
                    chunks_replicated,
                    chunks_collected,
                    index_updates,
                    bytes_added,
                    bytes_removed,
                )| {
                    StorageFactDelta {
                        content_additions,
                        content_removals,
                        chunks_stored,
                        chunks_replicated,
                        chunks_collected,
                        index_updates,
                        bytes_added,
                        bytes_removed,
                    }
                },
            )
    }

    /// Helper to check if two deltas are equal
    fn deltas_equal(a: &StorageFactDelta, b: &StorageFactDelta) -> bool {
        a.content_additions == b.content_additions
            && a.content_removals == b.content_removals
            && a.chunks_stored == b.chunks_stored
            && a.chunks_replicated == b.chunks_replicated
            && a.chunks_collected == b.chunks_collected
            && a.index_updates == b.index_updates
            && a.bytes_added == b.bytes_added
            && a.bytes_removed == b.bytes_removed
    }

    proptest! {
        /// Idempotence: merging with self doubles the value (additive merge)
        #[test]
        fn merge_idempotent(a in arb_delta()) {
            let original = a.clone();
            let mut result = a.clone();
            result.merge(&original);
            // For additive deltas: a + a = 2a
            prop_assert_eq!(result.content_additions, original.content_additions * 2);
            prop_assert_eq!(result.bytes_added, original.bytes_added * 2);
        }

        /// Commutativity: a.merge(&b) == b.merge(&a) (result equivalence)
        #[test]
        fn merge_commutative(a in arb_delta(), b in arb_delta()) {
            let mut ab = a.clone();
            ab.merge(&b);

            let mut ba = b.clone();
            ba.merge(&a);

            prop_assert!(deltas_equal(&ab, &ba), "merge should be commutative");
        }

        /// Associativity: (a.merge(&b)).merge(&c) == a.merge(&(b.merge(&c)))
        #[test]
        fn merge_associative(a in arb_delta(), b in arb_delta(), c in arb_delta()) {
            // Left associative: (a merge b) merge c
            let mut left = a.clone();
            left.merge(&b);
            left.merge(&c);

            // Right associative: a merge (b merge c)
            let mut bc = b.clone();
            bc.merge(&c);
            let mut right = a.clone();
            right.merge(&bc);

            prop_assert!(deltas_equal(&left, &right), "merge should be associative");
        }

        /// Identity: merge with default (zero) leaves value unchanged
        #[test]
        fn merge_identity(a in arb_delta()) {
            let original = a.clone();
            let mut result = a.clone();
            result.merge(&StorageFactDelta::default());

            prop_assert!(deltas_equal(&result, &original), "merge with identity should preserve value");
        }
    }
}
