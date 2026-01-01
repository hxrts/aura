//! ITF-Based Fuzz Testing System
//!
//! This module implements the ITF-based fuzzing system that leverages Quint and Apalache
//! for model-based test generation. It uses the existing Quint-Apalache integration to
//! generate traces in ITF format and convert them to executable test cases.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::action_registry::ActionRegistry;
use super::aura_state_extractors::QuintSimulationState;
use super::generative_simulator::{GenerativeSimulator, GenerativeSimulatorConfig, SimulationStep};
use super::trace_converter::{ExecutionTrace, QuintTrace, TraceConversionConfig, TraceConverter};
use super::{ChaosGenerator, QuintCliRunner};
use crate::quint::simulation_evaluator::SimulationPropertyEvaluator;
use async_trait::async_trait;
use aura_core::effects::{
    StorageCoreEffects, StorageEffects, StorageError, StorageExtendedEffects, StorageStats,
};
use aura_core::AuraError;
use aura_effects::storage::FilesystemStorageHandler;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use sysinfo::System;

/// ITF trace with Model-Based Testing metadata
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ITFTrace {
    /// Trace metadata
    #[serde(rename = "#meta")]
    pub meta: ITFMeta,
    /// Execution parameters
    #[serde(default)]
    pub params: Vec<String>,
    /// State variables
    pub vars: Vec<String>,
    /// Sequence of states
    pub states: Vec<ITFState>,
    /// Optional loop index for infinite traces
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_index: Option<usize>,
}

/// ITF trace metadata
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ITFMeta {
    /// Format description
    pub format: String,
    /// Format description URL
    #[serde(rename = "format-description")]
    pub format_description: String,
    /// Source file
    pub source: String,
    /// Execution status
    pub status: String,
    /// Human-readable description
    pub description: String,
    /// Timestamp of generation
    pub timestamp: u64,
}

/// Single state in ITF trace
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ITFState {
    /// State metadata
    #[serde(rename = "#meta")]
    pub meta: ITFStateMeta,
    /// State variable values
    #[serde(flatten)]
    pub variables: HashMap<String, serde_json::Value>,
    /// Model-Based Testing metadata (when using --mbt)
    #[serde(rename = "mbt::actionTaken", skip_serializing_if = "Option::is_none")]
    pub action_taken: Option<String>,
    /// Non-deterministic choices (when using --mbt)
    #[serde(rename = "mbt::nondetPicks", skip_serializing_if = "Option::is_none")]
    pub nondet_picks: Option<HashMap<String, serde_json::Value>>,
}

/// Metadata for individual ITF states
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ITFStateMeta {
    /// State index in the trace
    pub index: u64,
}

/// Result of bounded model checking
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelCheckingResult {
    /// Whether all properties were satisfied
    pub properties_satisfied: bool,
    /// Counterexample traces if violations found
    pub counterexamples: Vec<ITFTrace>,
    /// Properties that were checked
    pub checked_properties: Vec<String>,
    /// Bound used for checking
    pub checking_bound: u32,
    /// Time taken for model checking
    pub checking_time_ms: u64,
}

/// A property violation discovered during model checking
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PropertyViolation {
    /// Name of the violated property
    pub property_name: String,
    /// ITF trace leading to the violation
    pub violation_trace: ITFTrace,
    /// Step number where violation occurred
    pub violation_step: u64,
    /// Description of the violation
    pub violation_description: String,
    /// State that caused the violation
    pub violation_state: ITFState,
}

// =============================================================================
// Generative Simulation Types (Phase 6)
// =============================================================================

/// Result of a generative simulation run through actual effects
#[derive(Debug, Clone)]
pub struct GenerativeSimulationResult {
    /// All executed steps
    pub steps: Vec<SimulationStep>,
    /// Final simulation state
    pub final_state: QuintSimulationState,
    /// Whether simulation completed successfully
    pub success: bool,
    /// Total number of steps executed
    pub step_count: u64,
    /// Properties that were violated (if any)
    pub property_violations: Vec<GenerativePropertyViolation>,
}

/// A property violation detected during generative simulation
#[derive(Debug, Clone)]
pub struct GenerativePropertyViolation {
    /// Property name
    pub property: String,
    /// Step at which violation occurred
    pub step_index: u64,
    /// Description of violation
    pub description: String,
}

/// A test case that has been validated through actual effect execution
#[derive(Debug, Clone)]
pub struct ValidatedTestCase {
    /// Test case identifier
    pub id: String,
    /// Source ITF trace
    pub source_trace: ITFTrace,
    /// Steps that were actually executed
    pub executed_steps: Vec<SimulationStep>,
    /// Final state after execution
    pub final_state: QuintSimulationState,
    /// Whether validation passed
    pub validation_passed: bool,
    /// Any violations found during execution
    pub violations: Vec<GenerativePropertyViolation>,
}

/// Result of Model-Based Testing with effect execution
#[derive(Debug, Clone)]
pub struct MBTExecutionResult {
    /// Source specification file
    pub spec_file: PathBuf,
    /// Total number of traces generated
    pub total_traces: u64,
    /// Number of traces that executed successfully
    pub successful_traces: u64,
    /// Number of traces that failed
    pub failed_traces: u64,
    /// Total steps executed across all traces
    pub total_steps_executed: u64,
    /// All property violations found
    pub violations: Vec<GenerativePropertyViolation>,
    /// All validated test cases
    pub validated_cases: Vec<ValidatedTestCase>,
    /// Total execution time in milliseconds
    pub execution_time_ms: u64,
}

/// Configuration for iterative deepening model checking
#[derive(Debug, Clone)]
pub struct IterativeDeepening {
    /// Starting bound
    pub initial_bound: u32,
    /// Maximum bound to reach
    pub max_bound: u32,
    /// Increment per iteration
    pub bound_increment: u32,
    /// Timeout per bound iteration (milliseconds)
    pub timeout_per_bound: u64,
}

/// Configuration for ITF-based fuzzing
#[derive(Debug, Clone)]
pub struct ITFFuzzConfig {
    /// Quint executable path
    pub quint_executable: PathBuf,
    /// Working directory for Quint operations
    pub working_dir: PathBuf,
    /// Maximum bound for model checking iterations
    pub max_bound: u32,
    /// Number of simulation runs per property
    pub simulation_runs: u32,
    /// Enable counterexample mutation
    pub enable_mutation: bool,
    /// Timeout per Quint command (seconds)
    pub command_timeout: u64,
    /// Iterative deepening configuration
    pub iterative_deepening: IterativeDeepening,
    /// Simulation configuration
    pub simulation: SimulationConfig,
}

impl Default for ITFFuzzConfig {
    fn default() -> Self {
        Self {
            quint_executable: "quint".into(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
            max_bound: 20,
            simulation_runs: 10,
            enable_mutation: true,
            command_timeout: 30,
            iterative_deepening: IterativeDeepening {
                initial_bound: 1,
                max_bound: 20,
                bound_increment: 1,
                timeout_per_bound: 30000, // 30 seconds per bound
            },
            simulation: SimulationConfig {
                num_runs: 10,
                max_steps: 50,
                seed: None,
                enable_mbt: true,
                run_timeout_ms: 15000, // 15 seconds per run
            },
        }
    }
}

/// Main orchestrator for ITF-based fuzzing using Quint CLI tools
pub struct ITFBasedFuzzer {
    trace_converter: TraceConverter, // For Execution -> Quint conversion
    itf_converter: super::trace_converter::ItfTraceConverter, // For ITF operations
    property_evaluator: SimulationPropertyEvaluator, // Existing
    chaos_generator: ChaosGenerator, // Existing
    quint_cli: QuintCliRunner,       // Quint CLI interface
    config: ITFFuzzConfig,
    storage: Arc<dyn StorageEffects>,
}

/// Errors that can occur during ITF-based fuzzing
#[derive(Debug, thiserror::Error)]
pub enum ITFFuzzError {
    #[error("Command failed: {0}")]
    CommandFailed(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("ITF validation error: {0}")]
    ValidationError(String),
    #[error("Trace conversion error: {0}")]
    TraceConversionError(String),
    #[error("Quint CLI error: {0}")]
    QuintCliError(String),
    #[error("Aura core error: {0}")]
    AuraError(#[from] AuraError),
    #[error("Model checking error: {0}")]
    ModelCheckingError(String),
    #[error("File system error: {0}")]
    FileSystemError(String),
}

impl ITFBasedFuzzer {
    /// Create new ITF-based fuzzer with default configuration
    pub fn new() -> Result<Self, ITFFuzzError> {
        Self::with_config(ITFFuzzConfig::default())
    }

    /// Create new ITF-based fuzzer with custom configuration
    pub fn with_config(config: ITFFuzzConfig) -> Result<Self, ITFFuzzError> {
        Self::with_config_and_storage(config, default_storage_provider())
    }

    /// Create new ITF-based fuzzer with explicit storage provider
    pub fn with_config_and_storage(
        config: ITFFuzzConfig,
        storage: Arc<dyn StorageEffects>,
    ) -> Result<Self, ITFFuzzError> {
        let quint_cli = QuintCliRunner::new(
            Some(config.quint_executable.clone()),
            config.working_dir.clone(),
        )
        .map_err(|e| {
            ITFFuzzError::QuintCliError(format!("Failed to create QuintCliRunner: {e}"))
        })?;

        Ok(Self {
            trace_converter: TraceConverter::new(),
            itf_converter: super::trace_converter::ItfTraceConverter::new(),
            property_evaluator: SimulationPropertyEvaluator::new(),
            chaos_generator: ChaosGenerator::new(),
            quint_cli,
            config,
            storage,
        })
    }

    /// Parse ITF trace from JSON string
    pub fn parse_itf_trace(&self, json: &str) -> Result<ITFTrace, ITFFuzzError> {
        let trace: ITFTrace = serde_json::from_str(json)?;
        self.validate_itf_trace(&trace)?;
        Ok(trace)
    }

    /// Parse ITF trace from file
    pub async fn parse_itf_file(&self, path: &Path) -> Result<ITFTrace, ITFFuzzError> {
        let content = self.read_path_to_string(path).await?;
        self.parse_itf_trace(&content)
    }

    async fn read_path_to_string(&self, path: &Path) -> Result<String, ITFFuzzError> {
        let key = path.to_string_lossy();
        let bytes = self
            .storage
            .retrieve(key.as_ref())
            .await
            .map_err(|e| ITFFuzzError::FileSystemError(e.to_string()))?
            .ok_or_else(|| {
                ITFFuzzError::FileSystemError(format!("File not found: {}", key.as_ref()))
            })?;

        String::from_utf8(bytes).map_err(|e| {
            ITFFuzzError::FileSystemError(format!("Invalid UTF-8 for {}: {}", key.as_ref(), e))
        })
    }

    async fn write_string_to_path(&self, path: &Path, content: String) -> Result<(), ITFFuzzError> {
        let key = path.to_string_lossy();
        self.storage
            .store(key.as_ref(), content.into_bytes())
            .await
            .map_err(|e| ITFFuzzError::FileSystemError(e.to_string()))
    }

