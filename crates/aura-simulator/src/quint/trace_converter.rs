//! Trace Conversion for Quint Integration
//!
//! This module provides functionality to convert simulation execution traces
//! into formats compatible with Quint formal verification, enabling
//! trace-based property verification and temporal analysis.

use crate::quint::types::{
    PropertyEvaluationResult, QuintInvariant, QuintSpec, QuintTemporalProperty, QuintValue,
    ValidationResult,
};
use serde_json;
// Note: Testing module to be imported when module structure is finalized
// use crate::testing::{ExecutionTrace, PropertyViolation, ViolationDetectionReport};

// Types for converting Quint traces into simulator-friendly structures
#[derive(Debug, Clone)]
pub struct ExecutionTrace {
    pub steps: Vec<String>,
}

impl ExecutionTrace {
    pub fn new(capacity: u32) -> Self {
        Self {
            steps: Vec::with_capacity(capacity as usize),
        }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn add_state(&mut self, state: String) {
        self.steps.push(state);
    }

    pub fn get_all_states(&self) -> Vec<Box<dyn SimulationState>> {
        self.steps
            .iter()
            .filter_map(|raw| {
                serde_json::from_str::<std::collections::HashMap<String, QuintValue>>(raw)
                    .ok()
                    .map(|vars| {
                        let time = vars
                            .get("time")
                            .and_then(|v| match v {
                                QuintValue::Int(i) => Some(*i as u64),
                                _ => None,
                            })
                            .unwrap_or(0);
                        Box::new(JsonSimulationState { vars, time }) as Box<dyn SimulationState>
                    })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct JsonSimulationState {
    vars: std::collections::HashMap<String, QuintValue>,
    time: u64,
}

impl SimulationState for JsonSimulationState {
    fn get_variable(&self, name: &str) -> Option<QuintValue> {
        self.vars.get(name).cloned()
    }

    fn get_all_variables(&self) -> std::collections::HashMap<String, QuintValue> {
        self.vars.clone()
    }

    fn get_current_time(&self) -> u64 {
        self.time
    }

    fn get_metadata(&self) -> std::collections::HashMap<String, QuintValue> {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "participants".to_string(),
            QuintValue::Int(
                self.vars
                    .get("participants")
                    .and_then(|v| match v {
                        QuintValue::Int(i) => Some(*i),
                        _ => None,
                    })
                    .unwrap_or(0),
            ),
        );
        metadata
    }
}

#[derive(Debug, Clone)]
pub struct PropertyViolation {
    pub property_name: String,
    pub property_type: PropertyViolationType,
    pub violation_type: String,
    pub detected_at: u64,
    pub violation_state: SimulationStateSnapshot,
    pub violation_details: ViolationDetails,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct SimulationStateSnapshot {
    pub time: u64,
    pub tick: u64,
    pub participant_count: u32,
    pub active_sessions: u32,
    pub completed_sessions: u32,
    pub state_hash: String,
}

#[derive(Debug, Clone)]
pub struct ViolationDetectionReport {
    pub violations: Vec<PropertyViolation>,
}

// Additional types needed by test code
#[derive(Debug, Clone)]
pub enum PropertyViolationType {
    Invariant,
    Safety,
    Liveness,
}

#[derive(Debug, Clone)]
pub struct ViolationDetails {
    pub description: String,
    pub evidence: Vec<String>,
    pub potential_causes: Vec<String>,
    pub severity: ViolationSeverity,
    pub remediation_suggestions: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ViolationSeverity {
    Low,
    Medium,
    High,
    Critical,
}

use crate::quint::types::SimulationState;
use aura_core::AuraError;

pub type Result<T> = std::result::Result<T, AuraError>;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

/// ITF (Intermediate Trace Format) expression types for Quint integration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ItfExpression {
    /// Boolean value
    Bool(bool),
    /// Numeric value
    Number(serde_json::Number),
    /// Big integer value (serialized with #bigint marker)
    #[serde(serialize_with = "serialize_bigint")]
    BigInt { value: String },
    /// String value
    String(String),
    /// List of expressions
    List(Vec<ItfExpression>),
    /// Set of expressions
    Set { elements: Vec<ItfExpression> },
    /// Tuple of expressions
    Tuple { elements: Vec<ItfExpression> },
    /// Map with key-value pairs
    Map {
        pairs: Vec<(ItfExpression, ItfExpression)>,
    },
    /// Record with named fields
    Record(HashMap<String, ItfExpression>),
}

/// Custom serializer for BigInt expressions that includes the #bigint marker
fn serialize_bigint<S>(value: &String, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry("#bigint", value)?;
    map.end()
}

/// ITF trace state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItfState {
    /// Metadata for this state
    pub meta: Option<HashMap<String, serde_json::Value>>,
    /// State variables
    pub variables: HashMap<String, ItfExpression>,
}

/// ITF trace metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItfMetadata {
    /// Format version
    pub format_version: Option<String>,
    /// Source of the trace
    pub source: Option<String>,
    /// Creation timestamp
    pub created_at: Option<String>,
    /// Additional metadata
    pub additional: HashMap<String, serde_json::Value>,
}

/// Complete ITF trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItfTrace {
    /// Trace metadata
    pub meta: Option<ItfMetadata>,
    /// Trace parameters
    pub params: Option<HashMap<String, ItfExpression>>,
    /// Variable names
    pub vars: Vec<String>,
    /// Trace states
    pub states: Vec<ItfState>,
    /// Loop index for cyclic traces
    pub loop_index: Option<usize>,
}

/// ITF trace converter for Quint integration
pub struct ItfTraceConverter {
    /// Configuration for conversion
    _config: TraceConversionConfig,
}

impl Default for ItfTraceConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl ItfTraceConverter {
    /// Create a new ITF trace converter
    pub fn new() -> Self {
        Self {
            _config: TraceConversionConfig::default(),
        }
    }

    /// Validate an ITF trace
    pub fn validate_itf_trace(&self, trace: &ItfTrace) -> Result<()> {
        // Basic validation
        if trace.vars.is_empty() {
            return Err(AuraError::invalid(
                "ITF trace must have variables".to_string(),
            ));
        }

        for state in &trace.states {
            for var in &trace.vars {
                if !state.variables.contains_key(var) {
                    return Err(AuraError::invalid(format!("State missing variable: {var}")));
                }
            }
        }

        Ok(())
    }

    /// Serialize ITF trace to JSON
    pub fn serialize_itf_to_json(&self, trace: &ItfTrace, pretty: bool) -> Result<String> {
        let result = if pretty {
            serde_json::to_string_pretty(trace)
        } else {
            serde_json::to_string(trace)
        };
        result.map_err(|e| AuraError::invalid(format!("JSON serialization failed: {e}")))
    }

    /// Parse ITF trace from JSON
    pub fn parse_itf_from_json(&self, json: &str) -> Result<ItfTrace> {
        serde_json::from_str(json)
            .map_err(|e| AuraError::invalid(format!("JSON parsing failed: {e}")))
    }
}

/// Converts simulation execution traces to Quint-compatible formats
///
/// This converter transforms internal simulation state sequences into
/// formats that can be verified against Quint temporal properties
/// and invariants.
pub struct TraceConverter {
    /// Configuration for trace conversion
    config: TraceConversionConfig,
    /// Cache for repeated conversions
    conversion_cache: HashMap<String, QuintTrace>,
    /// Statistics about conversion performance
    conversion_stats: ConversionStatistics,
}

/// Configuration for trace conversion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConversionConfig {
    /// Maximum trace length to convert
    pub max_trace_length: u64,
    /// Whether to include detailed state information
    pub include_detailed_state: bool,
    /// Whether to include protocol-level events
    pub include_protocol_events: bool,
    /// Whether to include network events
    pub include_network_events: bool,
    /// Sampling rate for large traces (1.0 = no sampling)
    pub sampling_rate: f64,
    /// Whether to compress repeated states
    pub compress_repeated_states: bool,
}

impl Default for TraceConversionConfig {
    fn default() -> Self {
        Self {
            max_trace_length: 10000,
            include_detailed_state: true,
            include_protocol_events: true,
            include_network_events: true,
            sampling_rate: 1.0,
            compress_repeated_states: true,
        }
    }
}

/// Quint-compatible execution trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintTrace {
    /// Unique identifier for this trace
    pub trace_id: String,
    /// Sequence of Quint-compatible states
    pub states: Vec<QuintTraceState>,
    /// Events that occurred between states
    pub events: Vec<QuintTraceEvent>,
    /// Metadata about the trace
    pub metadata: QuintTraceMetadata,
}

