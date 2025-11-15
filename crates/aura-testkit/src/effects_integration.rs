//! Enhanced effect system integration for testkit
//!
//! This module provides builders and utilities for creating effect systems
//! that work with the new stateless architecture defined in work/021.md.
//! It supports different execution modes for unit tests, integration tests,
//! and simulation scenarios.

use aura_core::{AuraResult, DeviceId};
use std::path::PathBuf;

// Import the actual stateless effect system types
use aura_protocol::effects::system::{AuraEffectSystem, EffectSystemConfig, StorageConfig};
use aura_protocol::handlers::ExecutionMode;

/// Configuration for mock handlers in test scenarios
#[derive(Debug, Clone)]
pub struct MockHandlerConfig {
    /// Use deterministic time for reproducible tests
    pub deterministic_time: bool,
    /// Use mock network handlers instead of real ones
    pub mock_network: bool,
    /// Use mock storage handlers instead of real ones
    pub mock_storage: bool,
    /// Time acceleration factor for faster test execution
    pub time_acceleration: Option<f64>,
    /// Initial timestamp for deterministic time
    pub initial_timestamp: u64,
    /// Storage directory for file-based tests
    pub storage_dir: Option<PathBuf>,
}

impl Default for MockHandlerConfig {
    fn default() -> Self {
        Self {
            deterministic_time: true,
            mock_network: true,
            mock_storage: true,
            time_acceleration: None,
            initial_timestamp: 1_000_000,
            storage_dir: None,
        }
    }
}

/// Builder for creating test-compatible effect systems using the new stateless architecture
#[derive(Debug)]
pub struct TestEffectsBuilder {
    device_id: DeviceId,
    seed: u64,
    mock_config: MockHandlerConfig,
}

impl TestEffectsBuilder {
    /// Create builder for unit tests with full mocking
    pub fn for_unit_tests(device_id: DeviceId) -> Self {
        Self {
            device_id,
            seed: 42,
            mock_config: MockHandlerConfig {
                deterministic_time: true,
                mock_network: true,
                mock_storage: true,
                time_acceleration: None,
                initial_timestamp: 1_000_000,
                storage_dir: None,
            },
        }
    }

    /// Create builder for integration tests with selective mocking
    pub fn for_integration_tests(device_id: DeviceId) -> Self {
        Self {
            device_id,
            seed: 42,
            mock_config: MockHandlerConfig {
                deterministic_time: true,
                mock_network: false, // Use real network handlers for integration
                mock_storage: false, // Use real storage handlers for integration
                time_acceleration: Some(10.0), // Accelerated time for faster tests
                initial_timestamp: 1_000_000,
                storage_dir: Some(PathBuf::from("/tmp/aura-test")),
            },
        }
    }

    /// Create builder for simulation scenarios
    pub fn for_simulation(device_id: DeviceId) -> Self {
        Self {
            device_id,
            seed: 42,
            mock_config: MockHandlerConfig {
                deterministic_time: true,
                mock_network: false, // Use real network for realistic simulation
                mock_storage: false, // Use real storage for state persistence
                time_acceleration: None, // Real-time simulation
                initial_timestamp: 1_000_000,
                storage_dir: Some(PathBuf::from("/tmp/aura-simulation")),
            },
        }
    }

    /// Set the random seed for deterministic behavior
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Configure time acceleration for faster test execution
    pub fn with_time_acceleration(mut self, factor: f64) -> Self {
        self.mock_config.time_acceleration = Some(factor);
        self
    }

    /// Set initial timestamp for deterministic time
    pub fn with_initial_timestamp(mut self, timestamp: u64) -> Self {
        self.mock_config.initial_timestamp = timestamp;
        self
    }

    /// Set storage directory for file-based tests
    pub fn with_storage_dir(mut self, dir: PathBuf) -> Self {
        self.mock_config.storage_dir = Some(dir);
        self
    }

    /// Enable or disable network mocking
    pub fn with_mock_network(mut self, mock: bool) -> Self {
        self.mock_config.mock_network = mock;
        self
    }

    /// Enable or disable storage mocking
    pub fn with_mock_storage(mut self, mock: bool) -> Self {
        self.mock_config.mock_storage = mock;
        self
    }
}