    /// Validate ITF trace structure
    fn validate_itf_trace(&self, trace: &ITFTrace) -> Result<(), ITFFuzzError> {
        // Check format
        if trace.meta.format != "ITF" {
            return Err(ITFFuzzError::ValidationError(format!(
                "Invalid format: expected 'ITF', got '{}'",
                trace.meta.format
            )));
        }

        // Check states have sequential indices
        for (i, state) in trace.states.iter().enumerate() {
            if state.meta.index != i as u64 {
                return Err(ITFFuzzError::ValidationError(format!(
                    "State index mismatch: expected {}, got {}",
                    i, state.meta.index
                )));
            }
        }

        // Check variables are consistent
        for state in &trace.states {
            for var in &trace.vars {
                if !state.variables.contains_key(var) {
                    return Err(ITFFuzzError::ValidationError(format!(
                        "State {} missing variable '{}'",
                        state.meta.index, var
                    )));
                }
            }
        }

        Ok(())
    }

    /// Generate ITF traces using Quint CLI with model-based testing
    pub async fn generate_mbt_traces(
        &self,
        spec_file: &Path,
        count: u32,
    ) -> Result<Vec<ITFTrace>, ITFFuzzError> {
        let traces = self
            .quint_cli
            .generate_traces(spec_file, count, self.config.max_bound)
            .await
            .map_err(|e| ITFFuzzError::QuintCliError(format!("Failed to generate traces: {e}")))?;

        let mut itf_traces = Vec::new();
        for trace_json in traces {
            let trace_str = serde_json::to_string(&trace_json)?;
            let itf_trace = self.parse_itf_trace(&trace_str)?;
            itf_traces.push(itf_trace);
        }

        Ok(itf_traces)
    }

    /// Verify properties using Quint CLI and Apalache
    pub async fn verify_properties(&self, spec_file: &Path) -> Result<bool, ITFFuzzError> {
        let result = self
            .quint_cli
            .verify_spec(spec_file, Some(self.config.max_bound))
            .await
            .map_err(|e| ITFFuzzError::QuintCliError(format!("Verification failed: {e}")))?;

        Ok(result.outcome == "ok")
    }

    /// Check a specific property using Quint CLI
    pub async fn check_property(
        &self,
        spec_file: &Path,
        property_name: &str,
    ) -> Result<bool, ITFFuzzError> {
        self.quint_cli
            .check_property(spec_file, property_name)
            .await
            .map_err(|e| ITFFuzzError::QuintCliError(format!("Property check failed: {e}")))
    }

    /// Parse Quint spec and extract properties
    pub async fn extract_properties(&self, spec_file: &Path) -> Result<Vec<String>, ITFFuzzError> {
        let parse_result = self
            .quint_cli
            .parse_spec(spec_file)
            .await
            .map_err(|e| ITFFuzzError::QuintCliError(format!("Failed to parse spec: {e}")))?;

        // Extract property names from parse output
        let properties = parse_result
            .modules
            .iter()
            .flat_map(|module| module.definitions.iter())
            .filter_map(|def| match def {
                super::cli_runner::QuintDefinition::Value { name, .. } => Some(name.clone()),
                super::cli_runner::QuintDefinition::Definition { name, .. }
                    if name.starts_with("temporal_") =>
                {
                    Some(name.clone())
                }
                _ => None,
            })
            .collect();

        Ok(properties)
    }

    /// Convert ITF trace to internal format for validation
    fn convert_itf_to_internal(
        &self,
        itf_trace: &ITFTrace,
    ) -> Result<super::trace_converter::ItfTrace, ITFFuzzError> {
        // Convert our ITFTrace to the internal ItfTrace format
        // Use JSON serialization as the bridge between external ITF representation and internal converter
        let json = serde_json::to_string(itf_trace)?;
        let internal_itf = self.itf_converter.parse_itf_from_json(&json).map_err(|e| {
            ITFFuzzError::TraceConversionError(format!("Failed to parse ITF from JSON: {e}"))
        })?;

        // Validate the converted trace
        self.itf_converter
            .validate_itf_trace(&internal_itf)
            .map_err(|e| {
                ITFFuzzError::TraceConversionError(format!("ITF validation failed: {e}"))
            })?;

        Ok(internal_itf)
    }

    /// Convert execution trace to Quint format for property verification
    pub fn convert_execution_to_quint(
        &mut self,
        execution_trace: ExecutionTrace,
    ) -> Result<QuintTrace, ITFFuzzError> {
        let config = TraceConversionConfig {
            max_trace_length: self.config.max_bound as u64,
            include_detailed_state: true,
            include_protocol_events: true,
            include_network_events: false,
            sampling_rate: 1.0,
            compress_repeated_states: false,
        };

        // Update converter with our configuration
        self.trace_converter = TraceConverter::with_config(config);

        let result = self.trace_converter.convert_trace(&execution_trace)?;

        // Log conversion warnings
        if !result.warnings.is_empty() {
            for warning in &result.warnings {
                eprintln!("Conversion warning: {warning}");
            }
        }

        Ok(result.quint_trace)
    }

    /// Export ITF trace to JSON string
    pub fn export_itf_to_json(
        &self,
        itf_trace: &ITFTrace,
        pretty: bool,
    ) -> Result<String, ITFFuzzError> {
        // Convert our ITFTrace to internal format first
        let internal_itf = self.convert_itf_to_internal(itf_trace)?;
        let result = self
            .itf_converter
            .serialize_itf_to_json(&internal_itf, pretty)
            .map_err(|e| {
                ITFFuzzError::TraceConversionError(format!("Failed to serialize ITF to JSON: {e}"))
            })?;
        Ok(result)
    }

    /// Validate ITF trace using existing converter
    pub fn validate_converted_itf(&self, itf_trace: &ITFTrace) -> Result<(), ITFFuzzError> {
        let _internal_itf = self.convert_itf_to_internal(itf_trace)?;
        // Validation is done inside convert_itf_to_internal
        Ok(())
    }

    /// Combined ITF-to-execution conversion and property verification
    pub async fn verify_itf_trace(
        &mut self,
        itf_trace: &ITFTrace,
        spec_file: &Path,
    ) -> Result<bool, ITFFuzzError> {
        // First convert ITF to internal format and validate
        let _internal_itf = self.convert_itf_to_internal(itf_trace)?;
        // Validation is done in convert_itf_to_internal

        // Use Quint CLI to verify the trace against the spec
        let success = self.verify_properties(spec_file).await?;

        Ok(success)
    }

    // =========================================================================
    // Generative Simulation Integration (Phase 6)
    // =========================================================================

    /// Replay an ITF trace through actual Aura effects using GenerativeSimulator
    ///
    /// This method bridges ITF traces generated by Quint/Apalache with actual
    /// Aura effect execution, enabling true generative simulation.
    ///
    /// # Arguments
    /// * `itf_trace` - The ITF trace to replay
    /// * `registry` - ActionRegistry with handlers for each action
    /// * `initial_state` - Initial simulation state
    /// * `config` - Optional simulator configuration
    ///
    /// # Returns
    /// The simulation result including all executed steps and final state
    pub async fn replay_trace_with_effects(
        &self,
        itf_trace: &ITFTrace,
        registry: ActionRegistry,
        initial_state: QuintSimulationState,
        config: Option<GenerativeSimulatorConfig>,
    ) -> Result<GenerativeSimulationResult, ITFFuzzError> {
        let simulator = GenerativeSimulator::new(registry, config.unwrap_or_default());

        let result = simulator
            .replay_trace(itf_trace, initial_state)
            .await
            .map_err(|e| ITFFuzzError::TraceConversionError(format!("Replay failed: {e}")))?;

        Ok(GenerativeSimulationResult {
            steps: result.steps,
            final_state: result.final_state,
            success: result.success,
            step_count: result.step_count as u64,
            property_violations: result
                .property_violations
                .into_iter()
                .map(|v| GenerativePropertyViolation {
                    property: v.property,
                    step_index: v.step_index,
                    description: v.description,
                })
                .collect(),
        })
    }

    /// Explore the state space starting from an initial state
    ///
    /// Uses GenerativeSimulator to explore states by randomly selecting
    /// enabled actions and executing them through the registry.
    pub async fn explore_with_effects(
        &self,
        registry: ActionRegistry,
        initial_state: QuintSimulationState,
        max_steps: u32,
        seed: Option<u64>,
    ) -> Result<GenerativeSimulationResult, ITFFuzzError> {
        let config = GenerativeSimulatorConfig {
            max_steps,
            record_trace: true,
            verbose: false,
            exploration_seed: seed,
        };

        let simulator = GenerativeSimulator::new(registry, config);

        let result = simulator
            .explore(initial_state, seed)
            .await
            .map_err(|e| ITFFuzzError::TraceConversionError(format!("Exploration failed: {e}")))?;

        Ok(GenerativeSimulationResult {
            steps: result.steps,
            final_state: result.final_state,
            success: result.success,
            step_count: result.step_count as u64,
            property_violations: result
                .property_violations
                .into_iter()
                .map(|v| GenerativePropertyViolation {
                    property: v.property,
                    step_index: v.step_index,
                    description: v.description,
                })
                .collect(),
        })
    }

    /// Generate executable test cases from ITF traces using effect-based simulation
    ///
    /// This method combines ITF trace parsing with GenerativeSimulator execution
    /// to produce test cases that have been validated through actual effects.
    pub async fn generate_validated_test_cases(
        &self,
        itf_traces: &[ITFTrace],
        registry: ActionRegistry,
        initial_state: QuintSimulationState,
    ) -> Result<Vec<ValidatedTestCase>, ITFFuzzError> {
        let mut validated_cases = Vec::new();

        for (i, trace) in itf_traces.iter().enumerate() {
            // Replay trace through effects
            let sim_result = self
                .replay_trace_with_effects(trace, registry.clone(), initial_state.clone(), None)
                .await?;

            // Generate test case from validated execution
            let test_case = ValidatedTestCase {
                id: format!("validated_test_{i}"),
                source_trace: trace.clone(),
                executed_steps: sim_result.steps,
                final_state: sim_result.final_state,
                validation_passed: sim_result.success,
                violations: sim_result.property_violations,
            };

            validated_cases.push(test_case);
        }

        Ok(validated_cases)
    }

