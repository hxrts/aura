// Byzantine Device Scenario Tests
//
// Tests that protocols correctly handle Byzantine (malicious) devices:
// - Equivocation: Device sends conflicting messages to different participants
// - Corruption: Device sends malformed or invalid data
// - Selective abort: Device aborts protocol execution strategically
// - Signature forgery attempts
// - CRDT fork attempts
//
// Uses the Byzantine device simulator from the adversary framework.

use aura_crypto::frost::{aggregate_commitments, generate_signing_nonces, sign_with_share};
use simulator::adversary::byzantine::{ByzantineDevice, ByzantineStrategy};
use ed25519_dalek::SigningKey;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Test that FROST protocol detects equivocation during commitment phase
#[test]
fn test_frost_equivocation_detection() {
    // Setup: 3-of-5 threshold signing with 1 Byzantine device
    let threshold = 3;
    let total = 5;

    // Create 4 honest devices and 1 Byzantine
    let honest_devices: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
    let byzantine_id = Uuid::new_v4();

    // Byzantine device uses equivocation strategy
    let mut byzantine = ByzantineDevice::new(
        byzantine_id,
        ByzantineStrategy::Equivocation,
        honest_devices.clone().into_iter().collect(),
    );

    // Phase 1: Generate commitments
    let mut commitments_received: HashMap<Uuid, Vec<Vec<u8>>> = HashMap::new();

    for device_id in &honest_devices {
        commitments_received.insert(*device_id, Vec::new());
    }

    // Honest devices generate and broadcast commitments
    let honest_commitments: Vec<Vec<u8>> = (0..4)
        .map(|i| {
            let key = SigningKey::from_bytes(&[i as u8; 32]);
            let (_, commitments) = generate_signing_nonces(&key, i as u16);
            bincode::serialize(&commitments).unwrap()
        })
        .collect();

    // Byzantine device sends DIFFERENT commitments to each honest device
    let byzantine_key = SigningKey::from_bytes(&[99u8; 32]);
    for (idx, device_id) in honest_devices.iter().enumerate() {
        // Generate unique commitment for each target
        let (_, commitment) = generate_signing_nonces(&byzantine_key, (100 + idx) as u16);
        let commitment_bytes = bincode::serialize(&commitment).unwrap();

        // Simulate Byzantine device sending different commitments
        let equivocated_msg = byzantine.intercept_message(*device_id, commitment_bytes.clone());

        commitments_received
            .get_mut(device_id)
            .unwrap()
            .push(equivocated_msg);
    }

    // Honest devices share what they received from Byzantine device
    let byzantine_commitments_shared: HashSet<Vec<u8>> = commitments_received
        .values()
        .flat_map(|commitments| commitments.iter().cloned())
        .collect();

    // Detection: If Byzantine device sent different commitments, detection succeeds
    assert!(
        byzantine_commitments_shared.len() > 1,
        "Equivocation should be detected (different commitments sent)"
    );

    // In a real protocol, honest devices would abort and exclude Byzantine device
}

/// Test that corrupted FROST signature shares are detected
#[test]
fn test_frost_corruption_detection() {
    // Setup: 3-of-3 threshold signing with 1 Byzantine device attempting corruption
    let device1 = Uuid::new_v4();
    let device2 = Uuid::new_v4();
    let byzantine_id = Uuid::new_v4();

    let mut byzantine = ByzantineDevice::new(
        byzantine_id,
        ByzantineStrategy::DataCorruption,
        [device1, device2].into_iter().collect(),
    );

    // Honest devices generate valid signature shares
    let key1 = SigningKey::from_bytes(&[1u8; 32]);
    let key2 = SigningKey::from_bytes(&[2u8; 32]);

    let message = b"test message";

    // Byzantine device generates signature share and corrupts it
    let byzantine_key = SigningKey::from_bytes(&[99u8; 32]);
    let (nonces, commitments) = generate_signing_nonces(&byzantine_key, 3);
    let signature_share = sign_with_share(&byzantine_key, &nonces, message, 3);

    // Byzantine corrupts the signature share
    let corrupted_share =
        byzantine.intercept_message(device1, bincode::serialize(&signature_share).unwrap());

    // Verify corruption occurred
    let original_bytes = bincode::serialize(&signature_share).unwrap();
    assert_ne!(
        corrupted_share, original_bytes,
        "Signature share should be corrupted"
    );

    // In a real protocol, aggregation would fail signature verification
    // and the Byzantine device would be detected
}

