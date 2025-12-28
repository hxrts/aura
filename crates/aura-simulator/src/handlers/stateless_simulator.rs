//! Stateless Simulator Handler
//!
//! This module provides a refactored simulator handler that follows the stateless effect pattern
//! as described in the 8-layer architecture documentation. Instead of maintaining internal state
//! with mutexes, it delegates all operations to effect handlers.
//!
//! ## Architecture Alignment
//!
//! This handler properly implements Layer 6 (Runtime Composition) by:
//! - Using stateless effect handlers from Layer 3 (Implementation)
//! - Delegating coordination to Layer 4 (Orchestration) patterns
//! - Eliminating internal state and mutex-based synchronization
//!
//! ## Migration from CoreSimulatorHandler
//!
//! The original CoreSimulatorHandler violated the stateless effect pattern by:
//! - Maintaining `Arc<Mutex<CoreHandlerState>>` internal state
//! - Directly mutating scenario and checkpoint collections
//! - Implementing coordination logic at the wrong architectural layer
//!
//! This refactored version fixes these issues by using dependency injection
//! and pure effect delegation.

use aura_core::{
    effects::{
        CheckpointId, FaultInjectionConfig, FaultType, ScenarioId, ScenarioState,
        SimulationEffects, SimulationMetrics,
    },
    AuraError, Result as AuraResult,
};
use serde_json::Value;
use std::{collections::HashMap, time::Duration};
use tracing::{debug, info};

/// Stateless simulator handler that delegates all operations to effect handlers
///
/// This handler implements the proper stateless effect pattern by:
/// - Taking effect dependencies via dependency injection
/// - Delegating all state management to external services via effects
/// - Maintaining no internal mutable state
/// - Following the 8-layer architecture principles
#[derive(Debug)]
pub struct StatelessSimulatorHandler<E>
where
    E: SimulationEffects,
{
    /// Injected simulation effects for all operations
    effects: E,
    /// Handler configuration
    config: StatelessSimulatorConfig,
}

/// Configuration for the stateless simulator handler
#[derive(Debug, Clone)]
pub struct StatelessSimulatorConfig {
    /// Default scenario timeout
    pub default_scenario_timeout: Duration,
    /// Maximum number of concurrent scenarios
    pub max_concurrent_scenarios: usize,
    /// Checkpoint retention policy
    pub checkpoint_retention_days: u32,
    /// Enable detailed operation logging
    pub verbose_logging: bool,
}

impl Default for StatelessSimulatorConfig {
    fn default() -> Self {
        Self {
            default_scenario_timeout: Duration::from_secs(300), // 5 minutes
            max_concurrent_scenarios: 10,
            checkpoint_retention_days: 7,
            verbose_logging: false,
        }
    }
}