/// Individual state in a Quint trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintTraceState {
    /// Step number in the trace
    pub step: u64,
    /// Simulation time when this state was captured
    pub time: u64,
    /// State variables mapped to Quint values
    pub variables: HashMap<String, QuintValue>,
    /// Protocol-specific state information
    pub protocol_state: QuintProtocolState,
    /// Network state information
    pub network_state: QuintNetworkState,
}

/// Protocol state in Quint-compatible format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintProtocolState {
    /// Active protocol sessions
    pub active_sessions: QuintValue,
    /// Current protocol phase
    pub current_phase: QuintValue,
    /// Protocol variables
    pub variables: HashMap<String, QuintValue>,
}

/// Network state in Quint-compatible format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintNetworkState {
    /// Current network partitions
    pub partitions: QuintValue,
    /// Message delivery statistics
    pub message_stats: HashMap<String, QuintValue>,
    /// Network failure conditions
    pub failure_conditions: HashMap<String, QuintValue>,
}

/// Event in a Quint trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintTraceEvent {
    /// Event identifier
    pub event_id: String,
    /// Type of event
    pub event_type: String,
    /// When the event occurred
    pub timestamp: u64,
    /// Event parameters
    pub parameters: HashMap<String, QuintValue>,
    /// State before the event
    pub pre_state: Option<String>,
    /// State after the event
    pub post_state: Option<String>,
}

/// Metadata about a Quint trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintTraceMetadata {
    /// When the trace was created
    pub created_at: u64,
    /// Duration of the trace (in simulation time)
    pub duration: u64,
    /// Number of states in the trace
    pub state_count: u64,
    /// Number of events in the trace
    pub event_count: u64,
    /// Quality metrics for the trace
    pub quality_metrics: TraceQualityMetrics,
    /// Source of the trace (simulation run ID, etc.)
    pub source: String,
}

/// Quality metrics for converted traces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceQualityMetrics {
    /// Completeness of state capture (0.0 to 1.0)
    pub state_completeness: f64,
    /// Event coverage (0.0 to 1.0)
    pub event_coverage: f64,
    /// Temporal consistency score (0.0 to 1.0)
    pub temporal_consistency: f64,
    /// Data fidelity score (0.0 to 1.0)
    pub data_fidelity: f64,
}

/// Statistics about trace conversion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionStatistics {
    /// Total traces converted
    pub traces_converted: u64,
    /// Total conversion time (milliseconds)
    pub total_conversion_time_ms: u64,
    /// Average conversion time per trace
    pub average_conversion_time_ms: f64,
    /// Cache hit rate
    pub cache_hit_rate: f64,
    /// Total states converted
    pub total_states_converted: u64,
    /// Total events converted
    pub total_events_converted: u64,
}

/// Result of trace conversion operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConversionResult {
    /// Converted Quint trace
    pub quint_trace: QuintTrace,
    /// Conversion performance metrics
    pub conversion_metrics: ConversionPerformanceMetrics,
    /// Any warnings or issues during conversion
    pub warnings: Vec<String>,
}

/// Performance metrics for individual trace conversions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionPerformanceMetrics {
    /// Time taken to convert this trace (milliseconds)
    pub conversion_time_ms: u64,
    /// Memory usage during conversion (bytes)
    pub memory_usage_bytes: u64,
    /// Compression ratio achieved
    pub compression_ratio: f64,
    /// Number of states processed
    pub states_processed: u64,
    /// Number of events processed
    pub events_processed: u64,
}

/// Fragment of a trace for focused analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceFragment {
    /// Fragment identifier
    pub fragment_id: String,
    /// Starting position in the original trace
    pub start_position: u64,
    /// Ending position in the original trace
    pub end_position: u64,
    /// States in this fragment
    pub states: Vec<QuintTraceState>,
    /// Events in this fragment
    pub events: Vec<QuintTraceEvent>,
    /// Reason for extracting this fragment
    pub extraction_reason: String,
}