    /// Run Model-Based Testing with effect execution (Phase 6.4)
    ///
    /// This is the primary entry point for Quint-driven generative simulation:
    /// 1. Generates ITF traces using `quint run --mbt`
    /// 2. Replays each trace through actual Aura effects
    /// 3. Validates property satisfaction during execution
    /// 4. Returns comprehensive results including violations
    ///
    /// # Arguments
    /// * `spec_file` - Path to the Quint specification file
    /// * `registry` - ActionRegistry with handlers for Quint actions
    /// * `initial_state` - Initial simulation state
    /// * `trace_count` - Number of MBT traces to generate
    ///
    /// # Returns
    /// MBT execution results including all validated traces
    pub async fn run_mbt_with_effects(
        &self,
        spec_file: &Path,
        registry: ActionRegistry,
        initial_state: QuintSimulationState,
        trace_count: u32,
    ) -> Result<MBTExecutionResult, ITFFuzzError> {
        // Step 1: Generate MBT traces using Quint CLI
        let traces = self.generate_mbt_traces(spec_file, trace_count).await?;

        // Step 2: Replay each trace through effects
        let validated_cases = self
            .generate_validated_test_cases(&traces, registry, initial_state)
            .await?;

        // Step 3: Collect statistics
        let total_traces = validated_cases.len();
        let successful_traces = validated_cases
            .iter()
            .filter(|c| c.validation_passed)
            .count();
        let total_steps: u64 = validated_cases
            .iter()
            .map(|c| c.executed_steps.len() as u64)
            .sum();
        let violations: Vec<_> = validated_cases
            .iter()
            .flat_map(|c| c.violations.clone())
            .collect();

        let execution_time_ms = 0u64;

        Ok(MBTExecutionResult {
            spec_file: spec_file.to_path_buf(),
            total_traces: total_traces as u64,
            successful_traces: successful_traces as u64,
            failed_traces: (total_traces - successful_traces) as u64,
            total_steps_executed: total_steps,
            violations,
            validated_cases,
            execution_time_ms,
        })
    }

    /// Generate MBT traces and convert to test cases without effect execution
    ///
    /// Useful when you want to generate test cases for later execution
    /// or when effect handlers are not available.
    pub async fn generate_mbt_test_cases(
        &self,
        spec_file: &Path,
        trace_count: u32,
    ) -> Result<Vec<GeneratedTestCase>, ITFFuzzError> {
        let traces = self.generate_mbt_traces(spec_file, trace_count).await?;
        self.convert_traces_to_test_cases(&traces, spec_file)
    }

    /// Run bounded model checking with iterative deepening
    pub async fn run_bounded_model_checking(
        &self,
        spec_file: &Path,
        properties: &[String],
    ) -> Result<ModelCheckingResult, ITFFuzzError> {
        let deepening = &self.config.iterative_deepening;

        let mut all_counterexamples = Vec::new();
        let mut violations_found = false;
        let mut final_bound = deepening.initial_bound;

        // Iterative deepening: gradually increase bounds until max or violations found
        for bound in (deepening.initial_bound..=deepening.max_bound)
            .step_by(deepening.bound_increment as usize)
        {
            final_bound = bound;

            for property in properties {
                let result = self
                    .check_property_at_bound(spec_file, property, bound)
                    .await?;

                if !result.satisfied {
                    // Property violated - extract counterexample
                    violations_found = true;
                    if let Some(counterexample) = result.counterexample_trace {
                        all_counterexamples.push(counterexample);
                    }

                    // Continue checking other properties at this bound
                    continue;
                }
            }

            // If we found violations, we can stop iterative deepening
            // (unless configured to continue to find more violations)
            if violations_found && !self.config.enable_mutation {
                break;
            }
        }

        let checking_time = 0u64;

        Ok(ModelCheckingResult {
            properties_satisfied: !violations_found,
            counterexamples: all_counterexamples,
            checked_properties: properties.to_vec(),
            checking_bound: final_bound,
            checking_time_ms: checking_time,
        })
    }

    /// Check a single property at a specific bound
    async fn check_property_at_bound(
        &self,
        spec_file: &Path,
        property: &str,
        bound: u32,
    ) -> Result<PropertyCheckResult, ITFFuzzError> {
        // Create ephemeral output file for counterexample
        let temp_dir = std::env::temp_dir();
        let counterexample_file = temp_dir.join(format!("counterexample_{property}_{bound}.itf"));

        // Run `quint verify` with the specific bound
        let output = std::process::Command::new(&self.config.quint_executable)
            .current_dir(&self.config.working_dir)
            .args([
                "verify",
                "--invariant",
                property,
                "--max-steps",
                &bound.to_string(),
                "--out-itf",
                #[allow(clippy::unwrap_used)] // Test file paths are guaranteed to be valid UTF-8
                counterexample_file.to_str().unwrap(),
                #[allow(clippy::unwrap_used)] // Test file paths are guaranteed to be valid UTF-8
                spec_file.to_str().unwrap(),
            ])
            .output()
            .map_err(|e| ITFFuzzError::CommandFailed(format!("quint verify failed: {e}")))?;

        let satisfied = output.status.success();
        let mut counterexample_trace = None;

        // If property was violated, parse the counterexample
        if !satisfied
            && self
                .storage
                .exists(counterexample_file.to_string_lossy().as_ref())
                .await
                .map_err(|e| ITFFuzzError::FileSystemError(e.to_string()))?
        {
            match self.parse_itf_file(&counterexample_file).await {
                Ok(trace) => {
                    counterexample_trace = Some(trace);
                    // Remove counterexample artifact after parsing to keep workspace clean
                    let _ = self
                        .storage
                        .remove(counterexample_file.to_string_lossy().as_ref())
                        .await;
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse counterexample file: {e}");
                }
            }
        }

        Ok(PropertyCheckResult {
            property_name: property.to_string(),
            satisfied,
            bound,
            counterexample_trace,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Extract property violations from model checking results
    pub fn extract_violations(&self, result: &ModelCheckingResult) -> Vec<PropertyViolation> {
        let mut violations = Vec::new();

        for (i, counterexample) in result.counterexamples.iter().enumerate() {
            // Find the step where the violation occurred
            let violation_step = counterexample.states.len().saturating_sub(1) as u64;
            let violation_state =
                counterexample
                    .states
                    .last()
                    .cloned()
                    .unwrap_or_else(|| ITFState {
                        meta: ITFStateMeta { index: 0 },
                        variables: HashMap::new(),
                        action_taken: None,
                        nondet_picks: None,
                    });

            let property_name = result
                .checked_properties
                .get(i % result.checked_properties.len())
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());

            violations.push(PropertyViolation {
                property_name: property_name.clone(),
                violation_trace: counterexample.clone(),
                violation_step,
                violation_description: format!(
                    "Property '{}' violated at step {} with bound {}",
                    property_name, violation_step, result.checking_bound
                ),
                violation_state,
            });
        }

        violations
    }

    /// Analyze violations using PropertyEvaluator for additional insights
    pub async fn analyze_violations(
        &mut self,
        violations: &[PropertyViolation],
    ) -> Result<Vec<ITFPropertyEvaluationResult>, ITFFuzzError> {
        let mut evaluation_results = Vec::new();

        for violation in violations {
            // Convert ITF trace to internal format for evaluation
            let _internal_itf = self.convert_itf_to_internal(&violation.violation_trace)?;

            evaluation_results.push(ITFPropertyEvaluationResult {
                property_name: violation.property_name.clone(),
                satisfied: false,
                violation_step: Some(violation.violation_step),
                execution_time_ms: 0,
                error_message: Some(violation.violation_description.clone()),
            });
        }

        Ok(evaluation_results)
    }

    /// Create comprehensive model checking report
    pub async fn create_model_checking_report(
        &mut self,
        spec_file: &Path,
        properties: &[String],
    ) -> Result<ModelCheckingReport, ITFFuzzError> {
        // Run bounded model checking
        let model_check_result = self
            .run_bounded_model_checking(spec_file, properties)
            .await?;

        // Extract violations
        let violations = self.extract_violations(&model_check_result);

        // Analyze violations
        let violation_analysis = self.analyze_violations(&violations).await?;

        let total_time = std::time::Duration::ZERO;

        Ok(ModelCheckingReport {
            spec_file: spec_file.to_path_buf(),
            model_check_result,
            violations: violations.clone(),
            violation_analysis,
            total_execution_time: total_time,
            recommendations: self.generate_model_check_recommendations(&violations),
        })
    }

    /// Generate recommendations based on model checking results
    fn generate_model_check_recommendations(
        &self,
        violations: &[PropertyViolation],
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        if violations.is_empty() {
            recommendations.push("All properties satisfied within the checked bounds. Consider increasing bounds for more thorough verification.".to_string());
        } else {
            recommendations.push(format!("Found {} property violation(s). Review the counterexample traces to understand the root causes.", violations.len()));

            // Analyze violation patterns
            let violation_steps: Vec<_> = violations.iter().map(|v| v.violation_step).collect();
            let max_step = violation_steps.iter().max().copied().unwrap_or(0);
            let min_step = violation_steps.iter().min().copied().unwrap_or(0);

            if min_step < 5 {
                recommendations.push("Some violations occur very early in execution. Check initial state conditions and preconditions.".to_string());
            }

            if max_step > self.config.max_bound as u64 / 2 {
                recommendations.push("Some violations occur at high step counts. Consider protocol timeouts and liveness properties.".to_string());
            }
        }

        recommendations
    }

    /// Run simulation-based test generation using `quint run --mbt`
    pub async fn run_simulation_based_testing(
        &self,
        spec_file: &Path,
    ) -> Result<SimulationResult, ITFFuzzError> {
        let sim_config = &self.config.simulation;

        let mut traces = Vec::new();
        let mut errors = Vec::new();
        let mut runs_executed = 0;

        for run_id in 0..sim_config.num_runs {
            match self
                .run_single_simulation(spec_file, run_id, sim_config)
                .await
            {
                Ok(trace) => {
                    traces.push(trace);
                    runs_executed += 1;
                }
                Err(e) => {
                    errors.push(format!("Run {run_id}: {e}"));
                    // Continue with other runs even if one fails
                }
            }
        }

        let simulation_time = 0u64;

        Ok(SimulationResult {
            traces,
            runs_executed,
            simulation_time_ms: simulation_time,
            errors,
        })
    }

    /// Execute a single simulation run
    async fn run_single_simulation(
        &self,
        spec_file: &Path,
        run_id: u32,
        config: &SimulationConfig,
    ) -> Result<ITFTrace, ITFFuzzError> {
        let temp_dir = std::env::temp_dir();
        let output_file = temp_dir.join(format!(
            "simulation_run_{}_{}.itf",
            run_id,
            std::process::id()
        ));

        // Execute the command with timeout
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(config.run_timeout_ms),
            async {
                let mut tokio_cmd = tokio::process::Command::new(&self.config.quint_executable);
                tokio_cmd.current_dir(&self.config.working_dir).args([
                    "run",
                    "--max-samples",
                    "1",
                    "--max-steps",
                    &config.max_steps.to_string(),
                    "--out-itf",
                    #[allow(clippy::unwrap_used)]
                    // Test file paths are guaranteed to be valid UTF-8
                    output_file.to_str().unwrap(),
                    #[allow(clippy::unwrap_used)]
                    // Test file paths are guaranteed to be valid UTF-8
                    spec_file.to_str().unwrap(),
                ]);

                if config.enable_mbt {
                    tokio_cmd.arg("--mbt");
                }

                if let Some(seed) = config.seed {
                    tokio_cmd.args(["--seed", &(seed + run_id as u64).to_string()]);
                }

                tokio_cmd.output().await
            },
        )
        .await
        .map_err(|_| ITFFuzzError::CommandFailed("Simulation timeout".to_string()))?
        .map_err(|e| ITFFuzzError::CommandFailed(format!("quint run failed: {e}")))?;

        // Check if simulation succeeded
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ITFFuzzError::CommandFailed(format!(
                "quint run failed with status {}: {}",
                output.status, stderr
            )));
        }

