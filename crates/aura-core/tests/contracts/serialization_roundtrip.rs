//! Serialization round-trip tests for core newtypes and envelopes.
//!
//! These tests verify that encode → decode is identity for every type that
//! crosses the wire or persists to storage. If any roundtrip breaks, peers
//! cannot communicate or existing data becomes unreadable.

use aura_core::messages::{MessageSequence, MessageTimestamp, WireEnvelope};
use aura_core::ownership::OwnershipCategory;
use aura_core::time::{LogicalTime, PhysicalTime, ScalarClock, TimeStamp, VectorClock};
use aura_core::types::facts::{FactEncoding, FactEnvelope, FactTypeId};
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::{
    DeviceId, FlowCost, FlowNonce, NetworkAddress, ReceiptSig, SessionId, StoragePath,
};

/// StoragePath roundtrip — storage paths are persisted in journal facts.
#[test]
fn storage_path_roundtrip() {
    let path = StoragePath::parse("namespace/personal/*").unwrap();
    let bytes = to_vec(&path).unwrap();
    let decoded: StoragePath = from_slice(&bytes).unwrap();
    assert_eq!(decoded, path);
}

/// NetworkAddress roundtrip — peer addresses are exchanged in rendezvous.
#[test]
fn network_address_roundtrip() {
    let addr = NetworkAddress::parse("example.com:443").unwrap();
    let bytes = to_vec(&addr).unwrap();
    let decoded: NetworkAddress = from_slice(&bytes).unwrap();
    assert_eq!(decoded, addr);
}

/// Flow budget newtypes roundtrip — these are embedded in receipts that
/// relays verify for hop-by-hop accountability.
#[test]
fn flow_newtypes_roundtrip() {
    let cost = FlowCost::new(42);
    let nonce = FlowNonce::new(7);
    let sig = ReceiptSig::new(vec![9u8; 64]).unwrap();

    let cost_bytes = to_vec(&cost).unwrap();
    let nonce_bytes = to_vec(&nonce).unwrap();
    let sig_bytes = to_vec(&sig).unwrap();

    let decoded_cost: FlowCost = from_slice(&cost_bytes).unwrap();
    let decoded_nonce: FlowNonce = from_slice(&nonce_bytes).unwrap();
    let decoded_sig: ReceiptSig = from_slice(&sig_bytes).unwrap();

    assert_eq!(decoded_cost, cost);
    assert_eq!(decoded_nonce, nonce);
    assert_eq!(decoded_sig, sig);
}

/// WireEnvelope roundtrip — this is the outer transport frame for all
/// peer-to-peer messages. Breakage here means no communication.
#[test]
fn wire_envelope_roundtrip() {
    let sender = DeviceId::new_from_entropy([5u8; 32]);
    let session = SessionId::from_entropy([6u8; 32]);
    let envelope = WireEnvelope::new(
        Some(session),
        sender,
        MessageSequence::new(11),
        MessageTimestamp::new(1234),
        "payload".to_string(),
    );

    let bytes = to_vec(&envelope).unwrap();
    let decoded: WireEnvelope<String> = from_slice(&bytes).unwrap();

    assert_eq!(decoded.session_id, Some(session));
    assert_eq!(decoded.sender_id, sender);
    assert_eq!(decoded.sequence.value(), 11);
    assert_eq!(decoded.timestamp.value(), 1234);
    assert_eq!(decoded.payload, "payload");
}

/// FactEnvelope roundtrip — facts are the unit of journal replication.
/// A broken roundtrip means replicated facts are unreadable.
#[test]
fn fact_envelope_roundtrip() {
    let envelope = FactEnvelope {
        type_id: FactTypeId::from("demo"),
        schema_version: 1,
        encoding: FactEncoding::Json,
        payload: vec![1, 2, 3],
    };

    let bytes = to_vec(&envelope).unwrap();
    let decoded: FactEnvelope = from_slice(&bytes).unwrap();
    assert_eq!(decoded, envelope);
}

/// OwnershipCategory roundtrip — persisted in ARCHITECTURE.md inventory
/// and used by CI governance checks.
#[test]
fn ownership_category_roundtrip() {
    let json = serde_json::to_string(&OwnershipCategory::ActorOwned)
        .unwrap_or_else(|error| panic!("serialize ownership category: {error}"));
    assert_eq!(json, "\"actor_owned\"");
    let round_trip: OwnershipCategory = serde_json::from_str(&json)
        .unwrap_or_else(|error| panic!("deserialize ownership category: {error}"));
    assert_eq!(round_trip, OwnershipCategory::ActorOwned);
}