impl TraceConverter {
    /// Create a new trace converter with default configuration
    pub fn new() -> Self {
        Self {
            config: TraceConversionConfig::default(),
            conversion_cache: HashMap::new(),
            conversion_stats: ConversionStatistics::new(),
        }
    }

    /// Create a trace converter with custom configuration
    pub fn with_config(config: TraceConversionConfig) -> Self {
        Self {
            config,
            conversion_cache: HashMap::new(),
            conversion_stats: ConversionStatistics::new(),
        }
    }

    /// Convert simulation execution trace to Quint format
    pub fn convert_trace(
        &mut self,
        execution_trace: &ExecutionTrace,
    ) -> Result<TraceConversionResult> {
        let start_time = crate::utils::time::current_unix_timestamp_millis();

        // Generate unique trace ID
        let trace_id = format!("trace_{start_time}");

        // Check cache first
        if let Some(cached_trace) = self.conversion_cache.get(&trace_id) {
            self.conversion_stats.cache_hit_rate = (self.conversion_stats.cache_hit_rate
                * (self.conversion_stats.traces_converted as f64)
                + 1.0)
                / ((self.conversion_stats.traces_converted + 1) as f64);

            return Ok(TraceConversionResult {
                quint_trace: cached_trace.clone(),
                conversion_metrics: ConversionPerformanceMetrics {
                    conversion_time_ms: 0, // Cache hit
                    memory_usage_bytes: 0,
                    compression_ratio: 1.0,
                    states_processed: cached_trace.states.len() as u64,
                    events_processed: cached_trace.events.len() as u64,
                },
                warnings: Vec::new(),
            });
        }

        let mut warnings = Vec::new();
        let mut quint_states = Vec::new();
        let mut quint_events = Vec::new();

        // Apply sampling if needed
        let states_to_process = if execution_trace.len() as u64 > self.config.max_trace_length {
            warnings.push(format!(
                "Trace length {} exceeds maximum {}, applying sampling",
                execution_trace.len(),
                self.config.max_trace_length
            ));
            self.sample_states(execution_trace)?
        } else {
            execution_trace.get_all_states()
        };

        // Convert each state
        for (index, sim_state) in states_to_process.iter().enumerate() {
            let quint_state = self.convert_simulation_state(sim_state.as_ref(), index as u64)?;

            // Apply compression if enabled
            if self.config.compress_repeated_states {
                if let Some(last_state) = quint_states.last() {
                    if !self.states_differ_significantly(last_state, &quint_state) {
                        continue; // Skip repeated state
                    }
                }
            }

            quint_states.push(quint_state);
        }

        // Generate events between states
        for i in 0..quint_states.len().saturating_sub(1) {
            let event =
                self.generate_transition_event(&quint_states[i], &quint_states[i + 1], i as u32)?;
            quint_events.push(event);
        }

        // Calculate quality metrics
        let quality_metrics = self.calculate_quality_metrics(&quint_states, &quint_events);

        // Create trace metadata
        let metadata = QuintTraceMetadata {
            created_at: start_time,
            duration: if let (Some(first), Some(last)) = (quint_states.first(), quint_states.last())
            {
                last.time - first.time
            } else {
                0
            },
            state_count: quint_states.len() as u64,
            event_count: quint_events.len() as u64,
            quality_metrics,
            source: trace_id.clone(),
        };

        let quint_trace = QuintTrace {
            trace_id: trace_id.clone(),
            states: quint_states,
            events: quint_events.clone(),
            metadata,
        };

        // Cache the result
        self.conversion_cache.insert(trace_id, quint_trace.clone());

        let end_time = crate::utils::time::current_unix_timestamp_millis();
        let conversion_time = end_time - start_time;

        // Update statistics
        self.conversion_stats.traces_converted += 1;
        self.conversion_stats.total_conversion_time_ms += conversion_time;
        self.conversion_stats.average_conversion_time_ms =
            self.conversion_stats.total_conversion_time_ms as f64
                / self.conversion_stats.traces_converted as f64;
        self.conversion_stats.total_states_converted += quint_trace.states.len() as u64;
        self.conversion_stats.total_events_converted += quint_trace.events.len() as u64;

        Ok(TraceConversionResult {
            quint_trace,
            conversion_metrics: ConversionPerformanceMetrics {
                conversion_time_ms: conversion_time,
                memory_usage_bytes: (states_to_process.len() * 1000 + quint_events.len() * 500)
                    as u64, // Simple estimation
                compression_ratio: 1.0,
                states_processed: states_to_process.len() as u64,
                events_processed: quint_events.len() as u64,
            },
            warnings,
        })
    }

    /// Extract trace fragment around a property violation
    pub fn extract_violation_fragment(
        &self,
        quint_trace: &QuintTrace,
        violation: &PropertyViolation,
        context_window: u32,
    ) -> Result<TraceFragment> {
        // Find the state corresponding to the violation
        let violation_position = quint_trace
            .states
            .iter()
            .position(|state| state.time == violation.violation_state.time)
            .unwrap_or(quint_trace.states.len() / 2); // Default to middle if not found

        let context_window_usize = context_window as usize;
        let start_position = violation_position.saturating_sub(context_window_usize);
        let end_position = std::cmp::min(
            violation_position + context_window_usize,
            quint_trace.states.len(),
        );

        let fragment_states = quint_trace.states[start_position..end_position].to_vec();
        let fragment_events = quint_trace
            .events
            .iter()
            .filter(|event| {
                let event_step = event
                    .parameters
                    .get("step")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0) as usize;
                event_step >= start_position && event_step < end_position
            })
            .cloned()
            .collect();