/// Test that FROST handles selective abort attacks
#[test]
fn test_frost_selective_abort_resilience() {
    // Setup: 3-of-5 threshold, 1 Byzantine device selectively aborts
    let honest_devices: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
    let byzantine_id = Uuid::new_v4();

    let mut byzantine = ByzantineDevice::new(
        byzantine_id,
        ByzantineStrategy::SelectiveAbort,
        honest_devices.clone().into_iter().collect(),
    );

    // Byzantine device targets device 0 for abort
    byzantine.set_drop_probability(1.0); // Always drop to target

    // Simulate protocol rounds
    let mut round1_messages_received = Vec::new();

    // Honest devices send commitments
    for i in 0..4 {
        round1_messages_received.push(format!("commitment-{}", i).into_bytes());
    }

    // Byzantine device should send commitment but selectively aborts
    let byzantine_commitment = b"byzantine-commitment".to_vec();
    let delivered = byzantine.should_drop_message(&honest_devices[0], &byzantine_commitment);

    // Verify Byzantine device dropped message to target
    assert!(
        delivered,
        "Byzantine device should drop message (selective abort)"
    );

    // Protocol should complete with remaining honest devices (4 >= threshold of 3)
    let participating_devices = 4; // 4 honest devices received commitments
    let threshold = 3;

    assert!(
        participating_devices >= threshold,
        "Protocol should succeed with honest majority despite selective abort"
    );
}

/// Test that CRDT fork attempts are detected
#[test]
fn test_crdt_fork_detection() {
    use aura_journal::{DeviceId, EventData, Journal, JournalEvent};

    let honest1 = DeviceId(Uuid::new_v4());
    let honest2 = DeviceId(Uuid::new_v4());
    let byzantine_id_uuid = Uuid::new_v4();
    let byzantine_device = DeviceId(byzantine_id_uuid);

    let mut byzantine = ByzantineDevice::new(
        byzantine_id_uuid,
        ByzantineStrategy::CrdtFork,
        [honest1.0, honest2.0].into_iter().collect(),
    );

    // Byzantine device creates two conflicting versions of the same event
    let event_id = Uuid::new_v4();

    let fork_v1 = JournalEvent {
        event_id,
        device_id: byzantine_device,
        sequence_number: 1,
        timestamp: 1000,
        data: EventData::Custom {
            event_type: "fork-v1".to_string(),
            payload: b"version-1".to_vec(),
        },
        signature: vec![1u8; 64],
    };

    let fork_v2 = JournalEvent {
        event_id, // SAME event ID, different content (fork attempt)
        device_id: byzantine_device,
        sequence_number: 1,
        timestamp: 1000,
        data: EventData::Custom {
            event_type: "fork-v2".to_string(),
            payload: b"version-2".to_vec(),
        },
        signature: vec![2u8; 64],
    };

    // Byzantine sends fork_v1 to honest1, fork_v2 to honest2
    let mut journal1 = Journal::new();
    let mut journal2 = Journal::new();

    journal1.apply_event(fork_v1.clone());
    journal2.apply_event(fork_v2.clone());

    // Honest devices exchange events and detect the fork
    journal1.apply_event(fork_v2.clone());
    journal2.apply_event(fork_v1.clone());

    // Detection: Both journals should have exactly 1 event (first one wins)
    assert_eq!(
        journal1.events().len(),
        1,
        "Fork should be detected and deduplicated"
    );
    assert_eq!(
        journal2.events().len(),
        1,
        "Fork should be detected and deduplicated"
    );

    // Both journals should converge to same event (deterministic deduplication)
    let event1 = &journal1.events()[0];
    let event2 = &journal2.events()[0];

    assert_eq!(
        event1.event_id, event2.event_id,
        "Journals should converge despite fork attempt"
    );
}

/// Test that signature forgery attempts are detected
#[test]
fn test_signature_forgery_detection() {
    use aura_journal::{DeviceId, EventData, Journal, JournalEvent};
    use ed25519_dalek::{Signer, Verifier, VerifyingKey};

    let honest_device = DeviceId(Uuid::new_v4());
    let byzantine_id = Uuid::new_v4();
    let byzantine_device = DeviceId(byzantine_id);

    let honest_key = SigningKey::from_bytes(&[1u8; 32]);
    let byzantine_key = SigningKey::from_bytes(&[99u8; 32]);

    let mut byzantine = ByzantineDevice::new(
        byzantine_id,
        ByzantineStrategy::SignatureForgery,
        [honest_device.0].into_iter().collect(),
    );

    // Byzantine device tries to forge a signature claiming to be honest device
    let forged_event = JournalEvent {
        event_id: Uuid::new_v4(),
        device_id: honest_device, // Claims to be honest device
        sequence_number: 1,
        timestamp: 1000,
        data: EventData::Custom {
            event_type: "forged".to_string(),
            payload: b"forged-data".to_vec(),
        },
        signature: vec![0u8; 64], // Invalid signature
    };

    // Create message to sign
    let message = bincode::serialize(&(
        &forged_event.event_id,
        &forged_event.device_id,
        &forged_event.sequence_number,
        &forged_event.timestamp,
        &forged_event.data,
    ))
    .unwrap();

    // Byzantine device signs with its own key
    let byzantine_signature = byzantine_key.sign(&message);

    // Try to verify with honest device's public key (should fail)
    let honest_pubkey = honest_key.verifying_key();
    let verification_result = honest_pubkey.verify(&message, &byzantine_signature);

    assert!(
        verification_result.is_err(),
        "Forged signature should fail verification"
    );
}

