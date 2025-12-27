//! Effect system composer for simulation
//!
//! This module provides effect-based composition patterns using the agent runtime.
//! Enables clean composition of simulation effects following the 8-layer architecture.
//!
//! This composer uses the aura-agent runtime (Layer 6) to properly compose simulation
//! environments, unlike the deprecated stateless pattern which incorrectly used Layer 3
//! handlers directly.
//!
//! ## Decoupling via Factory Abstraction
//!
//! The composer uses `SimulationEnvironmentFactory` trait from aura-core to create
//! effect systems. This decouples the simulator from `AuraEffectSystem` internals:
//!
//! - Changes to `AuraEffectSystem` factory methods only require updating `EffectSystemFactory`
//! - The simulator remains stable as long as the trait contract is maintained
//! - Tests can provide mock factories if needed

use super::{
    stateless_simulator::SimulationTickResult, SimulationFaultHandler, SimulationScenarioHandler,
    SimulationTimeHandler,
};
use crate::quint::itf_fuzzer::{ITFBasedFuzzer, ITFFuzzConfig, ITFTrace};
use aura_agent::{AuraEffectSystem, EffectSystemFactory};
use aura_core::effects::{
    ChaosEffects, SimulationEnvironmentConfig, SimulationEnvironmentFactory, TestingEffects,
    TransportEnvelope,
};
use aura_core::identifiers::AuthorityId;
use aura_core::DeviceId;
use parking_lot::RwLock;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

/// Effect-based simulation composer
///
/// Replaces the former middleware stack pattern with direct effect composition
/// using the agent runtime. Provides clean, explicit dependency injection for
/// simulation effects while respecting the 8-layer architecture.
pub struct SimulationEffectComposer {
    device_id: DeviceId,
    authority_id: Option<AuthorityId>,
    effect_system: Option<Arc<AuraEffectSystem>>,
    time_handler: Option<Arc<SimulationTimeHandler>>,
    fault_handler: Option<Arc<SimulationFaultHandler>>,
    scenario_handler: Option<Arc<SimulationScenarioHandler>>,
    itf_fuzzer: Option<ITFBasedFuzzer>,
    seed: u64,
    /// Optional shared transport inbox for multi-agent simulations
    shared_transport_inbox: Option<Arc<RwLock<Vec<TransportEnvelope>>>>,
}