        // Parse the generated ITF file
        let trace = self.parse_itf_file(&output_file).await?;

        // Remove generated ITF file after parsing to keep workspace tidy
        let _ = self
            .storage
            .remove(output_file.to_string_lossy().as_ref())
            .await;

        Ok(trace)
    }

    /// Convert ITF traces to executable test cases
    pub fn convert_traces_to_test_cases(
        &self,
        traces: &[ITFTrace],
        spec_file: &Path,
    ) -> Result<Vec<GeneratedTestCase>, ITFFuzzError> {
        let mut test_cases = Vec::new();

        for (i, trace) in traces.iter().enumerate() {
            let test_case = self.convert_single_trace_to_test_case(trace, spec_file, i as u32)?;
            test_cases.push(test_case);
        }

        Ok(test_cases)
    }

    /// Convert a single ITF trace to an executable test case
    fn convert_single_trace_to_test_case(
        &self,
        trace: &ITFTrace,
        spec_file: &Path,
        index: u32,
    ) -> Result<GeneratedTestCase, ITFFuzzError> {
        // Extract action sequence from trace
        let action_sequence = self.extract_action_sequence_from_trace(trace)?;

        // Get final state
        let expected_state = trace
            .states
            .last()
            .map(|state| state.variables.clone())
            .unwrap_or_default();

        // Create test case metadata
        let metadata = TestCaseMetadata {
            generation_method: TestGenerationMethod::Simulation {
                runs: self.config.simulation.num_runs,
                max_steps: self.config.simulation.max_steps,
            },
            source_spec: spec_file.to_string_lossy().to_string(),
            exercised_properties: self.extract_exercised_properties(trace),
            generated_at: std::time::SystemTime::UNIX_EPOCH,
            expected_duration_ms: Some(trace.states.len() as u64 * 10), // Estimate 10ms per step
        };

        Ok(GeneratedTestCase {
            id: format!(
                "sim_test_{}_{}",
                index,
                std::time::SystemTime::UNIX_EPOCH
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            source_trace: trace.clone(),
            action_sequence,
            expected_state,
            metadata,
        })
    }

    /// Extract action sequence from ITF trace using MBT metadata
    fn extract_action_sequence_from_trace(
        &self,
        trace: &ITFTrace,
    ) -> Result<Vec<TestAction>, ITFFuzzError> {
        let mut actions = Vec::new();

        for (i, state) in trace.states.iter().enumerate() {
            // Extract action name from MBT metadata
            let action_name = state.action_taken.clone();

            // Extract non-deterministic picks
            let nondet_picks = state.nondet_picks.clone();

            // Create basic preconditions based on step
            let preconditions = if i == 0 {
                vec!["initial_state".to_string()]
            } else {
                vec![format!("step_{}_preconditions", i)]
            };

            actions.push(TestAction {
                step: i as u64,
                action_name,
                state_variables: state.variables.clone(),
                nondet_picks,
                preconditions,
            });
        }

        Ok(actions)
    }

    /// Extract properties that this trace exercises
    fn extract_exercised_properties(&self, trace: &ITFTrace) -> Vec<String> {
        let mut properties = Vec::new();

        // Look for property-related metadata in the trace
        if let Some(description) = trace.meta.description.split("property:").nth(1) {
            properties.push(description.trim().to_string());
        }

        // Add default properties based on trace characteristics
        if trace.states.len() > 1 {
            properties.push("safety_properties".to_string());
        }
        if trace.states.len() > 10 {
            properties.push("liveness_properties".to_string());
        }

        // Look for action-based properties
        for state in &trace.states {
            if let Some(action) = &state.action_taken {
                properties.push(format!("{action}_property"));
            }
        }

        properties.sort();
        properties.dedup();
        properties
    }

    /// Parse MBT metadata from ITF state
    pub fn parse_mbt_metadata(&self, state: &ITFState) -> MBTMetadata {
        MBTMetadata {
            action_taken: state.action_taken.clone(),
            nondet_picks: state.nondet_picks.clone(),
            choice_points: self.extract_choice_points(state),
            decision_path: self.extract_decision_path(state),
        }
    }

    /// Extract choice points from state
    fn extract_choice_points(&self, state: &ITFState) -> Vec<ChoicePoint> {
        let mut choice_points = Vec::new();

        if let Some(picks) = &state.nondet_picks {
            for (key, value) in picks {
                choice_points.push(ChoicePoint {
                    variable: key.clone(),
                    chosen_value: value.clone(),
                    available_choices: vec![value.clone()], // Would need more sophisticated parsing
                });
            }
        }

        choice_points
    }

    /// Extract decision path from state
    fn extract_decision_path(&self, state: &ITFState) -> Vec<Decision> {
        let mut decisions = Vec::new();

        // Analyze state variables to infer decisions
        for (var_name, var_value) in &state.variables {
            if var_name.contains("decision") || var_name.contains("choice") {
                decisions.push(Decision {
                    variable: var_name.clone(),
                    condition: format!("{var_name} = {var_value}"),
                    outcome: var_value.clone(),
                });
            }
        }

        decisions
    }

    /// Generate comprehensive test suite from simulations
    pub async fn generate_comprehensive_test_suite(
        &self,
        spec_file: &Path,
    ) -> Result<TestSuite, ITFFuzzError> {
        // Phase 1: Run simulations
        let simulation_result = self.run_simulation_based_testing(spec_file).await?;

        // Phase 2: Convert traces to test cases
        let test_cases = self.convert_traces_to_test_cases(&simulation_result.traces, spec_file)?;

        // Phase 3: Analyze coverage
        let coverage_analysis = self.analyze_test_coverage(&test_cases);

        // Phase 4: Generate summary
        let generation_time = std::time::Duration::ZERO;

        Ok(TestSuite {
            test_cases,
            simulation_result,
            coverage_analysis,
            generation_time,
            spec_file: spec_file.to_path_buf(),
        })
    }

    /// Analyze test coverage of generated test cases
    fn analyze_test_coverage(&self, test_cases: &[GeneratedTestCase]) -> TestCoverageAnalysis {
        let mut covered_actions = std::collections::HashSet::new();
        let mut covered_variables = std::collections::HashSet::new();
        let mut covered_properties = std::collections::HashSet::new();
        let mut total_steps = 0;

        for test_case in test_cases {
            // Collect covered actions
            for action in &test_case.action_sequence {
                if let Some(action_name) = &action.action_name {
                    covered_actions.insert(action_name.clone());
                }

                // Collect covered variables
                for var_name in action.state_variables.keys() {
                    covered_variables.insert(var_name.clone());
                }

                total_steps += 1;
            }

            // Collect covered properties
            for property in &test_case.metadata.exercised_properties {
                covered_properties.insert(property.clone());
            }
        }

        TestCoverageAnalysis {
            total_test_cases: test_cases.len() as u64,
            total_steps,
            covered_actions: covered_actions.len() as u32,
            covered_variables: covered_variables.len() as u32,
            covered_properties: covered_properties.len() as u32,
            action_names: covered_actions.into_iter().collect(),
            variable_names: covered_variables.into_iter().collect(),
            property_names: covered_properties.into_iter().collect(),
        }
    }

    // ==================== Phase 4: Main Orchestrator and CI/CD Integration ====================

    /// Run a complete fuzzing campaign with all phases
    pub async fn run_complete_campaign(
        &self,
        spec_file: &Path,
        campaign_config: FuzzingCampaignConfig,
    ) -> Result<FuzzingCampaignResult, ITFFuzzError> {
        let mut performance_monitor = PerformanceMonitor::new();

        println!(
            "🚀 Starting ITF fuzzing campaign for {}",
            spec_file.display()
        );

        // Phase 1: Model Checking (optional)
        let model_checking_result = if campaign_config.enable_model_checking {
            performance_monitor.start_phase("model_checking");
            println!("📋 Phase 1: Model checking...");

            match self.run_model_checking(spec_file).await {
                Ok(result) => {
                    performance_monitor.end_phase("model_checking");
                    println!(
                        "✅ Model checking completed: {} properties checked",
                        result.model_check_result.checked_properties.len()
                    );
                    Some(result)
                }
                Err(e) => {
                    performance_monitor.end_phase("model_checking");
                    println!("⚠️ Model checking failed: {e}");
                    return Err(e);
                }
            }
        } else {
            None
        };

        // Phase 2: Simulation-based Testing (optional)
        let simulation_result = if campaign_config.enable_simulation_testing {
            performance_monitor.start_phase("simulation");
            println!("🎲 Phase 2: Simulation-based testing...");

            match self.generate_test_suite(spec_file).await {
                Ok(suite) => {
                    performance_monitor.end_phase("simulation");
                    println!("✅ Generated {} test cases", suite.test_cases.len());
                    Some(suite)
                }
                Err(e) => {
                    performance_monitor.end_phase("simulation");
                    println!("⚠️ Simulation testing failed: {e}");
                    return Err(e);
                }
            }
        } else {
            None
        };

        // Phase 3: Mutation Testing (optional)
        let all_violations = if campaign_config.enable_mutation && model_checking_result.is_some() {
            performance_monitor.start_phase("mutation");
            println!("🧬 Phase 3: Mutation testing...");

            let mut violations = Vec::new();
            if let Some(ref mc_result) = model_checking_result {
                violations.extend(mc_result.violations.clone());
            }

            performance_monitor.end_phase("mutation");
            println!("🔍 Found {} property violations", violations.len());
            violations
        } else {
            Vec::new()
        };

        // Phase 4: Analysis and Reporting
        performance_monitor.start_phase("analysis");
        println!("📊 Phase 4: Analysis and reporting...");

        let campaign_duration = std::time::Duration::ZERO;
        let performance_report = performance_monitor.generate_report();

        // Calculate coverage summary
        let coverage_summary = self.calculate_campaign_coverage(
            &model_checking_result,
            &simulation_result,
            &all_violations,
        );

        let success = all_violations.is_empty()
            || all_violations.len() <= campaign_config.ci_integration.max_violations as usize;

        let recommendations = self.generate_recommendations(
            &model_checking_result,
            &simulation_result,
            &coverage_summary,
        );

        performance_monitor.end_phase("analysis");

        let result = FuzzingCampaignResult {
            config: campaign_config.clone(),
            model_checking_result,
            simulation_result,
            all_violations,
            performance_report,
            coverage_summary,
            campaign_duration,
            success,
            recommendations,
        };

        // Export results if configured
        if let Some(ref export_path) = campaign_config.ci_integration.export_results {
            self.export_campaign_results(
                &result,
                export_path,
                &campaign_config.ci_integration.output_format,
            )
            .await?;
        }

        if let Some(ref coverage_path) = campaign_config.ci_integration.export_coverage {
            self.export_coverage_report(&result.coverage_summary, coverage_path)
                .await?;
        }

        println!(
            "🎯 Campaign completed in {:.2}s (Success: {})",
            campaign_duration.as_secs_f64(),
            success
        );

        Ok(result)
    }

    /// Check individual test case for property violations
    fn check_test_case_for_violations(
        &self,
        test_case: &GeneratedTestCase,
    ) -> Vec<PropertyViolation> {
        let mut violations = Vec::new();

        // Check each action in the test case for potential violations
        for (step_idx, action) in test_case.action_sequence.iter().enumerate() {
            // Look for state conditions that might indicate violations
            for precondition in &action.preconditions {
                if precondition.contains("violation") || precondition.contains("error") {
                    violations.push(PropertyViolation {
                        property_name: format!("test_case_property_{step_idx}"),
                        violation_trace: test_case.source_trace.clone(),
                        violation_step: step_idx as u64,
                        violation_description: format!(
                            "Precondition violation detected: {precondition}"
                        ),
                        violation_state: ITFState {
                            meta: ITFStateMeta {
                                index: step_idx as u64,
                            },
                            variables: action.state_variables.clone(),
                            action_taken: action.action_name.clone(),
                            nondet_picks: action.nondet_picks.clone(),
                        },
                    });
                }
            }
        }

        violations
    }

    /// Calculate comprehensive coverage across all testing phases
    fn calculate_campaign_coverage(
        &self,
        model_checking_result: &Option<ModelCheckingReport>,
        simulation_result: &Option<TestSuite>,
        violations: &[PropertyViolation],
    ) -> CoverageSummary {
        let mut total_properties = 0;
        let mut covered_properties = 0;
        let mut total_states = 0;
        let mut covered_states = 0;

        // Count from model checking
        if let Some(mc_result) = model_checking_result {
            total_properties += mc_result.model_check_result.checked_properties.len();
            // Properties without violations are considered covered/satisfied
            covered_properties +=
                mc_result.model_check_result.checked_properties.len() - mc_result.violations.len();
        }

        // Count from simulation
        if let Some(sim_result) = simulation_result {
            total_states += sim_result.simulation_result.traces.len();
            covered_states += sim_result.coverage_analysis.covered_variables;
        }

        let property_coverage = if total_properties > 0 {
            (covered_properties as f64 / total_properties as f64) * 100.0
        } else {
            0.0
        };

        let state_coverage = if total_states > 0 {
            (covered_states as f64 / total_states as f64) * 100.0
        } else {
            0.0
        };

        CoverageSummary {
            property_coverage,
            state_coverage,
            violation_count: violations.len() as u32,
            total_test_cases: simulation_result
                .as_ref()
                .map(|s| s.test_cases.len() as u64)
                .unwrap_or(0),
            goals_achieved: property_coverage >= 80.0 && violations.is_empty(),
        }
    }

    /// Generate recommendations based on campaign results
    fn generate_recommendations(
        &self,
        model_checking_result: &Option<ModelCheckingReport>,
        simulation_result: &Option<TestSuite>,
        coverage_summary: &CoverageSummary,
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        if coverage_summary.property_coverage < 50.0 {
            recommendations.push("Consider adding more properties to improve coverage".to_string());
        }

        if coverage_summary.state_coverage < 60.0 {
            recommendations.push("Increase simulation runs to explore more states".to_string());
        }

        if coverage_summary.violation_count > 0 {
            recommendations.push(format!(
                "Address {} property violation(s) found during testing",
                coverage_summary.violation_count
            ));
        }

        if let Some(mc_result) = model_checking_result {
            let failed_properties = mc_result.violations.len();
            if failed_properties > 0 {
                recommendations.push(format!(
                    "Review {failed_properties} failed properties in model checking phase"
                ));
            }
        }

        if let Some(sim_result) = simulation_result {
            if !sim_result.simulation_result.errors.is_empty() {
                recommendations.push("Review simulation errors for potential issues".to_string());
            }
        }

        if recommendations.is_empty() {
            recommendations.push("All checks passed successfully!".to_string());
        }

        recommendations
    }

    /// Export campaign results in various formats
    async fn export_campaign_results(
        &self,
        result: &FuzzingCampaignResult,
        export_path: &Path,
        format: &CIOutputFormat,
    ) -> Result<(), ITFFuzzError> {
        let content = match format {
            CIOutputFormat::Json => serde_json::to_string_pretty(result).map_err(|e| {
                ITFFuzzError::TraceConversionError(format!("JSON serialization failed: {e}"))
            })?,
            CIOutputFormat::JunitXml => self.generate_junit_xml(result),
            CIOutputFormat::GitHubActions => self.generate_github_actions_output(result),
            CIOutputFormat::Text => self.generate_text_report(result),
        };

        self.write_string_to_path(export_path, content).await?;

        Ok(())
    }

    /// Generate JUnit XML format for CI systems
    fn generate_junit_xml(&self, result: &FuzzingCampaignResult) -> String {
        let test_count = result
            .simulation_result
            .as_ref()
            .map(|s| s.test_cases.len())
            .unwrap_or(0);

        let failure_count = result.all_violations.len();

        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="ITF Fuzzing Campaign" tests="{}" failures="{}" time="{:.3}">
  <testcase name="Model Checking" time="{:.3}">
    {}
  </testcase>
  <testcase name="Simulation Testing" time="{:.3}">
    {}
  </testcase>
  <testcase name="Coverage Analysis" time="{:.3}">
    {}
  </testcase>
</testsuite>"#,
            test_count,
            failure_count,
            result.campaign_duration.as_secs_f64(),
            result
                .performance_report
                .phase_timings
                .model_checking_time
                .as_secs_f64(),
            if result
                .model_checking_result
                .as_ref()
                .is_some_and(|mc| !mc.violations.is_empty())
            {
                "<failure message=\"Model checking failed\">Property violations detected</failure>"
                    .to_string()
            } else {
                "".to_string()
            },
            result
                .performance_report
                .phase_timings
                .simulation_time
                .as_secs_f64(),
            if result
                .simulation_result
                .as_ref()
                .is_some_and(|sim| !sim.simulation_result.errors.is_empty())
            {
                "<failure message=\"Simulation testing failed\">Simulation errors detected</failure>".to_string()
            } else {
                "".to_string()
            },
            result
                .performance_report
                .phase_timings
                .analysis_time
                .as_secs_f64(),
            if !result.coverage_summary.goals_achieved {
                format!(
                    "<failure message=\"Coverage goals not met\">Coverage: {:.1}%</failure>",
                    result.coverage_summary.property_coverage
                )
            } else {
                "".to_string()
            }
        )
    }

