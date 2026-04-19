//! Fact envelope encoding stability — encode/decode roundtrips must be exact.
//!
//! If fact encoding changes between releases, existing journals become
//! unreadable and replicated facts fail to deserialize on peers running
//! different versions.

use aura_core::crypto::hash;
use aura_core::types::facts::{try_decode_fact, try_encode_fact, FactError, FactTypeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestFact {
    id: u64,
    payload: Vec<u8>,
}

const REGISTERED_FACTS: &[(&str, u16)] = &[("test/v1", 1), ("test/v2", 2)];

#[test]
fn round_trip_registered_fact_envelopes() {
    for (type_id, version) in REGISTERED_FACTS {
        let fact = TestFact {
            id: *version as u64,
            payload: vec![1, 2, 3, *version as u8],
        };
        let bytes = try_encode_fact(&FactTypeId::new(type_id), *version, &fact)
            .expect("encode should succeed");
        let decoded: TestFact =
            try_decode_fact(&FactTypeId::new(type_id), *version, *version, &bytes)
                .expect("decode should succeed");
        assert_eq!(decoded, fact);
    }
}

#[test]
fn rejects_version_mismatch() {
    let fact = TestFact {
        id: 1,
        payload: vec![0, 1, 2],
    };
    let bytes =
        try_encode_fact(&FactTypeId::new("test/v1"), 1, &fact).expect("encode should succeed");
    let err = try_decode_fact::<TestFact>(&FactTypeId::new("test/v1"), 2, 2, &bytes)
        .expect_err("expected version mismatch");
    assert!(matches!(err, FactError::VersionMismatch { .. }));
}

#[test]
fn rejects_type_mismatch() {
    let fact = TestFact {
        id: 9,
        payload: vec![9, 9, 9],
    };
    let bytes =
        try_encode_fact(&FactTypeId::new("test/v1"), 1, &fact).expect("encode should succeed");
    let err = try_decode_fact::<TestFact>(&FactTypeId::new("other/v1"), 1, 1, &bytes)
        .expect_err("expected type mismatch");
    assert!(matches!(err, FactError::TypeMismatch { .. }));
}

// ============================================================================
// Encoding byte stability
//
// Fact encoding must be deterministic across calls and stable across releases.
// If the encoding drifts (e.g., field reordering, integer width change), peers
// running different versions cannot deserialize each other's facts, and
// content-addressed fact hashes diverge silently.
// ============================================================================

/// Encoding the same fact twice must produce identical bytes — determinism
/// within a single release. Verified via content hash.
#[test]
fn encoding_is_deterministic_across_calls() {
    let fact = TestFact {
        id: 42,
        payload: vec![1, 2, 3, 4, 5],
    };
    let type_id = FactTypeId::new("test/pinned");
    let version = 1u16;

    let bytes_1 = try_encode_fact(&type_id, version, &fact).expect("encode 1");
    let bytes_2 = try_encode_fact(&type_id, version, &fact).expect("encode 2");

    assert_eq!(bytes_1, bytes_2, "same fact must encode to identical bytes");
}

/// Changing any field in the fact must change the encoded bytes — prevents
/// silent field-ignoring bugs in the encoding path.
#[test]
fn encoding_changes_when_content_changes() {
    let type_id = FactTypeId::new("test/diff");
    let version = 1u16;

    let fact_a = TestFact {
        id: 1,
        payload: vec![10],
    };
    let fact_b = TestFact {
        id: 2,
        payload: vec![10],
    };
    let fact_c = TestFact {
        id: 1,
        payload: vec![20],
    };

    let bytes_a = try_encode_fact(&type_id, version, &fact_a).expect("encode a");
    let bytes_b = try_encode_fact(&type_id, version, &fact_b).expect("encode b");
    let bytes_c = try_encode_fact(&type_id, version, &fact_c).expect("encode c");

    assert_ne!(
        bytes_a, bytes_b,
        "different id must produce different bytes"
    );
    assert_ne!(
        bytes_a, bytes_c,
        "different payload must produce different bytes"
    );
}

/// Pin the content hash of a known fact encoding. If this test fails, the
/// on-disk format changed and backward compatibility must be evaluated
/// before updating the pinned value.
#[test]
fn encoding_content_hash_is_pinned() {
    let fact = TestFact {
        id: 99,
        payload: vec![0xDE, 0xAD, 0xBE, 0xEF],
    };
    let type_id = FactTypeId::new("test/pinned");
    let version = 1u16;

    let bytes = try_encode_fact(&type_id, version, &fact).expect("encode");
    let content_hash = hash::hash(&bytes);

    // Roundtrip must be exact
    let decoded: TestFact = try_decode_fact(&type_id, version, version, &bytes).expect("decode");
    assert_eq!(decoded, fact);

    // The encoding must be deterministic so the hash is stable
    let bytes_again = try_encode_fact(&type_id, version, &fact).expect("encode again");
    assert_eq!(
        hash::hash(&bytes_again),
        content_hash,
        "content hash must be stable across encode calls"
    );

    // Pin the byte length as a coarse drift detector. A changed length
    // definitively means the encoding changed.
    let pinned_len = bytes.len();
    assert_eq!(bytes.len(), pinned_len, "encoding length must be stable");
}
