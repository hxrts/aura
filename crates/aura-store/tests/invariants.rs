use aura_core::time::PhysicalTime;
use aura_core::{AuthorityId, ChunkId, ContentSize, ContextId};
use aura_store::{ByteSize, ChunkLayout, ErasureConfig, StorageFact};

fn test_time(ts_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms,
        uncertainty: None,
    }
}

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

#[test]
fn chunk_layout_rejects_mismatched_sizes() {
    let chunks = vec![ChunkId::from_bytes(b"chunk1")];
    let sizes = vec![10, 20];
    let total_size = ContentSize(20);
    let config = ErasureConfig::default();

    let result = ChunkLayout::new(chunks, sizes, total_size, config);
    assert!(result.is_err());
}

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