impl<E> StatelessSimulatorHandler<E>
where
    E: SimulationEffects,
{
    /// Create a new stateless simulator handler with injected effects
    pub fn new(effects: E) -> Self {
        Self {
            effects,
            config: StatelessSimulatorConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(effects: E, config: StatelessSimulatorConfig) -> Self {
        Self { effects, config }
    }

    /// Initialize a new simulation scenario
    ///
    /// Replaces the stateful `InitializeScenario` operation from CoreSimulatorHandler
    /// by delegating to the SimulationControlEffects trait.
    pub async fn initialize_scenario(
        &self,
        scenario_name: String,
        description: String,
        parameters: HashMap<String, Value>,
    ) -> AuraResult<ScenarioId> {
        if self.config.verbose_logging {
            debug!(scenario_name = %scenario_name, "Initializing simulation scenario");
        }

        // Delegate to effect system instead of maintaining internal state
        let scenario_id = self
            .effects
            .create_scenario(scenario_name.clone(), description, parameters)
            .await?;

        // Start the scenario immediately (following the original behavior)
        self.effects.start_scenario(&scenario_id).await?;

        info!(scenario_name = %scenario_name, scenario_id = %scenario_id, "Simulation scenario initialized");
        Ok(scenario_id)
    }

    /// Execute a single simulation tick
    ///
    /// Replaces the stateful tick counting from CoreSimulatorHandler by delegating
    /// time advancement to the SimulationControlEffects.
    pub async fn execute_tick(
        &self,
        tick_number: u64,
        delta_time: Duration,
    ) -> AuraResult<SimulationTickResult> {
        if self.config.verbose_logging {
            debug!(
                tick_number = tick_number,
                delta_ms = delta_time.as_millis(),
                "Executing simulation tick"
            );
        }

        // Advance simulation time instead of maintaining tick counters
        self.effects.advance_time(delta_time).await?;

        // Record the operation for metrics
        let execution_time = Duration::ZERO;
        self.effects
            .record_operation("simulation_tick", execution_time)
            .await?;

        // Get current simulation state from effects instead of internal state
        let simulation_time = self.effects.get_simulation_time().await?;

        Ok(SimulationTickResult {
            tick_number,
            execution_time,
            simulation_elapsed: simulation_time.elapsed(),
            delta_time,
        })
    }

    /// Inject a fault into the simulation
    ///
    /// Replaces stateful fault tracking with delegation to FaultInjectionEffects.
    pub async fn inject_fault(
        &self,
        fault_type: FaultType,
        duration: Option<Duration>,
    ) -> AuraResult<()> {
        if self.config.verbose_logging {
            debug!(fault_type = ?fault_type, "Injecting simulation fault");
        }

        let fault_config = FaultInjectionConfig {
            fault_type,
            parameters: HashMap::new(),
            duration,
            probability: 1.0,
        };

        // Delegate to effect system instead of storing in scenario metadata
        self.effects.inject_fault(fault_config.clone()).await?;

        info!(fault_type = ?fault_config.fault_type, "Simulation fault injected");
        Ok(())
    }

    /// Create a simulation checkpoint
    ///
    /// Replaces stateful checkpoint storage with delegation to SimulationControlEffects.
    pub async fn create_checkpoint(
        &self,
        checkpoint_name: String,
        description: Option<String>,
    ) -> AuraResult<CheckpointId> {
        if self.config.verbose_logging {
            debug!(checkpoint_name = %checkpoint_name, "Creating simulation checkpoint");
        }

        // Delegate to effect system instead of maintaining checkpoint HashMap
        let checkpoint_id = self
            .effects
            .create_checkpoint(checkpoint_name.clone())
            .await?;

        // Record checkpoint metadata if description provided
        if let Some(_desc) = description {
            self.effects
                .record_metric(
                    format!("checkpoint_{}_description", checkpoint_id),
                    1.0, // Just recording presence
                )
                .await?;
        }

        info!(checkpoint_name = %checkpoint_name, checkpoint_id = %checkpoint_id, "Simulation checkpoint created");
        Ok(checkpoint_id)
    }

    /// Restore from a simulation checkpoint
    ///
    /// Replaces stateful state rollback with delegation to SimulationControlEffects.
    pub async fn restore_checkpoint(&self, checkpoint_id: CheckpointId) -> AuraResult<()> {
        if self.config.verbose_logging {
            debug!(checkpoint_id = %checkpoint_id, "Restoring simulation checkpoint");
        }

        // Verify checkpoint exists using effect system
        let checkpoint = self.effects.get_checkpoint(&checkpoint_id).await?;

        if checkpoint.is_none() {
            return Err(AuraError::invalid(format!(
                "Checkpoint not found: {}",
                checkpoint_id
            )));
        }

        // Delegate to effect system instead of manual state restoration
        self.effects.restore_checkpoint(&checkpoint_id).await?;

        info!(checkpoint_id = %checkpoint_id, "Simulation checkpoint restored");
        Ok(())
    }

    /// Get current simulation metrics
    ///
    /// Replaces internal metrics counting with delegation to SimulationObservationEffects.
    pub async fn get_simulation_metrics(&self) -> AuraResult<SimulationMetrics> {
        self.effects.get_metrics().await
    }

    /// List active scenarios
    ///
    /// Replaces internal scenario HashMap with delegation to SimulationControlEffects.
    pub async fn list_scenarios(&self) -> AuraResult<Vec<ScenarioSummary>> {
        let scenarios = self.effects.list_scenarios().await?;

        Ok(scenarios
            .into_iter()
            .map(|scenario| ScenarioSummary {
                id: scenario.id,
                name: scenario.name,
                state: scenario.state,
                description: scenario.description,
            })
            .collect())
    }

    /// Check if a scenario is currently running
    pub async fn is_scenario_running(&self, scenario_id: &ScenarioId) -> AuraResult<bool> {
        let scenario = self.effects.get_scenario(scenario_id).await?;

        Ok(scenario
            .map(|s| s.state == ScenarioState::Running)
            .unwrap_or(false))
    }

    /// Pause a running scenario
    pub async fn pause_scenario(&self, scenario_id: &ScenarioId) -> AuraResult<()> {
        self.effects.pause_scenario(scenario_id).await?;

        info!(scenario_id = %scenario_id, "Scenario paused");
        Ok(())
    }

    /// Resume a paused scenario
    pub async fn resume_scenario(&self, scenario_id: &ScenarioId) -> AuraResult<()> {
        self.effects.resume_scenario(scenario_id).await?;

        info!(scenario_id = %scenario_id, "Scenario resumed");
        Ok(())
    }

    /// Stop a scenario
    pub async fn stop_scenario(&self, scenario_id: &ScenarioId) -> AuraResult<()> {
        self.effects.stop_scenario(scenario_id).await?;

        info!(scenario_id = %scenario_id, "Scenario stopped");
        Ok(())
    }

    /// Clear all active faults
    pub async fn clear_faults(&self) -> AuraResult<()> {
        self.effects.clear_faults().await?;

        info!("All simulation faults cleared");
        Ok(())
    }

}

/// Result of executing a simulation tick
#[derive(Debug, Clone)]
pub struct SimulationTickResult {
    pub tick_number: u64,
    pub execution_time: Duration,
    pub simulation_elapsed: Duration,
    pub delta_time: Duration,
}

/// Summary information about a scenario
#[derive(Debug, Clone)]
pub struct ScenarioSummary {
    pub id: ScenarioId,
    pub name: String,
    pub state: ScenarioState,
    pub description: String,
}


#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::NetworkFault;
    use aura_testkit::MockSimulationHandler;
    use tokio;

    #[tokio::test]
    async fn test_stateless_simulator_scenario_lifecycle() {
        let mock_effects = MockSimulationHandler::new();
        let handler = StatelessSimulatorHandler::new(mock_effects);

        // Initialize scenario
        let scenario_id = handler
            .initialize_scenario(
                "test_scenario".to_string(),
                "Test scenario".to_string(),
                HashMap::new(),
            )
            .await
            .unwrap();

        // Verify scenario is running
        assert!(handler.is_scenario_running(&scenario_id).await.unwrap());

        // Pause and resume
        handler.pause_scenario(&scenario_id).await.unwrap();
        handler.resume_scenario(&scenario_id).await.unwrap();

        // Stop scenario
        handler.stop_scenario(&scenario_id).await.unwrap();
        assert!(!handler.is_scenario_running(&scenario_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_stateless_simulator_tick_execution() {
        let mock_effects = MockSimulationHandler::new();
        let handler = StatelessSimulatorHandler::new(mock_effects);

        let tick_result = handler
            .execute_tick(1, Duration::from_millis(100))
            .await
            .unwrap();

        assert_eq!(tick_result.tick_number, 1);
        assert_eq!(tick_result.delta_time, Duration::from_millis(100));
        assert_eq!(tick_result.execution_time, Duration::ZERO);
    }

    #[tokio::test]
    async fn test_stateless_simulator_checkpoint_management() {
        let mock_effects = MockSimulationHandler::new();
        let handler = StatelessSimulatorHandler::new(mock_effects);

        // Create checkpoint
        let checkpoint_id = handler
            .create_checkpoint(
                "test_checkpoint".to_string(),
                Some("Test checkpoint description".to_string()),
            )
            .await
            .unwrap();

        // Restore checkpoint should succeed
        handler.restore_checkpoint(checkpoint_id).await.unwrap();
    }

    #[tokio::test]
    async fn test_stateless_simulator_fault_injection() {
        let mock_effects = MockSimulationHandler::new();
        let handler = StatelessSimulatorHandler::new(mock_effects);

        // Inject network partition fault
        handler
            .inject_fault(
                FaultType::Network(NetworkFault::Partition {
                    groups: vec![vec!["node1".to_string()]],
                }),
                Some(Duration::from_secs(30)),
            )
            .await
            .unwrap();

        // Inject packet loss fault
        handler
            .inject_fault(
                FaultType::Network(NetworkFault::PacketLoss { probability: 0.1 }),
                None,
            )
            .await
            .unwrap();

        // Clear all faults
        handler.clear_faults().await.unwrap();
    }

    #[tokio::test]
    async fn test_stateless_simulator_metrics() {
        let mock_effects = MockSimulationHandler::new();
        let handler = StatelessSimulatorHandler::new(mock_effects);

        // Execute some operations to generate metrics
        handler
            .execute_tick(1, Duration::from_millis(50))
            .await
            .unwrap();

        // Get metrics
        let metrics = handler.get_simulation_metrics().await.unwrap();
        assert!(metrics.operations_count > 0);
    }
}
