//! Testkit Bridge Module
//!
//! This module provides the integration layer between aura-testkit and aura-simulator,
//! enabling clean interoperability between foundation testing utilities and advanced
//! simulation orchestration.
//!
//! Designed for the stateless effect system architecture (work/021.md).
//!
//! ## Decoupling via Factory Abstraction
//!
//! Uses `SimulationEnvironmentFactory` trait from aura-core to create effect systems,
//! decoupling the simulator from `AuraEffectSystem` internals.

use crate::types::{Result as SimResult, SimulatorConfig, SimulatorContext, SimulatorError};
use aura_agent::{AuraEffectSystem, EffectSystemFactory};
use aura_core::effects::{SimulationEnvironmentConfig, SimulationEnvironmentFactory};
use aura_core::hash::hash;
use aura_core::identifiers::AuthorityId;
use aura_core::DeviceId;
use aura_testkit::{DeviceTestFixture, ProtocolTestFixture, TestExecutionMode};
use std::collections::HashMap;
use std::sync::Arc;

/// Bridge for integrating testkit with simulator using stateless effects
pub struct TestkitSimulatorBridge;

impl TestkitSimulatorBridge {
    /// Create effect systems for simulation from testkit fixtures
    ///
    /// This method creates multiple effect systems configured for simulation,
    /// using the device fixtures as the foundation for multi-device scenarios.
    ///
    /// Uses `SimulationEnvironmentFactory` trait for decoupled effect system creation.
    pub async fn create_simulation_effects(
        fixtures: &[DeviceTestFixture],
        seed: u64,
    ) -> SimResult<Vec<(DeviceId, Arc<AuraEffectSystem>)>> {
        let factory = EffectSystemFactory::new();
        let mut effect_systems = Vec::new();

        for fixture in fixtures {
            let device_id = fixture.device_id();

            // Create simulation config with authority derived from device ID
            let authority_id = AuthorityId::new_from_entropy(hash(&device_id.to_bytes().expect(
                "device ids from fixtures should always convert to bytes deterministically",
            )));
            let sim_config =
                SimulationEnvironmentConfig::new(seed, device_id).with_authority(authority_id);

            // Use factory to create effect system
            let effect_system = factory
                .create_simulation_environment(sim_config)
                .await
                .map_err(|e| {
                    SimulatorError::OperationFailed(format!(
                        "Effect system creation failed for device {}: {}",
                        device_id, e
                    ))
                })?;

            effect_systems.push((device_id, effect_system));
        }

        Ok(effect_systems)
    }

    /// Create device fixtures from simulation parameters
    ///
    /// This creates testkit device fixtures from simulator parameters,
    /// useful for bootstrapping simulations with testkit foundations.
    pub fn create_device_fixtures(device_count: usize, _seed: u64) -> Vec<DeviceTestFixture> {
        (0..device_count)
            .map(|i| {
                // Apply seed-based deterministic configuration
                DeviceTestFixture::new(i)
            })
            .collect()
    }

    /// Bridge execution mode between testkit and simulator
    pub fn bridge_execution_mode(test_mode: TestExecutionMode) -> String {
        match test_mode {
            TestExecutionMode::UnitTest => "Testing".to_string(),
            TestExecutionMode::Integration => "Testing".to_string(),
            TestExecutionMode::Simulation => "Simulation".to_string(),
        }
    }

    /// Convert fixture to simulator context
    pub async fn fixture_to_context(
        fixture: &ProtocolTestFixture,
        scenario_id: String,
    ) -> SimResult<SimulatorContext> {
        Ok(SimulatorContext {
            scenario_id,
            run_id: format!("run_{}", fixture.device_id()),
            participant_count: 1,
            threshold: 1,
            tick: 0,
            timestamp: std::time::Duration::from_secs(0),
            seed: 0,
            working_dir: std::env::temp_dir(),
            env: HashMap::new(),
            config: SimulatorConfig::default(),
            debug_mode: false,
            verbose: false,
            metadata: HashMap::new(),
        })
    }

