//! Anti-Entropy Correctness Tests
//!
//! Tests for the digest-based reconciliation protocol.
//! Validates digest computation determinism and configuration.

use aura_anti_entropy::{AntiEntropyConfig, BloomDigest, SyncError};
use aura_core::{identifiers::DeviceId, Hash32, ProtocolErrorCode};
use std::collections::BTreeSet;

// ============================================================================
// BloomDigest Tests
// ============================================================================

#[test]
fn empty_digest_is_empty() {
    let digest = BloomDigest::empty();

    assert!(digest.is_empty());
    assert_eq!(digest.len(), 0);
}

#[test]
fn digest_contains_added_cids() {
    let cid = Hash32([42u8; 32]);
    let digest = BloomDigest {
        cids: [cid].into_iter().collect(),
    };

    assert!(digest.contains(&cid));
    assert!(!digest.contains(&Hash32([0u8; 32])));
}

#[test]
fn digest_len_matches_cid_count() {
    let cids: BTreeSet<Hash32> = [[1u8; 32], [2u8; 32], [3u8; 32]]
        .into_iter()
        .map(Hash32)
        .collect();
    let digest = BloomDigest { cids };

    assert_eq!(digest.len(), 3);
    assert!(!digest.is_empty());
}

#[test]
fn digest_is_deterministic_from_same_cids() {
    let cids1: BTreeSet<Hash32> = [[1u8; 32], [2u8; 32], [3u8; 32]]
        .into_iter()
        .map(Hash32)
        .collect();
    let cids2: BTreeSet<Hash32> = [[1u8; 32], [2u8; 32], [3u8; 32]]
        .into_iter()
        .map(Hash32)
        .collect();

    let digest1 = BloomDigest { cids: cids1 };
    let digest2 = BloomDigest { cids: cids2 };

    assert_eq!(digest1, digest2);
}

#[test]
fn digest_order_independent() {
    // BTreeSet provides ordering, so order shouldn't matter
    let cids1: BTreeSet<Hash32> = [[1u8; 32], [2u8; 32], [3u8; 32]]
        .into_iter()
        .map(Hash32)
        .collect();
    let cids2: BTreeSet<Hash32> = [[3u8; 32], [1u8; 32], [2u8; 32]]
        .into_iter()
        .map(Hash32)
        .collect();

    let digest1 = BloomDigest { cids: cids1 };
    let digest2 = BloomDigest { cids: cids2 };

    assert_eq!(digest1, digest2);
}

#[test]
fn digest_differs_with_different_cids() {
    let cids1: BTreeSet<Hash32> = [[1u8; 32], [2u8; 32]].into_iter().map(Hash32).collect();
    let cids2: BTreeSet<Hash32> = [[1u8; 32], [3u8; 32]].into_iter().map(Hash32).collect();

    let digest1 = BloomDigest { cids: cids1 };
    let digest2 = BloomDigest { cids: cids2 };

    assert_ne!(digest1, digest2);
}

// ============================================================================
// SyncError Tests
// ============================================================================

#[test]
fn sync_error_codes_are_unique() {
    let device = DeviceId::new_from_entropy([0u8; 32]);
    let errors = vec![
        SyncError::PeerUnreachable(device.clone()),
        SyncError::VerificationFailed("test".to_string()),
        SyncError::NetworkError("test".to_string()),
        SyncError::RateLimitExceeded(device.clone()),
        SyncError::InvalidDigest(device),
        SyncError::OperationNotFound,
        SyncError::BackPressure,
        SyncError::TimeError,
        SyncError::AuthorizationFailed,
        SyncError::GuardChainFailure("test".to_string()),
    ];

    let codes: std::collections::HashSet<_> = errors.iter().map(|e| e.code()).collect();

    // Each error should have a unique code
    assert_eq!(
        codes.len(),
        errors.len(),
        "All error codes should be unique"
    );
}

#[test]
fn sync_error_display_is_meaningful() {
    let err = SyncError::VerificationFailed("invalid signature".to_string());
    let msg = format!("{}", err);

    assert!(msg.contains("verification"));
    assert!(msg.contains("invalid signature"));
}

#[test]
fn peer_unreachable_error_message() {
    let device = DeviceId::new_from_entropy([1u8; 32]);
    let err = SyncError::PeerUnreachable(device);
    let msg = format!("{}", err);

    assert!(msg.to_lowercase().contains("reachable") || msg.to_lowercase().contains("peer"));
}

#[test]
fn back_pressure_error_message() {
    let err = SyncError::BackPressure;
    let msg = format!("{}", err);

    assert!(msg.to_lowercase().contains("back pressure") || msg.to_lowercase().contains("pending"));
}

// ============================================================================
// AntiEntropyConfig Tests
// ============================================================================

#[test]
fn default_config_has_reasonable_values() {
    let config = AntiEntropyConfig::default();

    assert!(config.min_sync_interval_ms.get() > 0);
    assert!(config.max_ops_per_batch.get() > 0);
    assert!(config.max_concurrent_syncs.get() > 0);
    assert!(config.sync_timeout_ms.get() > 0);
}

#[test]
fn default_config_values_match_expected() {
    let config = AntiEntropyConfig::default();

    assert_eq!(config.min_sync_interval_ms.get(), 30_000);
    assert_eq!(config.max_ops_per_batch.get(), 100);
    assert_eq!(config.max_concurrent_syncs.get(), 5);
    assert_eq!(config.sync_timeout_ms.get(), 10_000);
}

// ============================================================================
// Digest Serialization Tests
// ============================================================================

#[test]
fn bloom_digest_serialization_roundtrip() {
    let cids: BTreeSet<Hash32> = [[1u8; 32], [2u8; 32], [3u8; 32]]
        .into_iter()
        .map(Hash32)
        .collect();
    let digest = BloomDigest { cids };

    let serialized = serde_json::to_string(&digest).expect("serialization should succeed");
    let deserialized: BloomDigest =
        serde_json::from_str(&serialized).expect("deserialization should succeed");

    assert_eq!(digest, deserialized);
}

#[test]
fn empty_digest_serialization_roundtrip() {
    let digest = BloomDigest::empty();

    let serialized = serde_json::to_string(&digest).expect("serialization should succeed");
    let deserialized: BloomDigest =
        serde_json::from_str(&serialized).expect("deserialization should succeed");

    assert_eq!(digest, deserialized);
    assert!(deserialized.is_empty());
}

// ============================================================================
// Property-Based Tests (Simple Versions)
// ============================================================================

#[test]
fn digest_set_operations() {
    let cid1 = Hash32([1u8; 32]);
    let cid2 = Hash32([2u8; 32]);
    let cid3 = Hash32([3u8; 32]);

    let digest1 = BloomDigest {
        cids: [cid1, cid2].into_iter().collect(),
    };
    let digest2 = BloomDigest {
        cids: [cid2, cid3].into_iter().collect(),
    };

    // Union operation
    let union: BTreeSet<Hash32> = digest1.cids.union(&digest2.cids).copied().collect();
    assert_eq!(union.len(), 3);
    assert!(union.contains(&cid1));
    assert!(union.contains(&cid2));
    assert!(union.contains(&cid3));

    // Intersection operation
    let intersection: BTreeSet<Hash32> =
        digest1.cids.intersection(&digest2.cids).copied().collect();
    assert_eq!(intersection.len(), 1);
    assert!(intersection.contains(&cid2));

    // Difference (what digest1 has that digest2 doesn't)
    let diff: BTreeSet<Hash32> = digest1.cids.difference(&digest2.cids).copied().collect();
    assert_eq!(diff.len(), 1);
    assert!(diff.contains(&cid1));
}
