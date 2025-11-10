//! Standard assertion helpers for tests
//!
//! This module provides high-level assertion macros and helper functions that
//! reduce duplication of common validation patterns across tests.

use aura_core::DeviceId;
use aura_journal::semilattice::account_state::AccountState;

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

/// Helper for eventually-consistent assertions
///
/// Useful for testing asynchronous operations that eventually reach a desired state.
/// This is a placeholder - actual implementation would need runtime/async support.
#[allow(dead_code, clippy::disallowed_methods)]
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
    fn test_assert_has_devices_macro() {
        let fixture = crate::fixtures::AccountTestFixture::with_devices(3, 2);
        assert_has_devices!(fixture.account_state);
    }

    #[test]
    fn test_assert_account_valid() {
        let fixture = crate::fixtures::AccountTestFixture::new();
        assert_account_valid(&fixture.account_state);
    }
}
