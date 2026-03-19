//! Content Addressing Tests
//!
//! Tests for content-based addressing using hash32 and content identifiers.

use aura_core::{ChunkId, ContentId, ContentSize, Hash32};

#[test]
fn test_hash32_from_bytes() {
    let data = b"hello world";
    let hash = Hash32::from_bytes(data);

    // Blake3 hash should be 32 bytes
    assert_eq!(hash.as_bytes().len(), 32);

    // Same input should produce same hash
    let hash2 = Hash32::from_bytes(data);
    assert_eq!(hash, hash2);
}

#[test]
fn test_hash32_hex_roundtrip() {
    let data = b"test data";
    let hash = Hash32::from_bytes(data);

    let hex_str = hash.to_hex();
    assert_eq!(hex_str.len(), 64); // 32 bytes * 2 chars per byte

    let decoded = Hash32::from_hex(&hex_str).unwrap();
    assert_eq!(hash, decoded);
}

#[test]
fn test_contentid_from_bytes() {
    let data = b"some content";
    let content_id = ContentId::from_bytes(data);

    assert_eq!(content_id.size(), Some(data.len() as u64));
    assert_eq!(content_id.hash(), &Hash32::from_bytes(data));
}

#[test]
fn test_contentid_from_value() {
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

#[test]
fn test_contentid_with_size() {
    let hash = Hash32::from_bytes(b"test");
    let content_id = ContentId::with_size(hash, 1024);

    assert_eq!(content_id.hash(), &hash);
    assert_eq!(content_id.size(), Some(1024));
}

#[test]
fn test_chunkid_from_bytes() {
    let data = b"chunk data";
    let chunk_id = ChunkId::from_bytes(data);

    assert_eq!(chunk_id.hash(), &Hash32::from_bytes(data));
    assert_eq!(chunk_id.sequence(), None);
    assert!(!chunk_id.is_sequenced());
}

#[test]
fn test_chunkid_with_sequence() {
    let hash = Hash32::from_bytes(b"chunk");
    let chunk_id = ChunkId::with_sequence(hash, 5);

    assert_eq!(chunk_id.hash(), &hash);
    assert_eq!(chunk_id.sequence(), Some(5));
    assert!(chunk_id.is_sequenced());
}

#[test]
fn test_chunkid_hex_roundtrip() {
    let data = b"chunk data";
    let chunk_id = ChunkId::from_bytes(data);

    let hex_str = chunk_id.to_hex();
    let decoded = ChunkId::from_hex(&hex_str).unwrap();

    assert_eq!(chunk_id.hash(), decoded.hash());
}

#[test]
fn test_contentsize_human_readable() {
    assert_eq!(ContentSize::new(512).human_readable(), "512 B");
    assert_eq!(ContentSize::new(1536).human_readable(), "1.5 KB");
    assert_eq!(ContentSize::new(2_097_152).human_readable(), "2.0 MB");
    assert_eq!(ContentSize::new(3_221_225_472).human_readable(), "3.0 GB");
}

#[test]
fn test_contentsize_conversions() {
    let size = ContentSize::from(1024u64);
    assert_eq!(size.bytes(), 1024);

    let size2 = ContentSize::from(512usize);
    assert_eq!(size2.bytes(), 512);

    let bytes: u64 = ContentSize::new(2048).into();
    assert_eq!(bytes, 2048);
}
