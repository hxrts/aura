//! Simulation-specific Quint property evaluator
//!
//! This module provides simulation-specific wrappers around the core Quint
//! functionality, adapting simulation state to work with Quint property evaluation.

use async_trait::async_trait;
use aura_core::effects::{
    Property, PropertyEvaluator, PropertySpec, QuintEvaluationEffects, QuintVerificationEffects,
    VerificationResult,
};
use aura_core::Result;
use aura_quint::QuintEffectHandler;
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
    pub fn with_evaluator(
        core_evaluator: QuintEffectHandler,
        config: SimulationEvaluatorConfig,
    ) -> Self {
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
                participant_value
                    .insert("is_byzantine".to_string(), Value::Bool(state.is_byzantine));
                participant_value.insert(
                    "local_time".to_string(),
                    Value::Number(state.local_time.into()),
                );

                // Add authority information
                if let Some(authority_id) = &state.authority_id {
                    participant_value.insert(
                        "authority_id".to_string(),
                        Value::String(authority_id.clone()),
                    );
                }

                Value::Object(participant_value)
            })
            .collect();

        adapted_state.insert("participants".to_string(), Value::Array(participants));

        // Extract global state information
        adapted_state.insert(
            "global_time".to_string(),
            Value::Number(simulation_state.global_time.into()),
        );
        adapted_state.insert(
            "step_count".to_string(),
            Value::Number(simulation_state.step_count.into()),
        );

        // Extract network state
        let mut network_state = Map::new();
        network_state.insert(
            "message_count".to_string(),
            Value::Number(simulation_state.network_state.total_messages.into()),
        );
        network_state.insert(
            "partition_count".to_string(),
            Value::Number(simulation_state.network_state.partition_count.into()),
        );
        adapted_state.insert("network".to_string(), Value::Object(network_state));

        // Add simulation metadata
        let mut metadata = Map::new();
        metadata.insert(
            "simulation_id".to_string(),
            Value::String(simulation_state.simulation_id.clone()),
        );
        if let Some(max_time) = self.config.max_simulation_time {
            metadata.insert(
                "max_simulation_time".to_string(),
                Value::Number(max_time.into()),
            );
        }
        adapted_state.insert("metadata".to_string(), Value::Object(metadata));

        if self.config.debug_state_adaptation {
            tracing::debug!(
                "Adapted state contains {} top-level keys",
                adapted_state.len()
            );
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
            tracing::debug!(
                "Evaluating property '{}' against simulation state",
                property.name
            );
        }

        // Adapt the simulation state to Quint format
        let adapted_state = self.adapt_simulation_state(simulation_state);

        // Delegate to the core evaluator
        self.core_evaluator
            .evaluate_property(property, &adapted_state)
            .await
    }

    /// Run verification for multiple properties against simulation state
    pub async fn verify_simulation_properties(
        &self,
        spec: &PropertySpec,
        simulation_state: &SimulationWorldState,
    ) -> Result<VerificationResult> {
        if self.config.debug_state_adaptation {
            tracing::debug!(
                "Running verification for spec '{}' against simulation state",
                spec.name
            );
        }

        // For simulation-specific verification, we adapt the state and use it
        // with the spec's initial state context
        let adapted_state = self.adapt_simulation_state(simulation_state);

        // Create a new spec with the adapted state as context
        let simulation_spec =
            PropertySpec::new(format!("simulation_{}", spec.name)).with_context(adapted_state);

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

    async fn evaluate_property(
        &self,
        property: &Property,
        state: &Value,
    ) -> Result<aura_core::effects::EvaluationResult> {
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
        self.core_evaluator
            .execute_step(current_state, action)
            .await
    }
}

#[async_trait]
impl QuintVerificationEffects for SimulationPropertyEvaluator {
    async fn verify_property(
        &self,
        property: &Property,
        state: &Value,
    ) -> Result<VerificationResult> {
        self.core_evaluator.verify_property(property, state).await
    }

    async fn generate_counterexample(
        &self,
        property: &Property,
    ) -> Result<Option<aura_core::effects::Counterexample>> {
        self.core_evaluator.generate_counterexample(property).await
    }

    async fn load_specification(&self, spec_path: &str) -> Result<PropertySpec> {
        self.core_evaluator.load_specification(spec_path).await
    }

    async fn run_model_checking(
        &self,
        spec: &PropertySpec,
        max_steps: usize,
    ) -> Result<VerificationResult> {
        self.core_evaluator
            .run_model_checking(spec, max_steps)
            .await
    }

    async fn validate_specification(&self, spec_source: &str) -> Result<Vec<String>> {
        self.core_evaluator
            .validate_specification(spec_source)
            .await
    }
}

