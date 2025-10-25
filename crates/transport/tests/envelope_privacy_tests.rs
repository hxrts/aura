#![allow(warnings, clippy::all)]
// DISABLED: This test uses an outdated envelope API that has been refactored
// TODO: Update tests to use the new Envelope structure
#![cfg(disabled)]
//! Envelope Encryption and Metadata Privacy Tests
//!
//! Tests confidentiality, unlinkability, and metadata resistance of envelopes.
//! Reference: security_test_proposal.md Part 1.2

use aura_transport::{Envelope, EnvelopePayload, Header, HeaderBare};

/// Test that all envelopes are constant size
/// Security property: Size doesn't leak information about content
#[test]
fn test_envelope_constant_size() {
    let k_box = [42u8; 32];
    let k_tag = [43u8; 32];
    let epoch = 100;

    // Test various payload sizes
    let test_sizes = vec![1, 10, 100, 500, 1000, 1500, 1900];

    for size in test_sizes {
        let payload = EnvelopePayload {
            content_type: "test".to_string(),
            data: vec![0u8; size],
            timestamp: 1000,
        };

        let envelope = create_envelope(&payload, &k_box, &k_tag, epoch, 5);
        let serialized = serialize_envelope(&envelope);

        assert_eq!(
            serialized.len(),
            2048,
            "Envelope size should always be 2048 bytes, got {} for payload size {}",
            serialized.len(),
            size
        );
    }
}

/// Test routing tag collision resistance
/// Security property: Routing tags should be unique within recognition window
#[test]
fn test_routing_tag_collision_resistance() {
    use std::collections::HashSet;

    let k_tag = [55u8; 32];
    let epoch = 100;

    let mut rtags = HashSet::new();
    let num_envelopes = 10000;

    for counter in 0..num_envelopes {
        let rtag = compute_routing_tag(&k_tag, epoch, counter);

        assert!(
            !rtags.contains(&rtag),
            "Routing tag collision detected at counter {}",
            counter
        );

        rtags.insert(rtag);
    }

    assert_eq!(rtags.len(), num_envelopes);
}

/// Test envelope unlinkability across epochs
/// Security property: Same relationship, different epochs should be unlinkable
#[test]
fn test_envelope_unlinkability_across_epochs() {
    let k_box = [99u8; 32];
    let k_tag = [100u8; 32];
    let counter = 5;

    let payload = EnvelopePayload {
        content_type: "test".to_string(),
        data: vec![1, 2, 3],
        timestamp: 1000,
    };

    // Create envelope at epoch 100
    let env_epoch_100 = create_envelope(&payload, &k_box, &k_tag, 100, counter);
    let rtag_100 = env_epoch_100.header.bare.rtag;

    // Create envelope at epoch 101 (same counter)
    let env_epoch_101 = create_envelope(&payload, &k_box, &k_tag, 101, counter);
    let rtag_101 = env_epoch_101.header.bare.rtag;

    assert_ne!(
        rtag_100, rtag_101,
        "Routing tags should differ across epochs"
    );
}

/// Test ciphertext indistinguishability
/// Security property: Different plaintexts should produce uncorrelated ciphertexts
#[test]
fn test_ciphertext_indistinguishability() {
    let k_box = [77u8; 32];
    let k_tag = [78u8; 32];
    let epoch = 200;

    let payload_a = EnvelopePayload {
        content_type: "type_a".to_string(),
        data: vec![0u8; 1000],
        timestamp: 1000,
    };

    let payload_b = EnvelopePayload {
        content_type: "type_b".to_string(),
        data: vec![1u8; 1000],
        timestamp: 1001,
    };

    let env_a = create_envelope(&payload_a, &k_box, &k_tag, epoch, 10);
    let env_b = create_envelope(&payload_b, &k_box, &k_tag, epoch, 11);

    // Calculate Hamming distance between ciphertexts
    let hamming_dist = hamming_distance(&env_a.ciphertext, &env_b.ciphertext);
    let ciphertext_len = env_a.ciphertext.len();

    // Expect roughly 50% of bits to differ (random-looking)
    let bit_diff_ratio = hamming_dist as f64 / (ciphertext_len * 8) as f64;

    assert!(
        bit_diff_ratio > 0.4 && bit_diff_ratio < 0.6,
        "Ciphertexts should appear random (bit diff ratio: {})",
        bit_diff_ratio
    );
}