        Ok(TraceFragment {
            fragment_id: format!(
                "violation_{}_{}",
                violation.property_name, violation.detected_at
            ),
            start_position: start_position as u64,
            end_position: end_position as u64,
            states: fragment_states,
            events: fragment_events,
            extraction_reason: format!("Property violation: {}", violation.property_name),
        })
    }

    /// Extract multiple fragments for comprehensive analysis
    pub fn extract_analysis_fragments(
        &self,
        quint_trace: &QuintTrace,
        violation_report: &ViolationDetectionReport,
    ) -> Result<Vec<TraceFragment>> {
        let mut fragments = Vec::new();

        for violation in &violation_report.violations {
            let fragment = self.extract_violation_fragment(quint_trace, violation, 10)?;
            fragments.push(fragment);
        }

        // Add a full context fragment if there are multiple violations
        if violation_report.violations.len() > 1 {
            let full_fragment = TraceFragment {
                fragment_id: format!("full_context_{}", violation_report.violations.len()),
                start_position: 0,
                end_position: quint_trace.states.len() as u64,
                states: quint_trace.states.clone(),
                events: quint_trace.events.clone(),
                extraction_reason: "Full context for multiple violations".to_string(),
            };
            fragments.push(full_fragment);
        }

        Ok(fragments)
    }

    /// Verify trace against Quint properties
    pub fn verify_trace_properties(
        &self,
        quint_trace: &QuintTrace,
        spec: &QuintSpec,
    ) -> Result<ValidationResult> {
        let mut validation_result = ValidationResult::new();

        // Verify invariants at each state
        for invariant in &spec.invariants {
            let result = self.verify_invariant_on_trace(quint_trace, invariant)?;
            validation_result.add_result(result);
        }

        // Verify temporal properties across the trace
        for temporal_property in &spec.temporal_properties {
            let result = self.verify_temporal_property_on_trace(quint_trace, temporal_property)?;
            validation_result.add_result(result);
        }

        Ok(validation_result)
    }

    /// Get current conversion statistics
    pub fn get_conversion_statistics(&self) -> &ConversionStatistics {
        &self.conversion_stats
    }

    /// Clear conversion cache
    pub fn clear_cache(&mut self) {
        self.conversion_cache.clear();
    }

    // Private implementation methods

    /// Sample states from a large trace
    fn sample_states(
        &self,
        execution_trace: &ExecutionTrace,
    ) -> Result<Vec<Box<dyn SimulationState>>> {
        let all_states = execution_trace.get_all_states();
        let sample_size = (all_states.len() as f64 * self.config.sampling_rate) as usize;
        let step_size = all_states.len() / sample_size.max(1);

        let sampled: Vec<Box<dyn SimulationState>> = all_states
            .into_iter()
            .step_by(step_size.max(1))
            .take(sample_size)
            .collect();

        Ok(sampled)
    }

    /// Convert simulation state to Quint trace state
    fn convert_simulation_state(
        &self,
        sim_state: &dyn SimulationState,
        step: u64,
    ) -> Result<QuintTraceState> {
        let mut variables = HashMap::new();

        // Extract state variables from simulation state using trait methods
        let state_vars = sim_state.get_all_variables();
        for (key, value) in state_vars {
            variables.insert(key, value);
        }

        // Add step information
        variables.insert("step".to_string(), QuintValue::Int(step as i64));

        // Add current time
        variables.insert(
            "time".to_string(),
            QuintValue::Int(sim_state.get_current_time() as i64),
        );

        Ok(QuintTraceState {
            step,
            time: sim_state.get_current_time(),
            variables,
            protocol_state: QuintProtocolState {
                active_sessions: QuintValue::List(vec![]),
                current_phase: QuintValue::String("unknown_phase".to_string()),
                variables: HashMap::new(),
            },
            network_state: QuintNetworkState {
                partitions: QuintValue::List(vec![]),
                message_stats: HashMap::new(),
                failure_conditions: HashMap::new(),
            },
        })
    }

    /// Generate transition event between two states
    fn generate_transition_event(
        &self,
        from_state: &QuintTraceState,
        to_state: &QuintTraceState,
        index: u32,
    ) -> Result<QuintTraceEvent> {
        let mut parameters = HashMap::new();
        parameters.insert(
            "from_step".to_string(),
            QuintValue::Int(from_state.step as i64),
        );
        parameters.insert("to_step".to_string(), QuintValue::Int(to_state.step as i64));
        parameters.insert(
            "time_delta".to_string(),
            QuintValue::Int((to_state.time - from_state.time) as i64),
        );

        Ok(QuintTraceEvent {
            event_id: format!("transition_{index}"),
            event_type: "state_transition".to_string(),
            timestamp: to_state.time,
            parameters,
            pre_state: Some(format!("state_{}", from_state.step)),
            post_state: Some(format!("state_{}", to_state.step)),
        })
    }

    /// Check if two states differ significantly
    fn states_differ_significantly(
        &self,
        state1: &QuintTraceState,
        state2: &QuintTraceState,
    ) -> bool {
        // Simple implementation - in reality would check specific variable changes
        state1.step != state2.step
            || state1.protocol_state.current_phase != state2.protocol_state.current_phase
            || state1.variables.len() != state2.variables.len()
    }

    /// Calculate quality metrics for a converted trace
    fn calculate_quality_metrics(
        &self,
        states: &[QuintTraceState],
        events: &[QuintTraceEvent],
    ) -> TraceQualityMetrics {
        TraceQualityMetrics {
            state_completeness: if states.is_empty() { 0.0 } else { 1.0 },
            event_coverage: if events.is_empty() { 0.0 } else { 1.0 },
            temporal_consistency: self.calculate_temporal_consistency(states),
            // Assume full fidelity for converted traces; future versions can compute this from trace completeness
            data_fidelity: 0.95,
        }
    }

    /// Calculate temporal consistency score
    fn calculate_temporal_consistency(&self, states: &[QuintTraceState]) -> f64 {
        if states.len() < 2 {
            return 1.0;
        }

        let mut consistent_transitions = 0;
        let total_transitions = states.len() - 1;

        for i in 0..total_transitions {
            if states[i + 1].time >= states[i].time && states[i + 1].step > states[i].step {
                consistent_transitions += 1;
            }
        }

        consistent_transitions as f64 / total_transitions as f64
    }

    /// Verify an invariant across all states in the trace
    fn verify_invariant_on_trace(
        &self,
        trace: &QuintTrace,
        invariant: &QuintInvariant,
    ) -> Result<PropertyEvaluationResult> {
        let start_time = crate::utils::time::current_unix_timestamp_millis();

        // Check invariant at each state
        for state in &trace.states {
            let holds = self.evaluate_invariant_at_state(invariant, state)?;
            if !holds {
                let end_time = crate::utils::time::current_unix_timestamp_millis();
                return Ok(PropertyEvaluationResult {
                    property_name: invariant.name.clone(),
                    holds: false,
                    details: format!(
                        "Invariant '{}' violated at step {}",
                        invariant.name, state.step
                    ),
                    witness: None,
                    evaluation_time_ms: end_time - start_time,
                });
            }
        }

        let end_time = crate::utils::time::current_unix_timestamp_millis();
        Ok(PropertyEvaluationResult {
            property_name: invariant.name.clone(),
            holds: true,
            details: format!("Invariant '{}' holds across entire trace", invariant.name),
            witness: Some(format!("Verified across {} states", trace.states.len())),
            evaluation_time_ms: end_time - start_time,
        })
    }

    /// Verify a temporal property across the trace
    fn verify_temporal_property_on_trace(
        &self,
        trace: &QuintTrace,
        property: &QuintTemporalProperty,
    ) -> Result<PropertyEvaluationResult> {
        let start_time = crate::utils::time::current_unix_timestamp_millis();

        let holds = self.evaluate_temporal_property_on_trace(property, trace)?;

        let end_time = crate::utils::time::current_unix_timestamp_millis();
        Ok(PropertyEvaluationResult {
            property_name: property.name.clone(),
            holds,
            details: format!(
                "Temporal property '{}' evaluation: {}",
                property.name, holds
            ),
            witness: if holds {
                Some("Property satisfied by trace".to_string())
            } else {
                None
            },
            evaluation_time_ms: end_time - start_time,
        })
    }

    fn evaluate_invariant_at_state(
        &self,
        invariant: &QuintInvariant,
        state: &QuintTraceState,
    ) -> Result<bool> {
        if !invariant.enabled {
            return Ok(true);
        }

        if let Some(val) = state.variables.get(&invariant.expression) {
            return Ok(match val {
                QuintValue::Bool(b) => *b,
                QuintValue::Int(i) => *i != 0,
                QuintValue::String(s) => !s.is_empty(),
                QuintValue::List(list) => !list.is_empty(),
                QuintValue::Set(set) => !set.is_empty(),
                QuintValue::Map(map) => !map.is_empty(),
                QuintValue::Record(record) => !record.is_empty(),
            });
        }

        Ok(false)
    }

    fn evaluate_temporal_property_on_trace(
        &self,
        property: &QuintTemporalProperty,
        trace: &QuintTrace,
    ) -> Result<bool> {
        if !property.enabled {
            return Ok(true);
        }

        for state in &trace.states {
            let satisfied = if let Some(val) = state.variables.get(&property.expression) {
                matches!(val, QuintValue::Bool(true))
            } else {
                false
            };

            if !satisfied {
                return Ok(false);
            }
        }

        Ok(true)
    }

    // ===== Bidirectional Conversion: Fault History =====

    /// Convert fault injection history to Quint trace events
    ///
    /// Takes a sequence of fault injection records from the simulator
    /// and converts them to QuintTraceEvents that can be included
    /// in traces for verification against fault-tolerance properties.
    pub fn convert_fault_history_to_events(
        &self,
        fault_history: &[FaultInjectionRecord],
    ) -> Result<Vec<QuintTraceEvent>> {
        let mut events = Vec::with_capacity(fault_history.len());

        for (index, record) in fault_history.iter().enumerate() {
            let mut parameters = HashMap::new();
            parameters.insert(
                "fault_type".to_string(),
                QuintValue::String(record.fault_type.clone()),
            );
            parameters.insert(
                "target".to_string(),
                QuintValue::String(record.target.clone()),
            );
            parameters.insert(
                "duration_ms".to_string(),
                QuintValue::Int(record.duration_ms as i64),
            );
            parameters.insert(
                "severity".to_string(),
                QuintValue::String(record.severity.clone()),
            );

            // Include affected participants if available
            if !record.affected_participants.is_empty() {
                parameters.insert(
                    "affected_participants".to_string(),
                    QuintValue::List(
                        record
                            .affected_participants
                            .iter()
                            .map(|p| QuintValue::String(p.clone()))
                            .collect(),
                    ),
                );
            }

            events.push(QuintTraceEvent {
                event_id: format!("fault_{index}"),
                event_type: "fault_injection".to_string(),
                timestamp: record.timestamp,
                parameters,
                pre_state: record.pre_fault_state.clone(),
                post_state: record.post_fault_state.clone(),
            });
        }

        Ok(events)
    }

    /// Merge fault events into an existing Quint trace
    ///
    /// Inserts fault injection events at the appropriate time points
    /// in the trace, preserving temporal ordering.
    pub fn merge_fault_events_into_trace(
        &self,
        quint_trace: &mut QuintTrace,
        fault_events: Vec<QuintTraceEvent>,
    ) {
        // Merge and sort by timestamp
        quint_trace.events.extend(fault_events);
        quint_trace.events.sort_by_key(|e| e.timestamp);

        // Update metadata
        quint_trace.metadata.event_count = quint_trace.events.len() as u64;
    }

    // ===== Bidirectional Conversion: Journal Facts =====

    /// Convert journal facts to Quint trace states
    ///
    /// Takes a sequence of journal fact records and converts them
    /// to QuintTraceStates that can be verified against invariants.
    pub fn convert_journal_facts_to_states(
        &self,
        journal_facts: &[JournalFactRecord],
    ) -> Result<Vec<QuintTraceState>> {
        let mut states = Vec::with_capacity(journal_facts.len());

        for (step, fact) in journal_facts.iter().enumerate() {
            let mut variables = HashMap::new();

            // Core fact data
            variables.insert(
                "fact_type".to_string(),
                QuintValue::String(fact.fact_type.clone()),
            );
            variables.insert(
                "authority_id".to_string(),
                QuintValue::String(fact.authority_id.clone()),
            );
            variables.insert(
                "context_id".to_string(),
                QuintValue::String(fact.context_id.clone()),
            );
            variables.insert("step".to_string(), QuintValue::Int(step as i64));
            variables.insert("time".to_string(), QuintValue::Int(fact.timestamp as i64));

            // Include fact-specific data
            for (key, value) in &fact.data {
                variables.insert(key.clone(), value.clone());
            }

            // Include causal dependencies
            if !fact.causal_deps.is_empty() {
                variables.insert(
                    "causal_deps".to_string(),
                    QuintValue::List(
                        fact.causal_deps
                            .iter()
                            .map(|d| QuintValue::String(d.clone()))
                            .collect(),
                    ),
                );
            }

            states.push(QuintTraceState {
                step: step as u64,
                time: fact.timestamp,
                variables,
                protocol_state: QuintProtocolState {
                    active_sessions: QuintValue::List(vec![]),
                    current_phase: QuintValue::String(fact.fact_type.clone()),
                    variables: HashMap::new(),
                },
                network_state: QuintNetworkState {
                    partitions: QuintValue::List(vec![]),
                    message_stats: HashMap::new(),
                    failure_conditions: HashMap::new(),
                },
            });
        }

        Ok(states)
    }

    /// Convert Quint trace states back to journal-compatible records
    ///
    /// This enables round-trip verification: simulation -> Quint -> simulation
    pub fn convert_states_to_journal_facts(
        &self,
        quint_states: &[QuintTraceState],
    ) -> Result<Vec<JournalFactRecord>> {
        let mut facts = Vec::with_capacity(quint_states.len());

        for state in quint_states {
            let fact_type = state
                .variables
                .get("fact_type")
                .and_then(|v| v.as_string())
                .unwrap_or("unknown")
                .to_string();

            let authority_id = state
                .variables
                .get("authority_id")
                .and_then(|v| v.as_string())
                .unwrap_or("")
                .to_string();

            let context_id = state
                .variables
                .get("context_id")
                .and_then(|v| v.as_string())
                .unwrap_or("")
                .to_string();

            let causal_deps = state
                .variables
                .get("causal_deps")
                .and_then(|v| match v {
                    QuintValue::List(list) => Some(
                        list.iter()
                            .filter_map(|item| item.as_string().map(String::from))
                            .collect(),
                    ),
                    _ => None,
                })
                .unwrap_or_default();

            // Extract remaining data excluding metadata fields
            let mut data = HashMap::new();
            for (key, value) in &state.variables {
                if ![
                    "fact_type",
                    "authority_id",
                    "context_id",
                    "step",
                    "time",
                    "causal_deps",
                ]
                .contains(&key.as_str())
                {
                    data.insert(key.clone(), value.clone());
                }
            }

            facts.push(JournalFactRecord {
                fact_type,
                authority_id,
                context_id,
                timestamp: state.time,
                data,
                causal_deps,
            });
        }

        Ok(facts)
    }

    /// Export trace to ITF format for Quint verification
    ///
    /// Creates an ITF trace that can be fed back to Quint for
    /// property verification against the formal specification.
    pub fn export_to_itf(&self, quint_trace: &QuintTrace) -> Result<ItfTrace> {
        let mut itf_states = Vec::with_capacity(quint_trace.states.len());

        for state in &quint_trace.states {
            let mut itf_variables = HashMap::new();

            for (key, value) in &state.variables {
                itf_variables.insert(key.clone(), self.quint_value_to_itf(value)?);
            }

            itf_states.push(ItfState {
                meta: Some(HashMap::from([(
                    "index".to_string(),
                    serde_json::Value::Number(state.step.into()),
                )])),
                variables: itf_variables,
            });
        }

        Ok(ItfTrace {
            meta: Some(ItfMetadata {
                format_version: Some("1.0".to_string()),
                source: Some(quint_trace.metadata.source.clone()),
                created_at: Some(format!("{}", quint_trace.metadata.created_at)),
                additional: HashMap::new(),
            }),
            params: None,
            vars: quint_trace
                .states
                .first()
                .map(|s| s.variables.keys().cloned().collect())
                .unwrap_or_default(),
            states: itf_states,
            loop_index: None,
        })
    }

    /// Convert QuintValue to ITF expression
    #[allow(clippy::only_used_in_recursion)]
    fn quint_value_to_itf(&self, value: &QuintValue) -> Result<ItfExpression> {
        Ok(match value {
            QuintValue::Bool(b) => ItfExpression::Bool(*b),
            QuintValue::Int(i) => ItfExpression::int(*i),
            QuintValue::String(s) => ItfExpression::String(s.clone()),
            QuintValue::List(list) => {
                let elements: Vec<ItfExpression> = list
                    .iter()
                    .map(|v| self.quint_value_to_itf(v))
                    .collect::<Result<Vec<_>>>()?;
                ItfExpression::List(elements)
            }
            QuintValue::Set(set) => {
                let elements: Vec<ItfExpression> = set
                    .iter()
                    .map(|v| self.quint_value_to_itf(v))
                    .collect::<Result<Vec<_>>>()?;
                ItfExpression::Set { elements }
            }
            QuintValue::Map(map) => {
                let pairs: Vec<(ItfExpression, ItfExpression)> = map
                    .iter()
                    .map(|(k, v)| {
                        Ok((
                            ItfExpression::String(k.clone()),
                            self.quint_value_to_itf(v)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                ItfExpression::Map { pairs }
            }
            QuintValue::Record(record) => {
                let mut itf_record = HashMap::new();
                for (k, v) in record {
                    itf_record.insert(k.clone(), self.quint_value_to_itf(v)?);
                }
                ItfExpression::Record(itf_record)
            }
        })
    }
}

/// Record of a fault injection for trace conversion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultInjectionRecord {
    /// Type of fault (network_delay, message_drop, partition, byzantine, etc.)
    pub fault_type: String,
    /// Target of the fault (device ID, connection, etc.)
    pub target: String,
    /// When the fault was injected
    pub timestamp: u64,
    /// Duration of the fault in milliseconds
    pub duration_ms: u64,
    /// Severity level
    pub severity: String,
    /// Participants affected by this fault
    pub affected_participants: Vec<String>,
    /// State reference before the fault
    pub pre_fault_state: Option<String>,
    /// State reference after the fault
    pub post_fault_state: Option<String>,
}

/// Record of a journal fact for trace conversion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalFactRecord {
    /// Type of the fact (e.g., "commitment", "attestation", "delta")
    pub fact_type: String,
    /// Authority that produced this fact
    pub authority_id: String,
    /// Context in which the fact was produced
    pub context_id: String,
    /// Timestamp when the fact was recorded
    pub timestamp: u64,
    /// Fact-specific data as Quint values
    pub data: HashMap<String, QuintValue>,
    /// Causal dependencies (hashes of prior facts)
    pub causal_deps: Vec<String>,
}

impl ConversionStatistics {
    fn new() -> Self {
        Self {
            traces_converted: 0,
            total_conversion_time_ms: 0,
            average_conversion_time_ms: 0.0,
            cache_hit_rate: 0.0,
            total_states_converted: 0,
            total_events_converted: 0,
        }
    }

    /// Calculate approximate memory usage of a Quint trace
    ///
    /// Estimates memory consumption by analyzing the serialized size of the trace
    /// data structures and their constituent elements.
    pub fn calculate_memory_usage(quint_trace: &QuintTrace) -> u64 {
        let mut total_bytes = 0u64;

        // Calculate base structure size
        total_bytes += std::mem::size_of::<QuintTrace>() as u64;

        // Calculate trace ID string size
        total_bytes += quint_trace.trace_id.len() as u64;

        // Calculate metadata size
        total_bytes += std::mem::size_of_val(&quint_trace.metadata) as u64;
        total_bytes += quint_trace.metadata.source.len() as u64;

        // Calculate states size
        for state in &quint_trace.states {
            total_bytes += std::mem::size_of_val(state) as u64;
            total_bytes += 8; // For the step number (u64)

            // Calculate state variables size
            for (key, value) in &state.variables {
                total_bytes += key.len() as u64;
                total_bytes += match value {
                    super::types::QuintValue::String(s) => s.len() as u64,
                    super::types::QuintValue::Int(_) => 8,
                    super::types::QuintValue::Bool(_) => 1,
                    super::types::QuintValue::List(arr) => arr.len() as u64 * 8,
                    super::types::QuintValue::Map(obj) => obj.len() as u64 * 16,
                    super::types::QuintValue::Record(obj) => obj.len() as u64 * 16,
                    super::types::QuintValue::Set(set) => set.len() as u64 * 8,
                };
            }
        }

        // Calculate events size
        for event in &quint_trace.events {
            total_bytes += std::mem::size_of_val(event) as u64;
            total_bytes += event.event_id.len() as u64;
            total_bytes += event.event_type.len() as u64;

            // Calculate event parameters size
            for (key, value) in &event.parameters {
                total_bytes += key.len() as u64;
                total_bytes += match value {
                    super::types::QuintValue::String(s) => s.len() as u64,
                    super::types::QuintValue::Int(_) => 8,
                    super::types::QuintValue::Bool(_) => 1,
                    super::types::QuintValue::List(arr) => arr.len() as u64 * 8,
                    super::types::QuintValue::Map(obj) => obj.len() as u64 * 16,
                    super::types::QuintValue::Record(obj) => obj.len() as u64 * 16,
                    super::types::QuintValue::Set(set) => set.len() as u64 * 8,
                };
            }
        }

        total_bytes
    }
}

impl Default for TraceConverter {
    fn default() -> Self {
        Self::new()
    }
}

// Helper trait for ITF expression creation in tests and examples
impl ItfExpression {
    /// Create an ITF integer expression (uses Number for small ints, BigInt for large)
    pub fn int(value: i64) -> Self {
        if value >= i32::MIN as i64 && value <= i32::MAX as i64 {
            ItfExpression::Number(serde_json::Number::from(value))
        } else {
            ItfExpression::BigInt {
                value: value.to_string(),
            }
        }
    }

    /// Create an ITF string expression
    pub fn string(value: impl Into<String>) -> Self {
        ItfExpression::String(value.into())
    }

    /// Create an ITF boolean expression
    pub fn bool(value: bool) -> Self {
        ItfExpression::Bool(value)
    }

    /// Create an ITF list expression
    pub fn list(elements: Vec<ItfExpression>) -> Self {
        ItfExpression::List(elements)
    }

    /// Create an ITF set expression
    pub fn set(elements: Vec<ItfExpression>) -> Self {
        ItfExpression::Set { elements }
    }

    /// Create an ITF tuple expression
    pub fn tuple(elements: Vec<ItfExpression>) -> Self {
        ItfExpression::Tuple { elements }
    }

    /// Create an ITF map expression
    pub fn map(pairs: Vec<(ItfExpression, ItfExpression)>) -> Self {
        ItfExpression::Map { pairs }
    }

    /// Create an ITF record expression
    pub fn record(fields: HashMap<String, ItfExpression>) -> Self {
        ItfExpression::Record(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock types for testing
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    pub struct TestSimulationState {
        pub tick: u64,
        pub time: u64,
        pub variables: HashMap<String, String>,
        pub protocol_state: ProtocolExecutionState,
        pub participants: Vec<String>,
        pub network_state: NetworkStateSnapshot,
    }

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    pub struct NetworkStateSnapshot {
        pub partitions: Vec<String>,
        pub message_stats: MessageDeliveryStats,
        pub failure_conditions: NetworkFailureConditions,
    }

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    pub struct MessageDeliveryStats {
        pub messages_sent: u64,
        pub messages_delivered: u64,
        pub messages_dropped: u64,
        pub average_latency_ms: f64,
    }

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    pub struct NetworkFailureConditions {
        pub partitions_active: bool,
        pub failure_rate: f64,
        pub drop_rate: f64,
        pub latency_range_ms: (u64, u64),
    }

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    pub struct ProtocolExecutionState {
        pub active_sessions: Vec<String>,
        pub completed_sessions: Vec<String>,
        pub queued_protocols: Vec<String>,
    }

    impl TestSimulationState {
        #[allow(dead_code)]
        pub fn mock() -> Self {
            Self {
                tick: 1,
                time: 1000,
                variables: HashMap::new(),
                protocol_state: ProtocolExecutionState {
                    active_sessions: Vec::new(),
                    completed_sessions: Vec::new(),
                    queued_protocols: Vec::new(),
                },
                participants: Vec::new(),
                network_state: NetworkStateSnapshot {
                    partitions: Vec::new(),
                    message_stats: MessageDeliveryStats {
                        messages_sent: 0,
                        messages_delivered: 0,
                        messages_dropped: 0,
                        average_latency_ms: 0.0,
                    },
                    failure_conditions: NetworkFailureConditions {
                        drop_rate: 0.0,
                        latency_range_ms: (0, 100),
                        partitions_active: false,
                        failure_rate: 0.0,
                    },
                },
            }
        }
    }

    impl SimulationState for TestSimulationState {
        fn get_variable(&self, name: &str) -> Option<QuintValue> {
            match name {
                "tick" => Some(QuintValue::Int(self.tick as i64)),
                "time" => Some(QuintValue::Int(self.time as i64)),
                _ => self
                    .variables
                    .get(name)
                    .map(|v| QuintValue::String(v.clone())),
            }
        }

        fn get_all_variables(&self) -> std::collections::HashMap<String, QuintValue> {
            let mut vars = std::collections::HashMap::new();
            vars.insert("tick".to_string(), QuintValue::Int(self.tick as i64));
            vars.insert("time".to_string(), QuintValue::Int(self.time as i64));

            for (k, v) in &self.variables {
                vars.insert(k.clone(), QuintValue::String(v.clone()));
            }

            vars
        }

        fn get_current_time(&self) -> u64 {
            self.time
        }

        fn get_metadata(&self) -> std::collections::HashMap<String, QuintValue> {
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "participants_count".to_string(),
                QuintValue::Int(self.participants.len() as i64),
            );
            metadata.insert(
                "active_sessions_count".to_string(),
                QuintValue::Int(self.protocol_state.active_sessions.len() as i64),
            );
            metadata
        }
    }

    #[test]
    fn test_trace_converter_creation() {
        let converter = TraceConverter::new();
        assert_eq!(converter.conversion_stats.traces_converted, 0);
    }

    #[test]
    fn test_convert_simple_trace() {
        let mut converter = TraceConverter::new();
        let mut execution_trace = ExecutionTrace::new(10);

        let _state = TestSimulationState {
            tick: 1,
            time: 1000,
            variables: HashMap::new(),
            protocol_state: ProtocolExecutionState {
                active_sessions: Vec::new(),
                completed_sessions: Vec::new(),
                queued_protocols: Vec::new(),
            },
            participants: Vec::new(),
            network_state: NetworkStateSnapshot {
                partitions: Vec::new(),
                message_stats: MessageDeliveryStats {
                    messages_sent: 0,
                    messages_delivered: 0,
                    messages_dropped: 0,
                    average_latency_ms: 0.0,
                },
                failure_conditions: NetworkFailureConditions {
                    drop_rate: 0.0,
                    latency_range_ms: (0, 100),
                    partitions_active: false,
                    failure_rate: 0.0,
                },
            },
        };

        // Serialize state as JSON for proper deserialization
        let state_json = serde_json::to_string(&HashMap::<String, QuintValue>::new())
            .expect("Failed to serialize empty state");
        execution_trace.add_state(state_json);

        let result = converter.convert_trace(&execution_trace);
        assert!(result.is_ok());

        let conversion_result = result.unwrap();
        assert_eq!(conversion_result.quint_trace.states.len(), 1);
        assert_eq!(conversion_result.quint_trace.events.len(), 0); // No transitions with single state
    }

    #[test]
    fn test_trace_fragment_extraction() {
        let converter = TraceConverter::new();

        // Create a simple QuintTrace
        let quint_trace = QuintTrace {
            trace_id: "test_trace".to_string(),
            states: vec![QuintTraceState {
                step: 0,
                time: 1000,
                variables: HashMap::new(),
                protocol_state: QuintProtocolState {
                    active_sessions: QuintValue::List(Vec::new()),
                    current_phase: QuintValue::String("phase1".to_string()),
                    variables: HashMap::new(),
                },
                network_state: QuintNetworkState {
                    partitions: QuintValue::List(Vec::new()),
                    message_stats: HashMap::new(),
                    failure_conditions: HashMap::new(),
                },
            }],
            events: Vec::new(),
            metadata: QuintTraceMetadata {
                created_at: 0,
                duration: 0,
                state_count: 1,
                event_count: 0,
                quality_metrics: TraceQualityMetrics {
                    state_completeness: 1.0,
                    event_coverage: 1.0,
                    temporal_consistency: 1.0,
                    data_fidelity: 1.0,
                },
                source: "test".to_string(),
            },
        };

        // Create a mock violation
        let violation = PropertyViolation {
            property_name: "test_property".to_string(),
            property_type: PropertyViolationType::Invariant,
            violation_type: "invariant_violation".to_string(),
            violation_state: SimulationStateSnapshot {
                tick: 1,
                time: 1000,
                participant_count: 0,
                active_sessions: 0,
                completed_sessions: 0,
                state_hash: "test_state".to_string(),
            },
            violation_details: ViolationDetails {
                description: "Test violation".to_string(),
                evidence: Vec::new(),
                potential_causes: Vec::new(),
                severity: ViolationSeverity::Medium,
                remediation_suggestions: Vec::new(),
            },
            confidence: 0.9,
            detected_at: 1000,
        };

        let fragment = converter.extract_violation_fragment(&quint_trace, &violation, 5);
        assert!(fragment.is_ok());

        let fragment = fragment.unwrap();
        assert_eq!(fragment.states.len(), 1);
        assert!(fragment.extraction_reason.contains("test_property"));
    }

    #[test]
    fn test_conversion_config() {
        let config = TraceConversionConfig {
            max_trace_length: 1000,
            include_detailed_state: false,
            sampling_rate: 0.5,
            ..Default::default()
        };

        let converter = TraceConverter::with_config(config);
        assert_eq!(converter.config.max_trace_length, 1000);
        assert!(!converter.config.include_detailed_state);
        assert_eq!(converter.config.sampling_rate, 0.5);
    }

    #[test]
    fn test_itf_comprehensive_features() {
        let converter = ItfTraceConverter::new();

        // Test complete ITF functionality
        let test_trace = ItfTrace {
            meta: Some(ItfMetadata {
                format_version: Some("1.0".to_string()),
                source: Some("aura-simulator".to_string()),
                created_at: Some("2023-01-01T00:00:00Z".to_string()),
                additional: HashMap::new(),
            }),
            params: None,
            vars: vec!["x".to_string(), "y".to_string()],
            states: vec![ItfState {
                meta: None,
                variables: {
                    let mut vars = HashMap::new();
                    vars.insert(
                        "x".to_string(),
                        ItfExpression::BigInt {
                            value: "123456789".to_string(),
                        },
                    );
                    vars.insert("y".to_string(), ItfExpression::Bool(true));
                    vars
                },
            }],
            loop_index: None,
        };

        // Test validation
        assert!(converter.validate_itf_trace(&test_trace).is_ok());

        // Test JSON serialization
        let json = converter.serialize_itf_to_json(&test_trace, true).unwrap();
        assert!(json.contains("#bigint"));

        // Test parsing back
        let parsed = converter.parse_itf_from_json(&json).unwrap();
        assert_eq!(parsed.vars.len(), 2);
    }
}
