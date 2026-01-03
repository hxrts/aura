#![allow(missing_docs)]
#![allow(clippy::expect_used)] // Test code uses expect for clarity
use aura_core::{Fact, FactValue, Journal};
use aura_sync::protocols::{AntiEntropyConfig, AntiEntropyProtocol, JournalDigest};

#[test]
fn anti_entropy_digest_is_stable_for_identical_inputs() {
    let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

    let mut facts = Fact::new();
    facts
        .insert_with_context(
            "example".to_string(),
            FactValue::Bytes(vec![1, 2, 3]),
            aura_core::ActorId::synthetic("anti-entropy-test"),
            aura_core::FactTimestamp::new(0),
            None,
        )
        .expect("fact insert should succeed");
    let journal = Journal::with_facts(facts);
    let ops = Vec::new();

    let digest_first = protocol
        .compute_digest(&journal, &ops)
        .unwrap_or_else(|e| panic!("digest computation succeeds: {e}"));
    let digest_second = protocol
        .compute_digest(&journal, &ops)
        .unwrap_or_else(|e| panic!("digest computation succeeds: {e}"));

    assert_eq!(digest_first, digest_second);

    let encoded =
        serde_json::to_vec(&digest_first).unwrap_or_else(|e| panic!("digest serializes: {e}"));
    let decoded: JournalDigest =
        serde_json::from_slice(&encoded).unwrap_or_else(|e| panic!("digest deserializes: {e}"));
    assert_eq!(digest_first, decoded);
}
