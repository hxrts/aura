//! High-level test fixtures for common testing scenarios
//!
//! This module provides comprehensive fixtures that combine the lower-level
//! test utilities into reusable, semantic test setup objects. These fixtures
//! eliminate boilerplate test setup across the codebase.
//!
//! Enhanced for stateless effect system architecture (work/021.md).

use crate::{
    builders::device::DeviceSetBuilder, test_account_with_threshold, TestEffectsBuilder,
    TestExecutionMode,
};
use aura_core::{AccountId, DeviceId};
use aura_journal::journal_api::Journal;

/// High-level fixture for protocol testing scenarios
///
/// Provides a complete, configured testing environment for protocol implementation
/// testing, including accounts, devices, and participant setup.
#[derive(Clone)]
pub struct ProtocolTestFixture {
    /// Test account with pre-configured devices
    pub account_state: Journal,
    /// Primary device ID for the test
    pub device_id: DeviceId,
    /// All device IDs in the test scenario
    pub all_device_ids: Vec<DeviceId>,
    /// Random seed used for deterministic generation
    pub seed: u64,
}

impl ProtocolTestFixture {
    /// Create a new protocol test fixture with default configuration
    ///
    /// Creates a 3-of-3 account with three devices and deterministic effects.
    pub async fn new() -> Self {
        Self::with_config(3, 3, 42).await
    }

    /// Create a protocol test fixture with specific configuration
    ///
    /// # Arguments
    /// * `threshold` - Minimum number of devices required for operations (M in M-of-N)
    /// * `total_devices` - Total number of devices (N in M-of-N)
    /// * `seed` - Random seed for deterministic generation
    pub async fn with_config(threshold: u16, total_devices: u16, seed: u64) -> Self {
        let account_state = test_account_with_threshold(seed, threshold, total_devices).await;

        let fixtures = DeviceSetBuilder::new(total_devices as usize)
            .with_seed(seed)
            .build();
        let device_id = fixtures
            .first()
            .map(|f| f.device_id())
            .unwrap_or_default();
        let all_device_ids: Vec<_> = fixtures.iter().map(|f| f.device_id()).collect();

        Self {
            account_state,
            device_id,
            all_device_ids,
            seed,
        }
    }

    /// Get the random seed for this fixture
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Get the account state for this fixture
    pub fn account_state(&self) -> &Journal {
        &self.account_state
    }

