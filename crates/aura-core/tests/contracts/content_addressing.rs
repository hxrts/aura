//! Content addressing contract tests.
//!
//! Content-addressed types (Hash32, ContentId, ChunkId) are the foundation
//! for fact deduplication and Merkle tree integrity. If hashing is
//! non-deterministic or hex roundtrips fail, peers disagree on content
//! identity and tree commitments diverge.

use aura_core::{ChunkId, ContentId, ContentSize, Hash32};

/// Hash32 must be 32 bytes and deterministic — same input always produces
/// the same hash. Non-deterministic hashing breaks content addressing.
#[test]
fn hash32_deterministic_and_fixed_width() {
    let data = b"hello world";
    let hash = Hash32::from_bytes(data);
    assert_eq!(hash.as_bytes().len(), 32);

    let hash2 = Hash32::from_bytes(data);
    assert_eq!(hash, hash2);
}

/// Hash32 hex encoding must roundtrip exactly — this is the on-wire and
/// display format for content hashes.
#[test]
fn hash32_hex_roundtrip() {
    let data = b"test data";
    let hash = Hash32::from_bytes(data);
    let hex_str = hash.to_hex();
    assert_eq!(hex_str.len(), 64); // 32 bytes × 2 hex chars

    let decoded = Hash32::from_hex(&hex_str).unwrap();
    assert_eq!(hash, decoded);
}

/// ContentId tracks both hash and size — size is needed for storage
/// budgeting without re-reading the content.
#[test]
fn contentid_captures_hash_and_size() {
    let data = b"some content";
    let content_id = ContentId::from_bytes(data);
    assert_eq!(content_id.size(), Some(data.len() as u64));
    assert_eq!(content_id.hash(), &Hash32::from_bytes(data));
}

/// ContentId from a serialized value must produce a valid hash with
/// non-zero size — this is the path for hashing structured facts.
#[test]
fn contentid_from_serialized_value() {
    #[derive(serde::Serialize)]
    struct TestData {
        id: u32,
        name: String,
    }

    let data = TestData {
        id: 42,
        name: String::from("test"),
    };

    let content_id = ContentId::from_value(&data).unwrap();
    assert!(content_id.size().unwrap() > 0);
}

/// ContentId can be constructed from an existing hash with explicit size.
#[test]
fn contentid_with_explicit_size() {
    let hash = Hash32::from_bytes(b"test");
    let content_id = ContentId::with_size(hash, 1024);
    assert_eq!(content_id.hash(), &hash);
    assert_eq!(content_id.size(), Some(1024));
}

/// ChunkId identifies storage chunks — sequence number is optional for
/// single-chunk content.
#[test]
fn chunkid_unsequenced() {
    let data = b"chunk data";
    let chunk_id = ChunkId::from_bytes(data);
    assert_eq!(chunk_id.hash(), &Hash32::from_bytes(data));
    assert_eq!(chunk_id.sequence(), None);
    assert!(!chunk_id.is_sequenced());
}

/// Sequenced chunks carry an explicit ordering index for multi-chunk content.
#[test]
fn chunkid_sequenced() {
    let hash = Hash32::from_bytes(b"chunk");
    let chunk_id = ChunkId::with_sequence(hash, 5);
    assert_eq!(chunk_id.hash(), &hash);
    assert_eq!(chunk_id.sequence(), Some(5));
    assert!(chunk_id.is_sequenced());
}

/// ChunkId hex roundtrip — chunks are referenced by hex hash in storage paths.
#[test]
fn chunkid_hex_roundtrip() {
    let data = b"chunk data";
    let chunk_id = ChunkId::from_bytes(data);
    let hex_str = chunk_id.to_hex();
    let decoded = ChunkId::from_hex(&hex_str).unwrap();
    assert_eq!(chunk_id.hash(), decoded.hash());
}

/// ContentSize human-readable formatting for UI display.
#[test]
fn contentsize_human_readable() {
    assert_eq!(ContentSize::new(512).human_readable(), "512 B");
    assert_eq!(ContentSize::new(1536).human_readable(), "1.5 KB");
    assert_eq!(ContentSize::new(2_097_152).human_readable(), "2.0 MB");
    assert_eq!(ContentSize::new(3_221_225_472).human_readable(), "3.0 GB");
}

/// ContentSize converts to/from u64 and usize without loss.
#[test]
fn contentsize_conversions() {
    let size = ContentSize::from(1024u64);
    assert_eq!(size.bytes(), 1024);

    let size2 = ContentSize::from(512usize);
    assert_eq!(size2.bytes(), 512);

    let bytes: u64 = ContentSize::new(2048).into();
    assert_eq!(bytes, 2048);
}
