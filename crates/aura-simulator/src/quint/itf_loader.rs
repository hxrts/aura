//! ITF Trace Loader for Model-Based Testing
//!
//! Provides functionality to load ITF traces from disk and convert them to
//! executable test cases for the GenerativeSimulator.

use std::path::Path;

use serde_json::Value;

use super::aura_state_extractors::QuintSimulationState;
use super::itf_fuzzer::{ITFMeta, ITFState, ITFStateMeta, ITFTrace};
use aura_core::{AuraError, Result};
use std::collections::HashMap;

/// Loader for ITF traces from disk
pub struct ITFLoader;

impl ITFLoader {
    /// Load an ITF trace from a JSON file
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<ITFTrace> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| AuraError::invalid(format!("Failed to read ITF file: {}", e)))?;
        Self::parse_json(&content)
    }

    /// Parse ITF trace from JSON string
    pub fn parse_json(json: &str) -> Result<ITFTrace> {
        serde_json::from_str(json)
            .map_err(|e| AuraError::invalid(format!("Failed to parse ITF JSON: {}", e)))
    }

    /// Extract state sequence from ITF trace for state-diff based replay
    pub fn extract_state_sequence(trace: &ITFTrace) -> Vec<HashMap<String, Value>> {
        trace.states.iter().map(|s| s.variables.clone()).collect()
    }

    /// Infer action from state diff between two consecutive states
    ///
    /// This is used when traces don't have `mbt::actionTaken` metadata.
    /// Returns the inferred action name and parameters based on what changed.
    pub fn infer_action_from_diff(
        _prev_state: &HashMap<String, Value>,
        next_state: &HashMap<String, Value>,
        vars: &[String],
    ) -> InferredAction {
        // Analyze which state variables changed to infer the action
        let mut changed_vars = Vec::new();
        for var in vars {
            if let Some(value) = next_state.get(var) {
                changed_vars.push((var.clone(), value.clone()));
            }
        }

        // Infer action based on common patterns in our specs
        let action_name = Self::infer_action_name(&changed_vars);
        let params = Self::extract_params_from_state(next_state);

        InferredAction {
            name: action_name,
            params,
            changed_vars,
        }
    }

    fn infer_action_name(changed_vars: &[(String, Value)]) -> String {
        // Pattern matching based on our protocol specs
        for (var, _value) in changed_vars {
            match var.as_str() {
                // Capability properties spec
                "budgets" => return "chargeFlowBudget".to_string(),
                "tokens" => return "createToken".to_string(),
                "evaluationResults" => return "evaluateCapability".to_string(),

                // FROST protocol spec
                "frostSessions" => return "initiateFROSTSession".to_string(),
                "commitments" => return "submitCommitment".to_string(),
                "signatureShares" => return "submitSignatureShare".to_string(),

                // DKG protocol spec
                "participants" => return "registerParticipant".to_string(),
                "verificationMaps" => return "submitVerification".to_string(),
                "lifecycleStatus" => return "updateLifecycle".to_string(),

                // Consensus protocol spec
                "instances" => return "startConsensus".to_string(),
                "committedFacts" => return "commitFact".to_string(),
                "proposals" => return "submitWitnessShare".to_string(),

                // Anti-entropy spec
                "nodeStates" => return "localWrite".to_string(),
                "pendingDeltas" => return "createDelta".to_string(),
                "syncSessions" => return "initiateSyncSession".to_string(),

                // Epochs spec
                "epochs" => return "transitionEpoch".to_string(),
                "receipts" => return "createReceipt".to_string(),
                "operations" => return "createOperation".to_string(),

                // Cross-interaction spec
                "recoveryInstances" => return "startRecovery".to_string(),
                "consensusInstances" => return "startConcurrentConsensus".to_string(),
                "contextStates" => return "updateContext".to_string(),

                _ => continue,
            }
        }

        // Default to step if no specific action detected
        "step".to_string()
    }

    fn extract_params_from_state(state: &HashMap<String, Value>) -> Value {
        // Extract common parameter patterns from state
        let mut params = serde_json::Map::new();

        // Extract identifiers
        for (key, value) in state {
            if key.ends_with("Id") || key.ends_with("_id") || key == "threshold" || key == "epoch" {
                params.insert(key.clone(), value.clone());
            }
        }

        Value::Object(params)
    }

    /// Convert ITF trace to simulation-ready state sequence with inferred actions
    pub fn to_simulation_sequence(trace: &ITFTrace) -> SimulationSequence {
        let mut steps = Vec::new();
        let states = Self::extract_state_sequence(trace);

        for i in 1..states.len() {
            let inferred = Self::infer_action_from_diff(&states[i - 1], &states[i], &trace.vars);

            steps.push(SimulationSequenceStep {
                index: i,
                action: inferred.name,
                params: inferred.params,
                pre_state: states[i - 1].clone(),
                post_state: states[i].clone(),
            });
        }

        SimulationSequence {
            source: trace.meta.source.clone(),
            description: trace.meta.description.clone(),
            initial_state: states.first().cloned().unwrap_or_default(),
            steps,
        }
    }

    /// Create QuintSimulationState from ITF state variables
    pub fn to_quint_simulation_state(
        variables: &HashMap<String, Value>,
    ) -> Result<QuintSimulationState> {
        let mut state = QuintSimulationState::new();

        // Initialize with a default context and authority if not present
        let ctx = aura_core::types::ContextId::new_from_entropy([1u8; 32]);
        let auth = aura_core::types::AuthorityId::new_from_entropy([2u8; 32]);

        // Extract budget limit from state if present
        let limit = variables
            .get("flowBudgetLimit")
            .and_then(|v| v.as_u64())
            .unwrap_or(100);

        state.init_context(ctx, auth, limit);

        // Apply additional state from ITF variables
        state
            .update_from_quint(&serde_json::to_value(variables).unwrap_or(Value::Null))
            .map_err(AuraError::invalid)?;

        Ok(state)
    }
}