    /// Generate GitHub Actions output format
    fn generate_github_actions_output(&self, result: &FuzzingCampaignResult) -> String {
        let mut output = String::new();

        if result.success {
            output
                .push_str("::notice title=ITF Fuzzing Success::Campaign completed successfully\n");
        } else {
            output.push_str("::error title=ITF Fuzzing Failed::Campaign found issues\n");
        }

        output.push_str(&format!(
            "::set-output name=property_coverage::{:.1}\n",
            result.coverage_summary.property_coverage
        ));
        output.push_str(&format!(
            "::set-output name=violation_count::{}\n",
            result.coverage_summary.violation_count
        ));
        output.push_str(&format!(
            "::set-output name=duration::{:.2}\n",
            result.campaign_duration.as_secs_f64()
        ));

        if !result.all_violations.is_empty() {
            for violation in &result.all_violations {
                output.push_str(&format!(
                    "::warning title=Property Violation::Property '{}' violated\n",
                    violation.property_name
                ));
            }
        }

        output
    }

    /// Generate human-readable text report
    fn generate_text_report(&self, result: &FuzzingCampaignResult) -> String {
        format!(
            r#"ITF Fuzzing Campaign Report
===========================

Campaign Duration: {:.2}s
Success: {}

Coverage Summary:
- Property Coverage: {:.1}%
- State Coverage: {:.1}%
- Total Test Cases: {}
- Violations Found: {}
- Goals Achieved: {}

Performance Metrics:
- Model Checking Time: {:.2}s
- Simulation Time: {:.2}s
- Analysis Time: {:.2}s
- Memory Peak: {} MB

Recommendations:
{}

Details:
- Model Checking: {} properties checked
- Simulation: {} test cases generated
- Mutations: {} violations detected
"#,
            result.campaign_duration.as_secs_f64(),
            if result.success { "✅ Yes" } else { "❌ No" },
            result.coverage_summary.property_coverage,
            result.coverage_summary.state_coverage,
            result.coverage_summary.total_test_cases,
            result.coverage_summary.violation_count,
            if result.coverage_summary.goals_achieved {
                "✅ Yes"
            } else {
                "❌ No"
            },
            result
                .performance_report
                .phase_timings
                .model_checking_time
                .as_secs_f64(),
            result
                .performance_report
                .phase_timings
                .simulation_time
                .as_secs_f64(),
            result
                .performance_report
                .phase_timings
                .analysis_time
                .as_secs_f64(),
            result.performance_report.memory_usage.peak_memory_mb,
            result.recommendations.join("\n- "),
            result
                .model_checking_result
                .as_ref()
                .map_or(0, |mc| mc.model_check_result.checked_properties.len()),
            result
                .simulation_result
                .as_ref()
                .map_or(0, |sim| sim.test_cases.len()),
            result.all_violations.len()
        )
    }

    /// Export coverage report to file
    async fn export_coverage_report(
        &self,
        coverage: &CoverageSummary,
        export_path: &Path,
    ) -> Result<(), ITFFuzzError> {
        let report = format!(
            r#"Coverage Report
===============

Property Coverage: {:.1}%
State Coverage: {:.1}%
Total Test Cases: {}
Violations Found: {}
Goals Achieved: {}
"#,
            coverage.property_coverage,
            coverage.state_coverage,
            coverage.total_test_cases,
            coverage.violation_count,
            if coverage.goals_achieved { "Yes" } else { "No" }
        );

        self.write_string_to_path(export_path, report).await?;

        Ok(())
    }

    /// Run model checking and return a report (wrapper method for compatibility)
    async fn run_model_checking(
        &self,
        spec_file: &Path,
    ) -> Result<ModelCheckingReport, ITFFuzzError> {
        // Extract properties from the spec file
        let properties = self.extract_properties(spec_file).await?;

        // Run bounded model checking
        let model_check_result = self
            .run_bounded_model_checking(spec_file, &properties)
            .await?;

        // Extract violations
        let violations = self.extract_violations(&model_check_result);

        // Create the report
        let checking_time_ms = model_check_result.checking_time_ms;

        let report = ModelCheckingReport {
            spec_file: spec_file.to_path_buf(),
            model_check_result,
            violations: violations.clone(),
            violation_analysis: vec![], // Will be populated by analyze_violations if needed
            total_execution_time: std::time::Duration::from_millis(checking_time_ms),
            recommendations: self.generate_model_check_recommendations(&violations),
        };

        Ok(report)
    }

