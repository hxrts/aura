//! Effect system composer for simulation
//!
//! This module provides effect-based composition patterns using the agent runtime.
//! Enables clean composition of simulation effects following the 8-layer architecture.
//!
//! This composer uses the aura-agent runtime (Layer 6) to properly compose simulation
//! environments, unlike the deprecated stateless pattern which incorrectly used Layer 3
//! handlers directly.

use super::{
    SimulationFaultHandler, SimulationScenarioHandler, SimulationTickResult, SimulationTimeHandler,
};
use aura_agent::{AgentBuilder, AuraEffectSystem};
use aura_core::effects::{ChaosEffects, TestingEffects, TimeEffects};
use aura_core::identifiers::AuthorityId;
use aura_core::DeviceId;
use std::sync::Arc;
use tracing::{debug, info};

/// Effect-based simulation composer
///
/// Replaces the former middleware stack pattern with direct effect composition
/// using the agent runtime. Provides clean, explicit dependency injection for
/// simulation effects while respecting the 8-layer architecture.
pub struct SimulationEffectComposer {
    device_id: DeviceId,
    effect_system: Option<Arc<AuraEffectSystem>>,
    time_handler: Option<Arc<SimulationTimeHandler>>,
    fault_handler: Option<Arc<SimulationFaultHandler>>,
    scenario_handler: Option<Arc<SimulationScenarioHandler>>,
    seed: u64,
}

impl SimulationEffectComposer {
    /// Create a new effect composer for the given device
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            effect_system: None,
            time_handler: None,
            fault_handler: None,
            scenario_handler: None,
            seed: 42, // Default deterministic seed
        }
    }

    /// Set the seed for deterministic simulation
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Add core effect system using agent runtime
    pub fn with_effect_system(mut self) -> Result<Self, SimulationComposerError> {
        let authority_id = AuthorityId::new();
        let agent = AgentBuilder::new()
            .with_authority(authority_id)
            .build_testing()
            .map_err(|e| SimulationComposerError::EffectSystemCreationFailed(e.to_string()))?;
        let effect_system = agent.runtime().effects().clone();

        self.effect_system = Some(Arc::new(effect_system));
        Ok(self)
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
            seed: self.seed,
        })
    }

    /// Create a typical testing environment with all handlers
    pub fn for_testing(
        device_id: DeviceId,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        Self::new(device_id)
            .with_seed(42) // Deterministic for testing
            .with_effect_system()?
            .with_time_control()
            .with_fault_injection()
            .with_scenario_management()
            .build()
    }

    /// Create a simulation environment with specific seed
    pub fn for_simulation(
        device_id: DeviceId,
        seed: u64,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        Self::new(device_id)
            .with_seed(seed)
            .with_effect_system()?
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
        match &self.time_handler {
            Some(handler) => Ok(handler.current_timestamp().await),
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
            let tick_start = std::time::Instant::now();

            // Execute tick operations through scenario handler
            let mut tick_data = std::collections::HashMap::new();
            tick_data.insert("tick".to_string(), tick.to_string());

            scenario_handler
                .record_event("tick_execute", tick_data)
                .await
                .map_err(|e| SimulationComposerError::EffectOperationFailed(e.to_string()))?;

            let execution_time = tick_start.elapsed();
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
    pub fn create_testing_environment(
        device_id: DeviceId,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        SimulationEffectComposer::for_testing(device_id)
    }

    /// Create a deterministic simulation environment for reproducible testing
    pub fn create_deterministic_environment(
        device_id: DeviceId,
        seed: u64,
    ) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        SimulationEffectComposer::for_simulation(device_id, seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::DeviceId;

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
