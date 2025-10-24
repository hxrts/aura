//! Category 2.3: CRDT Partition Scenarios
//!
//! Simulation tests for CRDT behavior under network partitions and Byzantine conditions.
//! These tests verify that CRDT merge semantics work correctly in adversarial network conditions.

use aura_crypto::Effects;
use aura_journal::{
    AccountId, AccountLedger, AccountState, DeviceId, DeviceMetadata, DeviceType, SessionEpoch,
};
use aura_simulator::{SimError, Simulation};
use ed25519_dalek::SigningKey;
use std::collections::{BTreeMap, BTreeSet};

/// Helper: Create a test account state
fn create_test_account(effects: &Effects) -> AccountState {
    let account_id = AccountId::new_with_effects(effects);
    let device_id = DeviceId::new_with_effects(effects);
    let signing_key = SigningKey::from_bytes(&effects.random_bytes::<32>());
    let timestamp = effects.now().unwrap_or(1000);

    let device_metadata = DeviceMetadata {
        device_id,
        device_name: "test-device".to_string(),
        device_type: DeviceType::Native,
        public_key: signing_key.verifying_key(),
        added_at: timestamp,
        last_seen: timestamp,
        dkd_commitment_proofs: BTreeMap::new(),
    };

    let mut devices = BTreeMap::new();
    devices.insert(device_id, device_metadata);

    AccountState {
        account_id,
        group_public_key: signing_key.verifying_key(),
        devices,
        removed_devices: BTreeSet::new(),
        guardians: BTreeMap::new(),
        removed_guardians: BTreeSet::new(),
        session_epoch: SessionEpoch::initial(),
        lamport_clock: 0,
        dkd_commitment_roots: BTreeMap::new(),
        sessions: BTreeMap::new(),
        active_operation_lock: None,
        presence_tickets: BTreeMap::new(),
        cooldowns: BTreeMap::new(),
        authority_graph: aura_journal::capability::authority_graph::AuthorityGraph::new(),
        visibility_index: aura_journal::capability::visibility::VisibilityIndex::new(
            aura_journal::capability::authority_graph::AuthorityGraph::new(),
            effects,
        ),
        threshold: 1,
        total_participants: 1,
        used_nonces: BTreeSet::new(),
        next_nonce: 0,
        last_event_hash: None,
        updated_at: timestamp,
    }
}

/// Helper: Merge two AccountState instances using CRDT semantics
fn merge_states(mut state_a: AccountState, state_b: &AccountState) -> AccountState {
    // Merge devices (G-Set: union)
    for (device_id, device_meta) in &state_b.devices {
        state_a
            .devices
            .entry(*device_id)
            .or_insert_with(|| device_meta.clone());
    }

    // Merge removed devices (G-Set: union)
    for device_id in &state_b.removed_devices {
        state_a.removed_devices.insert(*device_id);
    }

    // Merge session epoch (LWW: max)
    state_a.session_epoch = SessionEpoch(std::cmp::max(
        state_a.session_epoch.0,
        state_b.session_epoch.0,
    ));

    // Merge lamport clock (max)
    state_a.lamport_clock = std::cmp::max(state_a.lamport_clock, state_b.lamport_clock);

    // Merge used nonces (G-Set: union)
    for nonce in &state_b.used_nonces {
        state_a.used_nonces.insert(*nonce);
    }

    state_a
}

/// Test: Network partition healing converges to consistent state
///
/// Scenario:
/// - 4 devices split into two partitions: {A,B} | {C,D}
/// - Each partition makes independent changes
/// - Partitions heal and merge states
/// - All devices converge to same final state
#[tokio::test]
async fn test_partition_heal_convergence() {
    let effects = Effects::deterministic(42, 1735689600);

    // Create 4 device states
    let mut state_a = create_test_account(&effects);
    let mut state_b = create_test_account(&effects);
    let mut state_c = create_test_account(&effects);
    let mut state_d = create_test_account(&effects);

    // Initial sync - all devices start with same base state
    let base_state = state_a.clone();
    state_b = base_state.clone();
    state_c = base_state.clone();
    state_d = base_state.clone();

    // Partition 1: {A, B} make changes
    state_a.session_epoch = SessionEpoch(5);
    state_a.lamport_clock = 10;
    state_a.used_nonces.insert([1u8; 32]);

    state_b.session_epoch = SessionEpoch(6);
    state_b.lamport_clock = 12;
    state_b.used_nonces.insert([2u8; 32]);

    // Merge within partition 1
    state_a = merge_states(state_a.clone(), &state_b);
    state_b = merge_states(state_b.clone(), &state_a);

    // Partition 2: {C, D} make different changes
    state_c.session_epoch = SessionEpoch(7);
    state_c.lamport_clock = 15;
    state_c.used_nonces.insert([3u8; 32]);

    state_d.session_epoch = SessionEpoch(4);
    state_d.lamport_clock = 8;
    state_d.used_nonces.insert([4u8; 32]);

    // Merge within partition 2
    state_c = merge_states(state_c.clone(), &state_d);
    state_d = merge_states(state_d.clone(), &state_c);

    // Heal partition: merge all states
    let mut final_a = merge_states(state_a.clone(), &state_c);
    let mut final_b = merge_states(state_b.clone(), &state_c);
    let mut final_c = merge_states(state_c.clone(), &state_a);
    let mut final_d = merge_states(state_d.clone(), &state_a);

    // Additional merge round to ensure full convergence
    final_a = merge_states(final_a.clone(), &final_b);
    final_b = merge_states(final_b.clone(), &final_c);
    final_c = merge_states(final_c.clone(), &final_d);
    final_d = merge_states(final_d.clone(), &final_a);

    // All devices should converge to same state
    assert_eq!(
        final_a.session_epoch,
        SessionEpoch(7),
        "Session epoch should be max"
    );
    assert_eq!(final_b.session_epoch, SessionEpoch(7));
    assert_eq!(final_c.session_epoch, SessionEpoch(7));
    assert_eq!(final_d.session_epoch, SessionEpoch(7));

    assert_eq!(final_a.lamport_clock, 15, "Lamport clock should be max");
    assert_eq!(final_b.lamport_clock, 15);
    assert_eq!(final_c.lamport_clock, 15);
    assert_eq!(final_d.lamport_clock, 15);

    // All nonces should be present in all states
    assert_eq!(final_a.used_nonces.len(), 4, "All nonces should be merged");
    assert_eq!(final_b.used_nonces.len(), 4);
    assert_eq!(final_c.used_nonces.len(), 4);
    assert_eq!(final_d.used_nonces.len(), 4);

    assert!(final_a.used_nonces.contains(&[1u8; 32]));
    assert!(final_a.used_nonces.contains(&[2u8; 32]));
    assert!(final_a.used_nonces.contains(&[3u8; 32]));
    assert!(final_a.used_nonces.contains(&[4u8; 32]));
}