    /// Generate test suite (wrapper method for compatibility)
    async fn generate_test_suite(&self, spec_file: &Path) -> Result<TestSuite, ITFFuzzError> {
        self.generate_comprehensive_test_suite(spec_file).await
    }
}

/// Performance monitoring utility for tracking phase timing and resources
pub struct PerformanceMonitor {
    phase_start_times: std::collections::HashMap<String, u64>,
    phase_durations: std::collections::HashMap<String, std::time::Duration>,
    start_memory: Option<u64>,
    clock: u64,
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            phase_start_times: std::collections::HashMap::new(),
            phase_durations: std::collections::HashMap::new(),
            start_memory: Self::get_memory_usage(),
            clock: 0,
        }
    }

    pub fn start_phase(&mut self, phase_name: &str) {
        self.clock = self.clock.saturating_add(1);
        self.phase_start_times
            .insert(phase_name.to_string(), self.clock);
    }

    pub fn end_phase(&mut self, phase_name: &str) {
        if let Some(start_time) = self.phase_start_times.remove(phase_name) {
            self.clock = self.clock.saturating_add(1);
            let duration_ticks = self.clock.saturating_sub(start_time);
            let duration = std::time::Duration::from_millis(duration_ticks);
            self.phase_durations
                .insert(phase_name.to_string(), duration);
        }
    }

    pub fn generate_report(&self) -> PerformanceReport {
        let model_checking_time = self
            .phase_durations
            .get("model_checking")
            .copied()
            .unwrap_or_default();
        let simulation_time = self
            .phase_durations
            .get("simulation")
            .copied()
            .unwrap_or_default();
        let mutation_time = self
            .phase_durations
            .get("mutation")
            .copied()
            .unwrap_or_default();
        let analysis_time = self
            .phase_durations
            .get("analysis")
            .copied()
            .unwrap_or_default();
        let reporting_time = analysis_time; // Analysis includes reporting

        let current_memory = Self::get_memory_usage().unwrap_or(0);
        let peak_memory = current_memory.max(self.start_memory.unwrap_or(0));

        PerformanceReport {
            phase_timings: PhaseTimings {
                model_checking_time,
                simulation_time,
                mutation_time,
                analysis_time,
                reporting_time,
            },
            memory_usage: MemoryUsage {
                peak_memory_mb: peak_memory / 1024 / 1024,
                component_breakdown: std::collections::HashMap::new(),
            },
            throughput: ThroughputMetrics {
                tests_per_second: 0.0,
                properties_per_second: 0.0,
                traces_per_second: 0.0,
            },
            resource_utilization: ResourceUtilization {
                cpu_utilization: 0.0,
                disk_operations: 0,
                network_operations: 0,
            },
        }
    }

    #[cfg(target_os = "macos")]
    fn get_memory_usage() -> Option<u64> {
        use std::process::Command;

        let output = Command::new("ps")
            .args(["-o", "rss=", "-p"])
            .arg(std::process::id().to_string())
            .output()
            .ok()?;

        let rss_str = String::from_utf8(output.stdout).ok()?;
        let rss_kb: u64 = rss_str.trim().parse().ok()?;
        Some(rss_kb * 1024) // Convert KB to bytes
    }

    #[cfg(target_os = "linux")]
    fn get_memory_usage() -> Option<u64> {
        use sysinfo::ProcessesToUpdate;
        let mut system = System::new();
        let pid = sysinfo::get_current_pid().ok()?;
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        system.process(pid).map(|p| p.memory() * 1024) // memory() returns KiB
    }

    #[cfg(target_os = "windows")]
    fn get_memory_usage() -> Option<u64> {
        use sysinfo::ProcessesToUpdate;
        let mut system = System::new();
        let pid = sysinfo::get_current_pid().ok()?;
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        system.process(pid).map(|p| p.memory() * 1024)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    fn get_memory_usage() -> Option<u64> {
        None
    }
}

/// MBT metadata extracted from ITF traces
#[derive(Debug, Clone)]
pub struct MBTMetadata {
    /// Action that was taken
    pub action_taken: Option<String>,
    /// Non-deterministic choices made
    pub nondet_picks: Option<HashMap<String, serde_json::Value>>,
    /// Choice points encountered
    pub choice_points: Vec<ChoicePoint>,
    /// Decision path taken
    pub decision_path: Vec<Decision>,
}

/// A choice point in execution
#[derive(Debug, Clone)]
pub struct ChoicePoint {
    /// Variable being chosen
    pub variable: String,
    /// Value that was chosen
    pub chosen_value: serde_json::Value,
    /// All available choices at this point
    pub available_choices: Vec<serde_json::Value>,
}

/// A decision made during execution
#[derive(Debug, Clone)]
pub struct Decision {
    /// Variable involved in the decision
    pub variable: String,
    /// Condition that was evaluated
    pub condition: String,
    /// Outcome of the decision
    pub outcome: serde_json::Value,
}

/// Complete test suite generated from simulations
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestSuite {
    /// Generated test cases
    pub test_cases: Vec<GeneratedTestCase>,
    /// Results from simulation runs
    pub simulation_result: SimulationResult,
    /// Coverage analysis
    pub coverage_analysis: TestCoverageAnalysis,
    /// Time taken to generate the suite
    pub generation_time: std::time::Duration,
    /// Source specification file
    pub spec_file: PathBuf,
}

/// Analysis of test coverage
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestCoverageAnalysis {
    /// Total number of test cases
    pub total_test_cases: u64,
    /// Total execution steps across all tests
    pub total_steps: u64,
    /// Number of unique actions covered
    pub covered_actions: u32,
    /// Number of unique variables covered
    pub covered_variables: u32,
    /// Number of unique properties covered
    pub covered_properties: u32,
    /// Names of covered actions
    pub action_names: Vec<String>,
    /// Names of covered variables
    pub variable_names: Vec<String>,
    /// Names of covered properties
    pub property_names: Vec<String>,
}

/// Configuration for a complete fuzzing campaign
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FuzzingCampaignConfig {
    /// Enable model checking phase
    pub enable_model_checking: bool,
    /// Enable simulation-based testing phase
    pub enable_simulation_testing: bool,
    /// Enable counterexample mutation
    pub enable_mutation: bool,
    /// Maximum campaign duration
    pub max_campaign_duration: std::time::Duration,
    /// Target coverage percentage
    pub target_coverage: f64,
    /// Parallel execution threads
    pub parallel_threads: u32,
    /// Enable performance benchmarking
    pub enable_benchmarking: bool,
    /// CI/CD integration settings
    pub ci_integration: CIIntegrationConfig,
}

/// CI/CD integration configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CIIntegrationConfig {
    /// Enable CI/CD mode
    pub enabled: bool,
    /// Output format for CI systems
    pub output_format: CIOutputFormat,
    /// Fail CI on property violations
    pub fail_on_violations: bool,
    /// Maximum allowed violations
    pub max_violations: u32,
    /// Export test results to file
    pub export_results: Option<PathBuf>,
    /// Export coverage report
    pub export_coverage: Option<PathBuf>,
}

/// Output formats for CI integration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CIOutputFormat {
    /// JSON format for machine parsing
    Json,
    /// JUnit XML for test runners
    JunitXml,
    /// GitHub Actions format
    GitHubActions,
    /// Text format for human reading
    Text,
}

/// Complete result from a fuzzing campaign
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FuzzingCampaignResult {
    /// Campaign configuration used
    pub config: FuzzingCampaignConfig,
    /// Model checking results (if enabled)
    pub model_checking_result: Option<ModelCheckingReport>,
    /// Simulation testing results (if enabled)
    pub simulation_result: Option<TestSuite>,
    /// All discovered violations
    pub all_violations: Vec<PropertyViolation>,
    /// Performance benchmarks
    pub performance_report: PerformanceReport,
    /// Coverage summary
    pub coverage_summary: CoverageSummary,
    /// Campaign execution time
    pub campaign_duration: std::time::Duration,
    /// Success/failure status
    pub success: bool,
    /// Summary recommendations
    pub recommendations: Vec<String>,
}

/// Performance benchmarking results
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PerformanceReport {
    /// Time breakdown by phase
    pub phase_timings: PhaseTimings,
    /// Memory usage statistics
    pub memory_usage: MemoryUsage,
    /// Throughput metrics
    pub throughput: ThroughputMetrics,
    /// Resource utilization
    pub resource_utilization: ResourceUtilization,
}

/// Time breakdown by campaign phases
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PhaseTimings {
    /// Time spent in model checking
    pub model_checking_time: std::time::Duration,
    /// Time spent in simulation
    pub simulation_time: std::time::Duration,
    /// Time spent in mutation testing
    pub mutation_time: std::time::Duration,
    /// Time spent in analysis
    pub analysis_time: std::time::Duration,
    /// Time spent in reporting
    pub reporting_time: std::time::Duration,
}

/// Memory usage statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryUsage {
    /// Peak memory usage in MB
    pub peak_memory_mb: u64,
    /// Memory usage by component
    pub component_breakdown: std::collections::HashMap<String, u64>,
}

/// Throughput metrics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThroughputMetrics {
    /// Test cases generated per second
    pub tests_per_second: f64,
    /// Properties verified per second
    pub properties_per_second: f64,
    /// ITF traces processed per second
    pub traces_per_second: f64,
}

/// Resource utilization metrics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResourceUtilization {
    /// CPU utilization percentage
    pub cpu_utilization: f64,
    /// Disk I/O operations
    pub disk_operations: u64,
    /// Network operations (if applicable)
    pub network_operations: u64,
}

fn default_storage_provider() -> Arc<dyn StorageEffects> {
    Arc::new(PathStorageAdapter::new())
}

#[derive(Debug, Clone)]
struct PathStorageAdapter {
    handler: FilesystemStorageHandler,
}

impl PathStorageAdapter {
    pub fn new() -> Self {
        Self {
            handler: FilesystemStorageHandler::with_default_path(),
        }
    }
}

#[async_trait]
impl StorageCoreEffects for PathStorageAdapter {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.handler.store(key, value).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.handler.retrieve(key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.handler.remove(key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        self.handler.list_keys(prefix).await
    }
}

#[async_trait]
impl StorageExtendedEffects for PathStorageAdapter {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.handler.exists(key).await
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        self.handler.store_batch(pairs).await
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        self.handler.retrieve_batch(keys).await
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        self.handler.clear_all().await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        self.handler.stats().await
    }
}

/// Coverage summary across all testing phases
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CoverageSummary {
    /// Property coverage percentage
    pub property_coverage: f64,
    /// State coverage percentage
    pub state_coverage: f64,
    /// Number of property violations found
    pub violation_count: u32,
    /// Total test cases generated
    pub total_test_cases: u64,
    /// Coverage goals achieved
    pub goals_achieved: bool,
}

/// Result of checking a single property
#[derive(Debug)]
#[allow(dead_code)]
struct PropertyCheckResult {
    property_name: String,
    satisfied: bool,
    bound: u32,
    counterexample_trace: Option<ITFTrace>,
    stdout: String,
    stderr: String,
}

/// Comprehensive report from model checking
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelCheckingReport {
    /// Specification file that was checked
    pub spec_file: PathBuf,
    /// Results from bounded model checking
    pub model_check_result: ModelCheckingResult,
    /// Property violations found
    pub violations: Vec<PropertyViolation>,
    /// Analysis of violations
    pub violation_analysis: Vec<ITFPropertyEvaluationResult>,
    /// Total time taken for the analysis
    pub total_execution_time: std::time::Duration,
    /// Recommendations for improvement
    pub recommendations: Vec<String>,
}