impl SimulationEffectComposer {
    /// Create a new effect composer for the given device
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            authority_id: None,
            effect_system: None,
            time_handler: None,
            fault_handler: None,
            scenario_handler: None,
            itf_fuzzer: None,
            seed: 42, // Default deterministic seed
            shared_transport_inbox: None,
        }
    }

    /// Set the seed for deterministic simulation
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Set the authority ID used for routing and authority-scoped effects
    pub fn with_authority(mut self, authority_id: AuthorityId) -> Self {
        self.authority_id = Some(authority_id);
        self
    }

    /// Set the shared transport inbox for multi-agent communication
    pub fn with_shared_transport_inbox(
        mut self,
        inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> Self {
        self.shared_transport_inbox = Some(inbox);
        self
    }
    /// Add core effect system using agent runtime (async version for use within tokio runtime)
    ///
    /// Uses `SimulationEnvironmentFactory` trait for decoupled effect system creation.
    pub async fn with_effect_system_async(mut self) -> Result<Self, SimulationComposerError> {
        let factory = EffectSystemFactory::new();
        let sim_config = self.build_simulation_config();

        let effect_system = if let Some(inbox) = self.shared_transport_inbox.clone() {
            factory
                .create_simulation_environment_with_shared_transport(sim_config, inbox)
                .await
                .map_err(|e| SimulationComposerError::EffectSystemCreationFailed(e.to_string()))?
        } else {
            factory
                .create_simulation_environment(sim_config)
                .await
                .map_err(|e| SimulationComposerError::EffectSystemCreationFailed(e.to_string()))?
        };

        self.effect_system = Some(effect_system);
        Ok(self)
    }

    /// Build a SimulationEnvironmentConfig from the composer's current state
    fn build_simulation_config(&self) -> SimulationEnvironmentConfig {
        let mut config = SimulationEnvironmentConfig::new(self.seed, self.device_id);
        if let Some(authority_id) = self.authority_id {
            config = config.with_authority(authority_id);
        }
        config
    }

    /// Add simulation-specific time control
    pub fn with_time_control(mut self) -> Self {
        self.time_handler = Some(Arc::new(SimulationTimeHandler::new()));
        self
    }

    /// Add fault injection capabilities
    pub fn with_fault_injection(mut self) -> Self {
        self.fault_handler = Some(Arc::new(SimulationFaultHandler::new(self.seed)));
        self
    }

    /// Add scenario management capabilities
    pub fn with_scenario_management(mut self) -> Self {
        self.scenario_handler = Some(Arc::new(SimulationScenarioHandler::new(self.seed)));
        self
    }

    /// Add ITF-based fuzzing capabilities for model-based testing
    ///
    /// Enables generation of test scenarios from Quint specifications
    /// and verification of simulation results against formal properties.
    pub fn with_itf_fuzzer(mut self) -> Result<Self, SimulationComposerError> {
        let fuzzer = ITFBasedFuzzer::new().map_err(|e| {
            SimulationComposerError::EffectSystemCreationFailed(format!(
                "Failed to create ITF fuzzer: {}",
                e
            ))
        })?;
        self.itf_fuzzer = Some(fuzzer);
        Ok(self)
    }

    /// Add ITF-based fuzzing with custom configuration
    pub fn with_itf_fuzzer_config(
        mut self,
        config: ITFFuzzConfig,
    ) -> Result<Self, SimulationComposerError> {
        let fuzzer = ITFBasedFuzzer::with_config(config).map_err(|e| {
            SimulationComposerError::EffectSystemCreationFailed(format!(
                "Failed to create ITF fuzzer: {}",
                e
            ))
        })?;
        self.itf_fuzzer = Some(fuzzer);
        Ok(self)
    }

    /// Build the composed simulation environment
    pub fn build(self) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        let effect_system =
            self.effect_system
                .ok_or(SimulationComposerError::MissingRequiredComponent(
                    "effect_system".to_string(),
                ))?;

        Ok(ComposedSimulationEnvironment {
            device_id: self.device_id,
            effect_system,
            time_handler: self.time_handler,
            fault_handler: self.fault_handler,
            scenario_handler: self.scenario_handler,
            itf_fuzzer: self.itf_fuzzer,
            seed: self.seed,
        })
    }

    /// Create a typical testing environment with all handlers
    pub async fn for_testing(
        device_id: DeviceId,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        Self::new(device_id)
            .with_seed(42) // Deterministic for testing
            .with_effect_system_async()
            .await?
            .with_time_control()
            .with_fault_injection()
            .with_scenario_management()
            .build()
    }

    /// Deprecated: Use `for_testing` instead
    #[deprecated(since = "0.1.0", note = "Use for_testing instead")]
    pub async fn for_testing_async(
        device_id: DeviceId,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        Self::for_testing(device_id).await
    }

    /// Create a simulation environment with specific seed
    pub async fn for_simulation(
        device_id: DeviceId,
        seed: u64,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        Self::new(device_id)
            .with_seed(seed)
            .with_effect_system_async()
            .await?
            .with_time_control()
            .with_fault_injection()
            .with_scenario_management()
            .build()
    }

    /// Deprecated: Use `for_simulation` instead
    #[deprecated(since = "0.1.0", note = "Use for_simulation instead")]
    pub async fn for_simulation_async(
        device_id: DeviceId,
        seed: u64,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        Self::for_simulation(device_id, seed).await
    }

    /// Create a simulation environment with shared transport inbox for multi-agent simulations
    ///
    /// This factory enables communication between multiple simulated agents (e.g., Bob, Alice, Carol)
    /// by providing a shared transport layer that routes messages based on destination authority.
    pub async fn for_simulation_async_with_shared_transport(
        device_id: DeviceId,
        seed: u64,
        shared_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        Self::new(device_id)
            .with_seed(seed)
            .with_shared_transport_inbox(shared_inbox)
            .with_effect_system_async()
            .await?
            .with_time_control()
            .with_fault_injection()
            .with_scenario_management()
            .build()
    }
}

/// Composed simulation environment with effect handlers
///
/// Provides unified access to all simulation capabilities through
/// proper effect system composition using the agent runtime.
pub struct ComposedSimulationEnvironment {
    device_id: DeviceId,
    effect_system: Arc<AuraEffectSystem>,
    time_handler: Option<Arc<SimulationTimeHandler>>,
    fault_handler: Option<Arc<SimulationFaultHandler>>,
    scenario_handler: Option<Arc<SimulationScenarioHandler>>,
    itf_fuzzer: Option<ITFBasedFuzzer>,
    seed: u64,
}

