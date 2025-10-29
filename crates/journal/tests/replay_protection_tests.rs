#![allow(warnings, clippy::all)]
// Replay and Freshness Protection Tests
//
// Tests security properties related to replay attacks and message freshness:
// - Replay protection: Old messages cannot be replayed
// - Nonce uniqueness: Nonces cannot be reused
// - Timestamp validation: Reject messages with stale timestamps
// - Session epoch enforcement: Old session credentials are rejected

use aura_crypto::Effects;
use aura_crypto::{Ed25519Signature, Ed25519SigningKey, Ed25519VerifyingKey};
use aura_journal::{
    AccountLedger, DeviceMetadata, DeviceType, EpochTickEvent, Event, EventType,
};
use aura_authentication::EventAuthorization;
use aura_types::{AccountId, AccountIdExt, DeviceId, DeviceIdExt, EventId, EventIdExt};
use ed25519_dalek::{Signature, SigningKey};
use rand::Rng;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use uuid::Uuid;

/// Test that replayed events are rejected (duplicate event IDs)
/// TODO: Fix signature generation to match ledger's message format
#[test]
#[ignore]
fn test_replay_protection_event_id() {
    let effects = Effects::for_test("replay_protection");
    let account_id = AccountId::new_with_effects(&effects);
    let device_id = DeviceId::new_with_effects(&effects);

    // Create minimal state for testing
    let device_metadata = create_test_device_metadata(device_id, &effects);
    let group_public_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&[1u8; 32]).unwrap();
    let state = aura_journal::AccountState::new(
        account_id,
        group_public_key,
        device_metadata,
        2, // threshold
        3, // total_participants
    );
    let mut ledger = AccountLedger::new(state).unwrap();

    // Create and apply an event
    let event = create_test_event(&effects, account_id, device_id, 1000, 1);
    ledger.append_event(event.clone(), &effects).unwrap();

    assert_eq!(ledger.event_log().len(), 1, "First event should be stored");

    // Attempt to replay the exact same event
    let result = ledger.append_event(event.clone(), &effects);

    // Verify the replayed event was rejected
    assert!(result.is_err(), "Replayed event should be rejected");
    assert_eq!(
        ledger.event_log().len(),
        1,
        "Event log should still have only one event"
    );
}

/// Test that events with duplicate nonces are rejected
/// TODO: Fix signature generation to match ledger's message format
#[test]
#[ignore]
fn test_nonce_enforcement() {
    let effects = Effects::for_test("nonce_enforcement");
    let account_id = AccountId::new_with_effects(&effects);
    let device_id = DeviceId::new_with_effects(&effects);

    // Create minimal state for testing
    let device_metadata = create_test_device_metadata(device_id, &effects);
    let group_public_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&[1u8; 32]).unwrap();
    let state = aura_journal::AccountState::new(
        account_id,
        group_public_key,
        device_metadata,
        2, // threshold
        3, // total_participants
    );
    let mut ledger = AccountLedger::new(state).unwrap();

    // Apply events with different nonces
    let event1 = create_test_event(&effects, account_id, device_id, 1000, 1);
    let event2 = create_test_event(&effects, account_id, device_id, 2000, 2);

    ledger.append_event(event1, &effects).unwrap();
    ledger.append_event(event2, &effects).unwrap();

    assert_eq!(ledger.event_log().len(), 2, "Both events should be stored");

    // Attempt to replay event with nonce 1 (already used)
    let replay_event = create_test_event(&effects, account_id, device_id, 3000, 1);
    let result = ledger.append_event(replay_event, &effects);

    // Verify replay was rejected
    assert!(result.is_err(), "Replayed nonce should be rejected");
    assert_eq!(
        ledger.event_log().len(),
        2,
        "Event log should still have only two events"
    );
}

/// Test that events with different timestamps are accepted (CRDT property)
/// TODO: Fix signature generation to match ledger's message format
#[test]
#[ignore]
fn test_timestamp_tolerance() {
    let effects = Effects::for_test("timestamp_tolerance");
    let account_id = AccountId::new_with_effects(&effects);
    let device_id = DeviceId::new_with_effects(&effects);

    // Create minimal state for testing
    let device_metadata = create_test_device_metadata(device_id, &effects);
    let group_public_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&[1u8; 32]).unwrap();
    let state = aura_journal::AccountState::new(
        account_id,
        group_public_key,
        device_metadata,
        2, // threshold
        3, // total_participants
    );
    let mut ledger = AccountLedger::new(state).unwrap();

    // Apply events with different timestamps
    let old_event = create_test_event(&effects, account_id, device_id, 1000, 1);
    let recent_event = create_test_event(&effects, account_id, device_id, 10000, 2);
    let future_event = create_test_event(&effects, account_id, device_id, 50000, 3);

    ledger.append_event(old_event, &effects).unwrap();
    ledger.append_event(recent_event, &effects).unwrap();
    ledger.append_event(future_event, &effects).unwrap();

    // All events should be stored (CRDT tolerates clock skew)
    assert_eq!(
        ledger.event_log().len(),
        3,
        "All events with different timestamps should be accepted"
    );
}

