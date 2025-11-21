//! Simulation-specific Quint property evaluator
//!
//! This module provides simulation-specific wrappers around the core Quint
//! functionality, adapting simulation state to work with Quint property evaluation.

use aura_core::effects::{
    Property, PropertyEvaluator, PropertySpec, QuintEvaluationEffects, QuintVerificationEffects,
    VerificationResult,
};
use aura_core::Result;
use aura_quint::QuintEffectHandler;
use async_trait::async_trait;
use serde_json::{Map, Value};

/// Simulation-specific property evaluator that adapts simulation state for Quint
#[derive(Debug, Clone)]
pub struct SimulationPropertyEvaluator {
    /// Core Quint evaluator handler (Layer 3: Implementation)
    core_evaluator: QuintEffectHandler,
    /// Configuration for simulation-specific adaptations
    config: SimulationEvaluatorConfig,
}

/// Configuration for simulation-specific Quint evaluation
#[derive(Debug, Clone)]
pub struct SimulationEvaluatorConfig {
    /// Enable debug logging for state adaptation
    pub debug_state_adaptation: bool,
    /// Enable Byzantine participant filtering
    pub filter_byzantine_participants: bool,
    /// Maximum simulation time to include in state (steps)
    pub max_simulation_time: Option<u64>,
}

impl Default for SimulationEvaluatorConfig {
    fn default() -> Self {
        Self {
            debug_state_adaptation: false,
            filter_byzantine_participants: true,
            max_simulation_time: Some(1000),
        }
    }
}

impl SimulationPropertyEvaluator {
    /// Create a new simulation property evaluator with default configuration
    pub fn new() -> Self {
        Self::with_config(SimulationEvaluatorConfig::default())
    }

    /// Create a new simulation property evaluator with custom configuration
    pub fn with_config(config: SimulationEvaluatorConfig) -> Self {
        Self {
            core_evaluator: QuintEffectHandler::new(),
            config,
        }
    }

    /// Create a new simulation property evaluator with custom core evaluator
    pub fn with_evaluator(core_evaluator: QuintEffectHandler, config: SimulationEvaluatorConfig) -> Self {
        Self {
            core_evaluator,
            config,
        }
    }

    /// Adapt simulation state to Quint-compatible format
    pub fn adapt_simulation_state(&self, simulation_state: &SimulationWorldState) -> Value {
        if self.config.debug_state_adaptation {
            tracing::debug!("Adapting simulation state to Quint format");
        }

        let mut adapted_state = Map::new();

        // Extract participant information
        let participants: Vec<Value> = simulation_state
            .participant_states
            .iter()
            .filter(|(participant_id, state)| {
                // Filter out Byzantine participants if configured
                if self.config.filter_byzantine_participants && state.is_byzantine {
                    if self.config.debug_state_adaptation {
                        tracing::debug!("Filtering out Byzantine participant: {}", participant_id);
                    }
                    false
                } else {
                    true
                }
            })
            .map(|(participant_id, state)| {
                let mut participant_value = Map::new();
                participant_value.insert("id".to_string(), Value::String(participant_id.clone()));
                participant_value.insert("is_byzantine".to_string(), Value::Bool(state.is_byzantine));
                participant_value.insert("local_time".to_string(), Value::Number(state.local_time.into()));
                
                // Add authority information
                if let Some(authority_id) = &state.authority_id {
                    participant_value.insert("authority_id".to_string(), Value::String(authority_id.clone()));
                }
                
                Value::Object(participant_value)
            })
            .collect();

        adapted_state.insert("participants".to_string(), Value::Array(participants));

        // Extract global state information
        adapted_state.insert("global_time".to_string(), Value::Number(simulation_state.global_time.into()));
        adapted_state.insert("step_count".to_string(), Value::Number(simulation_state.step_count.into()));

        // Extract network state
        let mut network_state = Map::new();
        network_state.insert("message_count".to_string(), Value::Number(simulation_state.network_state.total_messages.into()));
        network_state.insert("partition_count".to_string(), Value::Number(simulation_state.network_state.partition_count.into()));
        adapted_state.insert("network".to_string(), Value::Object(network_state));

        // Add simulation metadata
        let mut metadata = Map::new();
        metadata.insert("simulation_id".to_string(), Value::String(simulation_state.simulation_id.clone()));
        if let Some(max_time) = self.config.max_simulation_time {
            metadata.insert("max_simulation_time".to_string(), Value::Number(max_time.into()));
        }
        adapted_state.insert("metadata".to_string(), Value::Object(metadata));

        if self.config.debug_state_adaptation {
            tracing::debug!("Adapted state contains {} top-level keys", adapted_state.len());
        }

        Value::Object(adapted_state)
    }