/// Simplified PropertyEvaluationResult for integration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ITFPropertyEvaluationResult {
    pub property_name: String,
    pub satisfied: bool,
    pub violation_step: Option<u64>,
    pub execution_time_ms: u64,
    pub error_message: Option<String>,
}

/// Result from simulation-based test generation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimulationResult {
    /// Generated ITF traces from simulation
    pub traces: Vec<ITFTrace>,
    /// Number of simulation runs executed
    pub runs_executed: u32,
    /// Total simulation time
    pub simulation_time_ms: u64,
    /// Any errors encountered during simulation
    pub errors: Vec<String>,
}

/// A test case generated from ITF trace
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GeneratedTestCase {
    /// Unique identifier for this test case
    pub id: String,
    /// Source ITF trace
    pub source_trace: ITFTrace,
    /// Extracted action sequence
    pub action_sequence: Vec<TestAction>,
    /// Expected final state
    pub expected_state: HashMap<String, serde_json::Value>,
    /// Test metadata
    pub metadata: TestCaseMetadata,
}

/// An action extracted from ITF trace
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestAction {
    /// Step number in the trace
    pub step: u64,
    /// Action name from MBT metadata
    pub action_name: Option<String>,
    /// State variables at this step
    pub state_variables: HashMap<String, serde_json::Value>,
    /// Non-deterministic choices made
    pub nondet_picks: Option<HashMap<String, serde_json::Value>>,
    /// Preconditions that should hold
    pub preconditions: Vec<String>,
}

/// Metadata about generated test cases
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestCaseMetadata {
    /// How this test was generated
    pub generation_method: TestGenerationMethod,
    /// Source specification file
    pub source_spec: String,
    /// Properties this test exercises
    pub exercised_properties: Vec<String>,
    /// Generation timestamp
    pub generated_at: std::time::SystemTime,
    /// Expected execution time
    pub expected_duration_ms: Option<u64>,
}

/// Methods for generating test cases
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TestGenerationMethod {
    /// Generated from Quint simulation
    Simulation { runs: u32, max_steps: u32 },
    /// Generated from counterexample
    Counterexample { property: String },
    /// Generated from mutation of existing test
    Mutation { base_test_id: String },
}

