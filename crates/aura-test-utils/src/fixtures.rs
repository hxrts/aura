//! High-level test fixtures for common testing scenarios
//!
//! This module provides comprehensive fixtures that combine the lower-level
//! test utilities into reusable, semantic test setup objects. These fixtures
//! eliminate boilerplate test setup across the codebase.

use crate::{test_account_with_threshold, test_effects_deterministic};
use aura_crypto::Effects;
use aura_journal::AccountState;
use aura_types::{AccountId, DeviceId};

/// High-level fixture for protocol testing scenarios
///
/// Provides a complete, configured testing environment for protocol implementation
/// testing, including effects, accounts, devices, and participant setup.
#[derive(Clone)]
pub struct ProtocolTestFixture {
    /// Deterministic effects for reproducible test execution
    pub effects: Effects,
    /// Test account with pre-configured devices
    pub account_state: AccountState,
    /// Primary device ID for the test
    pub device_id: DeviceId,
    /// All device IDs in the test scenario
    pub all_device_ids: Vec<DeviceId>,
}

impl ProtocolTestFixture {
    /// Create a new protocol test fixture with default configuration
    ///
    /// Creates a 3-of-3 account with three devices and deterministic effects.
    pub fn new() -> Self {
        Self::with_config(3, 3, 42)
    }

    /// Create a protocol test fixture with specific configuration
    ///
    /// # Arguments
    /// * `threshold` - Minimum number of devices required for operations (M in M-of-N)
    /// * `total_devices` - Total number of devices (N in M-of-N)
    /// * `seed` - Random seed for deterministic effects
    pub fn with_config(threshold: u16, total_devices: u16, seed: u64) -> Self {
        let effects = test_effects_deterministic(seed, 1000);
        let account_state = test_account_with_threshold(&effects, threshold, total_devices);
        let device_id = account_state
            .devices
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(DeviceId::new);
        let all_device_ids: Vec<_> = account_state.devices.keys().cloned().collect();

        Self {
            effects,
            account_state,
            device_id,
            all_device_ids,
        }
    }

    /// Get the effects for this fixture
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    /// Get the account state for this fixture
    pub fn account_state(&self) -> &AccountState {
        &self.account_state
    }

    /// Get a mutable reference to the account state
    pub fn account_state_mut(&mut self) -> &mut AccountState {
        &mut self.account_state
    }

    /// Get the primary device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get all device IDs
    pub fn all_devices(&self) -> &[DeviceId] {
        &self.all_device_ids
    }

    /// Advance simulated time by N seconds
    pub fn advance_time(&self, seconds: u64) -> aura_crypto::Result<()> {
        self.effects.advance_time(seconds)
    }

    /// Get current simulated timestamp
    pub fn current_time(&self) -> aura_crypto::Result<u64> {
        self.effects.now()
    }
}

impl Default for ProtocolTestFixture {
    fn default() -> Self {
        Self::new()
    }
}

/// High-level fixture for cryptographic operation testing
///
/// Provides a complete testing environment for crypto primitives including
/// FROST signatures, key derivation, encryption, and threshold operations.
#[derive(Clone)]
pub struct CryptoTestFixture {
    /// Deterministic effects for reproducible crypto operations
    pub effects: Effects,
    /// Test signing key for asymmetric operations
    pub signing_key: ed25519_dalek::SigningKey,
    /// Corresponding verification key
    pub verify_key: ed25519_dalek::VerifyingKey,
}

impl CryptoTestFixture {
    /// Create a new crypto test fixture
    pub fn new() -> Self {
        Self::with_seed(42)
    }

    /// Create a crypto test fixture with specific random seed
    pub fn with_seed(seed: u64) -> Self {
        let effects = test_effects_deterministic(seed, 1000);
        let (signing_key, verify_key) = crate::test_key_pair(&effects);

        Self {
            effects,
            signing_key,
            verify_key,
        }
    }

    /// Get the effects for this fixture
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    /// Get the signing key
    pub fn signing_key(&self) -> &ed25519_dalek::SigningKey {
        &self.signing_key
    }

    /// Get the verification key
    pub fn verify_key(&self) -> &ed25519_dalek::VerifyingKey {
        &self.verify_key
    }
}

impl Default for CryptoTestFixture {
    fn default() -> Self {
        Self::new()
    }
}

/// High-level fixture for account lifecycle testing
///
/// Provides a complete testing environment for account creation, initialization,
/// recovery, and state transitions.
#[derive(Clone)]
pub struct AccountTestFixture {
    /// Deterministic effects for reproducible account operations
    pub effects: Effects,
    /// The account being tested
    pub account_id: AccountId,
    /// Account state with initial configuration
    pub account_state: AccountState,
    /// Primary device for account operations
    pub primary_device: DeviceId,
    /// All configured devices in the account
    pub all_devices: Vec<DeviceId>,
}

impl AccountTestFixture {
    /// Create a new account test fixture with default configuration
    pub fn new() -> Self {
        Self::with_devices(3, 2)
    }

    /// Create an account test fixture with specific device configuration
    ///
    /// # Arguments
    /// * `total_devices` - Total number of devices (N in M-of-N)
    /// * `threshold` - Minimum devices required (M in M-of-N)
    pub fn with_devices(total_devices: u16, threshold: u16) -> Self {
        Self::with_seed(42, total_devices, threshold)
    }

    /// Create an account test fixture with specific configuration
    pub fn with_seed(seed: u64, total_devices: u16, threshold: u16) -> Self {
        let effects = test_effects_deterministic(seed, 1000);
        let account_state = test_account_with_threshold(&effects, threshold, total_devices);
        let account_id = account_state.account_id;
        let primary_device = account_state
            .devices
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(DeviceId::new);
        let all_devices: Vec<_> = account_state.devices.keys().cloned().collect();

        Self {
            effects,
            account_id,
            account_state,
            primary_device,
            all_devices,
        }
    }

    /// Get the effects for this fixture
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    /// Get the account ID
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Get the account state
    pub fn account_state(&self) -> &AccountState {
        &self.account_state
    }

    /// Get a mutable reference to account state
    pub fn account_state_mut(&mut self) -> &mut AccountState {
        &mut self.account_state
    }

    /// Get the primary device
    pub fn primary_device(&self) -> DeviceId {
        self.primary_device
    }

    /// Get all configured devices
    pub fn all_devices(&self) -> &[DeviceId] {
        &self.all_devices
    }
}

impl Default for AccountTestFixture {
    fn default() -> Self {
        Self::new()
    }
}
