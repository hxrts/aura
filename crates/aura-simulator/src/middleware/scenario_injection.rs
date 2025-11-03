//! Scenario injection middleware for dynamic test scenario modifications

use super::{Result, SimulatorContext, SimulatorHandler, SimulatorMiddleware, SimulatorOperation};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

/// Middleware for injecting scenarios and modifying simulation behavior
pub struct ScenarioInjectionMiddleware {
    /// Pre-defined scenarios to inject
    scenarios: HashMap<String, ScenarioDefinition>,
    /// Active injections
    active_injections: Vec<ActiveInjection>,
    /// Enable scenario randomization
    enable_randomization: bool,
    /// Injection probability (0.0 to 1.0)
    injection_probability: f64,
    /// Maximum number of concurrent injections
    max_concurrent_injections: usize,
}

impl ScenarioInjectionMiddleware {
    /// Create new scenario injection middleware
    pub fn new() -> Self {
        Self {
            scenarios: HashMap::new(),
            active_injections: Vec::new(),
            enable_randomization: false,
            injection_probability: 0.1,
            max_concurrent_injections: 3,
        }
    }

    /// Add predefined scenario
    pub fn with_scenario(mut self, id: String, scenario: ScenarioDefinition) -> Self {
        self.scenarios.insert(id, scenario);
        self
    }

    /// Enable randomization of scenario injection
    pub fn with_randomization(mut self, enable: bool, probability: f64) -> Self {
        self.enable_randomization = enable;
        self.injection_probability = probability.clamp(0.0, 1.0);
        self
    }

    /// Set maximum concurrent injections
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent_injections = max;
        self
    }

    /// Check if we should inject a scenario
    fn should_inject_scenario(&self, context: &SimulatorContext) -> bool {
        if !self.enable_randomization {
            return false;
        }

        if self.active_injections.len() >= self.max_concurrent_injections {
            return false;
        }

        // Use deterministic randomness based on seed and tick
        let mut seed = context.seed.wrapping_add(context.tick);
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let random_value = (seed >> 16) as f64 / u16::MAX as f64;

        random_value < self.injection_probability
    }

    /// Select a scenario to inject
    fn select_scenario(&self, context: &SimulatorContext) -> Option<&ScenarioDefinition> {
        if self.scenarios.is_empty() {
            return None;
        }

        // Use deterministic selection based on seed and tick
        let mut seed = context.seed.wrapping_add(context.tick * 2);
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let index = (seed as usize) % self.scenarios.len();

        self.scenarios.values().nth(index)
    }

    /// Inject a scenario
    fn inject_scenario(
        &mut self,
        scenario: &ScenarioDefinition,
        context: &SimulatorContext,
    ) -> Result<Value> {
        let injection = ActiveInjection {
            id: format!("injection_{}", context.tick),
            scenario_id: scenario.id.clone(),
            start_tick: context.tick,
            duration: scenario.duration,
            actions: scenario.actions.clone(),
            current_action: 0,
        };

        self.active_injections.push(injection);

        Ok(json!({
            "injected_scenario": scenario.id,
            "tick": context.tick,
            "duration": scenario.duration.map(|d| d.as_millis()),
            "actions_count": scenario.actions.len()
        }))
    }

    /// Process active injections
    fn process_active_injections(&mut self, context: &SimulatorContext) -> Vec<InjectionAction> {
        let mut actions_to_execute = Vec::new();

        // Update active injections
        self.active_injections.retain_mut(|injection| {
            // Check if injection has expired
            if let Some(duration) = injection.duration {
                let elapsed_ticks = context.tick - injection.start_tick;
                if Duration::from_millis(elapsed_ticks * 100) >= duration {
                    return false; // Remove expired injection
                }
            }

            // Check if there are more actions to execute
            if injection.current_action < injection.actions.len() {
                let action = injection.actions[injection.current_action].clone();
                actions_to_execute.push(action);
                injection.current_action += 1;
            }

            true // Keep active injection
        });

        actions_to_execute
    }

    /// Execute injection actions
    fn execute_injection_actions(
        &self,
        actions: Vec<InjectionAction>,
        context: &SimulatorContext,
    ) -> Result<Value> {
        let mut results = Vec::new();

        for action in actions {
            let result = match action {
                InjectionAction::InjectFault { fault_type, target } => {
                    json!({
                        "type": "fault_injection",
                        "fault_type": format!("{:?}", fault_type),
                        "target": target,
                        "tick": context.tick
                    })
                }

                InjectionAction::ModifyNetwork { latency, loss_rate } => {
                    json!({
                        "type": "network_modification",
                        "latency_ms": latency.map(|d| d.as_millis()),
                        "loss_rate": loss_rate,
                        "tick": context.tick
                    })
                }

                InjectionAction::AddParticipant {
                    participant_id,
                    role,
                } => {
                    json!({
                        "type": "participant_addition",
                        "participant_id": participant_id,
                        "role": role,
                        "tick": context.tick
                    })
                }

                InjectionAction::RemoveParticipant { participant_id } => {
                    json!({
                        "type": "participant_removal",
                        "participant_id": participant_id,
                        "tick": context.tick
                    })
                }

                InjectionAction::TriggerEvent {
                    event_type,
                    parameters,
                } => {
                    json!({
                        "type": "event_trigger",
                        "event_type": event_type,
                        "parameters": parameters,
                        "tick": context.tick
                    })
                }
            };

            results.push(result);
        }

        Ok(json!({
            "executed_actions": results,
            "tick": context.tick,
            "active_injections": self.active_injections.len()
        }))
    }
}