impl ComposedSimulationEnvironment {
    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the core effect system
    pub fn effect_system(&self) -> &Arc<AuraEffectSystem> {
        &self.effect_system
    }

    /// Get time effects handler (if available)
    pub fn time_effects(&self) -> Option<&Arc<SimulationTimeHandler>> {
        self.time_handler.as_ref()
    }

    /// Get chaos/fault effects handler (if available)
    pub fn chaos_effects(&self) -> Option<&Arc<SimulationFaultHandler>> {
        self.fault_handler.as_ref()
    }

    /// Get testing effects handler (if available)
    pub fn testing_effects(&self) -> Option<&Arc<SimulationScenarioHandler>> {
        self.scenario_handler.as_ref()
    }

    /// Get the seed used for this environment
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Check if this environment is deterministic
    pub fn is_deterministic(&self) -> bool {
        true // All simulation environments are deterministic
    }

    /// Access time effects through trait
    pub async fn current_timestamp(&self) -> Result<u64, SimulationComposerError> {
        use aura_core::effects::PhysicalTimeEffects;
        match &self.time_handler {
            Some(handler) => handler
                .physical_time()
                .await
                .map(|pt| pt.ts_ms)
                .map_err(|e| SimulationComposerError::EffectOperationFailed(e.to_string())),
            None => Err(SimulationComposerError::MissingRequiredComponent(
                "time_handler".to_string(),
            )),
        }
    }

    /// Inject faults through chaos effects
    pub async fn inject_network_delay(
        &self,
        delay_range: (std::time::Duration, std::time::Duration),
        affected_peers: Option<Vec<String>>,
    ) -> Result<(), SimulationComposerError> {
        match &self.fault_handler {
            Some(handler) => handler
                .inject_network_delay(delay_range, affected_peers)
                .await
                .map_err(|e| {
                    SimulationComposerError::EffectOperationFailed(format!(
                        "Chaos effect failed: {}",
                        e
                    ))
                }),
            None => Err(SimulationComposerError::MissingRequiredComponent(
                "fault_handler".to_string(),
            )),
        }
    }

    /// Record testing events
    pub async fn record_test_event(
        &self,
        event_type: &str,
        event_data: std::collections::HashMap<String, String>,
    ) -> Result<(), SimulationComposerError> {
        match &self.scenario_handler {
            Some(handler) => handler
                .record_event(event_type, event_data)
                .await
                .map_err(|e| {
                    SimulationComposerError::EffectOperationFailed(format!(
                        "Testing effect failed: {}",
                        e
                    ))
                }),
            None => Err(SimulationComposerError::MissingRequiredComponent(
                "scenario_handler".to_string(),
            )),
        }
    }

    /// Run a complete simulation scenario
    ///
    /// Executes a multi-tick simulation with optional early termination conditions.
    /// This demonstrates how complex simulation operations can be composed from
    /// simple effect operations using the agent runtime.
    pub async fn run_scenario(
        &self,
        scenario_name: String,
        scenario_description: String,
        config: SimulationScenarioConfig,
    ) -> Result<SimulationResults, SimulationComposerError> {
        info!(scenario_name = %scenario_name, "Running simulation scenario");

        let scenario_handler = self.scenario_handler.as_ref().ok_or_else(|| {
            SimulationComposerError::MissingRequiredComponent("scenario_handler".to_string())
        })?;

        // Initialize scenario
        let mut event_data = std::collections::HashMap::new();
        event_data.insert("description".to_string(), scenario_description.clone());
        event_data.insert("max_ticks".to_string(), config.max_ticks.to_string());

        scenario_handler
            .record_event("scenario_start", event_data)
            .await
            .map_err(|e| SimulationComposerError::EffectOperationFailed(e.to_string()))?;

        let mut results = SimulationResults::new(scenario_name.clone());

        // Execute simulation ticks
        for tick in 1..=config.max_ticks {
            // Execute tick operations through scenario handler
            let mut tick_data = std::collections::HashMap::new();
            tick_data.insert("tick".to_string(), tick.to_string());

            scenario_handler
                .record_event("tick_execute", tick_data)
                .await
                .map_err(|e| SimulationComposerError::EffectOperationFailed(e.to_string()))?;

            let execution_time = std::time::Duration::ZERO;
            let simulation_elapsed = config.tick_duration * tick as u32;

            let tick_result = SimulationTickResult {
                tick_number: tick,
                execution_time,
                simulation_elapsed,
                delta_time: config.tick_duration,
            };

            results.add_tick_result(tick_result);

            // Check for early termination conditions
            if let Some(condition) = &config.termination_condition {
                if condition.should_terminate(tick, &results) {
                    info!(tick = tick, "Scenario terminated early due to condition");
                    break;
                }
            }
        }

        // Record completion
        let mut completion_data = std::collections::HashMap::new();
        completion_data.insert(
            "ticks_executed".to_string(),
            results.tick_results.len().to_string(),
        );

        scenario_handler
            .record_event("scenario_complete", completion_data)
            .await
            .map_err(|e| SimulationComposerError::EffectOperationFailed(e.to_string()))?;

        info!(
            scenario_name = %scenario_name,
            ticks_executed = results.tick_results.len(),
            "Simulation scenario completed"
        );

        Ok(results)
    }

