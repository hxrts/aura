//! Storage domain invariant contracts — quota enforcement, chunk layout
//! validation, and content-address integrity.

use aura_core::time::PhysicalTime;
use aura_core::{AuthorityId, ChunkId, ContentSize, ContextId};
use aura_store::{ByteSize, ChunkLayout, ErasureConfig, StorageFact};

fn test_time(ts_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms,
        uncertainty: None,
    }
}

/// Quota used_bytes must not exceed quota_bytes — the fundamental storage
/// accounting invariant. If violated, authorities can consume unbounded space.
#[test]
fn quota_update_respects_usage_invariant() {
    let authority_id = AuthorityId::new_from_entropy([9u8; 32]);
    let context_id = ContextId::new_from_entropy([1u8; 32]);

    let ok = StorageFact::QuotaUpdated {
        authority_id,
        context_id,
        quota_bytes: ByteSize::new(1_000),
        used_bytes: ByteSize::new(500),
        updated_at: test_time(10),
    };
    assert!(ok.quota_is_valid());

    let bad = StorageFact::QuotaUpdated {
        authority_id,
        context_id,
        quota_bytes: ByteSize::new(1_000),
        used_bytes: ByteSize::new(1_500),
        updated_at: test_time(11),
    };
    assert!(!bad.quota_is_valid());
}

/// Chunk count must match size vector length — mismatched metadata would
/// cause out-of-bounds reads during chunk retrieval.
#[test]
fn chunk_layout_rejects_mismatched_sizes() {
    let chunks = vec![ChunkId::from_bytes(b"chunk1")];
    let sizes = vec![10, 20];
    let total_size = ContentSize(20);
    let config = ErasureConfig::default();

    let result = ChunkLayout::new(chunks, sizes, total_size, config);
    assert!(result.is_err());
}

/// Sum of chunk sizes must cover total content size — underflow means
/// missing data that can't be reconstructed.
#[test]
fn chunk_layout_rejects_size_underflow() {
    let chunks = vec![
        ChunkId::from_bytes(b"chunk1"),
        ChunkId::from_bytes(b"chunk2"),
    ];
    let sizes = vec![10, 10];
    let total_size = ContentSize(50);
    let config = ErasureConfig::default();

    let result = ChunkLayout::new(chunks, sizes, total_size, config);
    assert!(result.is_err());
}

// ============================================================================
// Content-address determinism (InvariantStoreContentAddressIntegrity)
// ============================================================================

/// Same content must always produce the same ContentId — this is the
/// foundation of content-addressed storage. If it's non-deterministic,
/// replicas disagree on what's stored and Merkle trees diverge.
#[test]
fn content_address_is_deterministic() {
    use aura_core::ContentId;

    let content = b"hello world";
    let id1 = ContentId::from_bytes(content);
    let id2 = ContentId::from_bytes(content);
    assert_eq!(id1, id2, "same content must produce same ContentId");

    // Different content must produce different IDs
    let different = b"hello world!";
    let id3 = ContentId::from_bytes(different);
    assert_ne!(
        id1, id3,
        "different content must produce different ContentId"
    );
}

/// ContentId hash is stable across repeated calls — needed for dedup.
#[test]
fn content_id_hash_stable() {
    use aura_core::ContentId;

    let content = b"determinism test vector";
    let id = ContentId::from_bytes(content);
    let hash1 = *id.hash();
    let id_again = ContentId::from_bytes(content);
    let hash2 = *id_again.hash();
    assert_eq!(hash1, hash2, "ContentId hash must be stable");
}

// ============================================================================
// StorageFact encoding roundtrip
// ============================================================================

/// StorageFact encode → decode must be identity. If encoding changes between
/// releases, storage facts in the journal become unreadable.
#[test]
fn storage_fact_encoding_roundtrip() {
    let fact = StorageFact::ContentAdded {
        authority_id: AuthorityId::new_from_entropy([20u8; 32]),
        content_id: aura_core::ContentId::from_bytes(b"test content"),
        size_bytes: ByteSize::new(1024),
        chunk_count: aura_store::ChunkCount::new(4),
        context_id: Some(ContextId::new_from_entropy([21u8; 32])),
        added_at: test_time(1000),
    };

    let encoded = fact.try_encode().expect("encode should succeed");
    let decoded = StorageFact::try_decode(&encoded).expect("decode should succeed");
    assert_eq!(decoded, fact, "roundtrip must preserve all fields");
}

/// Encoding must be deterministic — same fact encoded twice produces
/// identical bytes.
#[test]
fn storage_fact_encoding_deterministic() {
    let fact = StorageFact::ContentRemoved {
        authority_id: AuthorityId::new_from_entropy([30u8; 32]),
        content_id: aura_core::ContentId::from_bytes(b"removed content"),
        reason: Some("expired".to_string()),
        removed_at: test_time(2000),
    };

    let bytes1 = fact.try_encode().expect("encode 1");
    let bytes2 = fact.try_encode().expect("encode 2");
    assert_eq!(bytes1, bytes2, "encoding must be deterministic");
}

// ============================================================================
// Overlapping key merge
// ============================================================================

/// When two replicas add content with the same ContentId, join must merge
/// them correctly — the content should appear once in the merged state,
/// not duplicated or lost.
#[test]
fn overlapping_content_id_merge() {
    use aura_core::{ContentId, JoinSemilattice};
    use aura_store::{SearchIndexEntry, StorageState};
    use std::collections::BTreeSet;

    let content_id = ContentId::from_bytes(b"shared content");
    let authority_a = AuthorityId::new_from_entropy([40u8; 32]);
    let authority_b = AuthorityId::new_from_entropy([41u8; 32]);

    let terms_a: BTreeSet<String> = ["term-a"].iter().map(|s| s.to_string()).collect();
    let terms_b: BTreeSet<String> = ["term-b"].iter().map(|s| s.to_string()).collect();

    let entry_a =
        SearchIndexEntry::new(content_id.to_string(), terms_a, Vec::new(), test_time(100));
    let entry_b =
        SearchIndexEntry::new(content_id.to_string(), terms_b, Vec::new(), test_time(200));

    let mut state_a = StorageState::new();
    state_a.add_content(content_id.clone(), entry_a, authority_a, test_time(100));

    let mut state_b = StorageState::new();
    state_b.add_content(content_id, entry_b, authority_b, test_time(200));

    let merged = state_a.join(&state_b);

    // Merged state must contain the content (not lost)
    assert!(
        !merged.index.is_empty(),
        "merged state must contain the shared content"
    );

    // Join must be commutative even with overlapping keys
    let merged_rev = state_b.join(&state_a);
    assert_eq!(
        merged, merged_rev,
        "overlapping key merge must be commutative"
    );
}