    /// Convert harness to effect system
    ///
    /// Uses `SimulationEnvironmentFactory` trait for decoupled effect system creation.
    pub async fn harness_to_effects<H>(
        _harness: H,
        device_id: DeviceId,
        seed: u64,
    ) -> SimResult<Arc<AuraEffectSystem>> {
        let factory = EffectSystemFactory::new();

        // Create simulation config with authority derived from seed
        let authority_id = AuthorityId::new_from_entropy(hash(&seed.to_le_bytes()));
        let sim_config =
            SimulationEnvironmentConfig::new(seed, device_id).with_authority(authority_id);

        // Use factory to create effect system
        factory
            .create_simulation_environment(sim_config)
            .await
            .map_err(|e| {
                SimulatorError::OperationFailed(format!("Effect system creation failed: {}", e))
            })
    }
}

/// Configuration for handler created from testkit fixtures (legacy name for compatibility)
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// Device ID this handler serves
    pub device_id: DeviceId,
    /// Execution mode for the handler
    pub execution_mode: String,
    /// Whether to use deterministic time
    pub deterministic_time: bool,
    /// Whether fault injection is enabled
    pub fault_injection_enabled: bool,
    /// Whether property checking is enabled
    pub property_checking_enabled: bool,
    /// Whether performance monitoring is enabled
    pub performance_monitoring: bool,
}

impl MiddlewareConfig {
    /// Create config for unit testing
    pub fn for_unit_tests(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: "Testing".to_string(),
            deterministic_time: true,
            fault_injection_enabled: false,
            property_checking_enabled: false,
            performance_monitoring: false,
        }
    }

    /// Create config for integration testing
    pub fn for_integration_tests(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: "Testing".to_string(),
            deterministic_time: true,
            fault_injection_enabled: false,
            property_checking_enabled: true,
            performance_monitoring: true,
        }
    }

    /// Create config for simulation
    pub fn for_simulation(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: "Simulation".to_string(),
            deterministic_time: true,
            fault_injection_enabled: true,
            property_checking_enabled: true,
            performance_monitoring: true,
        }
    }
}

/// Utility functions for common testkit-simulator integration patterns
pub mod patterns {
    use super::*;

    /// Create a simple testing setup from testkit devices
    pub fn setup_simple_testing(
        device_count: usize,
        seed: u64,
    ) -> SimResult<Vec<DeviceTestFixture>> {
        let fixtures = TestkitSimulatorBridge::create_device_fixtures(device_count, seed);
        Ok(fixtures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_device_creation() {
        let fixtures = TestkitSimulatorBridge::create_device_fixtures(3, 42);
        assert_eq!(fixtures.len(), 3);

        // Ensure device IDs are unique
        let device_ids: std::collections::HashSet<_> =
            fixtures.iter().map(|f| f.device_id()).collect();
        assert_eq!(device_ids.len(), 3);
    }

    #[test]
    fn test_execution_mode_bridging() {
        assert_eq!(
            TestkitSimulatorBridge::bridge_execution_mode(TestExecutionMode::UnitTest),
            "Testing"
        );
        assert_eq!(
            TestkitSimulatorBridge::bridge_execution_mode(TestExecutionMode::Simulation),
            "Simulation"
        );
    }

    #[test]
    fn test_handler_config_creation() {
        let fixture = aura_testkit::DeviceTestFixture::new(0);
        let device_id = fixture.device_id();
        let config = MiddlewareConfig::for_simulation(device_id);

        assert_eq!(config.device_id, device_id);
        assert_eq!(config.execution_mode, "Simulation");
        assert!(config.fault_injection_enabled);
        assert!(config.property_checking_enabled);
    }

    #[test]
    fn test_simple_testing_pattern() {
        let result = patterns::setup_simple_testing(5, 42);
        assert!(result.is_ok());

        let fixtures = result.unwrap();
        assert_eq!(fixtures.len(), 5);
    }
}