    /// Get a mutable reference to the account state
    pub fn account_state_mut(&mut self) -> &mut Journal {
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

    /// Get deterministic key pair for this fixture
    pub fn key_pair(&self) -> (ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey) {
        crate::test_key_pair(self.seed)
    }

    /// Create protocol test fixture using stateless effects (new API)
    ///
    /// This uses the new stateless effect system architecture and is the
    /// recommended approach for new tests.
    pub async fn with_stateless_effects(
        threshold: u16,
        total_devices: u16,
        execution_mode: TestExecutionMode,
        seed: u64,
    ) -> Result<Self, StatelessFixtureError> {
        // For now, use the existing API until the stateless system is complete
        // This will be replaced with the actual stateless implementation
        let fixture = Self::with_config(threshold, total_devices, seed).await;

        // Use stateless effect system for proper effect injection
        let primary_device = fixture.device_id;
        let effects_builder = match execution_mode {
            TestExecutionMode::UnitTest => TestEffectsBuilder::for_unit_tests(primary_device),
            TestExecutionMode::Integration => {
                TestEffectsBuilder::for_integration_tests(primary_device)
            }
            TestExecutionMode::Simulation => TestEffectsBuilder::for_simulation(primary_device),
        };
        let _stateless_effects = effects_builder
            .with_seed(seed)
            .build()
            .map_err(|e| StatelessFixtureError::EffectSystemError(e.to_string()))?;

        Ok(fixture)
    }

    /// Create fixture from effects builder (new API)
    ///
    /// This method will be the primary way to create fixtures once the
    /// stateless effect system is fully implemented.
    pub async fn from_effects_builder(
        effects_builder: TestEffectsBuilder,
        threshold: u16,
        total_devices: u16,
    ) -> Result<Self, StatelessFixtureError> {
        let device_id = effects_builder.device_id();
        let seed = effects_builder.seed();

        // Build stateless effect system for account creation
        let stateless_effects = effects_builder
            .build()
            .map_err(|e| StatelessFixtureError::EffectSystemError(e.to_string()))?;

        // Integrate account creation with stateless effects
        let account_state = create_test_account_with_stateless_effects(
            &stateless_effects,
            seed,
            threshold,
            total_devices,
        )
        .await
        .map_err(|e| StatelessFixtureError::AccountCreationError(e.to_string()))?;

        let fixtures = DeviceSetBuilder::new(total_devices as usize)
            .with_seed(seed)
            .build();
        let all_device_ids: Vec<_> = fixtures.iter().map(|f| f.device_id()).collect();

        Ok(Self {
            account_state,
            device_id,
            all_device_ids,
            seed,
        })
    }

    /// Create fixture for unit testing (convenience method)
    pub async fn for_unit_tests(device_id: DeviceId) -> Result<Self, StatelessFixtureError> {
        let effects_builder = TestEffectsBuilder::for_unit_tests(device_id);
        Self::from_effects_builder(effects_builder, 2, 3).await
    }

    /// Create fixture for integration testing (convenience method)
    pub async fn for_integration_tests(device_id: DeviceId) -> Result<Self, StatelessFixtureError> {
        let effects_builder = TestEffectsBuilder::for_integration_tests(device_id);
        Self::from_effects_builder(effects_builder, 3, 5).await
    }

    /// Create fixture for simulation testing (convenience method)
    pub async fn for_simulation(device_id: DeviceId) -> Result<Self, StatelessFixtureError> {
        let effects_builder = TestEffectsBuilder::for_simulation(device_id);
        Self::from_effects_builder(effects_builder, 3, 5).await
    }
}

// Note: Default implementation removed because async constructors cannot implement Default
// Use ProtocolTestFixture::new().await instead

/// High-level fixture for cryptographic operation testing
///
/// Provides a complete testing environment for crypto primitives including
/// FROST signatures, key derivation, encryption, and threshold operations.
#[derive(Clone)]
pub struct CryptoTestFixture {
    /// Test signing key for asymmetric operations
    pub signing_key: ed25519_dalek::SigningKey,
    /// Corresponding verification key
    pub verify_key: ed25519_dalek::VerifyingKey,
    /// Random seed for deterministic generation
    pub seed: u64,
}

impl CryptoTestFixture {
    /// Create a new crypto test fixture
    pub fn new() -> Self {
        Self::with_seed(42)
    }

    /// Create a crypto test fixture with specific random seed
    pub fn with_seed(seed: u64) -> Self {
        let (signing_key, verify_key) = crate::test_key_pair(seed);

        Self {
            signing_key,
            verify_key,
            seed,
        }
    }

    /// Get the random seed for this fixture
    pub fn seed(&self) -> u64 {
        self.seed
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
    /// The account being tested
    pub account_id: AccountId,
    /// Account state with initial configuration
    pub account_state: Journal,
    /// Primary device for account operations
    pub primary_device: DeviceId,
    /// All configured devices in the account
    pub all_devices: Vec<DeviceId>,
    /// Random seed for deterministic generation
    pub seed: u64,
}

impl AccountTestFixture {
    /// Create a new account test fixture with default configuration
    pub async fn new() -> Self {
        Self::with_devices(3, 2).await
    }

    /// Create an account test fixture with specific device configuration
    ///
    /// # Arguments
    /// * `total_devices` - Total number of devices (N in M-of-N)
    /// * `threshold` - Minimum devices required (M in M-of-N)
    pub async fn with_devices(total_devices: u16, threshold: u16) -> Self {
        Self::with_seed(42, total_devices, threshold).await
    }