    /// Evaluate a property against the simulation state
    pub async fn evaluate_simulation_property(
        &self,
        property: &Property,
        simulation_state: &SimulationWorldState,
    ) -> Result<aura_core::effects::EvaluationResult> {
        if self.config.debug_state_adaptation {
            tracing::debug!("Evaluating property '{}' against simulation state", property.name);
        }

        // Adapt the simulation state to Quint format
        let adapted_state = self.adapt_simulation_state(simulation_state);
        
        // Delegate to the core evaluator
        self.core_evaluator.evaluate_property(property, &adapted_state).await
    }

    /// Run verification for multiple properties against simulation state
    pub async fn verify_simulation_properties(
        &self,
        spec: &PropertySpec,
        simulation_state: &SimulationWorldState,
    ) -> Result<VerificationResult> {
        if self.config.debug_state_adaptation {
            tracing::debug!("Running verification for spec '{}' against simulation state", spec.name);
        }

        // For simulation-specific verification, we adapt the state and use it
        // with the spec's initial state context
        let adapted_state = self.adapt_simulation_state(simulation_state);
        
        // Create a new spec with the adapted state as context
        let simulation_spec = PropertySpec::new(format!("simulation_{}", spec.name))
            .with_context(adapted_state);

        // Add all properties from the original spec
        let mut updated_spec = simulation_spec;
        for property in &spec.properties {
            updated_spec = updated_spec.with_property(property.clone());
        }
        
        // Delegate to core verification
        self.core_evaluator.run_verification(&updated_spec).await
    }
}

impl Default for SimulationPropertyEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

// Forward the core effect traits through the simulation evaluator
#[async_trait]
impl QuintEvaluationEffects for SimulationPropertyEvaluator {
    async fn load_property_spec(&self, spec_source: &str) -> Result<PropertySpec> {
        self.core_evaluator.load_property_spec(spec_source).await
    }

    async fn evaluate_property(&self, property: &Property, state: &Value) -> Result<aura_core::effects::EvaluationResult> {
        self.core_evaluator.evaluate_property(property, state).await
    }

    async fn run_verification(&self, spec: &PropertySpec) -> Result<VerificationResult> {
        self.core_evaluator.run_verification(spec).await
    }

    async fn parse_expression(&self, expression: &str) -> Result<Value> {
        self.core_evaluator.parse_expression(expression).await
    }

    async fn create_initial_state(&self, spec: &PropertySpec) -> Result<Value> {
        self.core_evaluator.create_initial_state(spec).await
    }

    async fn execute_step(&self, current_state: &Value, action: &str) -> Result<Value> {
        self.core_evaluator.execute_step(current_state, action).await
    }
}

#[async_trait]
impl QuintVerificationEffects for SimulationPropertyEvaluator {
    async fn verify_property(&self, property: &Property, state: &Value) -> Result<VerificationResult> {
        self.core_evaluator.verify_property(property, state).await
    }

    async fn generate_counterexample(&self, property: &Property) -> Result<Option<aura_core::effects::Counterexample>> {
        self.core_evaluator.generate_counterexample(property).await
    }

    async fn load_specification(&self, spec_path: &str) -> Result<PropertySpec> {
        self.core_evaluator.load_specification(spec_path).await
    }

    async fn run_model_checking(&self, spec: &PropertySpec, max_steps: usize) -> Result<VerificationResult> {
        self.core_evaluator.run_model_checking(spec, max_steps).await
    }

    async fn validate_specification(&self, spec_source: &str) -> Result<Vec<String>> {
        self.core_evaluator.validate_specification(spec_source).await
    }
}

// Implement PropertyEvaluator for simulation state
#[async_trait]
impl PropertyEvaluator<SimulationWorldState> for SimulationPropertyEvaluator {
    async fn check_property(&self, property: &Property, state: &SimulationWorldState) -> Result<bool> {
        let result = self.evaluate_simulation_property(property, state).await?;
        Ok(result.passed)
    }

    async fn evaluate_property_detailed(&self, property: &Property, state: &SimulationWorldState) -> Result<aura_core::effects::EvaluationResult> {
        self.evaluate_simulation_property(property, state).await
    }
}