    /// Get the ITF fuzzer (if available)
    pub fn itf_fuzzer(&self) -> Option<&ITFBasedFuzzer> {
        self.itf_fuzzer.as_ref()
    }

    /// Get mutable access to the ITF fuzzer (if available)
    pub fn itf_fuzzer_mut(&mut self) -> Option<&mut ITFBasedFuzzer> {
        self.itf_fuzzer.as_mut()
    }

    /// Generate ITF traces from a Quint specification for model-based testing
    ///
    /// Uses the ITF fuzzer to generate traces from a Quint spec,
    /// which can then be used to drive simulation scenarios.
    pub async fn generate_itf_scenarios(
        &self,
        spec_file: &Path,
        count: u32,
    ) -> Result<Vec<ITFTrace>, SimulationComposerError> {
        let fuzzer = self.itf_fuzzer.as_ref().ok_or_else(|| {
            SimulationComposerError::MissingRequiredComponent("itf_fuzzer".to_string())
        })?;

        fuzzer
            .generate_mbt_traces(spec_file, count)
            .await
            .map_err(|e| {
                SimulationComposerError::EffectOperationFailed(format!(
                    "Failed to generate ITF traces: {}",
                    e
                ))
            })
    }

    /// Run a simulation scenario driven by an ITF trace
    ///
    /// Converts the ITF trace states to simulation events and executes
    /// the scenario, recording the results for property verification.
    pub async fn run_itf_scenario(
        &mut self,
        itf_trace: &ITFTrace,
        scenario_name: String,
    ) -> Result<SimulationResults, SimulationComposerError> {
        info!(
            scenario_name = %scenario_name,
            trace_states = itf_trace.states.len(),
            "Running ITF-driven simulation scenario"
        );

        let scenario_handler = self.scenario_handler.as_ref().ok_or_else(|| {
            SimulationComposerError::MissingRequiredComponent("scenario_handler".to_string())
        })?;

        // Initialize scenario from ITF metadata
        let mut event_data = std::collections::HashMap::new();
        event_data.insert("source".to_string(), itf_trace.meta.source.clone());
        event_data.insert(
            "trace_states".to_string(),
            itf_trace.states.len().to_string(),
        );

        scenario_handler
            .record_event("itf_scenario_start", event_data)
            .await
            .map_err(|e| SimulationComposerError::EffectOperationFailed(e.to_string()))?;

        let mut results = SimulationResults::new(scenario_name.clone());

        // Execute each ITF state as a simulation tick
        for (tick, itf_state) in itf_trace.states.iter().enumerate() {
            // Record state variables
            let mut tick_data = std::collections::HashMap::new();
            tick_data.insert("tick".to_string(), tick.to_string());
            tick_data.insert("itf_index".to_string(), itf_state.meta.index.to_string());

            // Include action if available (MBT mode)
            if let Some(action) = &itf_state.action_taken {
                tick_data.insert("action".to_string(), action.clone());
            }

            // Record state variable values
            for (var_name, var_value) in &itf_state.variables {
                tick_data.insert(format!("var_{}", var_name), var_value.to_string());
            }

            scenario_handler
                .record_event("itf_state_execute", tick_data)
                .await
                .map_err(|e| SimulationComposerError::EffectOperationFailed(e.to_string()))?;

            let execution_time = std::time::Duration::ZERO;

            let tick_result = SimulationTickResult {
                tick_number: (tick + 1) as u64,
                execution_time,
                simulation_elapsed: std::time::Duration::from_millis((tick + 1) as u64 * 100),
                delta_time: std::time::Duration::from_millis(100),
            };

            results.add_tick_result(tick_result);
        }

        // Record completion
        let mut completion_data = std::collections::HashMap::new();
        completion_data.insert(
            "ticks_executed".to_string(),
            results.tick_results.len().to_string(),
        );

        scenario_handler
            .record_event("itf_scenario_complete", completion_data)
            .await
            .map_err(|e| SimulationComposerError::EffectOperationFailed(e.to_string()))?;

        info!(
            scenario_name = %scenario_name,
            ticks_executed = results.tick_results.len(),
            "ITF-driven simulation scenario completed"
        );

        Ok(results)
    }