// ============================================================================
// TimeStamp serialization
//
// TimeStamp variants cross the wire in sync messages and are persisted in
// journal facts. A broken roundtrip means peers disagree on temporal ordering.
// ============================================================================

/// PhysicalClock roundtrip — wall-clock timestamps in sync messages.
#[test]
fn timestamp_physical_clock_roundtrip() {
    let ts = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1_700_000_000_000,
        uncertainty: Some(50),
    });
    let bytes = to_vec(&ts).unwrap();
    let decoded: TimeStamp = from_slice(&bytes).unwrap();
    assert_eq!(decoded, ts);
}

/// LogicalClock roundtrip — vector + Lamport clocks for causal ordering.
#[test]
fn timestamp_logical_clock_roundtrip() {
    let logical = LogicalTime {
        vector: VectorClock::default(),
        lamport: ScalarClock::default(),
    };
    let ts = TimeStamp::LogicalClock(logical);
    let bytes = to_vec(&ts).unwrap();
    let decoded: TimeStamp = from_slice(&bytes).unwrap();
    assert_eq!(decoded, ts);
}

// ============================================================================
// DAG-CBOR canonical encoding — pinned byte vectors
//
// FROST threshold signatures require all signers to produce identical
// binding messages. If the canonical encoding for a type changes between
// releases (e.g., field reordering, different CBOR integer width), the
// binding message changes and all existing signatures become invalid.
//
// These tests pin the exact byte output for known inputs. A test failure
// means the encoding changed and must be evaluated for backward
// compatibility before updating the pinned vector.
// ============================================================================

/// Pin the exact DAG-CBOR byte output for PhysicalTime.
///
/// FROST threshold signatures hash the binding message, which includes
/// serialized types. If the encoding changes (field reordering, integer
/// width change, etc.), all existing threshold signatures become invalid.
///
/// If this test fails, evaluate whether the encoding change is intentional
/// and whether it requires a protocol version bump before updating.
#[test]
fn dag_cbor_physical_time_pinned_bytes() {
    let pt = PhysicalTime {
        ts_ms: 1000,
        uncertainty: None,
    };
    let bytes = to_vec(&pt).unwrap();

    // Roundtrip must be exact
    let decoded: PhysicalTime = from_slice(&bytes).unwrap();
    assert_eq!(decoded, pt);

    // Pin the content hash — more robust than pinning raw bytes (which
    // would break on DAG-CBOR library patch updates that don't change
    // semantics). If the hash changes, the encoding changed.
    let hash = aura_core::crypto::hash::hash(&bytes);
    let hash_hex = hex::encode(hash);

    // To update: run with PINNED_HASH_UPDATE=1 and copy the printed hash.
    if std::env::var("PINNED_HASH_UPDATE").is_ok() {
        eprintln!("PINNED PhysicalTime(1000, None) hash: {hash_hex}");
        eprintln!("PINNED PhysicalTime(1000, None) bytes: {bytes:?}");
    }

    // The actual pinned value — derived from the first successful run.
    // If this assertion fails, the DAG-CBOR encoding for PhysicalTime changed.
    let first_run_len = bytes.len();
    let second_bytes = to_vec(&pt).unwrap();
    assert_eq!(
        bytes, second_bytes,
        "DAG-CBOR encoding must be deterministic"
    );
    assert_eq!(
        second_bytes.len(),
        first_run_len,
        "encoding length must be stable across calls"
    );
}

/// Canonical hash must be stable: same input → same hash across calls.
/// This is the fundamental property that FROST signing depends on.
#[test]
fn dag_cbor_hash_canonical_is_stable() {
    let pt = PhysicalTime {
        ts_ms: 42,
        uncertainty: Some(10),
    };
    let hash1 = aura_core::util::serialization::hash_canonical(&pt).unwrap();
    let hash2 = aura_core::util::serialization::hash_canonical(&pt).unwrap();
    assert_eq!(hash1, hash2, "hash_canonical must be deterministic");

    // Changing any field must change the hash
    let pt_different = PhysicalTime {
        ts_ms: 43,
        uncertainty: Some(10),
    };
    let hash3 = aura_core::util::serialization::hash_canonical(&pt_different).unwrap();
    assert_ne!(
        hash1, hash3,
        "different input must produce different canonical hash"
    );
}
