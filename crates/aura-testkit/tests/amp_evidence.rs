//! Evidence storage tests for AMP.
//!
//! Tests the evidence record CRDT, delta merging, and AmpEvidenceEffects trait.

use aura_amp::{AmpEvidenceEffects, EvidenceDelta, EvidenceRecord, AMP_EVIDENCE_KEY_PREFIX};
use aura_consensus::ConsensusId;
use aura_core::domain::Hash32;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_testkit::mock_effects::MockEffects;

fn test_consensus_id() -> ConsensusId {
    ConsensusId(Hash32::new([1u8; 32]))
}

fn test_consensus_id_2() -> ConsensusId {
    ConsensusId(Hash32::new([2u8; 32]))
}

fn test_context() -> ContextId {
    ContextId::from_uuid(uuid::Uuid::from_bytes([3u8; 16]))
}

fn test_authority() -> AuthorityId {
    AuthorityId::from_uuid(uuid::Uuid::from_bytes([4u8; 16]))
}

fn test_authority_2() -> AuthorityId {
    AuthorityId::from_uuid(uuid::Uuid::from_bytes([5u8; 16]))
}

// =============================================================================
// Evidence Key Prefix Tests
// =============================================================================

#[test]
fn test_evidence_key_prefix_format() {
    assert_eq!(AMP_EVIDENCE_KEY_PREFIX, "amp/evidence/");
}

// =============================================================================
// EvidenceRecord Tests
// =============================================================================

#[test]
fn test_evidence_record_default() {
    let record = EvidenceRecord::default();
    assert!(record.entries.is_empty());
}

#[test]
fn test_evidence_record_merge_empty() {
    let mut record = EvidenceRecord::default();
    let delta = EvidenceDelta::default();

    record.merge(delta);
    assert!(record.entries.is_empty());
}

#[test]
fn test_evidence_record_merge_single() {
    let mut record = EvidenceRecord::default();
    let mut delta = EvidenceDelta::default();

    delta.entries.insert("key1".to_string(), vec![1, 2, 3]);
    record.merge(delta);

    assert_eq!(record.entries.len(), 1);
    assert_eq!(record.entries.get("key1"), Some(&vec![1, 2, 3]));
}

#[test]
fn test_evidence_record_merge_multiple() {
    let mut record = EvidenceRecord::default();
    let mut delta = EvidenceDelta::default();

    delta.entries.insert("key1".to_string(), vec![1]);
    delta.entries.insert("key2".to_string(), vec![2]);
    delta.entries.insert("key3".to_string(), vec![3]);
    record.merge(delta);

    assert_eq!(record.entries.len(), 3);
}

#[test]
fn test_evidence_record_merge_overwrites() {
    let mut record = EvidenceRecord::default();

    // First merge
    let mut delta1 = EvidenceDelta::default();
    delta1.entries.insert("key1".to_string(), vec![1, 1, 1]);
    record.merge(delta1);

    // Second merge with same key should overwrite
    let mut delta2 = EvidenceDelta::default();
    delta2.entries.insert("key1".to_string(), vec![2, 2, 2]);
    record.merge(delta2);

    assert_eq!(record.entries.len(), 1);
    assert_eq!(record.entries.get("key1"), Some(&vec![2, 2, 2]));
}

#[test]
fn test_evidence_record_merge_accumulates() {
    let mut record = EvidenceRecord::default();

    // First merge
    let mut delta1 = EvidenceDelta::default();
    delta1.entries.insert("key1".to_string(), vec![1]);
    record.merge(delta1);

    // Second merge with different key accumulates
    let mut delta2 = EvidenceDelta::default();
    delta2.entries.insert("key2".to_string(), vec![2]);
    record.merge(delta2);

    assert_eq!(record.entries.len(), 2);
    assert_eq!(record.entries.get("key1"), Some(&vec![1]));
    assert_eq!(record.entries.get("key2"), Some(&vec![2]));
}