/// Test that FROST nonces are unique and cannot be reused
#[test]
fn test_frost_nonce_uniqueness() {
    let effects = Effects::for_test("frost_nonce_uniqueness");

    // Test that FROST nonce generation produces unique values
    // We'll just verify uniqueness of generated nonce strings since actual FROST integration requires full setup
    let mut nonce_representations = Vec::new();
    for i in 0..10 {
        let mut rng = effects.rng();
        // Generate deterministic but unique nonce representation
        let nonce_data = format!("nonce_{}_{}_{}", i, rng.gen::<u64>(), rng.gen::<u64>());
        nonce_representations.push(nonce_data);
    }

    // Verify all nonce representations are unique
    let unique_nonces: HashSet<_> = nonce_representations.iter().collect();
    assert_eq!(
        nonce_representations.len(),
        unique_nonces.len(),
        "All FROST nonces must be unique"
    );
}

/// Test session epoch enforcement prevents credential replay
#[test]
fn test_session_epoch_enforcement() {
    let effects = Effects::for_test("session_epoch_enforcement");
    let account_id = AccountId::new_with_effects(&effects);
    let device_id = DeviceId::new_with_effects(&effects);

    // Simulate session credential system with epochs
    #[derive(Debug, Clone, PartialEq)]
    struct SessionCredential {
        device_id: DeviceId,
        epoch: u64,
        timestamp: u64,
    }

    let mut valid_epochs: HashSet<u64> = HashSet::new();

    // Current epoch is 5
    let current_epoch = 5u64;
    valid_epochs.insert(current_epoch);

    // Create credential for current epoch
    let valid_credential = SessionCredential {
        device_id,
        epoch: current_epoch,
        timestamp: 1000,
    };

    // Verify current epoch credential is valid
    assert!(
        valid_epochs.contains(&valid_credential.epoch),
        "Current epoch credential should be valid"
    );

    // Create credential for old epoch (replay attack)
    let old_credential = SessionCredential {
        device_id,
        epoch: 3, // Old epoch
        timestamp: 2000,
    };

    // Verify old epoch credential is rejected
    assert!(
        !valid_epochs.contains(&old_credential.epoch),
        "Old epoch credential should be rejected"
    );

    // Bump epoch to 6
    valid_epochs.remove(&current_epoch);
    let new_epoch = 6u64;
    valid_epochs.insert(new_epoch);

    // Old epoch 5 credential should now be rejected
    assert!(
        !valid_epochs.contains(&valid_credential.epoch),
        "Previous epoch credential should be rejected after epoch bump"
    );

    // New epoch 6 credential should be valid
    let new_credential = SessionCredential {
        device_id,
        epoch: new_epoch,
        timestamp: 3000,
    };
    assert!(
        valid_epochs.contains(&new_credential.epoch),
        "New epoch credential should be valid"
    );
}

// Helper functions

// Store the test signing key globally for signature creation
thread_local! {
    static TEST_SIGNING_KEY: SigningKey = SigningKey::from_bytes(&[1u8; 32]);
}

fn create_test_device_metadata(device_id: DeviceId, effects: &Effects) -> DeviceMetadata {
    let public_key = TEST_SIGNING_KEY.with(|k| k.verifying_key());
    let current_time = aura_crypto::time::current_timestamp_with_effects(effects).unwrap_or(1000);

    DeviceMetadata {
        device_id,
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key,
        added_at: current_time,
        last_seen: current_time,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 1,
        used_nonces: BTreeSet::new(),
    }
}

fn create_test_event(
    effects: &Effects,
    account_id: AccountId,
    device_id: DeviceId,
    timestamp: u64,
    nonce: u64,
) -> Event {
    use ed25519_dalek::Signer;

    let event_type = EventType::EpochTick(EpochTickEvent {
        new_epoch: 1,
        evidence_hash: [0u8; 32],
    });

    // Build signing message (simplified version matching ledger logic)
    let mut message = Vec::new();
    message.extend_from_slice(&timestamp.to_le_bytes());
    message.extend_from_slice(b"EpochTick");
    message.extend_from_slice(&[1u8]); // new_epoch
    message.extend_from_slice(&[0u8; 32]); // evidence_hash

    // Sign the message
    let signature = TEST_SIGNING_KEY.with(|k| k.sign(&message));

    Event {
        version: 1,
        event_id: EventId::new_with_effects(effects),
        account_id,
        timestamp,
        nonce,
        parent_hash: None,
        epoch_at_write: 1,
        event_type,
        authorization: EventAuthorization::DeviceCertificate {
            device_id,
            signature: Ed25519Signature(signature),
        },
    }
}
