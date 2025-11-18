//! Standard assertion helpers for tests
//!
//! This module provides high-level assertion macros and helper functions that
//! reduce duplication of common validation patterns across tests.

use aura_core::DeviceId;
use aura_core::{JoinSemilattice, MeetSemiLattice};
use aura_journal::semilattice::account_state::AccountState;
use std::time::Duration;
use tokio::time::{sleep, Instant};

/// Assert that an account state has the expected number of devices
#[macro_export]
macro_rules! assert_device_count {
    ($account_state:expr, $expected:expr) => {
        assert_eq!(
            $account_state.device_registry.devices.len(),
            $expected,
            "Expected {} devices in account state, found {}",
            $expected,
            $account_state.device_registry.devices.len()
        )
    };
}

/// Assert that a specific device exists in the account state
#[macro_export]
macro_rules! assert_device_exists {
    ($account_state:expr, $device_id:expr) => {
        assert!(
            $account_state
                .device_registry
                .devices
                .contains_key(&$device_id),
            "Device {:?} not found in account state",
            $device_id
        )
    };
}

/// Assert that a device does not exist in the account state
#[macro_export]
macro_rules! assert_device_not_exists {
    ($account_state:expr, $device_id:expr) => {
        assert!(
            !$account_state
                .device_registry
                .devices
                .contains_key(&$device_id),
            "Device {:?} should not exist in account state",
            $device_id
        )
    };
}

/// Assert that the account has active devices
#[macro_export]
macro_rules! assert_has_devices {
    ($account_state:expr) => {
        assert!(
            !$account_state.device_registry.devices.is_empty(),
            "Account must have at least one device"
        )
    };
}

/// Helper function to assert protocol state matches expected value
///
/// Useful for verifying protocol machines have advanced to correct state.
pub fn assert_protocol_state<T: PartialEq + std::fmt::Debug>(
    actual: &T,
    expected: &T,
    context: &str,
) {
    assert_eq!(actual, expected, "Protocol state mismatch ({})", context);
}

/// Helper function to assert a device has a specific public key
pub fn assert_device_key(
    account_state: &AccountState,
    device_id: DeviceId,
    expected_key: &ed25519_dalek::VerifyingKey,
) {
    assert_device_exists!(account_state, device_id);
    let device = account_state
        .device_registry
        .devices
        .get(&device_id)
        .expect("Device not found in registry");
    assert_eq!(
        &device.public_key, expected_key,
        "Device {:?} has unexpected public key",
        device_id
    );
}

/// Helper function to assert all devices have been initialized
pub fn assert_all_devices_initialized(account_state: &AccountState) {
    for device in account_state.device_registry.devices.values() {
        assert!(
            device.public_key.to_bytes().len() == 32,
            "Device not properly initialized with public key"
        );
    }
}

/// Helper function to assert account is in valid state
///
/// Performs basic sanity checks on account state.
pub fn assert_account_valid(account_state: &AccountState) {
    // Check that we have at least one device
    assert!(
        !account_state.device_registry.devices.is_empty(),
        "Account must have at least one device"
    );

    // Check account ID is valid
    assert!(
        !account_state.account_id.to_string().is_empty(),
        "Account must have a valid account ID"
    );
}

/// Helper function to assert device metadata is properly configured
pub fn assert_device_metadata_valid(account_state: &AccountState, device_id: DeviceId) {
    let device = account_state
        .device_registry
        .devices
        .get(&device_id)
        .expect("Device not found in registry");
    assert!(
        !device.device_name.is_empty(),
        "Device name cannot be empty"
    );
    assert!(
        device.added_at > 0,
        "Device must have valid added_at timestamp"
    );
}

/// Helper for asserting cryptographic properties
///
/// Validates that cryptographic operations maintain their invariants.
pub fn assert_crypto_property_signature(signature: &ed25519_dalek::Signature) {
    // Check signature is correct length
    assert_eq!(signature.to_bytes().len(), 64, "Signature must be 64 bytes");
}

/// Helper for asserting key properties
pub fn assert_crypto_property_key(key: &ed25519_dalek::VerifyingKey) {
    // Check key is correct length
    assert_eq!(key.to_bytes().len(), 32, "Public key must be 32 bytes");
}

// ============================================================================
// CRDT and Semilattice Assertions
// ============================================================================

/// Assert two values eventually become equal (for CRDT convergence testing)
///
/// Polls a getter function until it returns the expected value or times out.
/// Useful for testing eventually-consistent distributed systems.
///
/// # Example
///
/// ```rust,no_run
/// use aura_testkit::assertions::assert_eventually_eq;
/// use std::time::Duration;
///
/// # async fn example() {
/// let mut counter = 0;
/// assert_eventually_eq(
///     || { counter += 1; counter },
///     5,
///     Duration::from_secs(1),
///     "counter should reach 5"
/// ).await;
/// # }
/// ```
pub async fn assert_eventually_eq<T, F>(
    mut getter: F,
    expected: T,
    timeout: Duration,
    context: &str,
) where
    T: PartialEq + std::fmt::Debug,
    F: FnMut() -> T,
{
    let start = Instant::now();
    #[allow(unused_assignments)]
    let mut last_value = None;

    loop {
        let actual = getter();
        if actual == expected {
            return;
        }

        last_value = Some(format!("{:?}", actual));

        if start.elapsed() > timeout {
            panic!(
                "Values did not converge within {:?} ({})\nExpected: {:?}\nLast value: {}",
                timeout,
                context,
                expected,
                last_value.unwrap_or_else(|| "none".to_string())
            );
        }

        sleep(Duration::from_millis(10)).await;
    }
}