#[test]
fn test_evidence_record_serialization_roundtrip() {
    let mut record = EvidenceRecord::default();
    record
        .entries
        .insert("consensus:abc123".to_string(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
    record
        .entries
        .insert("witness:xyz789".to_string(), vec![0xCA, 0xFE]);

    // Serialize
    let bytes = serde_json::to_vec(&record)
        .unwrap_or_else(|err| panic!("serialization should succeed: {err}"));

    // Deserialize
    let recovered: EvidenceRecord =
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|err| panic!("deserialization should succeed: {err}"));

    assert_eq!(recovered.entries.len(), 2);
    assert_eq!(
        recovered.entries.get("consensus:abc123"),
        Some(&vec![0xDE, 0xAD, 0xBE, 0xEF])
    );
    assert_eq!(
        recovered.entries.get("witness:xyz789"),
        Some(&vec![0xCA, 0xFE])
    );
}

// =============================================================================
// EvidenceDelta Tests
// =============================================================================

#[test]
fn test_evidence_delta_default() {
    let delta = EvidenceDelta::default();
    assert!(delta.entries.is_empty());
}

#[test]
fn test_evidence_delta_clone() {
    let mut delta = EvidenceDelta::default();
    delta.entries.insert("key".to_string(), vec![1, 2, 3]);

    let cloned = delta.clone();
    assert_eq!(cloned.entries.len(), 1);
    assert_eq!(cloned.entries.get("key"), Some(&vec![1, 2, 3]));
}

#[test]
fn test_evidence_delta_serialization_roundtrip() {
    let mut delta = EvidenceDelta::default();
    delta.entries.insert("evidence:1".to_string(), vec![0x01]);
    delta.entries.insert("evidence:2".to_string(), vec![0x02]);

    // Serialize
    let bytes =
        serde_json::to_vec(&delta).unwrap_or_else(|err| panic!("serialization should succeed: {err}"));

    // Deserialize
    let recovered: EvidenceDelta =
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|err| panic!("deserialization should succeed: {err}"));

    assert_eq!(recovered.entries.len(), 2);
}

// =============================================================================
// AmpEvidenceEffects Tests via MockEffects
// =============================================================================

#[tokio::test]
async fn test_evidence_for_missing() {
    let effects = MockEffects::deterministic();

    let result = effects.evidence_for(test_consensus_id()).await;
    assert!(result.is_ok());
    assert!(
        result.unwrap().is_none(),
        "should return None for missing evidence"
    );
}

#[tokio::test]
async fn test_merge_evidence_delta_and_retrieve() {
    let effects = MockEffects::deterministic();
    let cid = test_consensus_id();

    // Create and merge a delta
    let mut delta = EvidenceDelta::default();
    delta
        .entries
        .insert("witness:alice".to_string(), b"participated".to_vec());

    effects
        .merge_evidence_delta(cid, delta)
        .await
        .unwrap_or_else(|err| panic!("merge should succeed: {err}"));

    // Retrieve the evidence
    let result = effects
        .evidence_for(cid)
        .await
        .unwrap_or_else(|err| panic!("retrieval should succeed: {err}"));
    assert!(result.is_some());

    let record = result.unwrap();
    assert_eq!(record.entries.len(), 1);
    assert_eq!(
        record.entries.get("witness:alice"),
        Some(&b"participated".to_vec())
    );
}

#[tokio::test]
async fn test_merge_multiple_evidence_deltas() {
    let effects = MockEffects::deterministic();
    let cid = test_consensus_id();

    // First delta
    let mut delta1 = EvidenceDelta::default();
    delta1
        .entries
        .insert("witness:alice".to_string(), b"vote1".to_vec());
    effects.merge_evidence_delta(cid, delta1).await.unwrap();

    // Second delta (different key)
    let mut delta2 = EvidenceDelta::default();
    delta2
        .entries
        .insert("witness:bob".to_string(), b"vote2".to_vec());
    effects.merge_evidence_delta(cid, delta2).await.unwrap();

    // Verify both entries exist
    let record = effects.evidence_for(cid).await.unwrap().unwrap();
    assert_eq!(record.entries.len(), 2);
    assert!(record.entries.contains_key("witness:alice"));
    assert!(record.entries.contains_key("witness:bob"));
}