impl TestEffectsBuilder {
    /// Get the device ID this builder will create effects for
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the seed for deterministic behavior
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Get the mock configuration
    pub fn mock_config(&self) -> &MockHandlerConfig {
        &self.mock_config
    }

    /// Build the stateless effect system
    pub fn build(self) -> AuraResult<AuraEffectSystem> {
        let config = EffectSystemConfig {
            device_id: self.device_id,
            execution_mode: self.determine_execution_mode(),
            default_flow_limit: 10_000,
            initial_epoch: aura_core::session_epochs::Epoch::from(1),
            storage_config: StorageConfig::for_testing(),
        };

        AuraEffectSystem::new(config)
    }

    /// Build the system for a specific execution mode
    pub fn build_for_mode(self, mode: TestExecutionMode) -> AuraResult<AuraEffectSystem> {
        let config = EffectSystemConfig {
            device_id: self.device_id,
            execution_mode: mode.to_execution_mode(self.seed),
            default_flow_limit: 10_000,
            initial_epoch: aura_core::session_epochs::Epoch::from(1),
            storage_config: StorageConfig::for_testing(),
        };

        AuraEffectSystem::new(config)
    }

    /// Determine the appropriate execution mode from configuration
    fn determine_execution_mode(&self) -> ExecutionMode {
        if !self.mock_config.mock_network && !self.mock_config.mock_storage {
            // Real handlers - use simulation mode
            ExecutionMode::Simulation { seed: self.seed }
        } else {
            // Mock handlers - use testing mode
            ExecutionMode::Testing
        }
    }

    // build_placeholder() method removed - use build() for unified effect system
}

/// Execution mode configuration for testkit integration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestExecutionMode {
    /// Pure unit testing with full mocking
    UnitTest,
    /// Integration testing with selective mocking
    Integration,
    /// Simulation testing for complex scenarios
    Simulation,
}

impl TestExecutionMode {
    /// Convert to the ExecutionMode enum from the stateless effect system
    pub fn to_execution_mode(self, seed: u64) -> ExecutionMode {
        match self {
            TestExecutionMode::UnitTest => ExecutionMode::Testing,
            TestExecutionMode::Integration => ExecutionMode::Testing,
            TestExecutionMode::Simulation => ExecutionMode::Simulation { seed },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effects_builder_unit_test_config() {
        let device_id = DeviceId::new();
        let builder = TestEffectsBuilder::for_unit_tests(device_id);

        assert_eq!(builder.device_id(), device_id);
        assert_eq!(builder.seed(), 42);
        assert!(builder.mock_config().deterministic_time);
        assert!(builder.mock_config().mock_network);
        assert!(builder.mock_config().mock_storage);
        assert_eq!(builder.mock_config().time_acceleration, None);
    }

    #[test]
    fn test_effects_builder_integration_config() {
        let device_id = DeviceId::new();
        let builder = TestEffectsBuilder::for_integration_tests(device_id);

        assert_eq!(builder.device_id(), device_id);
        assert!(!builder.mock_config().mock_network); // Real network for integration
        assert!(!builder.mock_config().mock_storage); // Real storage for integration
        assert_eq!(builder.mock_config().time_acceleration, Some(10.0));
    }

    #[test]
    fn test_effects_builder_simulation_config() {
        let device_id = DeviceId::new();
        let builder = TestEffectsBuilder::for_simulation(device_id);

        assert_eq!(builder.device_id(), device_id);
        assert!(!builder.mock_config().mock_network); // Real network for simulation
        assert!(!builder.mock_config().mock_storage); // Real storage for simulation
        assert_eq!(builder.mock_config().time_acceleration, None); // Real-time
    }

    #[test]
    fn test_builder_customization() {
        let device_id = DeviceId::new();
        let builder = TestEffectsBuilder::for_unit_tests(device_id)
            .with_seed(12345)
            .with_time_acceleration(5.0)
            .with_initial_timestamp(2_000_000)
            .with_storage_dir(PathBuf::from("/custom/path"));

        assert_eq!(builder.seed(), 12345);
        assert_eq!(builder.mock_config().time_acceleration, Some(5.0));
        assert_eq!(builder.mock_config().initial_timestamp, 2_000_000);
        assert_eq!(
            builder.mock_config().storage_dir,
            Some(PathBuf::from("/custom/path"))
        );
    }
}