// Implement PropertyEvaluator for simulation state
#[async_trait]
impl PropertyEvaluator<SimulationWorldState> for SimulationPropertyEvaluator {
    async fn check_property(
        &self,
        property: &Property,
        state: &SimulationWorldState,
    ) -> Result<bool> {
        let result = self.evaluate_simulation_property(property, state).await?;
        Ok(result.passed)
    }

    async fn evaluate_property_detailed(
        &self,
        property: &Property,
        state: &SimulationWorldState,
    ) -> Result<aura_core::effects::EvaluationResult> {
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

// =============================================================================
// Extended State Integration (Phase 4)
// =============================================================================

use super::aura_state_extractors::QuintSimulationState;

/// Extended evaluator that supports capability properties simulation state
impl SimulationPropertyEvaluator {
    /// Adapt capability simulation state to Quint-compatible format
    ///
    /// This method uses the structured `QuintSimulationState` for capability
    /// properties evaluation, providing type-safe state extraction.
    pub fn adapt_capability_state(&self, capability_state: &QuintSimulationState) -> Value {
        if self.config.debug_state_adaptation {
            tracing::debug!("Adapting capability simulation state to Quint format");
        }
        capability_state.to_quint()
    }

    /// Evaluate a property against capability simulation state
    pub async fn evaluate_capability_property(
        &self,
        property: &Property,
        capability_state: &QuintSimulationState,
    ) -> Result<aura_core::effects::EvaluationResult> {
        if self.config.debug_state_adaptation {
            tracing::debug!(
                "Evaluating property '{}' against capability state",
                property.name
            );
        }

        let adapted_state = self.adapt_capability_state(capability_state);
        self.core_evaluator
            .evaluate_property(property, &adapted_state)
            .await
    }

    /// Run verification against capability simulation state
    pub async fn verify_capability_properties(
        &self,
        spec: &PropertySpec,
        capability_state: &QuintSimulationState,
    ) -> Result<VerificationResult> {
        if self.config.debug_state_adaptation {
            tracing::debug!("Running capability verification for spec '{}'", spec.name);
        }

        let adapted_state = self.adapt_capability_state(capability_state);
        let capability_spec =
            PropertySpec::new(format!("capability_{}", spec.name)).with_context(adapted_state);

        let mut updated_spec = capability_spec;
        for property in &spec.properties {
            updated_spec = updated_spec.with_property(property.clone());
        }

        self.core_evaluator.run_verification(&updated_spec).await
    }
}

// =============================================================================
// StateMapper Integration (Phase 4.5)
// =============================================================================

use super::state_mapper::SimulationStateMapper;

/// StateMapper-based evaluator for bidirectional state synchronization
impl SimulationPropertyEvaluator {
    /// Create a state mapper initialized with capability simulation state
    ///
    /// This mapper can be used to:
    /// 1. Load Aura state into Quint format
    /// 2. Execute Quint actions
    /// 3. Extract non-deterministic updates back to Aura state
    pub fn create_state_mapper(
        &self,
        capability_state: &QuintSimulationState,
    ) -> SimulationStateMapper {
        let mut mapper = SimulationStateMapper::new();
        mapper.load_from_simulation_state(capability_state);
        mapper
    }

    /// Evaluate property using StateMapper for state management
    ///
    /// This method provides full bidirectional state synchronization:
    /// - Initial state is loaded from `QuintSimulationState`
    /// - Property evaluation uses the adapted Quint state
    /// - Non-deterministic updates (if any) are captured in the mapper
    pub async fn evaluate_with_mapper(
        &self,
        property: &Property,
        mapper: &SimulationStateMapper,
    ) -> Result<aura_core::effects::EvaluationResult> {
        let adapted_state = mapper.to_quint();
        self.core_evaluator
            .evaluate_property(property, &adapted_state)
            .await
    }

    /// Apply non-deterministic updates from Quint execution to state mapper
    ///
    /// After Quint executes an action that makes non-deterministic choices,
    /// this method applies those updates to the mapper so they can be
    /// extracted back to Aura state.
    pub fn apply_nondet_to_mapper(
        &self,
        mapper: &mut SimulationStateMapper,
        quint_updates: &Value,
    ) -> Result<Vec<String>> {
        mapper.apply_nondet_updates(quint_updates)
    }

    /// Extract updated state from mapper back to QuintSimulationState
    ///
    /// This is the reverse of `create_state_mapper`, used after Quint
    /// execution to get the updated Aura state.
    pub fn extract_state_from_mapper(
        &self,
        mapper: &SimulationStateMapper,
    ) -> Result<QuintSimulationState> {
        mapper.extract_to_simulation_state()
    }

    /// Full evaluation cycle with state synchronization
    ///
    /// This is a convenience method that:
    /// 1. Creates a mapper from initial state
    /// 2. Evaluates the property
    /// 3. Optionally applies updates
    /// 4. Returns both result and updated state
    pub async fn evaluate_with_state_sync(
        &self,
        property: &Property,
        initial_state: &QuintSimulationState,
        quint_updates: Option<&Value>,
    ) -> Result<(aura_core::effects::EvaluationResult, QuintSimulationState)> {
        let mut mapper = self.create_state_mapper(initial_state);

        // Apply any updates from Quint execution
        if let Some(updates) = quint_updates {
            self.apply_nondet_to_mapper(&mut mapper, updates)?;
        }

        // Evaluate property
        let result = self.evaluate_with_mapper(property, &mapper).await?;

        // Extract final state
        let final_state = self.extract_state_from_mapper(&mapper)?;

        Ok((result, final_state))
    }
}

/// Capability property evaluator trait implementation
#[async_trait]
impl PropertyEvaluator<QuintSimulationState> for SimulationPropertyEvaluator {
    async fn check_property(
        &self,
        property: &Property,
        state: &QuintSimulationState,
    ) -> Result<bool> {
        let result = self.evaluate_capability_property(property, state).await?;
        Ok(result.passed)
    }

    async fn evaluate_property_detailed(
        &self,
        property: &Property,
        state: &QuintSimulationState,
    ) -> Result<aura_core::effects::EvaluationResult> {
        self.evaluate_capability_property(property, state).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::{Property, PropertyKind};
    use std::collections::HashMap;

    fn create_test_simulation_state() -> SimulationWorldState {
        let mut participants = HashMap::new();
        participants.insert(
            "participant_1".to_string(),
            ParticipantState {
                is_byzantine: false,
                local_time: 100,
                authority_id: Some("auth_1".to_string()),
            },
        );
        participants.insert(
            "participant_2".to_string(),
            ParticipantState {
                is_byzantine: true,
                local_time: 99,
                authority_id: Some("auth_2".to_string()),
            },
        );

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
            "global_time >= 0",
        );
        let simulation_state = create_test_simulation_state();

        let result = evaluator
            .evaluate_simulation_property(&property, &simulation_state)
            .await;
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
            "participants.length >= 1",
        );
        let simulation_state = create_test_simulation_state();

        let passed = evaluator.check_property(&property, &simulation_state).await;
        assert!(passed.is_ok());
    }

    // =========================================================================
    // Capability State Integration Tests (Phase 4)
    // =========================================================================

    use crate::quint::aura_state_extractors::CapabilityToken;
    use aura_core::types::{epochs::Epoch, AuthorityId, ContextId, FlowBudget};

    fn create_test_capability_state() -> QuintSimulationState {
        let mut state = QuintSimulationState::new();

        let ctx = ContextId::new_from_entropy([1u8; 32]);
        let auth1 = AuthorityId::new_from_entropy([2u8; 32]);
        let auth2 = AuthorityId::new_from_entropy([3u8; 32]);

        // Initialize context
        state.current_epoch.insert(ctx, 0);
        state.budgets.insert(
            ctx,
            FlowBudget {
                limit: 100,
                spent: 25,
                epoch: Epoch::new(0),
            },
        );

        // Initialize authorities with tokens
        state.tokens.insert(auth1, CapabilityToken::new(4)); // CAP_FULL
        state.tokens.insert(auth2, CapabilityToken::new(3)); // CAP_READ

        state
    }

    #[test]
    fn test_adapt_capability_state() {
        let evaluator = SimulationPropertyEvaluator::new();
        let capability_state = create_test_capability_state();

        let adapted = evaluator.adapt_capability_state(&capability_state);

        // Verify structure
        assert!(adapted.is_object());
        let obj = adapted.as_object().unwrap();
        assert!(obj.contains_key("budgets"));
        assert!(obj.contains_key("tokens"));
        assert!(obj.contains_key("current_epoch"));
        assert!(obj.contains_key("completed_ops"));
    }

    #[tokio::test]
    async fn test_capability_property_evaluation() {
        let evaluator = SimulationPropertyEvaluator::new();
        let property = Property::new(
            "spent_within_limit",
            "Spent Within Limit",
            PropertyKind::Invariant,
            "true", // Simplified - real Quint expression would be more complex
        );
        let capability_state = create_test_capability_state();

        let result = evaluator
            .evaluate_capability_property(&property, &capability_state)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_capability_property_evaluator_trait() {
        let evaluator = SimulationPropertyEvaluator::new();
        let property = Property::new(
            "test_invariant",
            "Test Invariant",
            PropertyKind::Invariant,
            "true",
        );
        let capability_state = create_test_capability_state();

        // Use PropertyEvaluator trait
        let result: Result<bool> = PropertyEvaluator::<QuintSimulationState>::check_property(
            &evaluator,
            &property,
            &capability_state,
        )
        .await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_capability_state_with_operations() {
        let mut state = QuintSimulationState::new();
        let ctx = ContextId::new_from_entropy([1u8; 32]);
        let auth1 = AuthorityId::new_from_entropy([2u8; 32]);
        let auth2 = AuthorityId::new_from_entropy([3u8; 32]);

        // Initialize
        state.init_context(ctx, auth1, 100);
        state.init_authority(auth1, 4);

        // Complete transport op
        let result = state.complete_transport_op(&ctx, &auth1, &auth2, 10);
        assert!(result.is_ok());

        // Verify state was updated
        assert_eq!(state.budgets.get(&ctx).unwrap().spent, 10);
        assert_eq!(state.completed_ops.len(), 1);

        // Verify Quint representation
        let quint = state.to_quint();
        let ops = quint["completed_ops"].as_array().unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["cost"], 10);
        assert_eq!(ops[0]["charged"], true);
    }

    // =========================================================================
    // StateMapper Integration Tests (Phase 4.5)
    // =========================================================================

    #[test]
    fn test_create_state_mapper() {
        let evaluator = SimulationPropertyEvaluator::new();
        let capability_state = create_test_capability_state();

        let mapper = evaluator.create_state_mapper(&capability_state);

        // Verify mapper has loaded state
        assert!(mapper.has_variable("budgets"));
        assert!(mapper.has_variable("tokens"));
        assert!(mapper.has_variable("simulation_state"));
    }

    #[test]
    fn test_extract_state_from_mapper() {
        let evaluator = SimulationPropertyEvaluator::new();
        let original_state = create_test_capability_state();

        let mapper = evaluator.create_state_mapper(&original_state);
        let extracted = evaluator.extract_state_from_mapper(&mapper).unwrap();

        // Verify extracted state matches original
        assert_eq!(original_state.budgets.len(), extracted.budgets.len());
        assert_eq!(original_state.tokens.len(), extracted.tokens.len());
    }

    #[test]
    fn test_apply_nondet_to_mapper() {
        let evaluator = SimulationPropertyEvaluator::new();
        let capability_state = create_test_capability_state();

        let mut mapper = evaluator.create_state_mapper(&capability_state);

        // Simulate non-deterministic update
        let updates = serde_json::json!({
            "tokens": {
                "test-auth": {"cap_level": 2, "attenuation_count": 1}
            }
        });

        let updated_vars = evaluator
            .apply_nondet_to_mapper(&mut mapper, &updates)
            .unwrap();
        assert!(updated_vars.contains(&"tokens".to_string()));
    }

    #[tokio::test]
    async fn test_evaluate_with_mapper() {
        let evaluator = SimulationPropertyEvaluator::new();
        let property = Property::new(
            "budget_check",
            "Budget Check",
            PropertyKind::Invariant,
            "true",
        );
        let capability_state = create_test_capability_state();

        let mapper = evaluator.create_state_mapper(&capability_state);
        let result = evaluator.evaluate_with_mapper(&property, &mapper).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_evaluate_with_state_sync() {
        let evaluator = SimulationPropertyEvaluator::new();
        let property = Property::new(
            "sync_test",
            "State Sync Test",
            PropertyKind::Invariant,
            "true",
        );
        let initial_state = create_test_capability_state();

        // Test without updates
        let (result, final_state) = evaluator
            .evaluate_with_state_sync(&property, &initial_state, None)
            .await
            .unwrap();

        assert!(result.passed);
        assert_eq!(initial_state.budgets.len(), final_state.budgets.len());
    }

    #[tokio::test]
    async fn test_evaluate_with_state_sync_and_updates() {
        let evaluator = SimulationPropertyEvaluator::new();
        let property = Property::new(
            "sync_update_test",
            "State Sync with Updates",
            PropertyKind::Invariant,
            "true",
        );
        let initial_state = create_test_capability_state();

        // Get an existing authority ID for the update
        let auth = AuthorityId::new_from_entropy([2u8; 32]);

        // Provide updates with valid authority format
        let updates = serde_json::json!({
            "tokens": {
                auth.to_string(): {"cap_level": 2, "attenuation_count": 1}
            }
        });

        let (result, final_state) = evaluator
            .evaluate_with_state_sync(&property, &initial_state, Some(&updates))
            .await
            .unwrap();

        assert!(result.passed);

        // Verify the update was applied
        let token = final_state.tokens.get(&auth).unwrap();
        assert_eq!(token.cap_level, 2);
        assert_eq!(token.attenuation_count, 1);
    }
}