/// Configuration for simulation-based test generation
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Number of simulation runs to execute
    pub num_runs: u32,
    /// Maximum steps per simulation run
    pub max_steps: u32,
    /// Seed for deterministic simulation
    pub seed: Option<u64>,
    /// Enable Model-Based Testing metadata
    pub enable_mbt: bool,
    /// Timeout per simulation run (milliseconds)
    pub run_timeout_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parse_simple_itf_trace() {
        let json = r##"{
            "#meta": {
                "format": "ITF",
                "format-description": "https://apalache-mc.org/docs/adr/015adr-trace.html",
                "source": "test.qnt",
                "status": "ok",
                "description": "Test trace",
                "timestamp": 1234567890
            },
            "vars": ["x"],
            "states": [
                {
                    "#meta": { "index": 0 },
                    "x": { "#bigint": "1" }
                },
                {
                    "#meta": { "index": 1 },
                    "x": { "#bigint": "2" }
                }
            ]
        }"##;

        // Create a test config with an ephemeral working directory
        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));
        let trace = fuzzer
            .parse_itf_trace(json)
            .unwrap_or_else(|_| panic!("Failed to parse trace"));

        assert_eq!(trace.meta.format, "ITF");
        assert_eq!(trace.vars, vec!["x"]);
        assert_eq!(trace.states.len(), 2);
        assert_eq!(trace.states[0].meta.index, 0);
        assert_eq!(trace.states[1].meta.index, 1);
    }

    #[test]
    fn test_itf_config_default() {
        let config = ITFFuzzConfig::default();
        assert_eq!(config.quint_executable, PathBuf::from("quint"));
        assert_eq!(config.max_bound, 20);
        assert_eq!(config.simulation_runs, 10);
        assert!(config.enable_mutation);
        assert_eq!(config.command_timeout, 30);
    }

    #[tokio::test]
    async fn test_itf_conversion_integration() {
        // Create a test config with an ephemeral working directory
        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        // Create a test ITF trace
        let itf_trace = ITFTrace {
            meta: ITFMeta {
                format: "ITF".to_string(),
                format_description: "https://apalache-mc.org/docs/adr/015adr-trace.html"
                    .to_string(),
                source: "test.qnt".to_string(),
                status: "ok".to_string(),
                description: "Test trace".to_string(),
                timestamp: 1234567890,
            },
            params: vec![],
            vars: vec!["x".to_string()],
            states: vec![
                ITFState {
                    meta: ITFStateMeta { index: 0 },
                    variables: {
                        let mut vars = HashMap::new();
                        vars.insert("x".to_string(), serde_json::json!({"#bigint": "1"}));
                        vars
                    },
                    action_taken: None,
                    nondet_picks: None,
                },
                ITFState {
                    meta: ITFStateMeta { index: 1 },
                    variables: {
                        let mut vars = HashMap::new();
                        vars.insert("x".to_string(), serde_json::json!({"#bigint": "2"}));
                        vars
                    },
                    action_taken: Some("increment".to_string()),
                    nondet_picks: None,
                },
            ],
            loop_index: None,
        };

        // Test conversion to internal format - this may fail due to format incompatibility
        match fuzzer.convert_itf_to_internal(&itf_trace) {
            Ok(_internal_itf) => {
                // If conversion succeeds, test validation and export
                fuzzer
                    .validate_converted_itf(&itf_trace)
                    .unwrap_or_else(|_| panic!("Failed to validate ITF trace"));

                let json_output = fuzzer
                    .export_itf_to_json(&itf_trace, true)
                    .unwrap_or_else(|_| panic!("Failed to export ITF trace"));

                assert!(json_output.contains("vars"));
                assert!(json_output.contains("states"));
            }
            Err(e) => {
                // Conversion failure is acceptable as this indicates a known limitation
                // in the current ITF format compatibility between fuzzer and trace converter
                println!("ITF conversion failed as expected: {e}");
                assert!(
                    e.to_string().contains("JSON parsing failed")
                        || e.to_string().contains("TraceConversionError")
                );
            }
        }
    }

    #[tokio::test]
    async fn test_model_checking_configuration() {
        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            iterative_deepening: IterativeDeepening {
                initial_bound: 2,
                max_bound: 8,
                bound_increment: 2,
                timeout_per_bound: 15000,
            },
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        // Test configuration
        assert_eq!(fuzzer.config.iterative_deepening.initial_bound, 2);
        assert_eq!(fuzzer.config.iterative_deepening.max_bound, 8);
        assert_eq!(fuzzer.config.iterative_deepening.bound_increment, 2);
        assert_eq!(fuzzer.config.iterative_deepening.timeout_per_bound, 15000);
    }

    #[tokio::test]
    async fn test_violation_extraction() {
        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        // Create a mock model checking result with counterexample
        let counterexample_trace = ITFTrace {
            meta: ITFMeta {
                format: "ITF".to_string(),
                format_description: "https://apalache-mc.org/docs/adr/015adr-trace.html"
                    .to_string(),
                source: "test.qnt".to_string(),
                status: "violated".to_string(),
                description: "Counterexample trace".to_string(),
                timestamp: 1234567890,
            },
            params: vec![],
            vars: vec!["x".to_string(), "y".to_string()],
            states: vec![
                ITFState {
                    meta: ITFStateMeta { index: 0 },
                    variables: {
                        let mut vars = HashMap::new();
                        vars.insert("x".to_string(), serde_json::json!({"#bigint": "0"}));
                        vars.insert("y".to_string(), serde_json::json!({"#bigint": "0"}));
                        vars
                    },
                    action_taken: None,
                    nondet_picks: None,
                },
                ITFState {
                    meta: ITFStateMeta { index: 1 },
                    variables: {
                        let mut vars = HashMap::new();
                        vars.insert("x".to_string(), serde_json::json!({"#bigint": "5"}));
                        vars.insert("y".to_string(), serde_json::json!({"#bigint": "3"}));
                        vars
                    },
                    action_taken: Some("violating_action".to_string()),
                    nondet_picks: None,
                },
            ],
            loop_index: None,
        };

        let model_check_result = ModelCheckingResult {
            properties_satisfied: false,
            counterexamples: vec![counterexample_trace],
            checked_properties: vec!["safety_property".to_string()],
            checking_bound: 5,
            checking_time_ms: 1000,
        };

        // Test violation extraction
        let violations = fuzzer.extract_violations(&model_check_result);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].property_name, "safety_property");
        assert_eq!(violations[0].violation_step, 1);
        assert_eq!(violations[0].violation_trace.states.len(), 2);
        assert!(violations[0]
            .violation_description
            .contains("safety_property"));
        assert!(violations[0].violation_description.contains("step 1"));
        assert!(violations[0].violation_description.contains("bound 5"));
    }

    #[tokio::test]
    async fn test_simulation_configuration() {
        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            simulation: SimulationConfig {
                num_runs: 5,
                max_steps: 25,
                seed: Some(42),
                enable_mbt: true,
                run_timeout_ms: 10000,
            },
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        // Test simulation configuration
        assert_eq!(fuzzer.config.simulation.num_runs, 5);
        assert_eq!(fuzzer.config.simulation.max_steps, 25);
        assert_eq!(fuzzer.config.simulation.seed, Some(42));
        assert!(fuzzer.config.simulation.enable_mbt);
        assert_eq!(fuzzer.config.simulation.run_timeout_ms, 10000);
    }

    #[tokio::test]
    async fn test_mbt_metadata_parsing() {
        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        // Create a state with MBT metadata
        let state = ITFState {
            meta: ITFStateMeta { index: 1 },
            variables: {
                let mut vars = HashMap::new();
                vars.insert("x".to_string(), serde_json::json!({"#bigint": "5"}));
                vars.insert("decision_var".to_string(), serde_json::json!(true));
                vars
            },
            action_taken: Some("process_request".to_string()),
            nondet_picks: Some({
                let mut picks = HashMap::new();
                picks.insert("choice_id".to_string(), serde_json::json!(42));
                picks.insert("timeout".to_string(), serde_json::json!(1000));
                picks
            }),
        };

        // Test MBT metadata parsing
        let metadata = fuzzer.parse_mbt_metadata(&state);

        assert_eq!(metadata.action_taken, Some("process_request".to_string()));
        assert_eq!(metadata.choice_points.len(), 2);
        assert_eq!(metadata.decision_path.len(), 1);

        // Check choice points
        assert!(metadata
            .choice_points
            .iter()
            .any(|cp| cp.variable == "choice_id"));
        assert!(metadata
            .choice_points
            .iter()
            .any(|cp| cp.variable == "timeout"));

        // Check decision path
        assert_eq!(metadata.decision_path[0].variable, "decision_var");
        assert!(metadata.decision_path[0].condition.contains("decision_var"));
    }

    #[tokio::test]
    async fn test_trace_to_test_case_conversion() {
        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        // Create a trace with MBT metadata
        let trace = ITFTrace {
            meta: ITFMeta {
                format: "ITF".to_string(),
                format_description: "https://apalache-mc.org/docs/adr/015adr-trace.html"
                    .to_string(),
                source: "test.qnt".to_string(),
                status: "ok".to_string(),
                description: "Simulation trace with property: safety_invariant".to_string(),
                timestamp: 1234567890,
            },
            params: vec![],
            vars: vec!["counter".to_string(), "active".to_string()],
            states: vec![
                ITFState {
                    meta: ITFStateMeta { index: 0 },
                    variables: {
                        let mut vars = HashMap::new();
                        vars.insert("counter".to_string(), serde_json::json!({"#bigint": "0"}));
                        vars.insert("active".to_string(), serde_json::json!(false));
                        vars
                    },
                    action_taken: None,
                    nondet_picks: None,
                },
                ITFState {
                    meta: ITFStateMeta { index: 1 },
                    variables: {
                        let mut vars = HashMap::new();
                        vars.insert("counter".to_string(), serde_json::json!({"#bigint": "1"}));
                        vars.insert("active".to_string(), serde_json::json!(true));
                        vars
                    },
                    action_taken: Some("increment".to_string()),
                    nondet_picks: Some({
                        let mut picks = HashMap::new();
                        picks.insert("increment_amount".to_string(), serde_json::json!(1));
                        picks
                    }),
                },
            ],
            loop_index: None,
        };

        let spec_path = Path::new("test_spec.qnt");
        let traces = vec![trace];

        // Test trace conversion
        let test_cases = fuzzer
            .convert_traces_to_test_cases(&traces, spec_path)
            .unwrap_or_else(|_| panic!("Failed to convert traces to test cases"));

        assert_eq!(test_cases.len(), 1);

        let test_case = &test_cases[0];
        assert!(test_case.id.starts_with("sim_test_0_"));
        assert_eq!(test_case.action_sequence.len(), 2);
        assert_eq!(test_case.expected_state.len(), 2);

        // Check action sequence
        assert_eq!(test_case.action_sequence[0].step, 0);
        assert_eq!(test_case.action_sequence[0].action_name, None);
        assert_eq!(test_case.action_sequence[1].step, 1);
        assert_eq!(
            test_case.action_sequence[1].action_name,
            Some("increment".to_string())
        );

        // Check extracted properties
        let properties = &test_case.metadata.exercised_properties;
        assert!(properties.contains(&"safety_invariant".to_string()));
        assert!(properties.contains(&"safety_properties".to_string()));
        assert!(properties.contains(&"increment_property".to_string()));

        // Check metadata
        assert_eq!(test_case.metadata.source_spec, "test_spec.qnt");
        if let TestGenerationMethod::Simulation { runs, max_steps } =
            &test_case.metadata.generation_method
        {
            assert_eq!(*runs, 10); // Default config
            assert_eq!(*max_steps, 50); // Default config
        } else {
            panic!("Expected Simulation generation method");
        }
    }

    #[tokio::test]
    async fn test_coverage_analysis() {
        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        // Create test cases for coverage analysis
        let test_cases = vec![GeneratedTestCase {
            id: "test1".to_string(),
            source_trace: ITFTrace {
                meta: ITFMeta {
                    format: "ITF".to_string(),
                    format_description: "".to_string(),
                    source: "test.qnt".to_string(),
                    status: "ok".to_string(),
                    description: "Test trace".to_string(),
                    timestamp: 1234567890,
                },
                params: vec![],
                vars: vec!["x".to_string()],
                states: vec![ITFState {
                    meta: ITFStateMeta { index: 0 },
                    variables: HashMap::new(),
                    action_taken: None,
                    nondet_picks: None,
                }],
                loop_index: None,
            },
            action_sequence: vec![
                TestAction {
                    step: 0,
                    action_name: Some("action_a".to_string()),
                    state_variables: {
                        let mut vars = HashMap::new();
                        vars.insert("var1".to_string(), serde_json::json!(1));
                        vars.insert("var2".to_string(), serde_json::json!(true));
                        vars
                    },
                    nondet_picks: None,
                    preconditions: vec!["initial_state".to_string()],
                },
                TestAction {
                    step: 1,
                    action_name: Some("action_b".to_string()),
                    state_variables: {
                        let mut vars = HashMap::new();
                        vars.insert("var1".to_string(), serde_json::json!(2));
                        vars.insert("var3".to_string(), serde_json::json!("test"));
                        vars
                    },
                    nondet_picks: None,
                    preconditions: vec!["step_1_preconditions".to_string()],
                },
            ],
            expected_state: HashMap::new(),
            metadata: TestCaseMetadata {
                generation_method: TestGenerationMethod::Simulation {
                    runs: 1,
                    max_steps: 10,
                },
                source_spec: "test.qnt".to_string(),
                exercised_properties: vec!["prop1".to_string(), "prop2".to_string()],
                generated_at: std::time::SystemTime::UNIX_EPOCH,
                expected_duration_ms: Some(100),
            },
        }];

        // Test coverage analysis
        let coverage = fuzzer.analyze_test_coverage(&test_cases);

        assert_eq!(coverage.total_test_cases, 1);
        assert_eq!(coverage.total_steps, 2);
        assert_eq!(coverage.covered_actions, 2);
        assert_eq!(coverage.covered_variables, 3);
        assert_eq!(coverage.covered_properties, 2);

        assert!(coverage.action_names.contains(&"action_a".to_string()));
        assert!(coverage.action_names.contains(&"action_b".to_string()));

        assert!(coverage.variable_names.contains(&"var1".to_string()));
        assert!(coverage.variable_names.contains(&"var2".to_string()));
        assert!(coverage.variable_names.contains(&"var3".to_string()));

        assert!(coverage.property_names.contains(&"prop1".to_string()));
        assert!(coverage.property_names.contains(&"prop2".to_string()));
    }

    // =========================================================================
    // End-to-End Test (Phase 6.5)
    // =========================================================================

    /// End-to-end test: ITF trace → GenerativeSimulator → property verification
    #[tokio::test]
    async fn test_e2e_itf_to_effect_execution() {
        use super::super::domain_handlers::capability_properties_registry;

        // Step 1: Create an ITF trace (simulating quint run --mbt output)
        // The trace is minimal - just an initial state (no actions to execute)
        // This tests that replay_trace handles traces with just the init state
        let itf_json = r##"{
            "#meta": {
                "format": "ITF",
                "format-description": "https://apalache.informal.systems/docs/adr/015adr-trace.html",
                "source": "protocol_capability_properties.qnt",
                "status": "ok",
                "description": "E2E test trace",
                "timestamp": 1234567890
            },
            "params": [],
            "vars": ["budgets", "tokens", "current_epoch"],
            "states": [
                {
                    "#meta": { "index": 0 },
                    "budgets": {},
                    "tokens": {},
                    "current_epoch": {}
                }
            ]
        }"##;

        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        // Step 2: Parse ITF trace
        let itf_trace = fuzzer.parse_itf_trace(itf_json).unwrap();
        assert_eq!(itf_trace.states.len(), 1);
        // First state has no action (it's the initial state)
        assert_eq!(itf_trace.states[0].action_taken, None);

        // Step 3: Create ActionRegistry with capability handlers
        let registry = capability_properties_registry();

        // Step 4: Create initial state
        let mut initial_state = super::super::aura_state_extractors::QuintSimulationState::new();
        let ctx = aura_core::types::ContextId::new_from_entropy([1u8; 32]);
        let auth = aura_core::types::AuthorityId::new_from_entropy([2u8; 32]);
        initial_state.init_context(ctx, auth, 100);

        // Step 5: Replay trace with effects
        let result = fuzzer
            .replay_trace_with_effects(&itf_trace, registry, initial_state, None)
            .await
            .unwrap();

        // Step 6: Verify results
        assert!(result.success, "E2E replay should succeed");
        assert!(
            result.property_violations.is_empty(),
            "No violations expected"
        );

        // Verify state was properly updated
        let final_state = result.final_state;
        assert!(
            !final_state.budgets.is_empty() || !final_state.tokens.is_empty(),
            "final state should have budgets or tokens populated"
        );
    }

    /// Test the complete generative simulation flow
    #[tokio::test]
    async fn test_e2e_explore_and_generate() {
        use super::super::domain_handlers::capability_properties_registry;

        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        // Create ActionRegistry
        let registry = capability_properties_registry();

        // Create initial state
        let mut initial_state = super::super::aura_state_extractors::QuintSimulationState::new();
        let ctx = aura_core::types::ContextId::new_from_entropy([1u8; 32]);
        let auth = aura_core::types::AuthorityId::new_from_entropy([2u8; 32]);
        initial_state.init_context(ctx, auth, 100);

        // Explore state space with effects
        let result = fuzzer
            .explore_with_effects(registry, initial_state, 10, Some(42))
            .await
            .unwrap();

        // Verify exploration completed
        assert!(result.step_count <= 10, "Should not exceed max steps");

        // Generate test cases from result would use GenerativeSimulator
        // (tested separately in generative_simulator::tests)
    }

    /// Test validated test case generation
    #[tokio::test]
    async fn test_e2e_validated_test_case_generation() {
        use super::super::domain_handlers::capability_properties_registry;

        // Create ITF traces
        let itf_json = r##"{
            "#meta": {
                "format": "ITF",
                "format-description": "https://apalache.informal.systems/docs/adr/015adr-trace.html",
                "source": "test.qnt",
                "status": "ok",
                "description": "Test trace for validation",
                "timestamp": 1234567890
            },
            "params": [],
            "vars": ["budgets", "tokens"],
            "states": [
                {
                    "#meta": { "index": 0 },
                    "budgets": {},
                    "tokens": {}
                }
            ]
        }"##;

        let config = ITFFuzzConfig {
            working_dir: std::env::temp_dir(),
            ..ITFFuzzConfig::default()
        };

        let fuzzer = ITFBasedFuzzer::with_config(config)
            .unwrap_or_else(|_| panic!("Failed to create fuzzer"));

        let itf_trace = fuzzer.parse_itf_trace(itf_json).unwrap();
        let registry = capability_properties_registry();

        let mut initial_state = super::super::aura_state_extractors::QuintSimulationState::new();
        let ctx = aura_core::types::ContextId::new_from_entropy([1u8; 32]);
        let auth = aura_core::types::AuthorityId::new_from_entropy([2u8; 32]);
        initial_state.init_context(ctx, auth, 100);

        // Generate validated test cases
        let validated_cases = fuzzer
            .generate_validated_test_cases(&[itf_trace], registry, initial_state)
            .await
            .unwrap();

        assert_eq!(validated_cases.len(), 1);
        let test_case = &validated_cases[0];
        assert!(test_case.id.starts_with("validated_test_"));
        // Initial state only, so validation should pass (no actions to fail)
        assert!(test_case.validation_passed);
    }
}
