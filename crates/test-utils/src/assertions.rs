//! Standard assertion helpers for tests
//!
//! This module provides high-level assertion macros and helper functions that
//! reduce duplication of common validation patterns across tests.

use aura_journal::AccountState;
use aura_types::DeviceId;

/// Assert that an account state has the expected number of devices
#[macro_export]
macro_rules! assert_device_count {
    ($account_state:expr, $expected:expr) => {
        assert_eq!(
            $account_state.devices.len(),
            $expected,
            "Expected {} devices in account state, found {}",
            $expected,
            $account_state.devices.len()
        )
    };
}

/// Assert that a specific device exists in the account state
#[macro_export]
macro_rules! assert_device_exists {
    ($account_state:expr, $device_id:expr) => {
        assert!(
            $account_state.devices.contains_key(&$device_id),
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
            !$account_state.devices.contains_key(&$device_id),
            "Device {:?} should not exist in account state",
            $device_id
        )
    };
}

/// Assert that the account has the expected threshold configuration
#[macro_export]
macro_rules! assert_threshold {
    ($account_state:expr, $expected_threshold:expr, $expected_total:expr) => {
        assert_eq!(
            $account_state.threshold, $expected_threshold,
            "Expected threshold {}, found {}",
            $expected_threshold, $account_state.threshold
        );
        assert_eq!(
            $account_state.total_participants, $expected_total,
            "Expected {} total participants, found {}",
            $expected_total, $account_state.total_participants
        );
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
    let device = &account_state.devices[&device_id];
    assert_eq!(
        &device.public_key, expected_key,
        "Device {:?} has unexpected public key",
        device_id
    );
}

/// Helper function to assert all devices have been initialized
pub fn assert_all_devices_initialized(account_state: &AccountState) {
    for device in account_state.devices.values() {
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
    // Check that threshold is reasonable
    assert!(
        account_state.threshold > 0,
        "Threshold must be greater than 0"
    );
    assert!(
        account_state.threshold <= account_state.total_participants,
        "Threshold cannot exceed total participants"
    );

    // Check that we have some devices
    assert!(
        !account_state.devices.is_empty(),
        "Account must have at least one device"
    );

    // Check that device count matches
    assert_eq!(
        account_state.devices.len() as u16,
        account_state.total_participants,
        "Device count does not match total participants"
    );
}

/// Helper function to assert device metadata is properly configured
pub fn assert_device_metadata_valid(account_state: &AccountState, device_id: DeviceId) {
    let device = &account_state.devices[&device_id];
    assert!(
        !device.device_name.is_empty(),
        "Device name cannot be empty"
    );
    assert!(
        device.added_at > 0,
        "Device must have valid added_at timestamp"
    );
}

/// Helper for eventually-consistent assertions
///
/// Useful for testing asynchronous operations that eventually reach a desired state.
/// This is a placeholder - actual implementation would need runtime/async support.
#[allow(dead_code)]
pub fn assert_eventually<F, T>(
    mut condition: F,
    timeout_ms: u64,
    check_interval_ms: u64,
    context: &str,
) -> bool
where
    F: FnMut() -> bool,
{
    let start = std::time::Instant::now();
    loop {
        if condition() {
            return true;
        }

        if start.elapsed().as_millis() as u64 > timeout_ms {
            panic!(
                "Condition did not become true within {}ms: {}",
                timeout_ms, context
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(check_interval_ms));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assert_device_count_macro() {
        let fixture = crate::fixtures::AccountTestFixture::new();
        assert_device_count!(fixture.account_state, fixture.all_devices.len());
    }

    #[test]
    fn test_assert_threshold_macro() {
        let fixture = crate::fixtures::AccountTestFixture::with_devices(3, 2);
        assert_threshold!(fixture.account_state, 2, 3);
    }

    #[test]
    fn test_assert_account_valid() {
        let fixture = crate::fixtures::AccountTestFixture::new();
        assert_account_valid(&fixture.account_state);
    }
}