    /// Create an account test fixture with specific configuration
    pub async fn with_seed(seed: u64, total_devices: u16, threshold: u16) -> Self {
        let account_state = test_account_with_threshold(seed, threshold, total_devices).await;
        let account_id = account_state.account_id();

        let fixtures = DeviceSetBuilder::new(total_devices as usize)
            .with_seed(seed)
            .build();
        let primary_device = fixtures
            .first()
            .map(|f| f.device_id())
            .unwrap_or_default();
        let all_devices: Vec<_> = fixtures.iter().map(|f| f.device_id()).collect();

        Self {
            account_id,
            account_state,
            primary_device,
            all_devices,
            seed,
        }
    }

    /// Get the random seed for this fixture
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Get the account ID
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Get the account state
    pub fn account_state(&self) -> &Journal {
        &self.account_state
    }

    /// Get a mutable reference to account state
    pub fn account_state_mut(&mut self) -> &mut Journal {
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

// Note: Default implementation removed because async constructors cannot implement Default
// Use AccountTestFixture::new().await instead

/// Error type for stateless fixture creation
#[derive(Debug)]
pub enum StatelessFixtureError {
    /// Effect system initialization failed
    EffectSystemError(String),
    /// Account creation failed
    AccountCreationError(String),
    /// Device configuration error
    DeviceConfigError(String),
    /// Invalid parameter
    InvalidParameter {
        /// Parameter name
        param: String,
        /// Reason for invalidity
        reason: String,
    },
}

impl std::fmt::Display for StatelessFixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatelessFixtureError::EffectSystemError(msg) => {
                write!(f, "Effect system initialization failed: {}", msg)
            }
            StatelessFixtureError::AccountCreationError(msg) => {
                write!(f, "Account creation failed: {}", msg)
            }
            StatelessFixtureError::DeviceConfigError(msg) => {
                write!(f, "Device configuration error: {}", msg)
            }
            StatelessFixtureError::InvalidParameter { param, reason } => {
                write!(f, "Invalid parameter '{}': {}", param, reason)
            }
        }
    }
}

impl std::error::Error for StatelessFixtureError {}

/// Enhanced fixture configuration for stateless architecture
#[derive(Debug, Clone)]
pub struct StatelessFixtureConfig {
    /// Execution mode for the effect system
    pub execution_mode: TestExecutionMode,
    /// Random seed for deterministic behavior
    pub seed: u64,
    /// Threshold for M-of-N operations
    pub threshold: u16,
    /// Total number of devices
    pub total_devices: u16,
    /// Primary device ID (if None, will be auto-generated)
    pub primary_device: Option<DeviceId>,
}

impl Default for StatelessFixtureConfig {
    fn default() -> Self {
        Self {
            execution_mode: TestExecutionMode::UnitTest,
            seed: 42,
            threshold: 2,
            total_devices: 3,
            primary_device: None,
        }
    }
}

impl StatelessFixtureConfig {
    /// Create config for unit testing
    pub fn for_unit_tests() -> Self {
        Self {
            execution_mode: TestExecutionMode::UnitTest,
            ..Default::default()
        }
    }

    /// Create config for integration testing
    pub fn for_integration_tests() -> Self {
        Self {
            execution_mode: TestExecutionMode::Integration,
            threshold: 3,
            total_devices: 5,
            ..Default::default()
        }
    }

    /// Create config for simulation
    pub fn for_simulation() -> Self {
        Self {
            execution_mode: TestExecutionMode::Simulation,
            threshold: 3,
            total_devices: 5,
            ..Default::default()
        }
    }
}

/// Enhanced factory methods using stateless architecture
impl CryptoTestFixture {
    /// Create crypto fixture using stateless effects (new API)
    pub async fn with_stateless_effects(
        _execution_mode: TestExecutionMode,
        seed: u64,
    ) -> Result<Self, StatelessFixtureError> {
        // For now, use existing implementation until stateless system is ready
        let fixture = Self::with_seed(seed);
        Ok(fixture)
    }