    /// Verify simulation results against a Quint specification
    ///
    /// Uses the ITF fuzzer to check properties defined in the spec
    /// against the recorded simulation state.
    pub async fn verify_against_spec(
        &self,
        spec_file: &Path,
    ) -> Result<bool, SimulationComposerError> {
        let fuzzer = self.itf_fuzzer.as_ref().ok_or_else(|| {
            SimulationComposerError::MissingRequiredComponent("itf_fuzzer".to_string())
        })?;

        fuzzer.verify_properties(spec_file).await.map_err(|e| {
            SimulationComposerError::EffectOperationFailed(format!(
                "Property verification failed: {}",
                e
            ))
        })
    }
}

/// Configuration for running a simulation scenario
#[derive(Debug, Clone)]
pub struct SimulationScenarioConfig {
    /// Maximum number of ticks to execute
    pub max_ticks: u64,
    /// Duration of each simulation tick
    pub tick_duration: std::time::Duration,
    /// Additional scenario parameters
    pub parameters: std::collections::HashMap<String, serde_json::Value>,
    /// Optional termination condition
    pub termination_condition: Option<TerminationCondition>,
}

impl Default for SimulationScenarioConfig {
    fn default() -> Self {
        Self {
            max_ticks: 100,
            tick_duration: std::time::Duration::from_millis(100),
            parameters: std::collections::HashMap::new(),
            termination_condition: None,
        }
    }
}

/// Condition for early scenario termination
#[derive(Debug, Clone)]
pub struct TerminationCondition {
    /// Name of this condition
    pub name: String,
    /// Type of termination condition
    pub condition_type: TerminationConditionType,
}

#[derive(Debug, Clone)]
pub enum TerminationConditionType {
    /// Terminate after a fixed number of ticks
    TickLimit(u64),
    /// Terminate after a fixed duration
    TimeLimit(std::time::Duration),
    /// Terminate when a metric reaches a threshold
    MetricThreshold {
        metric_name: String,
        threshold: f64,
        comparison: ComparisonOperator,
    },
}

#[derive(Debug, Clone)]
pub enum ComparisonOperator {
    GreaterThan,
    LessThan,
    Equal,
}

impl TerminationCondition {
    pub fn should_terminate(&self, tick: u64, _results: &SimulationResults) -> bool {
        match &self.condition_type {
            TerminationConditionType::TickLimit(limit) => tick >= *limit,
            TerminationConditionType::TimeLimit(_duration) => {
                // Implementation would check elapsed time
                false
            }
            TerminationConditionType::MetricThreshold { .. } => {
                // Implementation would check metric values
                false
            }
        }
    }
}

/// Results from running a simulation scenario
#[derive(Debug, Clone)]
pub struct SimulationResults {
    pub scenario_name: String,
    pub tick_results: Vec<SimulationTickResult>,
}

impl SimulationResults {
    pub fn new(scenario_name: String) -> Self {
        Self {
            scenario_name,
            tick_results: Vec::new(),
        }
    }

    pub fn add_tick_result(&mut self, result: SimulationTickResult) {
        self.tick_results.push(result);
    }

    pub fn total_execution_time(&self) -> std::time::Duration {
        self.tick_results.iter().map(|r| r.execution_time).sum()
    }

    pub fn average_tick_time(&self) -> std::time::Duration {
        if self.tick_results.is_empty() {
            std::time::Duration::ZERO
        } else {
            self.total_execution_time() / self.tick_results.len() as u32
        }
    }
}

/// Errors that can occur during simulation composition
#[derive(Debug, thiserror::Error)]
pub enum SimulationComposerError {
    /// Failed to create effect system
    #[error("Effect system creation failed: {0}")]
    EffectSystemCreationFailed(String),

    /// Missing required component
    #[error("Missing required component: {0}")]
    MissingRequiredComponent(String),

    /// Effect operation failed
    #[error("Effect operation failed: {0}")]
    EffectOperationFailed(String),
}

/// Factory functions for creating common simulation environments
pub mod factory {
    use super::*;

