//! Generative Simulator for Quint-Driven Simulations
//!
//! This module provides infrastructure for **generative simulations** where Quint
//! specifications drive actual Aura effect execution. It bridges the gap between
//! formal specifications and runtime behavior.
//!
//! # Architecture
//!
//! ```text
//! ITF Trace → GenerativeSimulator → ActionRegistry → Aura Effects → StateMapper
//!     ↓                                   ↓
//! NondetPicks → SeededRandom        Property Evaluation
//! ```
//!
//! # Key Components
//!
//! - `GenerativeSimulator`: Main orchestrator for trace replay and exploration
//! - `SeededRandomProvider`: RandomEffects implementation seeded from Quint nondet picks
//! - `SimulationStep`: Single step in a simulation execution
//! - `GeneratedTestCase`: Test case generated from simulation traces

use async_trait::async_trait;
use aura_core::effects::{RandomCoreEffects, RandomExtendedEffects};
use aura_core::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::action_registry::ActionRegistry;
use super::aura_state_extractors::QuintSimulationState;
use super::itf_fuzzer::ITFTrace;
use aura_core::effects::ActionResult;

// =============================================================================
// Core Types
// =============================================================================

/// Configuration for generative simulation
#[derive(Debug, Clone)]
pub struct GenerativeSimulatorConfig {
    /// Maximum steps to execute in explore mode
    pub max_steps: u32,
    /// Whether to record execution trace
    pub record_trace: bool,
    /// Enable verbose logging
    pub verbose: bool,
    /// Seed for random exploration (None = system entropy)
    pub exploration_seed: Option<u64>,
}

impl Default for GenerativeSimulatorConfig {
    fn default() -> Self {
        Self {
            max_steps: 1000,
            record_trace: true,
            verbose: false,
            exploration_seed: None,
        }
    }
}

/// A single step in the simulation execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationStep {
    /// Step index
    pub index: u64,
    /// Action that was executed
    pub action: String,
    /// Parameters passed to the action
    pub params: Value,
    /// Non-deterministic picks used
    pub nondet_picks: HashMap<String, Value>,
    /// State before action
    pub pre_state: Value,
    /// State after action
    pub post_state: Value,
    /// Whether action succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Result of a generative simulation run
#[derive(Debug, Clone)]
pub struct SimulationResult {
    /// All executed steps
    pub steps: Vec<SimulationStep>,
    /// Final simulation state
    pub final_state: QuintSimulationState,
    /// Whether simulation completed successfully
    pub success: bool,
    /// Total number of steps executed
    pub step_count: u32,
    /// Properties that were violated (if any)
    pub property_violations: Vec<PropertyViolation>,
}

/// A property violation detected during simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyViolation {
    /// Property name
    pub property: String,
    /// Step at which violation occurred
    pub step_index: u64,
    /// State at violation
    pub state: Value,
    /// Description of violation
    pub description: String,
}

/// Generated test case from simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedTestCase {
    /// Test case name
    pub name: String,
    /// Description
    pub description: String,
    /// Sequence of actions to execute
    pub actions: Vec<TestAction>,
    /// Expected final state properties
    pub expected_properties: Vec<String>,
    /// Source trace (if from ITF)
    pub source_trace: Option<String>,
}

/// A test action with parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestAction {
    /// Action name
    pub action: String,
    /// Action parameters
    pub params: Value,
    /// Non-deterministic picks to use
    pub nondet_picks: HashMap<String, Value>,
}

// =============================================================================
// Seeded Random Provider
// =============================================================================

/// Random effects provider seeded from Quint non-deterministic picks
///
/// This allows the simulation to use deterministic randomness that matches
/// what Quint chose during model checking or simulation.
#[derive(Debug)]
pub struct SeededRandomProvider {
    /// Queue of pre-determined random values
    values: Arc<Mutex<Vec<u64>>>,
    /// Fallback seed for when queue is empty
    fallback_seed: u64,
    /// Current position in fallback sequence
    fallback_counter: Arc<Mutex<u64>>,
}

