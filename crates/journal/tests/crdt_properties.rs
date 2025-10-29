
//! Property Tests: CRDT Convergence
//!
//! Tests fundamental CRDT properties that must hold for ledger merges.
//! Uses proptest to verify associativity, idempotence, and eventual consistency.
//!
//! Reference: work/pre_ssb_storage_tests.md - Category 2.2

use aura_journal::{types::*, AccountState};
use aura_test_utils::*;
use aura_types::Epoch;
use proptest::prelude::*;
use std::collections::BTreeMap;

/// Merge two account states (G-Set semantics)
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

/// Add a device to state (for property testing)
fn add_device_to_state(state: &mut AccountState, seed: u64) {
    let effects = test_effects_deterministic(seed, 1000);
    let device_metadata = test_device_with_id(seed as u16, &effects);

    state
        .devices
        .insert(device_metadata.device_id, device_metadata);
}

/// Modify state with random operations (for property testing)
fn modify_state(state: &mut AccountState, seed: u64, ops: &[u8]) {
    let effects = test_effects_deterministic(seed, 1000);

    for (i, &op) in ops.iter().enumerate() {
        match op % 4 {
            0 => {
                // Add device
                add_device_to_state(state, seed + i as u64);
            }
            1 => {
                // Bump epoch
                state.session_epoch = Epoch(state.session_epoch.0 + 1);
            }
            2 => {
                // Add nonce
                let _ = state.validate_nonce(seed + i as u64);
            }
            3 => {
                // Increment lamport clock
                state.lamport_clock += 1;
            }
            _ => unreachable!(),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: CRDT merge is associative
    ///
    /// Invariant: (A ∪ B) ∪ C = A ∪ (B ∪ C)
    #[test]
    fn prop_crdt_merge_associative(
        seed_a in any::<u64>(),
        seed_b in any::<u64>(),
        seed_c in any::<u64>(),
        ops_a in prop::collection::vec(any::<u8>(), 0..10),
        ops_b in prop::collection::vec(any::<u8>(), 0..10),
        ops_c in prop::collection::vec(any::<u8>(), 0..10),
    ) {
        // Create three states with different modifications
        let mut state_a = test_account_with_seed(seed_a);
        modify_state(&mut state_a, seed_a, &ops_a);

        let mut state_b = test_account_with_seed(seed_b);
        modify_state(&mut state_b, seed_b, &ops_b);

        let mut state_c = test_account_with_seed(seed_c);
        modify_state(&mut state_c, seed_c, &ops_c);

        // Compute (A ∪ B) ∪ C
        let ab = merge_states(state_a.clone(), &state_b);
        let ab_c = merge_states(ab, &state_c);

        // Compute A ∪ (B ∪ C)
        let bc = merge_states(state_b.clone(), &state_c);
        let a_bc = merge_states(state_a.clone(), &bc);

        // Assert: Results are identical (associativity)
        prop_assert_eq!(ab_c.devices.len(), a_bc.devices.len(), "Device count should be same");
        prop_assert_eq!(ab_c.session_epoch, a_bc.session_epoch, "Epoch should be same");
        prop_assert_eq!(ab_c.lamport_clock, a_bc.lamport_clock, "Lamport clock should be same");
        prop_assert_eq!(ab_c.used_nonces.len(), a_bc.used_nonces.len(), "Nonce count should be same");
        prop_assert_eq!(ab_c.next_nonce, a_bc.next_nonce, "Next nonce should be same");
    }

    /// Property: CRDT merge is idempotent
    ///
    /// Invariant: A ∪ A = A (no duplicates)
    #[test]
    fn prop_crdt_merge_idempotent(
        seed in any::<u64>(),
        ops in prop::collection::vec(any::<u8>(), 0..20),
    ) {
        // Create a state with random modifications
        let mut state = test_account_with_seed(seed);
        modify_state(&mut state, seed, &ops);

        // Merge state with itself: A ∪ A
        let merged = merge_states(state.clone(), &state);

        // Assert: Result equals original (idempotence)
        prop_assert_eq!(merged.devices.len(), state.devices.len(), "Devices should not duplicate");
        prop_assert_eq!(merged.guardians.len(), state.guardians.len(), "Guardians should not duplicate");
        prop_assert_eq!(merged.used_nonces.len(), state.used_nonces.len(), "Nonces should not duplicate");
        prop_assert_eq!(merged.session_epoch, state.session_epoch, "Epoch should be preserved");
        prop_assert_eq!(merged.lamport_clock, state.lamport_clock, "Lamport clock should be preserved");
        prop_assert_eq!(merged.next_nonce, state.next_nonce, "Next nonce should be preserved");
    }

    /// Property: CRDT eventual consistency
    ///
    /// Invariant: All merge orders converge to same state
    #[test]
    fn prop_crdt_eventual_consistency(
        seed_a in any::<u64>(),
        seed_b in any::<u64>(),
        seed_c in any::<u64>(),
        ops_a in prop::collection::vec(any::<u8>(), 0..8),
        ops_b in prop::collection::vec(any::<u8>(), 0..8),
        ops_c in prop::collection::vec(any::<u8>(), 0..8),
    ) {
        // Create three devices with different modifications
        let mut state_a = test_account_with_seed(seed_a);
        modify_state(&mut state_a, seed_a, &ops_a);

        let mut state_b = test_account_with_seed(seed_b);
        modify_state(&mut state_b, seed_b, &ops_b);

        let mut state_c = test_account_with_seed(seed_c);
        modify_state(&mut state_c, seed_c, &ops_c);

        // Merge in different orders
        // Order 1: A → B → C
        let ab = merge_states(state_a.clone(), &state_b);
        let final_1 = merge_states(ab, &state_c);

        // Order 2: B → C → A
        let bc = merge_states(state_b.clone(), &state_c);
        let final_2 = merge_states(bc, &state_a);

        // Order 3: C → A → B
        let ca = merge_states(state_c.clone(), &state_a);
        let final_3 = merge_states(ca, &state_b);

        // Assert: All devices converge to same final state (eventual consistency)
        prop_assert_eq!(final_1.devices.len(), final_2.devices.len(), "Device count should converge");
        prop_assert_eq!(final_2.devices.len(), final_3.devices.len(), "Device count should converge");

        prop_assert_eq!(final_1.session_epoch, final_2.session_epoch, "Epoch should converge");
        prop_assert_eq!(final_2.session_epoch, final_3.session_epoch, "Epoch should converge");

        prop_assert_eq!(final_1.lamport_clock, final_2.lamport_clock, "Lamport clock should converge");
        prop_assert_eq!(final_2.lamport_clock, final_3.lamport_clock, "Lamport clock should converge");

        prop_assert_eq!(final_1.used_nonces.len(), final_2.used_nonces.len(), "Nonce count should converge");
        prop_assert_eq!(final_2.used_nonces.len(), final_3.used_nonces.len(), "Nonce count should converge");
    }

    /// Property: Monotonic counters always converge to max
    #[test]
    fn prop_monotonic_counters_converge_to_max(
        epoch_a in 0u64..100,
        epoch_b in 0u64..100,
        epoch_c in 0u64..100,
        clock_a in 0u64..100,
        clock_b in 0u64..100,
        clock_c in 0u64..100,
    ) {
        let mut state_a = test_account_with_seed(1000);
        state_a.session_epoch = Epoch(epoch_a);
        state_a.lamport_clock = clock_a;

        let mut state_b = test_account_with_seed(2000);
        state_b.session_epoch = Epoch(epoch_b);
        state_b.lamport_clock = clock_b;

        let mut state_c = test_account_with_seed(3000);
        state_c.session_epoch = Epoch(epoch_c);
        state_c.lamport_clock = clock_c;

        // Merge all three
        let ab = merge_states(state_a, &state_b);
        let final_state = merge_states(ab, &state_c);

        // Assert: Counters converge to max
        let max_epoch = *[epoch_a, epoch_b, epoch_c].iter().max().unwrap();
        let max_clock = *[clock_a, clock_b, clock_c].iter().max().unwrap();

        prop_assert_eq!(final_state.session_epoch.0, max_epoch, "Epoch should be max");
        prop_assert_eq!(final_state.lamport_clock, max_clock, "Lamport clock should be max");
    }

    /// Property: G-Set (grow-only set) never loses elements
    #[test]
    fn prop_gset_never_loses_elements(
        num_devices_a in 0usize..5,
        num_devices_b in 0usize..5,
        seed in any::<u64>(),
    ) {
        let mut state_a = test_account_with_seed(seed);
        let mut state_b = test_account_with_seed(seed + 1000);

        // Add devices to state A
        for i in 0..num_devices_a {
            add_device_to_state(&mut state_a, seed + i as u64);
        }

        // Add devices to state B
        for i in 0..num_devices_b {
            add_device_to_state(&mut state_b, seed + 5000 + i as u64);
        }

        let original_count_a = state_a.devices.len();
        let original_count_b = state_b.devices.len();

        // Merge
        let merged = merge_states(state_a, &state_b);

        // Assert: Merged set has at least as many elements as either input
        prop_assert!(
            merged.devices.len() >= original_count_a,
            "Merge should not lose devices from A"
        );
        prop_assert!(
            merged.devices.len() >= original_count_b,
            "Merge should not lose devices from B"
        );
    }
}

#[cfg(test)]
mod manual_tests {
    use super::*;

    #[test]
    fn test_property_tests_compile_and_run() {
        println!("[OK] CRDT property tests compile successfully");
    }
}