impl Default for ScenarioInjectionMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatorMiddleware for ScenarioInjectionMiddleware {
    fn process(
        &self,
        operation: SimulatorOperation,
        context: &SimulatorContext,
        next: &dyn SimulatorHandler,
    ) -> Result<Value> {
        // For this demonstration, we'll use interior mutability in a real implementation
        // Here we simulate the injection logic

        match &operation {
            SimulatorOperation::ExecuteTick { .. } => {
                // Check if we should inject a scenario
                if self.should_inject_scenario(context) {
                    if let Some(scenario) = self.select_scenario(context) {
                        // In a real implementation, we would inject the scenario
                        let injection_result = json!({
                            "scenario_injection": {
                                "scenario_id": scenario.id,
                                "tick": context.tick,
                                "actions_count": scenario.actions.len()
                            }
                        });

                        // Add injection info to context metadata
                        let mut enhanced_context = context.clone();
                        enhanced_context
                            .metadata
                            .insert("scenario_injected".to_string(), scenario.id.clone());

                        // Call next handler with enhanced context
                        let mut result = next.handle(operation, &enhanced_context)?;

                        // Add injection results
                        if let Some(obj) = result.as_object_mut() {
                            obj.insert("scenario_injection".to_string(), injection_result);
                        }

                        return Ok(result);
                    }
                }

                // No injection, proceed normally
                next.handle(operation, context)
            }

            _ => {
                // For other operations, just pass through
                next.handle(operation, context)
            }
        }
    }

    fn name(&self) -> &str {
        "scenario_injection"
    }

    fn handles(&self, operation: &SimulatorOperation) -> bool {
        matches!(operation, SimulatorOperation::ExecuteTick { .. })
    }
}

/// Scenario definition for injection
#[derive(Debug, Clone)]
pub struct ScenarioDefinition {
    /// Unique scenario identifier
    pub id: String,
    /// Scenario description
    pub description: String,
    /// Duration of the scenario (None = until simulation ends)
    pub duration: Option<Duration>,
    /// Actions to execute during the scenario
    pub actions: Vec<InjectionAction>,
    /// Conditions for triggering this scenario
    pub trigger_conditions: Vec<TriggerCondition>,
}

/// Actions that can be injected into a scenario
#[derive(Debug, Clone)]
pub enum InjectionAction {
    /// Inject a fault
    InjectFault { fault_type: String, target: String },

    /// Modify network conditions
    ModifyNetwork {
        latency: Option<Duration>,
        loss_rate: Option<f64>,
    },

    /// Add a new participant
    AddParticipant {
        participant_id: String,
        role: String,
    },

    /// Remove a participant
    RemoveParticipant { participant_id: String },

    /// Trigger a custom event
    TriggerEvent {
        event_type: String,
        parameters: HashMap<String, Value>,
    },
}

/// Conditions for triggering scenario injection
#[derive(Debug, Clone)]
pub enum TriggerCondition {
    /// Trigger after specific tick count
    TickCount { min_tick: u64 },

    /// Trigger based on participant count
    ParticipantCount { min_count: usize, max_count: usize },

    /// Trigger based on state condition
    StateCondition { field: String, value: Value },

    /// Trigger randomly with probability
    RandomTrigger { probability: f64 },
}

/// Active injection tracking
#[derive(Debug, Clone)]
struct ActiveInjection {
    id: String,
    scenario_id: String,
    start_tick: u64,
    duration: Option<Duration>,
    actions: Vec<InjectionAction>,
    current_action: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpSimulatorHandler;
    use std::time::Duration;

    #[test]
    fn test_scenario_injection_creation() {
        let middleware = ScenarioInjectionMiddleware::new()
            .with_randomization(true, 0.5)
            .with_max_concurrent(2);

        assert_eq!(middleware.injection_probability, 0.5);
        assert_eq!(middleware.max_concurrent_injections, 2);
        assert!(middleware.enable_randomization);
    }

    #[test]
    fn test_scenario_injection_no_scenarios() {
        let middleware = ScenarioInjectionMiddleware::new();
        let handler = NoOpSimulatorHandler;
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let result = middleware.process(
            SimulatorOperation::ExecuteTick {
                tick_number: 1,
                delta_time: Duration::from_millis(100),
            },
            &context,
            &handler,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_scenario_definition() {
        let scenario = ScenarioDefinition {
            id: "test_scenario".to_string(),
            description: "Test scenario for injection".to_string(),
            duration: Some(Duration::from_secs(30)),
            actions: vec![InjectionAction::InjectFault {
                fault_type: "network_partition".to_string(),
                target: "node_1".to_string(),
            }],
            trigger_conditions: vec![TriggerCondition::TickCount { min_tick: 10 }],
        };

        assert_eq!(scenario.id, "test_scenario");
        assert_eq!(scenario.actions.len(), 1);
        assert_eq!(scenario.trigger_conditions.len(), 1);
    }

    #[test]
    fn test_injection_action_serialization() {
        let action = InjectionAction::ModifyNetwork {
            latency: Some(Duration::from_millis(500)),
            loss_rate: Some(0.1),
        };

        // Verify action can be cloned and formatted
        let _cloned = action.clone();
        let _formatted = format!("{:?}", action);
    }
}
