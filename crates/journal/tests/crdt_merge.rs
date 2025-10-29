
//! Unit Tests: CRDT Merge Correctness
//!
//! Tests that CRDT merge operations work correctly for the account ledger.
//! SSB gossip relies entirely on CRDT merge for envelope propagation.
//!
//! Reference: work/pre_ssb_storage_tests.md - Category 2.1

use aura_journal::{types::*, AccountState};
use aura_test_utils::*;
use aura_types::Epoch;
use std::collections::BTreeMap;

/// Merge two account states (simulates CRDT merge behavior)
///
/// This implements the CRDT merge semantics:
/// - Devices: G-Set (grow-only, union)
/// - Removed devices: G-Set (grow-only, union)
/// - Session epoch: LWW register (last-write-wins using max)
/// - Nonces: G-Set (union of used nonces)
/// - Lamport clock: max
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

    // Merge guardians (G-Set: union)
    for (guardian_id, guardian_meta) in &state_b.guardians {
        state_a
            .guardians
            .entry(*guardian_id)
            .or_insert_with(|| guardian_meta.clone());
    }

    // Merge removed guardians (G-Set: union)
    for guardian_id in &state_b.removed_guardians {
        state_a.removed_guardians.insert(*guardian_id);
    }

    // Merge session epoch (LWW: max)
    state_a.session_epoch = Epoch(std::cmp::max(
        state_a.session_epoch.0,
        state_b.session_epoch.0,
    ));

    // Merge lamport clock (max)
    state_a.lamport_clock = std::cmp::max(state_a.lamport_clock, state_b.lamport_clock);

    // Merge used nonces (G-Set: union)
    for nonce in &state_b.used_nonces {
        state_a.used_nonces.insert(*nonce);
    }

    // Merge next_nonce (max)
    state_a.next_nonce = std::cmp::max(state_a.next_nonce, state_b.next_nonce);

    // Update timestamp to latest
    state_a.updated_at = std::cmp::max(state_a.updated_at, state_b.updated_at);

    state_a
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn test_concurrent_event_merge_commutes() {
        // Device A adds a device to ledger
        let effects_a = test_effects_deterministic(100, 1000);
        let mut state_a = test_account_with_seed(100);

        let device_2_id = DeviceId::new_with_effects(&effects_a);
        let device_2_key = SigningKey::from_bytes(&effects_a.random_bytes::<32>());
        state_a.devices.insert(
            device_2_id,
            DeviceMetadata {
                device_id: device_2_id,
                public_key: device_2_key.verifying_key(),
                added_at: 1001,
                device_name: "Device 2".to_string(),
                device_type: DeviceType::Native,
                last_seen: 1001,
                dkd_commitment_proofs: BTreeMap::new(),
                next_nonce: 0,
                used_nonces: BTreeSet::new(),
            },
        );

        // Device B adds a different device to ledger
        let effects_b = test_effects_deterministic(200, 1000);
        let mut state_b = test_account_with_seed(100); // Same base state

        let device_3_id = DeviceId::new_with_effects(&effects_b);
        let device_3_key = SigningKey::from_bytes(&effects_b.random_bytes::<32>());
        state_b.devices.insert(
            device_3_id,
            DeviceMetadata {
                device_id: device_3_id,
                public_key: device_3_key.verifying_key(),
                added_at: 1002,
                device_name: "Device 3".to_string(),
                device_type: DeviceType::Native,
                last_seen: 1002,
                dkd_commitment_proofs: BTreeMap::new(),
                next_nonce: 0,
                used_nonces: BTreeSet::new(),
            },
        );

        // Merge A→B and B→A
        let merged_a_to_b = merge_states(state_a.clone(), &state_b);
        let merged_b_to_a = merge_states(state_b.clone(), &state_a);

        // Assert: Final state identical regardless of merge order (commutativity)
        assert_eq!(
            merged_a_to_b.devices.len(),
            merged_b_to_a.devices.len(),
            "Merged states should have same number of devices"
        );

        // Check that both devices are present in both merged states
        assert!(
            merged_a_to_b.devices.contains_key(&device_2_id),
            "Merged state should contain device 2"
        );
        assert!(
            merged_a_to_b.devices.contains_key(&device_3_id),
            "Merged state should contain device 3"
        );
        assert!(
            merged_b_to_a.devices.contains_key(&device_2_id),
            "Merged state should contain device 2"
        );
        assert!(
            merged_b_to_a.devices.contains_key(&device_3_id),
            "Merged state should contain device 3"
        );

        println!("[OK] test_concurrent_event_merge_commutes PASSED");
    }

    #[test]
    fn test_device_add_remove_convergence() {
        // Device A adds Device D
        let effects = test_effects_deterministic(300, 1000);
        let mut state_a = test_account_with_seed(300);

        let device_d_id = DeviceId::new_with_effects(&effects);
        let device_d_key = SigningKey::from_bytes(&effects.random_bytes::<32>());
        state_a.devices.insert(
            device_d_id,
            DeviceMetadata {
                device_id: device_d_id,
                public_key: device_d_key.verifying_key(),
                added_at: 1001,
                device_name: "Device D".to_string(),
                device_type: DeviceType::Native,
                last_seen: 1001,
                dkd_commitment_proofs: BTreeMap::new(),
                next_nonce: 0,
                used_nonces: BTreeSet::new(),
            },
        );

        // Device B removes Device D (tombstone)
        let mut state_b = test_account_with_seed(300); // Same base
        state_b.removed_devices.insert(device_d_id);

        // Merge in both directions
        let merged_a_to_b = merge_states(state_a.clone(), &state_b);
        let merged_b_to_a = merge_states(state_b.clone(), &state_a);

        // Assert: Remove wins (device exists in G-Set but also in tombstone set)
        assert!(
            merged_a_to_b.devices.contains_key(&device_d_id),
            "Device should be in devices set (G-Set never shrinks)"
        );
        assert!(
            merged_a_to_b.removed_devices.contains(&device_d_id),
            "Device should be tombstoned (remove wins)"
        );

        assert!(
            merged_b_to_a.devices.contains_key(&device_d_id),
            "Device should be in devices set (G-Set never shrinks)"
        );
        assert!(
            merged_b_to_a.removed_devices.contains(&device_d_id),
            "Device should be tombstoned (remove wins)"
        );

        // Both merges should be identical (deterministic conflict resolution)
        assert_eq!(
            merged_a_to_b.removed_devices.len(),
            merged_b_to_a.removed_devices.len(),
            "Conflict resolution should be deterministic"
        );

        println!("[OK] test_device_add_remove_convergence PASSED");
    }

    #[test]
    fn test_epoch_increment_convergence() {
        // Device A bumps epoch to 5
        let mut state_a = test_account_with_seed(400);
        state_a.session_epoch = Epoch(5);

        // Device B bumps epoch to 6
        let mut state_b = test_account_with_seed(400); // Same base
        state_b.session_epoch = Epoch(6);

        // Merge both directions
        let merged_a_to_b = merge_states(state_a.clone(), &state_b);
        let merged_b_to_a = merge_states(state_b.clone(), &state_a);

        // Assert: Final epoch = max(5,6) = 6 (monotonic counter converges to max)
        assert_eq!(
            merged_a_to_b.session_epoch.0, 6,
            "Epoch should converge to max value"
        );
        assert_eq!(
            merged_b_to_a.session_epoch.0, 6,
            "Epoch should converge to max value"
        );

        // Test with same epochs (idempotence)
        let mut state_c = test_account_with_seed(400);
        state_c.session_epoch = Epoch(5);

        let merged_c_to_a = merge_states(state_c.clone(), &state_a);
        assert_eq!(
            merged_c_to_a.session_epoch.0, 5,
            "Same epoch should remain unchanged"
        );

        println!("[OK] test_epoch_increment_convergence PASSED");
    }

    #[test]
    fn test_nonce_replay_prevention_after_merge() {
        // Device A uses nonce 42
        let mut state_a = test_account_with_seed(500);
        let nonce = 42u64;
        state_a
            .validate_nonce(nonce)
            .expect("First use of nonce should succeed");

        // Verify nonce is marked as used
        assert!(
            state_a.used_nonces.contains(&nonce),
            "Nonce should be marked as used"
        );

        // Device B merges A's ledger
        let state_b = test_account_with_seed(500); // Clean state
        let mut merged_state = merge_states(state_b, &state_a);

        // Assert: Nonce tracking survives CRDT merge
        assert!(
            merged_state.used_nonces.contains(&nonce),
            "Used nonce should be present after merge"
        );

        // Device B attempts to reuse nonce 42
        let result = merged_state.validate_nonce(nonce);

        // Assert: Replay detected and rejected
        assert!(
            result.is_err(),
            "Reusing merged nonce should fail (replay attack detected)"
        );
        assert!(
            result.unwrap_err().contains("already used"),
            "Error should mention nonce was already used"
        );

        // Test that new nonces still work
        let new_nonce = 43u64;
        let result2 = merged_state.validate_nonce(new_nonce);
        assert!(result2.is_ok(), "New nonce should be accepted after merge");

        println!("[OK] test_nonce_replay_prevention_after_merge PASSED");
    }

    #[test]
    fn test_lamport_clock_convergence() {
        // Device A has lamport clock = 10
        let mut state_a = test_account_with_seed(600);
        state_a.lamport_clock = 10;

        // Device B has lamport clock = 15
        let mut state_b = test_account_with_seed(600);
        state_b.lamport_clock = 15;

        // Merge both directions
        let merged_a_to_b = merge_states(state_a.clone(), &state_b);
        let merged_b_to_a = merge_states(state_b.clone(), &state_a);

        // Assert: Lamport clock converges to max
        assert_eq!(
            merged_a_to_b.lamport_clock, 15,
            "Lamport clock should converge to max"
        );
        assert_eq!(
            merged_b_to_a.lamport_clock, 15,
            "Lamport clock should converge to max"
        );

        println!("[OK] test_lamport_clock_convergence PASSED");
    }

    #[test]
    fn test_merge_is_idempotent() {
        // Create a state with some data
        let effects = test_effects_deterministic(700, 1000);
        let mut state = test_account_with_seed(700);

        // Add some devices and nonces
        let device_id = DeviceId::new_with_effects(&effects);
        let device_key = SigningKey::from_bytes(&effects.random_bytes::<32>());
        state.devices.insert(
            device_id,
            DeviceMetadata {
                device_id,
                public_key: device_key.verifying_key(),
                added_at: 1001,
                device_name: "Test Device".to_string(),
                device_type: DeviceType::Native,
                last_seen: 1001,
                dkd_commitment_proofs: BTreeMap::new(),
                next_nonce: 0,
                used_nonces: BTreeSet::new(),
            },
        );
        state.validate_nonce(100).unwrap();
        state.session_epoch = Epoch(5);
        state.lamport_clock = 10;

        // Merge state with itself: state ∪ state
        let merged = merge_states(state.clone(), &state);

        // Assert: Result equals original (no duplicates)
        assert_eq!(
            merged.devices.len(),
            state.devices.len(),
            "Idempotent merge should not duplicate devices"
        );
        assert_eq!(
            merged.used_nonces.len(),
            state.used_nonces.len(),
            "Idempotent merge should not duplicate nonces"
        );
        assert_eq!(
            merged.session_epoch.0, state.session_epoch.0,
            "Idempotent merge should preserve epoch"
        );
        assert_eq!(
            merged.lamport_clock, state.lamport_clock,
            "Idempotent merge should preserve lamport clock"
        );

        println!("[OK] test_merge_is_idempotent PASSED");
    }
}