/// Assert CRDT states have converged (are equal)
///
/// Useful for verifying that two replicas of a CRDT have reached the same state
/// after synchronization.
///
/// # Example
///
/// ```rust,no_run
/// use aura_testkit::assertions::assert_crdt_converged;
///
/// # fn example() {
/// let state1 = vec![1, 2, 3];
/// let state2 = vec![1, 2, 3];
/// assert_crdt_converged(&state1, &state2, "replica synchronization");
/// # }
/// ```
pub fn assert_crdt_converged<T>(state1: &T, state2: &T, context: &str)
where
    T: PartialEq + std::fmt::Debug,
{
    assert_eq!(
        state1, state2,
        "CRDT states did not converge ({})\nState 1: {:?}\nState 2: {:?}",
        context, state1, state2
    );
}

/// Assert join semilattice properties hold for a type
///
/// Verifies that the join operation satisfies:
/// - Commutativity: a ⊔ b = b ⊔ a
/// - Idempotency: a ⊔ a = a
/// - Associativity is assumed (can't test with just 2 values)
///
/// # Example
///
/// ```rust,no_run
/// use aura_testkit::assertions::assert_join_semilattice;
/// use aura_core::JoinSemilattice;
///
/// # fn example<T: JoinSemilattice + PartialEq + std::fmt::Debug + Clone>() {
/// # let a: T = todo!();
/// # let b: T = todo!();
/// assert_join_semilattice(&a, &b, "testing fact accumulation");
/// # }
/// ```
pub fn assert_join_semilattice<T>(a: &T, b: &T, context: &str)
where
    T: JoinSemilattice + PartialEq + std::fmt::Debug + Clone,
{
    // Test commutativity: a ⊔ b = b ⊔ a
    let ab = a.clone().join(&b.clone());
    let ba = b.clone().join(&a.clone());
    assert_eq!(
        ab, ba,
        "Join not commutative ({}):\na ⊔ b = {:?}\nb ⊔ a = {:?}",
        context, ab, ba
    );

    // Test idempotency: a ⊔ a = a
    let aa = a.clone().join(&a.clone());
    assert_eq!(
        aa, *a,
        "Join not idempotent ({}):\na ⊔ a = {:?}\na = {:?}",
        context, aa, a
    );
}

/// Assert meet semilattice properties hold for a type
///
/// Verifies that the meet operation satisfies:
/// - Commutativity: a ⊓ b = b ⊓ a
/// - Idempotency: a ⊓ a = a
///
/// # Example
///
/// ```rust,no_run
/// use aura_testkit::assertions::assert_meet_semilattice;
/// use aura_core::MeetSemiLattice;
///
/// # fn example<T: MeetSemiLattice + PartialEq + std::fmt::Debug + Clone>() {
/// # let a: T = todo!();
/// # let b: T = todo!();
/// assert_meet_semilattice(&a, &b, "testing capability refinement");
/// # }
/// ```
pub fn assert_meet_semilattice<T>(a: &T, b: &T, context: &str)
where
    T: MeetSemiLattice + PartialEq + std::fmt::Debug + Clone,
{
    // Test commutativity: a ⊓ b = b ⊓ a
    let ab = a.clone().meet(&b.clone());
    let ba = b.clone().meet(&a.clone());
    assert_eq!(
        ab, ba,
        "Meet not commutative ({}):\na ⊓ b = {:?}\nb ⊓ a = {:?}",
        context, ab, ba
    );

    // Test idempotency: a ⊓ a = a
    let aa = a.clone().meet(&a.clone());
    assert_eq!(
        aa, *a,
        "Meet not idempotent ({}):\na ⊓ a = {:?}\na = {:?}",
        context, aa, a
    );
}

/// Assert a condition eventually becomes true within timeout
///
/// General-purpose polling assertion for asynchronous operations.
///
/// # Example
///
/// ```rust,no_run
/// use aura_testkit::assertions::assert_eventually;
/// use std::time::Duration;
///
/// # async fn example() {
/// let mut count = 0;
/// assert_eventually(
///     || { count += 1; count >= 5 },
///     Duration::from_secs(1),
///     "count should reach 5"
/// ).await;
/// # }
/// ```
pub async fn assert_eventually<F>(mut condition: F, timeout: Duration, context: &str)
where
    F: FnMut() -> bool,
{
    let start = Instant::now();

    loop {
        if condition() {
            return;
        }

        if start.elapsed() > timeout {
            panic!(
                "Condition did not become true within {:?}: {}",
                timeout, context
            );
        }

        sleep(Duration::from_millis(10)).await;
    }
}

// ============================================================================
// Additional Assertion Macros
// ============================================================================

/// Assert account state epoch matches expected value
#[macro_export]
macro_rules! assert_epoch {
    ($account_state:expr, $expected:expr) => {
        assert_eq!(
            $account_state.epoch, $expected,
            "Expected epoch {}, found {}",
            $expected, $account_state.epoch
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_assert_device_count_macro() {
        let fixture = crate::fixtures::AccountTestFixture::new().await;
        assert_device_count!(fixture.account_state, fixture.all_devices.len());
    }

    #[tokio::test]
    async fn test_assert_has_devices_macro() {
        let fixture = crate::fixtures::AccountTestFixture::with_devices(3, 2).await;
        assert_has_devices!(fixture.account_state);
    }

    #[tokio::test]
    async fn test_assert_account_valid() {
        let fixture = crate::fixtures::AccountTestFixture::new().await;
        assert_account_valid(&fixture.account_state);
    }
}