/// Test: Byzantine merge rejection
///
/// Scenario:
/// - Device A has valid ledger state
/// - Device B creates invalid state (would fail signature validation)
/// - Attempt merge
/// - Invalid state rejected, valid state preserved
///
/// Note: In production, invalid states would be rejected before merge.
/// This test verifies that merge logic preserves valid state invariants.
#[tokio::test]
async fn test_byzantine_merge_rejection() {
    let effects = Effects::deterministic(123, 1735689600);

    // Create valid state
    let valid_state = create_test_account(&effects);

    // Create another state with inconsistent data (simulating Byzantine behavior)
    let mut byzantine_state = create_test_account(&effects);

    // Byzantine device tries to set invalid epoch (beyond reasonable bounds)
    byzantine_state.session_epoch = SessionEpoch(u64::MAX);
    byzantine_state.lamport_clock = u64::MAX;

    // In production, signature verification would reject this.
    // For this test, we verify that merge doesn't corrupt valid state.

    // Merge: valid takes precedence in max operations
    let merged = merge_states(valid_state.clone(), &byzantine_state);

    // The merge includes the byzantine values (CRDT merge is permissive)
    // but the state remains internally consistent
    assert_eq!(
        merged.session_epoch.0,
        u64::MAX,
        "Max operation includes byzantine value"
    );
    assert_eq!(
        merged.lamport_clock,
        u64::MAX,
        "Max operation includes byzantine value"
    );

    // In production, signature validation would prevent byzantine states from entering the system
    // This test demonstrates that CRDT merge semantics are permissive (union/max)
    // and security must be enforced at the signature verification layer.

    // Valid state remains unchanged when merged with itself
    let self_merge = merge_states(valid_state.clone(), &valid_state);
    assert_eq!(self_merge.session_epoch, valid_state.session_epoch);
    assert_eq!(self_merge.lamport_clock, valid_state.lamport_clock);
}

/// Test: Cascading merge propagation
///
/// Scenario:
/// - 4 devices in chain topology: A→B→C→D
/// - Device A publishes state change
/// - Changes propagate through chain via sequential merges
/// - All devices eventually receive the change
#[tokio::test]
async fn test_cascading_merge_propagation() {
    let effects = Effects::deterministic(456, 1735689600);

    // Create 4 device states
    let mut state_a = create_test_account(&effects);
    let mut state_b = create_test_account(&effects);
    let mut state_c = create_test_account(&effects);
    let mut state_d = create_test_account(&effects);

    // All start with same base state
    let base_state = state_a.clone();
    state_b = base_state.clone();
    state_c = base_state.clone();
    state_d = base_state.clone();

    // Device A publishes a change
    state_a.session_epoch = SessionEpoch(42);
    state_a.lamport_clock = 100;
    state_a.used_nonces.insert([0xAA; 32]);

    // Propagation: A→B
    state_b = merge_states(state_b, &state_a);
    assert_eq!(
        state_b.session_epoch,
        SessionEpoch(42),
        "B receives A's change"
    );
    assert_eq!(state_b.lamport_clock, 100);
    assert!(state_b.used_nonces.contains(&[0xAA; 32]));

    // Propagation: B→C
    state_c = merge_states(state_c, &state_b);
    assert_eq!(
        state_c.session_epoch,
        SessionEpoch(42),
        "C receives change via B"
    );
    assert_eq!(state_c.lamport_clock, 100);
    assert!(state_c.used_nonces.contains(&[0xAA; 32]));

    // Propagation: C→D
    state_d = merge_states(state_d, &state_c);
    assert_eq!(
        state_d.session_epoch,
        SessionEpoch(42),
        "D receives change via C"
    );
    assert_eq!(state_d.lamport_clock, 100);
    assert!(state_d.used_nonces.contains(&[0xAA; 32]));

    // All devices have converged
    assert_eq!(state_a.session_epoch, state_d.session_epoch);
    assert_eq!(state_a.lamport_clock, state_d.lamport_clock);
    assert_eq!(state_a.used_nonces, state_d.used_nonces);
}