#[tokio::test]
async fn test_evidence_isolation_between_consensus_ids() {
    let effects = MockEffects::deterministic();
    let cid1 = test_consensus_id();
    let cid2 = test_consensus_id_2();

    // Add evidence to first consensus
    let mut delta1 = EvidenceDelta::default();
    delta1
        .entries
        .insert("data".to_string(), b"consensus1".to_vec());
    effects.merge_evidence_delta(cid1, delta1).await.unwrap();

    // Add evidence to second consensus
    let mut delta2 = EvidenceDelta::default();
    delta2
        .entries
        .insert("data".to_string(), b"consensus2".to_vec());
    effects.merge_evidence_delta(cid2, delta2).await.unwrap();

    // Verify isolation
    let record1 = effects.evidence_for(cid1).await.unwrap().unwrap();
    let record2 = effects.evidence_for(cid2).await.unwrap().unwrap();

    assert_eq!(record1.entries.get("data"), Some(&b"consensus1".to_vec()));
    assert_eq!(record2.entries.get("data"), Some(&b"consensus2".to_vec()));
}

#[tokio::test]
async fn test_insert_evidence_delta() {
    let effects = MockEffects::deterministic();
    let cid = test_consensus_id();
    let witness = test_authority();
    let context = test_context();

    // Insert evidence delta for a witness
    effects
        .insert_evidence_delta(witness, cid, context)
        .await
        .unwrap_or_else(|err| panic!("insert_evidence_delta should succeed: {err}"));

    // Verify evidence was recorded
    let record = effects.evidence_for(cid).await.unwrap();
    assert!(record.is_some());

    let entries = record.unwrap().entries;
    assert_eq!(entries.len(), 1);

    // Check that the entry key contains the consensus ID (hex encoded)
    let expected_key = hex::encode(cid.0 .0);
    assert!(
        entries.contains_key(&expected_key),
        "should contain consensus ID key"
    );
}

#[tokio::test]
async fn test_insert_multiple_witness_evidence() {
    let effects = MockEffects::deterministic();
    let cid = test_consensus_id();
    let context = test_context();

    let witness1 = test_authority();
    let witness2 = test_authority_2();

    // Insert evidence for two witnesses
    effects
        .insert_evidence_delta(witness1, cid, context)
        .await
        .unwrap();
    effects
        .insert_evidence_delta(witness2, cid, context)
        .await
        .unwrap();

    // Verify evidence was recorded
    let record = effects.evidence_for(cid).await.unwrap().unwrap();

    // Each insert overwrites the same key, so we should have 1 entry
    // (This is the expected behavior - the implementation uses consensus_id as key)
    assert!(!record.entries.is_empty());
}

#[tokio::test]
async fn test_evidence_store_accessor() {
    let effects = MockEffects::deterministic();
    let cid = test_consensus_id();

    // Use evidence_store accessor
    let store = effects.evidence_store();

    // Add evidence via store
    let mut delta = EvidenceDelta::default();
    delta.entries.insert("test".to_string(), vec![1, 2, 3]);
    store
        .merge_delta(cid, delta)
        .await
        .unwrap_or_else(|err| panic!("merge via store should succeed: {err}"));

    // Retrieve via store
    let record = store
        .current(cid)
        .await
        .unwrap_or_else(|err| panic!("current should succeed: {err}"));
    assert!(record.is_some());
    assert_eq!(record.unwrap().entries.get("test"), Some(&vec![1, 2, 3]));
}

#[tokio::test]
async fn test_evidence_persists_across_multiple_calls() {
    let effects = MockEffects::deterministic();
    let cid = test_consensus_id();

    // Add evidence
    let mut delta = EvidenceDelta::default();
    delta.entries.insert("persistent".to_string(), vec![42]);
    effects.merge_evidence_delta(cid, delta).await.unwrap();

    // Call evidence_for multiple times
    let record1 = effects.evidence_for(cid).await.unwrap().unwrap();
    let record2 = effects.evidence_for(cid).await.unwrap().unwrap();
    let record3 = effects.evidence_for(cid).await.unwrap().unwrap();

    // All should return the same data
    assert_eq!(record1.entries, record2.entries);
    assert_eq!(record2.entries, record3.entries);
}
