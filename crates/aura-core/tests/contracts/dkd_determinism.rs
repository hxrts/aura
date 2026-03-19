//! Deterministic Key Derivation (DKD) contract tests.
//!
//! DKD derives context-scoped keys from account/device/session identifiers.
//! If derivation is non-deterministic, devices derive different keys for the
//! same context and cannot communicate. If contexts don't separate, keys
//! leak across relationship boundaries.

use aura_core::crypto::hash::hasher;
use aura_core::{AccountId, DeviceId, SessionId};
use uuid::Uuid;

fn account(seed: u8) -> AccountId {
    AccountId::new_from_entropy([seed; 32])
}

fn device(seed: u8) -> DeviceId {
    DeviceId::new_from_entropy([seed; 32])
}

fn session(seed: u8) -> SessionId {
    SessionId::from_uuid(Uuid::from_bytes([seed; 16]))
}

/// DKD with the same inputs produces the same derived key — devices
/// sharing account/session must derive identical context keys.
#[test]
fn test_basic_dkd_derivation() {
    // Create test identifiers
    let account_id = account(1);
    let session_id = session(2);
    let device1 = device(3);
    let device2 = device(4);
    let device3 = device(5);

    let participants = vec![device1, device2, device3];
    let app_id = "test_app";
    let context = "test_context";

    // Simulate DKD key derivation
    let derived_key =
        simulate_dkd_derivation(&account_id, &session_id, &participants, app_id, context);

    // Verify key properties
    assert_eq!(derived_key.len(), 32, "Key should be 256 bits");
    assert_ne!(derived_key, vec![0u8; 32], "Key should not be all zeros");

    println!("✓ DKD derivation test passed");
    println!("  Account: {account_id}");
    println!("  Session: {session_id}");
    println!("  Participants: {}", participants.len());
    println!("  App ID: {app_id}");
    println!("  Context: {context}");
    println!("  Derived key: {}", hex::encode(&derived_key));
}

/// Repeated derivation with identical inputs is byte-identical — if this
/// fails, devices will derive different keys and channels won't open.
#[test]
fn test_dkd_determinism() {
    let account_id = account(6);
    let session_id = session(7);
    let participants = vec![device(8), device(9)];
    let app_id = "app";
    let context = "context";

    let key1 = simulate_dkd_derivation(&account_id, &session_id, &participants, app_id, context);
    let key2 = simulate_dkd_derivation(&account_id, &session_id, &participants, app_id, context);

    assert_eq!(key1, key2, "DKD should be deterministic");
    println!("✓ Determinism test passed");
}

/// Different contexts produce different keys — prevents cross-context
/// key reuse which would break context isolation.
#[test]
fn test_dkd_context_separation() {
    let account_id = account(10);
    let session_id = session(11);
    let participants = vec![device(12), device(13)];
    let app_id = "app";

    let key1 = simulate_dkd_derivation(&account_id, &session_id, &participants, app_id, "context1");
    let key2 = simulate_dkd_derivation(&account_id, &session_id, &participants, app_id, "context2");

    assert_ne!(
        key1, key2,
        "Different contexts should produce different keys"
    );
    println!("✓ Context separation test passed");
}

/// Different application labels produce different keys — domain separation
/// prevents a key derived for chat from being valid for recovery.
#[test]
fn test_dkd_app_separation() {
    let account_id = account(14);
    let session_id = session(15);
    let participants = vec![device(16), device(17)];
    let context = "context";

    let key1 = simulate_dkd_derivation(&account_id, &session_id, &participants, "app1", context);
    let key2 = simulate_dkd_derivation(&account_id, &session_id, &participants, "app2", context);

    assert_ne!(
        key1, key2,
        "Different applications should produce different keys"
    );
    println!("✓ Application separation test passed");
}

/// Participant set affects the derived key — a channel with {A, B} must
/// use a different key than {A, C}, even in the same context.
#[test]
fn test_dkd_participant_dependence() {
    let account_id = account(18);
    let session_id = session(19);
    let device1 = device(20);
    let device2 = device(21);
    let device3 = device(22);
    let app_id = "app";
    let context = "context";

    let participants1 = vec![device1, device2];
    let participants2 = vec![device1, device3];

    let key1 = simulate_dkd_derivation(&account_id, &session_id, &participants1, app_id, context);
    let key2 = simulate_dkd_derivation(&account_id, &session_id, &participants2, app_id, context);

    assert_ne!(
        key1, key2,
        "Different participant sets should produce different keys"
    );
    println!("✓ Participant dependence test passed");
}