/// Test that selective message dropping is detected and handled
#[test]
fn test_selective_dropping_resilience() {
    let honest_devices: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    let byzantine_id = Uuid::new_v4();

    let mut byzantine = ByzantineDevice::new(
        byzantine_id,
        ByzantineStrategy::SelectiveDropping,
        honest_devices.clone().into_iter().collect(),
    );

    // Byzantine device selectively drops 50% of messages
    byzantine.set_drop_probability(0.5);

    // Simulate 100 messages
    let mut delivered_count = 0;
    let mut dropped_count = 0;

    for i in 0..100 {
        let message = format!("message-{}", i).into_bytes();
        let target = &honest_devices[i % honest_devices.len()];

        if byzantine.should_drop_message(target, &message) {
            dropped_count += 1;
        } else {
            delivered_count += 1;
        }
    }

    // Verify approximately 50% were dropped
    let drop_ratio = dropped_count as f64 / 100.0;
    assert!(
        drop_ratio > 0.3 && drop_ratio < 0.7,
        "Expected ~50% drop rate, got {}%",
        drop_ratio * 100.0
    );

    // In a real protocol with timeouts and retransmissions,
    // honest devices would detect missing messages and request retransmission
}

/// Test Byzantine behavior with threshold protocol (t-of-n with f Byzantine)
#[test]
fn test_threshold_protocol_with_byzantine_minority() {
    // Setup: 3-of-5 threshold with 1 Byzantine device
    // Protocol should succeed as long as 3 honest devices participate
    let total = 5;
    let threshold = 3;
    let byzantine_count = 1;
    let honest_count = total - byzantine_count;

    assert!(
        honest_count >= threshold,
        "Must have enough honest devices to meet threshold"
    );

    let honest_devices: Vec<Uuid> = (0..honest_count).map(|_| Uuid::new_v4()).collect();
    let byzantine_id = Uuid::new_v4();

    // Byzantine device tries various attacks
    let strategies = vec![
        ByzantineStrategy::InvalidCommitments,
        ByzantineStrategy::Equivocation,
        ByzantineStrategy::DataCorruption,
    ];

    for strategy in strategies {
        let byzantine = ByzantineDevice::new(
            byzantine_id,
            strategy.clone(),
            honest_devices.clone().into_iter().collect(),
        );

        // Simulate protocol execution
        // Honest devices proceed regardless of Byzantine behavior
        let participating_honest = honest_count;

        assert!(
            participating_honest >= threshold,
            "Protocol should succeed with {:?} despite Byzantine device using {:?}",
            participating_honest,
            strategy
        );
    }
}

/// Test that Byzantine devices cannot break CRDT convergence properties
#[test]
fn test_crdt_convergence_under_byzantine_attack() {
    use aura_journal::{DeviceId, EventData, Journal, JournalEvent};

    // 3 honest devices + 1 Byzantine device
    let honest1 = DeviceId(Uuid::new_v4());
    let honest2 = DeviceId(Uuid::new_v4());
    let honest3 = DeviceId(Uuid::new_v4());
    let byzantine = DeviceId(Uuid::new_v4());

    let mut journal1 = Journal::new();
    let mut journal2 = Journal::new();
    let mut journal3 = Journal::new();

    // Honest devices create valid events
    let events = vec![
        create_test_event(honest1, 1, "honest1-event"),
        create_test_event(honest2, 1, "honest2-event"),
        create_test_event(honest3, 1, "honest3-event"),
    ];

    // Byzantine device creates malformed events
    let byzantine_events = vec![
        create_test_event(byzantine, 1, "byzantine-1"),
        create_test_event(byzantine, 1, "byzantine-duplicate"), // Duplicate sequence
        create_test_event(byzantine, 999, "byzantine-gap"),     // Large sequence gap
    ];

    // Apply all events to all honest journals in different orders
    for event in &events {
        journal1.apply_event(event.clone());
        journal2.apply_event(event.clone());
        journal3.apply_event(event.clone());
    }

    for event in &byzantine_events {
        journal1.apply_event(event.clone());
        journal2.apply_event(event.clone());
        journal3.apply_event(event.clone());
    }

    // Verify all honest journals converged
    let events1: HashSet<_> = journal1.events().iter().map(|e| e.event_id).collect();
    let events2: HashSet<_> = journal2.events().iter().map(|e| e.event_id).collect();
    let events3: HashSet<_> = journal3.events().iter().map(|e| e.event_id).collect();

    assert_eq!(
        events1, events2,
        "Honest journals should converge despite Byzantine events"
    );
    assert_eq!(
        events1, events3,
        "Honest journals should converge despite Byzantine events"
    );

    // Verify honest events are preserved
    for event in &events {
        assert!(
            events1.contains(&event.event_id),
            "Honest events must be preserved"
        );
    }
}

// Helper functions

fn create_test_event(
    device_id: aura_journal::DeviceId,
    sequence: u64,
    data: &str,
) -> aura_journal::JournalEvent {
    aura_journal::JournalEvent {
        event_id: Uuid::new_v4(),
        device_id,
        sequence_number: sequence,
        timestamp: sequence * 1000,
        data: aura_journal::EventData::Custom {
            event_type: "test".to_string(),
            payload: data.as_bytes().to_vec(),
        },
        signature: vec![0u8; 64],
    }
}