/// An action inferred from state diff
#[derive(Debug, Clone)]
pub struct InferredAction {
    /// Inferred action name
    pub name: String,
    /// Inferred parameters
    pub params: Value,
    /// Variables that changed
    pub changed_vars: Vec<(String, Value)>,
}

/// A sequence of simulation steps derived from ITF trace
#[derive(Debug, Clone)]
pub struct SimulationSequence {
    /// Source file
    pub source: String,
    /// Description
    pub description: String,
    /// Initial state
    pub initial_state: HashMap<String, Value>,
    /// Steps to execute
    pub steps: Vec<SimulationSequenceStep>,
}

/// Single step in simulation sequence
#[derive(Debug, Clone)]
pub struct SimulationSequenceStep {
    /// Step index
    pub index: usize,
    /// Action to execute
    pub action: String,
    /// Action parameters
    pub params: Value,
    /// State before action
    pub pre_state: HashMap<String, Value>,
    /// Expected state after action
    pub post_state: HashMap<String, Value>,
}

/// Builder for creating ITF traces programmatically (for testing)
pub struct ITFTraceBuilder {
    vars: Vec<String>,
    states: Vec<ITFState>,
    source: String,
    description: String,
}

impl ITFTraceBuilder {
    /// Create a new ITF trace builder
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            vars: Vec::new(),
            states: Vec::new(),
            source: source.into(),
            description: String::new(),
        }
    }

    /// Set trace description
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Declare state variables
    pub fn vars(mut self, vars: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.vars = vars.into_iter().map(|v| v.into()).collect();
        self
    }

    /// Add a state to the trace
    pub fn add_state(mut self, variables: HashMap<String, Value>) -> Self {
        let index = self.states.len();
        self.states.push(ITFState {
            meta: ITFStateMeta { index },
            variables,
            action_taken: None,
            nondet_picks: None,
        });
        self
    }

    /// Add a state with action metadata (MBT style)
    pub fn add_state_with_action(
        mut self,
        variables: HashMap<String, Value>,
        action: impl Into<String>,
        nondet_picks: Option<HashMap<String, Value>>,
    ) -> Self {
        let index = self.states.len();
        self.states.push(ITFState {
            meta: ITFStateMeta { index },
            variables,
            action_taken: Some(action.into()),
            nondet_picks,
        });
        self
    }

    /// Build the ITF trace
    pub fn build(self) -> ITFTrace {
        ITFTrace {
            meta: ITFMeta {
                format: "ITF".to_string(),
                format_description: "https://apalache-mc.org/docs/adr/015adr-trace.html"
                    .to_string(),
                source: self.source,
                status: "ok".to_string(),
                description: self.description,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            },
            params: Vec::new(),
            vars: self.vars,
            states: self.states,
            loop_index: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_consensus_trace() {
        // Test loading the consensus trace we generated
        let trace_path = std::path::Path::new("../../traces/consensus.itf.json");
        if trace_path.exists() {
            let trace = ITFLoader::load_from_file(trace_path).expect("Failed to load trace");
            assert!(!trace.states.is_empty());
            assert!(trace.vars.contains(&"instances".to_string()));
        }
    }

    #[test]
    fn test_infer_action_from_consensus_state() {
        let mut prev_state = HashMap::new();
        prev_state.insert("instances".to_string(), serde_json::json!({}));

        let mut next_state = HashMap::new();
        next_state.insert(
            "instances".to_string(),
            serde_json::json!({"cns1": {"phase": "FastPathActive"}}),
        );

        let inferred =
            ITFLoader::infer_action_from_diff(&prev_state, &next_state, &["instances".to_string()]);

        assert_eq!(inferred.name, "startConsensus");
    }

    #[test]
    fn test_to_simulation_sequence() {
        let trace = ITFTraceBuilder::new("test.qnt")
            .vars(["instances", "committedFacts"])
            .add_state({
                let mut m = HashMap::new();
                m.insert("instances".to_string(), serde_json::json!({}));
                m.insert("committedFacts".to_string(), serde_json::json!([]));
                m
            })
            .add_state({
                let mut m = HashMap::new();
                m.insert(
                    "instances".to_string(),
                    serde_json::json!({"cns1": {"phase": "FastPathActive"}}),
                );
                m.insert("committedFacts".to_string(), serde_json::json!([]));
                m
            })
            .build();

        let sequence = ITFLoader::to_simulation_sequence(&trace);

        assert_eq!(sequence.steps.len(), 1);
        assert_eq!(sequence.steps[0].action, "startConsensus");
    }

    #[test]
    fn test_builder_with_actions() {
        let trace = ITFTraceBuilder::new("test.qnt")
            .description("Test trace with actions")
            .vars(["x", "y"])
            .add_state_with_action(
                {
                    let mut m = HashMap::new();
                    m.insert("x".to_string(), serde_json::json!(0));
                    m.insert("y".to_string(), serde_json::json!(0));
                    m
                },
                "init",
                None,
            )
            .add_state_with_action(
                {
                    let mut m = HashMap::new();
                    m.insert("x".to_string(), serde_json::json!(1));
                    m.insert("y".to_string(), serde_json::json!(0));
                    m
                },
                "incrementX",
                Some({
                    let mut picks = HashMap::new();
                    picks.insert("delta".to_string(), serde_json::json!(1));
                    picks
                }),
            )
            .build();

        assert_eq!(trace.states.len(), 2);
        assert_eq!(trace.states[1].action_taken, Some("incrementX".to_string()));
        assert!(trace.states[1].nondet_picks.is_some());
    }
}