impl SeededRandomProvider {
    /// Create a new seeded random provider
    pub fn new(seed: u64) -> Self {
        Self {
            values: Arc::new(Mutex::new(Vec::new())),
            fallback_seed: seed,
            fallback_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Create from non-deterministic picks
    pub fn from_nondet_picks(picks: &HashMap<String, Value>, seed: u64) -> Self {
        let mut values = Vec::new();

        // Extract integer values from picks as random seeds
        for value in picks.values() {
            if let Some(n) = value.as_u64() {
                values.push(n);
            } else if let Some(n) = value.as_i64() {
                values.push(n as u64);
            }
        }

        Self {
            values: Arc::new(Mutex::new(values)),
            fallback_seed: seed,
            fallback_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Get next value from queue or fallback
    fn next_value(&self) -> u64 {
        let mut values = self.values.lock().unwrap();
        if let Some(v) = values.pop() {
            v
        } else {
            // Use simple LCG fallback
            let mut counter = self.fallback_counter.lock().unwrap();
            *counter = counter.wrapping_add(1);
            self.fallback_seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(*counter)
        }
    }

    /// Add values to the queue
    pub fn push_values(&self, values: impl IntoIterator<Item = u64>) {
        let mut queue = self.values.lock().unwrap();
        queue.extend(values);
    }
}

#[async_trait]
impl RandomCoreEffects for SeededRandomProvider {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut result = Vec::with_capacity(len);
        for _ in 0..len {
            result.push((self.next_value() & 0xFF) as u8);
        }
        result
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut result = [0u8; 32];
        for (i, byte) in result.iter_mut().enumerate() {
            *byte = ((self.next_value() >> (i % 8)) & 0xFF) as u8;
        }
        result
    }

    async fn random_u64(&self) -> u64 {
        self.next_value()
    }
}

// =============================================================================
// Generative Simulator
// =============================================================================

/// Generative simulator that drives Aura effects from Quint specifications
///
/// This is the main orchestrator for generative simulations, providing:
/// - Trace replay from ITF format
/// - State space exploration
/// - Test case generation
pub struct GenerativeSimulator {
    /// Action registry mapping action names to handlers
    registry: ActionRegistry,
    /// Configuration
    config: GenerativeSimulatorConfig,
}

impl GenerativeSimulator {
    /// Create a new generative simulator
    pub fn new(registry: ActionRegistry, config: GenerativeSimulatorConfig) -> Self {
        Self { registry, config }
    }

    /// Create with default configuration
    pub fn with_registry(registry: ActionRegistry) -> Self {
        Self::new(registry, GenerativeSimulatorConfig::default())
    }

    // =========================================================================
    // Trace Replay (5.2)
    // =========================================================================

    /// Replay an ITF trace against the action registry
    ///
    /// This method takes an ITF trace (from Quint simulation or model checking)
    /// and replays each step using the registered action handlers. Non-deterministic
    /// picks from the trace are used to seed randomness.
    ///
    /// # Arguments
    /// * `trace` - The ITF trace to replay
    /// * `initial_state` - Initial simulation state
    ///
    /// # Returns
    /// Result containing the simulation result with all executed steps
    pub async fn replay_trace(
        &self,
        trace: &ITFTrace,
        initial_state: QuintSimulationState,
    ) -> Result<SimulationResult> {
        let mut current_state = initial_state;
        let mut steps = Vec::new();
        let mut property_violations = Vec::new();

        if self.config.verbose {
            tracing::info!("Replaying ITF trace with {} states", trace.states.len());
        }

        for (index, itf_state) in trace.states.iter().enumerate() {
            // Skip initial state (index 0 has no action)
            if index == 0 {
                continue;
            }

            // Extract action and nondet picks
            let action_name = itf_state
                .action_taken
                .as_ref()
                .ok_or_else(|| aura_core::AuraError::invalid("ITF state missing action_taken"))?;

            let nondet_picks = itf_state.nondet_picks.clone().unwrap_or_default();

            // Get action parameters from state variables
            let params = self.extract_action_params(&itf_state.variables);

            // Record pre-state
            let pre_state = current_state.to_quint();

            // Execute action
            let result = self
                .execute_action(action_name, &params, &nondet_picks, &current_state)
                .await;

            match result {
                Ok((_action_result, new_state)) => {
                    let post_state = new_state.to_quint();

                    if self.config.record_trace {
                        steps.push(SimulationStep {
                            index: index as u64,
                            action: action_name.clone(),
                            params: params.clone(),
                            nondet_picks: nondet_picks.clone(),
                            pre_state,
                            post_state: post_state.clone(),
                            success: true,
                            error: None,
                        });
                    }

                    // Check for property violations
                    if let Some(violation) = self.check_step_properties(index as u32, &post_state) {
                        property_violations.push(violation);
                    }

                    current_state = new_state;
                }
                Err(e) => {
                    if self.config.record_trace {
                        steps.push(SimulationStep {
                            index: index as u64,
                            action: action_name.clone(),
                            params: params.clone(),
                            nondet_picks: nondet_picks.clone(),
                            pre_state,
                            post_state: Value::Null,
                            success: false,
                            error: Some(e.to_string()),
                        });
                    }

                    return Ok(SimulationResult {
                        steps,
                        final_state: current_state,
                        success: false,
                        step_count: index as u32,
                        property_violations,
                    });
                }
            }
        }

        Ok(SimulationResult {
            step_count: steps.len() as u32,
            steps,
            final_state: current_state,
            success: true,
            property_violations,
        })
    }

    // =========================================================================
    // State Space Exploration (5.3)
    // =========================================================================

    /// Explore the state space by executing enabled actions
    ///
    /// This method performs bounded exploration of the state space by:
    /// 1. Finding all enabled actions in current state
    /// 2. Randomly selecting one to execute
    /// 3. Repeating until max_steps or no enabled actions
    ///
    /// # Arguments
    /// * `initial_state` - Initial simulation state
    /// * `seed` - Optional seed for deterministic exploration
    ///
    /// # Returns
    /// Result containing the exploration result
    pub async fn explore(
        &self,
        initial_state: QuintSimulationState,
        seed: Option<u64>,
    ) -> Result<SimulationResult> {
        let mut current_state = initial_state;
        let mut steps = Vec::new();
        let mut property_violations = Vec::new();

        let seed = seed.or(self.config.exploration_seed).unwrap_or(42);
        let random = SeededRandomProvider::new(seed);

        if self.config.verbose {
            tracing::info!(
                "Starting exploration with max_steps={}, seed={}",
                self.config.max_steps,
                seed
            );
        }

        for step_index in 0..self.config.max_steps {
            let state_value = current_state.to_quint();

            // Find enabled actions
            let enabled_actions = self.find_enabled_actions(&state_value);

            if enabled_actions.is_empty() {
                if self.config.verbose {
                    tracing::info!("No enabled actions at step {}, stopping", step_index);
                }
                break;
            }

            // Randomly select an action
            let action_idx = random.random_range(0, enabled_actions.len() as u64).await as usize;
            let action_name = &enabled_actions[action_idx];

            // Generate random parameters
            let params = self.generate_random_params(action_name, &random).await;
            let nondet_picks = HashMap::new(); // Empty for exploration

            // Record pre-state
            let pre_state = state_value.clone();

            // Execute action
            let result = self
                .execute_action(action_name, &params, &nondet_picks, &current_state)
                .await;

            match result {
                Ok((_, new_state)) => {
                    let post_state = new_state.to_quint();

                    if self.config.record_trace {
                        steps.push(SimulationStep {
                            index: step_index as u64,
                            action: action_name.clone(),
                            params: params.clone(),
                            nondet_picks: nondet_picks.clone(),
                            pre_state,
                            post_state: post_state.clone(),
                            success: true,
                            error: None,
                        });
                    }

                    // Check for property violations
                    if let Some(violation) =
                        self.check_step_properties(step_index, &post_state)
                    {
                        property_violations.push(violation);
                    }

                    current_state = new_state;
                }
                Err(e) => {
                    if self.config.verbose {
                        tracing::warn!(
                            "Action {} failed at step {}: {}",
                            action_name,
                            step_index,
                            e
                        );
                    }

                    if self.config.record_trace {
                        steps.push(SimulationStep {
                            index: step_index as u64,
                            action: action_name.clone(),
                            params,
                            nondet_picks,
                            pre_state,
                            post_state: Value::Null,
                            success: false,
                            error: Some(e.to_string()),
                        });
                    }
                    // Continue exploring despite failure
                }
            }
        }

        Ok(SimulationResult {
            step_count: steps.len() as u32,
            steps,
            final_state: current_state,
            success: property_violations.is_empty(),
            property_violations,
        })
    }

    // =========================================================================
    // Test Case Generation (5.5)
    // =========================================================================

    /// Generate test cases from a simulation result
    ///
    /// Converts the recorded steps from a simulation into executable test cases
    /// that can be run independently.
    pub fn generate_test_cases(&self, result: &SimulationResult) -> Vec<GeneratedTestCase> {
        let mut test_cases = Vec::new();

        // Generate a single test case from the full trace
        if !result.steps.is_empty() {
            let actions: Vec<TestAction> = result
                .steps
                .iter()
                .filter(|s| s.success)
                .map(|step| TestAction {
                    action: step.action.clone(),
                    params: step.params.clone(),
                    nondet_picks: step.nondet_picks.clone(),
                })
                .collect();

            test_cases.push(GeneratedTestCase {
                name: format!("generated_test_{}", result.step_count),
                description: format!("Generated test with {} successful steps", actions.len()),
                actions,
                expected_properties: vec![],
                source_trace: None,
            });
        }

        // Generate test cases for each property violation
        for violation in &result.property_violations {
            let actions: Vec<TestAction> = result
                .steps
                .iter()
                .take((violation.step_index + 1) as usize)
                .filter(|s| s.success)
                .map(|step| TestAction {
                    action: step.action.clone(),
                    params: step.params.clone(),
                    nondet_picks: step.nondet_picks.clone(),
                })
                .collect();

            test_cases.push(GeneratedTestCase {
                name: format!("violation_{}_{}", violation.property, violation.step_index),
                description: format!(
                    "Test reproducing violation of {} at step {}",
                    violation.property, violation.step_index
                ),
                actions,
                expected_properties: vec![format!("NOT {}", violation.property)],
                source_trace: None,
            });
        }

        test_cases
    }

    /// Generate test case from ITF trace
    pub fn generate_test_from_trace(&self, trace: &ITFTrace) -> GeneratedTestCase {
        let actions: Vec<TestAction> = trace
            .states
            .iter()
            .skip(1) // Skip initial state
            .filter_map(|state| {
                state.action_taken.as_ref().map(|action| TestAction {
                    action: action.clone(),
                    params: self.extract_action_params(&state.variables),
                    nondet_picks: state.nondet_picks.clone().unwrap_or_default(),
                })
            })
            .collect();

        GeneratedTestCase {
            name: format!("itf_test_{}", trace.meta.timestamp),
            description: trace.meta.description.clone(),
            actions,
            expected_properties: vec![],
            source_trace: Some(trace.meta.source.clone()),
        }
    }

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// Execute a single action
    async fn execute_action(
        &self,
        action_name: &str,
        params: &Value,
        nondet_picks: &HashMap<String, Value>,
        current_state: &QuintSimulationState,
    ) -> Result<(ActionResult, QuintSimulationState)> {
        let state_value = current_state.to_quint();

        let action_result = self
            .registry
            .execute(action_name, params, nondet_picks, &state_value)
            .await?;

        // Apply state from resulting_state
        let mut new_state = current_state.clone();
        if let Some(updates) = action_result.resulting_state.as_object() {
            for (key, value) in updates {
                // Apply updates to the simulation state
                // This is simplified - full implementation would properly merge
                if key == "budgets" || key == "tokens" || key == "current_epoch" {
                    new_state
                        .update_from_quint(&serde_json::json!({ key: value }))
                        .map_err(aura_core::AuraError::invalid)?;
                }
            }
        }

        Ok((action_result, new_state))
    }

    /// Find all enabled actions in current state
    fn find_enabled_actions(&self, state: &Value) -> Vec<String> {
        self.registry
            .enabled_actions(state)
            .into_iter()
            .map(|d| d.name)
            .collect()
    }

    /// Extract action parameters from ITF state variables
    fn extract_action_params(&self, variables: &HashMap<String, Value>) -> Value {
        // Extract relevant parameters based on common patterns
        let mut params = serde_json::Map::new();

        for (key, value) in variables {
            // Skip meta fields
            if key.starts_with('#') || key.starts_with("mbt::") {
                continue;
            }
            params.insert(key.clone(), value.clone());
        }

        Value::Object(params)
    }

    /// Generate random parameters for an action
    async fn generate_random_params(
        &self,
        _action_name: &str,
        random: &SeededRandomProvider,
    ) -> Value {
        // Generate basic random parameters
        // Full implementation would use action schema
        serde_json::json!({
            "random_id": random.random_u64().await,
        })
    }

    /// Check for property violations after a step
    fn check_step_properties(
        &self,
        _step_index: u32,
        _state: &Value,
    ) -> Option<PropertyViolation> {
        // Placeholder - full implementation would evaluate properties
        None
    }
}

impl std::fmt::Debug for GenerativeSimulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenerativeSimulator")
            .field("config", &self.config)
            .field("action_count", &self.registry.action_names().len())
            .finish()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quint::action_registry::NoOpHandler;
    use crate::quint::domain_handlers::capability_properties_registry;
    use crate::quint::itf_fuzzer::{ITFMeta, ITFState, ITFStateMeta};

    fn create_test_registry() -> ActionRegistry {
        let mut registry = ActionRegistry::new();
        registry.register(NoOpHandler::new("no_op"));
        registry
    }

    fn create_test_state() -> QuintSimulationState {
        let mut state = QuintSimulationState::new();
        let ctx = aura_core::types::ContextId::new_from_entropy([1u8; 32]);
        let auth = aura_core::types::AuthorityId::new_from_entropy([2u8; 32]);
        state.init_context(ctx, auth, 100);
        state
    }

    #[test]
    fn test_seeded_random_provider() {
        let provider = SeededRandomProvider::new(12345);

        // Values should be deterministic
        let rt = tokio::runtime::Runtime::new().unwrap();
        let v1 = rt.block_on(provider.random_u64());
        let v2 = rt.block_on(provider.random_u64());

        // Different values (not same)
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_seeded_random_from_picks() {
        let mut picks = HashMap::new();
        picks.insert("choice1".to_string(), serde_json::json!(42));
        picks.insert("choice2".to_string(), serde_json::json!(100));

        let provider = SeededRandomProvider::from_nondet_picks(&picks, 12345);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let values: Vec<u64> = (0..5).map(|_| rt.block_on(provider.random_u64())).collect();

        // First values should come from picks
        assert!(values.contains(&42) || values.contains(&100));
    }

    #[test]
    fn test_seeded_random_range() {
        let provider = SeededRandomProvider::new(12345);

        let rt = tokio::runtime::Runtime::new().unwrap();
        for _ in 0..100 {
            let v = rt.block_on(provider.random_range(10, 20));
            assert!((10..20).contains(&v));
        }
    }

    #[test]
    fn test_generative_simulator_creation() {
        let registry = create_test_registry();
        let config = GenerativeSimulatorConfig::default();

        let simulator = GenerativeSimulator::new(registry, config);

        assert_eq!(simulator.config.max_steps, 1000);
        assert!(simulator.config.record_trace);
    }

    #[tokio::test]
    async fn test_explore_basic() {
        let registry = create_test_registry();
        let config = GenerativeSimulatorConfig {
            max_steps: 10,
            record_trace: true,
            verbose: false,
            exploration_seed: Some(42),
        };

        let simulator = GenerativeSimulator::new(registry, config);
        let initial_state = create_test_state();

        let result = simulator.explore(initial_state, Some(42)).await.unwrap();

        // Should complete successfully
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_explore_with_capability_registry() {
        let registry = capability_properties_registry();
        let config = GenerativeSimulatorConfig {
            max_steps: 5,
            record_trace: true,
            verbose: false,
            exploration_seed: Some(12345),
        };

        let simulator = GenerativeSimulator::new(registry, config);
        let initial_state = create_test_state();

        let result = simulator.explore(initial_state, Some(12345)).await.unwrap();

        // Should complete (may or may not execute actions depending on state)
        assert!(result.step_count <= 5);
    }

    #[test]
    fn test_generate_test_cases() {
        let registry = create_test_registry();
        let simulator = GenerativeSimulator::with_registry(registry);

        let result = SimulationResult {
            steps: vec![
                SimulationStep {
                    index: 0,
                    action: "initContext".to_string(),
                    params: serde_json::json!({}),
                    nondet_picks: HashMap::new(),
                    pre_state: serde_json::json!({}),
                    post_state: serde_json::json!({"budgets": {}}),
                    success: true,
                    error: None,
                },
                SimulationStep {
                    index: 1,
                    action: "initAuthority".to_string(),
                    params: serde_json::json!({}),
                    nondet_picks: HashMap::new(),
                    pre_state: serde_json::json!({"budgets": {}}),
                    post_state: serde_json::json!({"budgets": {}, "tokens": {}}),
                    success: true,
                    error: None,
                },
            ],
            final_state: create_test_state(),
            success: true,
            step_count: 2,
            property_violations: vec![],
        };

        let test_cases = simulator.generate_test_cases(&result);

        assert_eq!(test_cases.len(), 1);
        assert_eq!(test_cases[0].actions.len(), 2);
    }

    #[test]
    fn test_generate_test_from_trace() {
        let registry = create_test_registry();
        let simulator = GenerativeSimulator::with_registry(registry);

        let trace = ITFTrace {
            meta: ITFMeta {
                format: "ITF".to_string(),
                format_description: "https://apalache.informal.systems/docs/adr/015adr-trace.html"
                    .to_string(),
                source: "test.qnt".to_string(),
                status: "ok".to_string(),
                description: "Test trace".to_string(),
                timestamp: 1234567890,
            },
            params: vec![],
            vars: vec!["budgets".to_string(), "tokens".to_string()],
            states: vec![
                ITFState {
                    meta: ITFStateMeta { index: 0 },
                    variables: HashMap::new(),
                    action_taken: None,
                    nondet_picks: None,
                },
                ITFState {
                    meta: ITFStateMeta { index: 1 },
                    variables: HashMap::new(),
                    action_taken: Some("initContext".to_string()),
                    nondet_picks: Some(HashMap::new()),
                },
            ],
            loop_index: None,
        };

        let test_case = simulator.generate_test_from_trace(&trace);

        assert_eq!(test_case.name, "itf_test_1234567890");
        assert_eq!(test_case.actions.len(), 1);
        assert_eq!(test_case.actions[0].action, "initContext");
    }

    #[test]
    fn test_find_enabled_actions() {
        let registry = create_test_registry();
        let simulator = GenerativeSimulator::with_registry(registry);

        let state = serde_json::json!({});
        let enabled = simulator.find_enabled_actions(&state);

        // NoOpHandler is always enabled
        assert!(enabled.contains(&"no_op".to_string()));
    }

    #[test]
    fn test_extract_action_params() {
        let registry = create_test_registry();
        let simulator = GenerativeSimulator::with_registry(registry);

        let mut variables = HashMap::new();
        variables.insert("ctx".to_string(), serde_json::json!("context-123"));
        variables.insert("limit".to_string(), serde_json::json!(100));
        variables.insert("#meta".to_string(), serde_json::json!({"index": 0}));
        variables.insert("mbt::actionTaken".to_string(), serde_json::json!("init"));

        let params = simulator.extract_action_params(&variables);

        let obj = params.as_object().unwrap();
        assert!(obj.contains_key("ctx"));
        assert!(obj.contains_key("limit"));
        assert!(!obj.contains_key("#meta"));
        assert!(!obj.contains_key("mbt::actionTaken"));
    }
}