    /// Create crypto fixture for unit testing (convenience method)
    pub async fn for_unit_tests() -> Result<Self, StatelessFixtureError> {
        Self::with_stateless_effects(TestExecutionMode::UnitTest, 42).await
    }

    /// Create crypto fixture for integration testing (convenience method)
    pub async fn for_integration_tests() -> Result<Self, StatelessFixtureError> {
        Self::with_stateless_effects(TestExecutionMode::Integration, 42).await
    }
}

/// Enhanced factory methods using stateless architecture
impl AccountTestFixture {
    /// Create account fixture using stateless effects (new API)
    pub async fn with_stateless_effects(
        config: StatelessFixtureConfig,
    ) -> Result<Self, StatelessFixtureError> {
        // For now, use existing implementation until stateless system is ready
        let fixture = Self::with_seed(config.seed, config.total_devices, config.threshold).await;
        Ok(fixture)
    }

    /// Create account fixture for unit testing (convenience method)
    pub async fn for_unit_tests() -> Result<Self, StatelessFixtureError> {
        let config = StatelessFixtureConfig::for_unit_tests();
        Self::with_stateless_effects(config).await
    }

    /// Create account fixture for integration testing (convenience method)
    pub async fn for_integration_tests() -> Result<Self, StatelessFixtureError> {
        let config = StatelessFixtureConfig::for_integration_tests();
        Self::with_stateless_effects(config).await
    }
}

/// Create a test account using stateless effect system integration
///
/// This function demonstrates how to create test accounts using the stateless effect
/// architecture, providing proper effect injection for deterministic testing.
async fn create_test_account_with_stateless_effects<E>(
    effects: &E,
    _seed: u64,
    threshold: u16,
    total_devices: u16,
) -> aura_core::AuraResult<Journal>
where
    E: aura_core::effects::CryptoEffects + aura_core::effects::RandomEffects,
{
    // Generate device IDs using the effect system
    let mut device_ids = Vec::new();
    for _ in 0..total_devices {
        let device_uuid = effects.random_uuid().await;
        device_ids.push(DeviceId::from_uuid(device_uuid));
    }

    // Generate cryptographic keys using the effect system
    let mut device_keys = Vec::new();
    for _device_id in &device_ids {
        let (signing_key, verify_key) = effects.ed25519_generate_keypair().await.map_err(|e| {
            aura_core::AuraError::crypto(format!("Failed to generate keypair: {}", e))
        })?;
        device_keys.push((signing_key, verify_key));
    }

    // Create account journal using the first device ID to generate a unique AccountId
    let device_uuid: uuid::Uuid = device_ids[0].into();
    let account_id = AccountId::from_uuid(device_uuid);
    let account_state = Journal::new(account_id, effects).await?;

    // Device attestations are now fact-based; callers can attach the appropriate
    // AttestedOps when constructing higher-fidelity simulations. The stateless
    // fixture returns the initialized journal along with deterministically
    // generated device IDs so tests can layer tree updates explicitly.
    let _ = device_keys;
    let _ = threshold;

    Ok(account_state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stateless_fixture_config() {
        let config = StatelessFixtureConfig::for_unit_tests();
        assert_eq!(config.execution_mode, TestExecutionMode::UnitTest);
        assert_eq!(config.seed, 42);
        assert_eq!(config.threshold, 2);
        assert_eq!(config.total_devices, 3);
    }

    #[tokio::test]
    async fn test_protocol_fixture_creation() {
        let device_id = DeviceId::new();
        let result = ProtocolTestFixture::for_unit_tests(device_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_crypto_fixture_creation() {
        let result = CryptoTestFixture::for_unit_tests().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_account_fixture_creation() {
        let result = AccountTestFixture::for_unit_tests().await;
        assert!(result.is_ok());
    }
}