/// Test metadata minimization
/// Security property: Only necessary metadata is included
#[test]
fn test_metadata_minimization() {
    let k_box = [11u8; 32];
    let k_tag = [12u8; 32];

    let payload = EnvelopePayload {
        content_type: "test".to_string(),
        data: vec![1, 2, 3],
        timestamp: 5000,
    };

    let envelope = create_envelope(&payload, &k_box, &k_tag, 100, 50);

    // Verify header contains only essential metadata
    assert_eq!(envelope.header.bare.version, 1);
    assert_eq!(envelope.header.bare.epoch, 100);
    assert_eq!(envelope.header.bare.counter, 50);
    assert!(envelope.header.bare.rtag.len() == 16); // 128 bits

    // No sender/receiver identities in clear
    // No timestamps in clear
    // No size information in clear
}

/// Test routing tag determinism
/// Security property: Same inputs produce same routing tag
#[test]
fn test_routing_tag_determinism() {
    let k_tag = [222u8; 32];
    let epoch = 500;
    let counter = 1234;

    let rtag1 = compute_routing_tag(&k_tag, epoch, counter);
    let rtag2 = compute_routing_tag(&k_tag, epoch, counter);

    assert_eq!(rtag1, rtag2, "Routing tags should be deterministic");
}

/// Test different keys produce different routing tags
/// Security property: Routing tags are key-dependent
#[test]
fn test_routing_tag_key_separation() {
    let k_tag_a = [1u8; 32];
    let k_tag_b = [2u8; 32];
    let epoch = 100;
    let counter = 50;

    let rtag_a = compute_routing_tag(&k_tag_a, epoch, counter);
    let rtag_b = compute_routing_tag(&k_tag_b, epoch, counter);

    assert_ne!(
        rtag_a, rtag_b,
        "Different keys should produce different routing tags"
    );
}

// Helper functions

fn create_envelope(
    payload: &EnvelopePayload,
    k_box: &[u8; 32],
    k_tag: &[u8; 32],
    epoch: u64,
    counter: u64,
) -> Envelope {
    use aura_transport::SbbPublisher;

    let publisher = SbbPublisher::new();
    publisher
        .publish_envelope(payload.clone(), k_box, k_tag, counter, epoch, 3)
        .expect("Failed to create envelope")
        .envelope
}

fn serialize_envelope(envelope: &Envelope) -> Vec<u8> {
    // Simplified serialization for testing
    // In reality, would use proper CBOR serialization
    let mut bytes = Vec::new();

    // Header (fixed size)
    bytes.extend_from_slice(&envelope.header.bare.version.to_le_bytes());
    bytes.extend_from_slice(&envelope.header.bare.epoch.to_le_bytes());
    bytes.extend_from_slice(&envelope.header.bare.counter.to_le_bytes());
    bytes.extend_from_slice(&envelope.header.bare.rtag);
    bytes.extend_from_slice(&envelope.header.bare.ttl_epochs.to_le_bytes());

    // Ciphertext (variable, padded to reach 2048)
    bytes.extend_from_slice(&envelope.ciphertext);
    bytes.extend_from_slice(&envelope.padding);

    bytes
}

fn compute_routing_tag(k_tag: &[u8; 32], epoch: u64, counter: u64) -> Vec<u8> {
    use blake3::Hasher;

    let mut hasher = Hasher::new_keyed(k_tag);
    hasher.update(&epoch.to_le_bytes());
    hasher.update(&counter.to_le_bytes());
    hasher.update(b"rt"); // routing tag domain separator

    let hash = hasher.finalize();
    hash.as_bytes()[..16].to_vec() // Truncate to 128 bits
}

fn hamming_distance(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len());

    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x ^ y).count_ones() as usize)
        .sum()
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn test_hamming_distance_calculation() {
        assert_eq!(hamming_distance(&[0b00000000], &[0b11111111]), 8);
        assert_eq!(hamming_distance(&[0b10101010], &[0b01010101]), 8);
        assert_eq!(hamming_distance(&[0b11110000], &[0b00001111]), 8);
        assert_eq!(hamming_distance(&[0b00000000], &[0b00000000]), 0);
    }
}