/// Simulation world state abstraction for Quint integration
#[derive(Debug, Clone)]
pub struct SimulationWorldState {
    /// Unique identifier for this simulation
    pub simulation_id: String,
    /// Global simulation time
    pub global_time: u64,
    /// Total number of steps executed
    pub step_count: u64,
    /// States of all participants
    pub participant_states: std::collections::HashMap<String, ParticipantState>,
    /// Network state information
    pub network_state: NetworkState,
}

/// State of a single participant in the simulation
#[derive(Debug, Clone)]
pub struct ParticipantState {
    /// Whether this participant is Byzantine
    pub is_byzantine: bool,
    /// Local time for this participant
    pub local_time: u64,
    /// Authority ID if available
    pub authority_id: Option<String>,
}

/// Network state information
#[derive(Debug, Clone)]
pub struct NetworkState {
    /// Total number of messages sent
    pub total_messages: u64,
    /// Number of active network partitions
    pub partition_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::{PropertyKind, Property};
    use std::collections::HashMap;

    fn create_test_simulation_state() -> SimulationWorldState {
        let mut participants = HashMap::new();
        participants.insert("participant_1".to_string(), ParticipantState {
            is_byzantine: false,
            local_time: 100,
            authority_id: Some("auth_1".to_string()),
        });
        participants.insert("participant_2".to_string(), ParticipantState {
            is_byzantine: true,
            local_time: 99,
            authority_id: Some("auth_2".to_string()),
        });

        SimulationWorldState {
            simulation_id: "test_simulation_123".to_string(),
            global_time: 100,
            step_count: 50,
            participant_states: participants,
            network_state: NetworkState {
                total_messages: 200,
                partition_count: 0,
            },
        }
    }

    #[test]
    fn test_simulation_evaluator_creation() {
        let evaluator = SimulationPropertyEvaluator::new();
        assert!(!evaluator.config.debug_state_adaptation);
        assert!(evaluator.config.filter_byzantine_participants);
    }

    #[test]
    fn test_state_adaptation() {
        let evaluator = SimulationPropertyEvaluator::new();
        let simulation_state = create_test_simulation_state();
        
        let adapted = evaluator.adapt_simulation_state(&simulation_state);
        
        // Check that the adapted state has the expected structure
        assert!(adapted.is_object());
        let obj = adapted.as_object().unwrap();
        assert!(obj.contains_key("participants"));
        assert!(obj.contains_key("global_time"));
        assert!(obj.contains_key("network"));
        assert!(obj.contains_key("metadata"));
        
        // Check that Byzantine participants are filtered out
        let participants = &obj["participants"];
        assert!(participants.is_array());
        let participant_array = participants.as_array().unwrap();
        assert_eq!(participant_array.len(), 1); // Only non-Byzantine participant
    }

    #[test]
    fn test_state_adaptation_without_byzantine_filtering() {
        let config = SimulationEvaluatorConfig {
            debug_state_adaptation: false,
            filter_byzantine_participants: false,
            max_simulation_time: None,
        };
        let evaluator = SimulationPropertyEvaluator::with_config(config);
        let simulation_state = create_test_simulation_state();
        
        let adapted = evaluator.adapt_simulation_state(&simulation_state);
        let obj = adapted.as_object().unwrap();
        let participants = &obj["participants"];
        let participant_array = participants.as_array().unwrap();
        
        // Both participants should be included
        assert_eq!(participant_array.len(), 2);
    }

    #[tokio::test]
    async fn test_property_evaluation() {
        let evaluator = SimulationPropertyEvaluator::new();
        let property = Property::new(
            "test_prop",
            "Global Time Monotonic",
            PropertyKind::Invariant,
            "global_time >= 0"
        );
        let simulation_state = create_test_simulation_state();

        let result = evaluator.evaluate_simulation_property(&property, &simulation_state).await;
        assert!(result.is_ok());

        let eval_result = result.unwrap();
        assert_eq!(eval_result.property_id, property.id);
    }

    #[tokio::test]
    async fn test_property_evaluator_trait() {
        let evaluator = SimulationPropertyEvaluator::new();
        let property = Property::new(
            "test_prop",
            "Participant Count",
            PropertyKind::Invariant,
            "participants.length >= 1"
        );
        let simulation_state = create_test_simulation_state();

        let passed = evaluator.check_property(&property, &simulation_state).await;
        assert!(passed.is_ok());
    }
}