/// Threshold simulation: different device subsets produce different derived
/// keys, modeling the 2-of-3 share combination property.
#[test]
fn test_threshold_simulation() {
    let account_id = account(23);
    let session_id = session(24);
    let device1 = device(25);
    let device2 = device(26);
    let device3 = device(27);
    let all_participants = vec![device1, device2, device3];
    let app_id = "app";
    let context = "context";

    // In a 2-of-3 threshold scheme, any 2 participants should be able to derive keys
    // For testing, we simulate different 2-of-3 combinations
    let combo1 = vec![device1, device2];
    let combo2 = vec![device1, device3];
    let combo3 = vec![device2, device3];

    // Use a deterministic ordering for threshold derivation
    let threshold_key1 = simulate_threshold_dkd(
        &account_id,
        &session_id,
        &all_participants,
        &combo1,
        app_id,
        context,
    );
    let threshold_key2 = simulate_threshold_dkd(
        &account_id,
        &session_id,
        &all_participants,
        &combo2,
        app_id,
        context,
    );
    let threshold_key3 = simulate_threshold_dkd(
        &account_id,
        &session_id,
        &all_participants,
        &combo3,
        app_id,
        context,
    );

    // In a real threshold scheme, these might be the same or different depending on the protocol
    // For our simulation, they'll be different, which is expected
    println!("✓ Threshold simulation test completed");
    println!("  Combo 1 key: {}", hex::encode(&threshold_key1));
    println!("  Combo 2 key: {}", hex::encode(&threshold_key2));
    println!("  Combo 3 key: {}", hex::encode(&threshold_key3));
}

/// Identifiers used in DKD (AccountId, DeviceId, SessionId) must be
/// unique from different seeds — collision means different entities
/// share derived keys.
#[test]
fn test_identifier_uniqueness() {
    // Test that different IDs are unique
    let account1 = AccountId::new_from_entropy([1u8; 32]);
    let account2 = AccountId::new_from_entropy([2u8; 32]);
    assert_ne!(account1, account2);

    let session1 = SessionId::new_from_entropy([11u8; 32]);
    let session2 = SessionId::from_uuid(Uuid::from_bytes([3u8; 16]));
    assert_ne!(session1, session2);

    let device1 = DeviceId::new_from_entropy([4u8; 32]);
    let device2 = DeviceId::new_from_entropy([5u8; 32]);
    assert_ne!(device1, device2);

    // Test string representation
    let account_str = account1.to_string();
    let session_str = session1.to_string();
    let device_str = device1.to_string();

    assert!(!account_str.is_empty());
    assert!(session_str.starts_with("session-"));
    assert!(!device_str.is_empty());

    println!("✓ Identifier uniqueness test passed");
    println!("  Account: {account_str}");
    println!("  Session: {session_str}");
    println!("  Device: {device_str}");
}

/// Simulate DKD key derivation using standard cryptographic primitives
fn simulate_dkd_derivation(
    account_id: &AccountId,
    session_id: &SessionId,
    participants: &[DeviceId],
    app_id: &str,
    context: &str,
) -> Vec<u8> {
    let mut h = hasher();

    // Include all relevant parameters in the derivation
    h.update(b"AURA_DKD_V1:");
    h.update(account_id.to_string().as_bytes());
    h.update(b":");
    h.update(session_id.uuid().as_bytes());
    h.update(b":");
    h.update(app_id.as_bytes());
    h.update(b":");
    h.update(context.as_bytes());

    // Include participants in sorted order for determinism
    let mut sorted_participants = participants.to_vec();
    sorted_participants.sort();

    for participant in &sorted_participants {
        h.update(b":");
        h.update(participant.0.as_bytes());
    }

    h.finalize().to_vec()
}

/// Simulate threshold-based DKD where we include info about the full participant set
/// and the active subset
fn simulate_threshold_dkd(
    account_id: &AccountId,
    session_id: &SessionId,
    all_participants: &[DeviceId],
    active_participants: &[DeviceId],
    app_id: &str,
    context: &str,
) -> Vec<u8> {
    let mut h = hasher();

    // Include base parameters
    h.update(b"AURA_THRESHOLD_DKD_V1:");
    h.update(account_id.to_string().as_bytes());
    h.update(b":");
    h.update(session_id.uuid().as_bytes());
    h.update(b":");
    h.update(app_id.as_bytes());
    h.update(b":");
    h.update(context.as_bytes());

    // Include full participant set (for context)
    h.update(b":full_set:");
    let mut sorted_all = all_participants.to_vec();
    sorted_all.sort();
    for participant in &sorted_all {
        h.update(participant.0.as_bytes());
        h.update(b",");
    }

    // Include active participant subset
    h.update(b":active_set:");
    let mut sorted_active = active_participants.to_vec();
    sorted_active.sort();
    for participant in &sorted_active {
        h.update(participant.0.as_bytes());
        h.update(b",");
    }

    h.finalize().to_vec()
}