    /// Create a testing simulation environment with all handlers
    pub async fn create_testing_environment(
        device_id: DeviceId,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        SimulationEffectComposer::for_testing(device_id).await
    }

    /// Deprecated: Use `create_testing_environment` instead
    #[deprecated(since = "0.1.0", note = "Use create_testing_environment instead")]
    pub async fn create_testing_environment_async(
        device_id: DeviceId,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        create_testing_environment(device_id).await
    }

    /// Create a deterministic simulation environment for reproducible testing
    pub async fn create_deterministic_environment(
        device_id: DeviceId,
        seed: u64,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        SimulationEffectComposer::for_simulation(device_id, seed).await
    }

    /// Deprecated: Use `create_deterministic_environment` instead
    #[deprecated(since = "0.1.0", note = "Use create_deterministic_environment instead")]
    pub async fn create_deterministic_environment_async(
        device_id: DeviceId,
        seed: u64,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        create_deterministic_environment(device_id, seed).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_composer_builder_pattern() {
        use aura_testkit::DeviceTestFixture;
        let fixture = DeviceTestFixture::new(0);
        let device_id = fixture.device_id();

        // Test that we can build without effect system should fail
        let result = SimulationEffectComposer::new(device_id).build();
        assert!(result.is_err());

        if let Err(SimulationComposerError::MissingRequiredComponent(component)) = result {
            assert_eq!(component, "effect_system");
        } else {
            panic!("Expected MissingRequiredComponent error");
        }
    }

    #[test]
    fn test_seed_configuration() {
        use aura_testkit::DeviceTestFixture;
        let fixture = DeviceTestFixture::new(1);
        let device_id = fixture.device_id();
        let composer = SimulationEffectComposer::new(device_id).with_seed(999);
        assert_eq!(composer.seed, 999);
    }

    #[test]
    fn test_handler_composition() {
        use aura_testkit::DeviceTestFixture;
        let fixture = DeviceTestFixture::new(2);
        let device_id = fixture.device_id();

        // Build with handlers but no effect system
        let composer = SimulationEffectComposer::new(device_id)
            .with_seed(123)
            .with_time_control()
            .with_fault_injection()
            .with_scenario_management();

        // Should still fail without effect system
        let result = composer.build();
        assert!(result.is_err());
    }

    #[test]
    fn test_scenario_config_default() {
        let config = SimulationScenarioConfig::default();
        assert_eq!(config.max_ticks, 100);
        assert_eq!(config.tick_duration, std::time::Duration::from_millis(100));
        assert!(config.parameters.is_empty());
        assert!(config.termination_condition.is_none());
    }

    #[test]
    fn test_simulation_results() {
        let mut results = SimulationResults::new("test".to_string());
        assert_eq!(results.scenario_name, "test");
        assert_eq!(results.tick_results.len(), 0);
        assert_eq!(results.total_execution_time(), std::time::Duration::ZERO);
        assert_eq!(results.average_tick_time(), std::time::Duration::ZERO);

        // Add a tick result
        let tick_result = SimulationTickResult {
            tick_number: 1,
            execution_time: std::time::Duration::from_millis(10),
            simulation_elapsed: std::time::Duration::from_secs(1),
            delta_time: std::time::Duration::from_millis(100),
        };
        results.add_tick_result(tick_result);

        assert_eq!(results.tick_results.len(), 1);
        assert_eq!(
            results.total_execution_time(),
            std::time::Duration::from_millis(10)
        );
        assert_eq!(
            results.average_tick_time(),
            std::time::Duration::from_millis(10)
        );
    }

    #[test]
    fn test_termination_condition_tick_limit() {
        let condition = TerminationCondition {
            name: "tick_limit".to_string(),
            condition_type: TerminationConditionType::TickLimit(10),
        };

        let results = SimulationResults::new("test".to_string());

        assert!(!condition.should_terminate(5, &results));
        assert!(condition.should_terminate(10, &results));
        assert!(condition.should_terminate(15, &results));
    }

    #[test]
    fn test_error_display() {
        let err = SimulationComposerError::EffectSystemCreationFailed("test error".to_string());
        assert!(format!("{}", err).contains("Effect system creation failed"));

        let err = SimulationComposerError::MissingRequiredComponent("handler".to_string());
        assert!(format!("{}", err).contains("Missing required component"));

        let err = SimulationComposerError::EffectOperationFailed("operation failed".to_string());
        assert!(format!("{}", err).contains("Effect operation failed"));
    }
}
