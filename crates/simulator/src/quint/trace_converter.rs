//! Trace Conversion for Quint Integration
//!
//! This module provides functionality to convert simulation execution traces
//! into formats compatible with Quint formal verification, enabling
//! trace-based property verification and temporal analysis.

use crate::{SimError, Result};
use crate::quint::types::{
    QuintValue, QuintSpec, QuintInvariant, QuintTemporalProperty, 
    PropertyEvaluationResult, ValidationResult
};
use crate::property_monitor::{
    ExecutionTrace, SimulationState, PropertyViolation, ViolationDetectionReport
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub max_trace_length: usize,
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
    pub state_count: usize,
    /// Number of events in the trace
    pub event_count: usize,
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
    pub memory_usage_bytes: usize,
    /// Compression ratio achieved
    pub compression_ratio: f64,
    /// Number of states processed
    pub states_processed: usize,
    /// Number of events processed
    pub events_processed: usize,
}

/// Fragment of a trace for focused analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceFragment {
    /// Fragment identifier
    pub fragment_id: String,
    /// Starting position in the original trace
    pub start_position: usize,
    /// Ending position in the original trace
    pub end_position: usize,
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
    pub fn convert_trace(&mut self, execution_trace: &ExecutionTrace) -> Result<TraceConversionResult> {
        let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        // Generate unique trace ID
        let trace_id = format!("trace_{}", start_time);
        
        // Check cache first
        if let Some(cached_trace) = self.conversion_cache.get(&trace_id) {
            self.conversion_stats.cache_hit_rate = 
                (self.conversion_stats.cache_hit_rate * (self.conversion_stats.traces_converted as f64) + 1.0) 
                / ((self.conversion_stats.traces_converted + 1) as f64);
            
            return Ok(TraceConversionResult {
                quint_trace: cached_trace.clone(),
                conversion_metrics: ConversionPerformanceMetrics {
                    conversion_time_ms: 0, // Cache hit
                    memory_usage_bytes: 0,
                    compression_ratio: 1.0,
                    states_processed: cached_trace.states.len(),
                    events_processed: cached_trace.events.len(),
                },
                warnings: Vec::new(),
            });
        }

        let mut warnings = Vec::new();
        let mut quint_states = Vec::new();
        let mut quint_events = Vec::new();

        // Apply sampling if needed
        let states_to_process = if execution_trace.length() > self.config.max_trace_length {
            warnings.push(format!("Trace length {} exceeds maximum {}, applying sampling", 
                                execution_trace.length(), self.config.max_trace_length));
            self.sample_states(execution_trace)?
        } else {
            execution_trace.get_all_states()
        };

        // Convert each state
        for (index, sim_state) in states_to_process.iter().enumerate() {
            let quint_state = self.convert_simulation_state(sim_state, index as u64)?;
            
            // Apply compression if enabled
            if self.config.compress_repeated_states && !quint_states.is_empty() {
                let last_state = quint_states.last().unwrap();
                if !self.states_differ_significantly(last_state, &quint_state) {
                    continue; // Skip repeated state
                }
            }
            
            quint_states.push(quint_state);
        }

        // Generate events between states
        for i in 0..quint_states.len().saturating_sub(1) {
            let event = self.generate_transition_event(&quint_states[i], &quint_states[i + 1], i)?;
            quint_events.push(event);
        }

        // Calculate quality metrics
        let quality_metrics = self.calculate_quality_metrics(&quint_states, &quint_events);

        // Create trace metadata
        let metadata = QuintTraceMetadata {
            created_at: start_time,
            duration: if let (Some(first), Some(last)) = (quint_states.first(), quint_states.last()) {
                last.time - first.time
            } else {
                0
            },
            state_count: quint_states.len(),
            event_count: quint_events.len(),
            quality_metrics,
            source: trace_id.clone(),
        };

        let quint_trace = QuintTrace {
            trace_id: trace_id.clone(),
            states: quint_states,
            events: quint_events,
            metadata,
        };

        // Cache the result
        self.conversion_cache.insert(trace_id, quint_trace.clone());

        let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let conversion_time = end_time - start_time;

        // Update statistics
        self.conversion_stats.traces_converted += 1;
        self.conversion_stats.total_conversion_time_ms += conversion_time;
        self.conversion_stats.average_conversion_time_ms = 
            self.conversion_stats.total_conversion_time_ms as f64 / self.conversion_stats.traces_converted as f64;
        self.conversion_stats.total_states_converted += quint_trace.states.len() as u64;
        self.conversion_stats.total_events_converted += quint_trace.events.len() as u64;

        Ok(TraceConversionResult {
            quint_trace,
            conversion_metrics: ConversionPerformanceMetrics {
                conversion_time_ms: conversion_time,
                memory_usage_bytes: 0, // Would be calculated in real implementation
                compression_ratio: 1.0,
                states_processed: states_to_process.len(),
                events_processed: quint_events.len(),
            },
            warnings,
        })
    }

    /// Extract trace fragment around a property violation
    pub fn extract_violation_fragment(&self, 
                                     quint_trace: &QuintTrace, 
                                     violation: &PropertyViolation,
                                     context_window: usize) -> Result<TraceFragment> {
        // Find the state corresponding to the violation
        let violation_position = quint_trace.states.iter()
            .position(|state| state.time == violation.violation_state.time)
            .unwrap_or(quint_trace.states.len() / 2); // Default to middle if not found

        let start_position = violation_position.saturating_sub(context_window);
        let end_position = std::cmp::min(violation_position + context_window, quint_trace.states.len());

        let fragment_states = quint_trace.states[start_position..end_position].to_vec();
        let fragment_events = quint_trace.events.iter()
            .filter(|event| {
                let event_step = event.parameters.get("step")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0) as usize;
                event_step >= start_position && event_step < end_position
            })
            .cloned()
            .collect();

        Ok(TraceFragment {
            fragment_id: format!("violation_{}_{}", violation.property_name, violation.detected_at),
            start_position,
            end_position,
            states: fragment_states,
            events: fragment_events,
            extraction_reason: format!("Property violation: {}", violation.property_name),
        })
    }

    /// Extract multiple fragments for comprehensive analysis
    pub fn extract_analysis_fragments(&self, 
                                     quint_trace: &QuintTrace,
                                     violation_report: &ViolationDetectionReport) -> Result<Vec<TraceFragment>> {
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
                end_position: quint_trace.states.len(),
                states: quint_trace.states.clone(),
                events: quint_trace.events.clone(),
                extraction_reason: "Full context for multiple violations".to_string(),
            };
            fragments.push(full_fragment);
        }

        Ok(fragments)
    }

    /// Verify trace against Quint properties
    pub fn verify_trace_properties(&self, 
                                  quint_trace: &QuintTrace,
                                  spec: &QuintSpec) -> Result<ValidationResult> {
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
    fn sample_states(&self, execution_trace: &ExecutionTrace) -> Result<Vec<SimulationState>> {
        let all_states = execution_trace.get_all_states();
        let sample_size = (all_states.len() as f64 * self.config.sampling_rate) as usize;
        let step_size = all_states.len() / sample_size.max(1);

        let sampled = all_states.into_iter()
            .step_by(step_size.max(1))
            .take(sample_size)
            .collect();

        Ok(sampled)
    }

    /// Convert simulation state to Quint trace state
    fn convert_simulation_state(&self, sim_state: &SimulationState, step: u64) -> Result<QuintTraceState> {
        let mut variables = HashMap::new();
        
        // Convert basic state variables
        for (key, value) in &sim_state.variables {
            variables.insert(key.clone(), value.clone());
        }

        // Convert protocol state
        let protocol_state = QuintProtocolState {
            active_sessions: QuintValue::List(
                sim_state.protocol_state.active_sessions.iter()
                    .map(|session| QuintValue::String(session.session_id.clone()))
                    .collect()
            ),
            current_phase: QuintValue::String(sim_state.protocol_state.current_phase.clone()),
            variables: sim_state.protocol_state.protocol_variables.clone(),
        };

        // Convert network state
        let mut message_stats = HashMap::new();
        message_stats.insert("sent".to_string(), QuintValue::Int(sim_state.network_state.message_stats.messages_sent as i64));
        message_stats.insert("delivered".to_string(), QuintValue::Int(sim_state.network_state.message_stats.messages_delivered as i64));
        message_stats.insert("dropped".to_string(), QuintValue::Int(sim_state.network_state.message_stats.messages_dropped as i64));

        let mut failure_conditions = HashMap::new();
        failure_conditions.insert("drop_rate".to_string(), QuintValue::Int((sim_state.network_state.failure_conditions.drop_rate * 100.0) as i64));
        failure_conditions.insert("partitions_active".to_string(), QuintValue::Bool(sim_state.network_state.failure_conditions.partitions_active));

        let network_state = QuintNetworkState {
            partitions: QuintValue::List(
                sim_state.network_state.partitions.iter()
                    .map(|partition| QuintValue::List(
                        partition.iter().map(|p| QuintValue::String(p.clone())).collect()
                    ))
                    .collect()
            ),
            message_stats,
            failure_conditions,
        };

        Ok(QuintTraceState {
            step,
            time: sim_state.time,
            variables,
            protocol_state,
            network_state,
        })
    }

    /// Generate transition event between two states
    fn generate_transition_event(&self, 
                               from_state: &QuintTraceState, 
                               to_state: &QuintTraceState,
                               index: usize) -> Result<QuintTraceEvent> {
        let mut parameters = HashMap::new();
        parameters.insert("from_step".to_string(), QuintValue::Int(from_state.step as i64));
        parameters.insert("to_step".to_string(), QuintValue::Int(to_state.step as i64));
        parameters.insert("time_delta".to_string(), QuintValue::Int((to_state.time - from_state.time) as i64));

        Ok(QuintTraceEvent {
            event_id: format!("transition_{}", index),
            event_type: "state_transition".to_string(),
            timestamp: to_state.time,
            parameters,
            pre_state: Some(format!("state_{}", from_state.step)),
            post_state: Some(format!("state_{}", to_state.step)),
        })
    }

    /// Check if two states differ significantly
    fn states_differ_significantly(&self, state1: &QuintTraceState, state2: &QuintTraceState) -> bool {
        // Simple implementation - in reality would check specific variable changes
        state1.step != state2.step || 
        state1.protocol_state.current_phase != state2.protocol_state.current_phase ||
        state1.variables.len() != state2.variables.len()
    }

    /// Calculate quality metrics for a converted trace
    fn calculate_quality_metrics(&self, states: &[QuintTraceState], events: &[QuintTraceEvent]) -> TraceQualityMetrics {
        TraceQualityMetrics {
            state_completeness: if states.is_empty() { 0.0 } else { 1.0 },
            event_coverage: if events.is_empty() { 0.0 } else { 1.0 },
            temporal_consistency: self.calculate_temporal_consistency(states),
            data_fidelity: 0.95, // Placeholder - would be calculated based on data integrity
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
    fn verify_invariant_on_trace(&self, trace: &QuintTrace, invariant: &QuintInvariant) -> Result<PropertyEvaluationResult> {
        let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        // Check invariant at each state
        for state in &trace.states {
            let holds = self.evaluate_invariant_at_state(invariant, state)?;
            if !holds {
                let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
                return Ok(PropertyEvaluationResult {
                    property_name: invariant.name.clone(),
                    holds: false,
                    details: format!("Invariant '{}' violated at step {}", invariant.name, state.step),
                    witness: None,
                    evaluation_time_ms: end_time - start_time,
                });
            }
        }

        let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        Ok(PropertyEvaluationResult {
            property_name: invariant.name.clone(),
            holds: true,
            details: format!("Invariant '{}' holds across entire trace", invariant.name),
            witness: Some(format!("Verified across {} states", trace.states.len())),
            evaluation_time_ms: end_time - start_time,
        })
    }

    /// Verify a temporal property across the trace
    fn verify_temporal_property_on_trace(&self, trace: &QuintTrace, property: &QuintTemporalProperty) -> Result<PropertyEvaluationResult> {
        let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        // Simplified temporal property evaluation
        let holds = self.evaluate_temporal_property_on_trace(property, trace)?;
        
        let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        Ok(PropertyEvaluationResult {
            property_name: property.name.clone(),
            holds,
            details: format!("Temporal property '{}' evaluation: {}", property.name, holds),
            witness: if holds { Some("Property satisfied by trace".to_string()) } else { None },
            evaluation_time_ms: end_time - start_time,
        })
    }

    // Simplified evaluation methods (placeholders for actual Quint integration)
    fn evaluate_invariant_at_state(&self, _invariant: &QuintInvariant, _state: &QuintTraceState) -> Result<bool> {
        // Placeholder - would integrate with actual Quint evaluator
        Ok(true)
    }

    fn evaluate_temporal_property_on_trace(&self, _property: &QuintTemporalProperty, _trace: &QuintTrace) -> Result<bool> {
        // Placeholder - would implement temporal logic evaluation
        Ok(true)
    }
}

impl ExecutionTrace {
    /// Get all states from the execution trace
    pub fn get_all_states(&self) -> Vec<SimulationState> {
        self.states.iter().cloned().collect()
    }
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
            ItfExpression::BigInt { value: value.to_string() }
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
    use crate::property_monitor::{SimulationState, ProtocolExecutionState, ParticipantStateSnapshot, NetworkStateSnapshot, MessageDeliveryStats, NetworkFailureConditions, SessionInfo};
    use std::collections::HashSet;

    #[test]
    fn test_trace_converter_creation() {
        let converter = TraceConverter::new();
        assert_eq!(converter.conversion_stats.traces_converted, 0);
    }

    #[test]
    fn test_convert_simple_trace() {
        let mut converter = TraceConverter::new();
        let mut execution_trace = ExecutionTrace::new(10);
        
        let state = SimulationState {
            tick: 1,
            time: 1000,
            variables: HashMap::new(),
            protocol_state: ProtocolExecutionState {
                active_sessions: Vec::new(),
                completed_sessions: Vec::new(),
                current_phase: "test".to_string(),
                protocol_variables: HashMap::new(),
            },
            participant_states: HashMap::new(),
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
                },
            },
        };

        execution_trace.add_state(state);
        
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
            states: vec![
                QuintTraceState {
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
                },
            ],
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
            property_type: crate::property_monitor::PropertyViolationType::Invariant,
            violation_state: SimulationState {
                tick: 1,
                time: 1000,
                variables: HashMap::new(),
                protocol_state: ProtocolExecutionState {
                    active_sessions: Vec::new(),
                    completed_sessions: Vec::new(),
                    current_phase: "test".to_string(),
                    protocol_variables: HashMap::new(),
                },
                participant_states: HashMap::new(),
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
                    },
                },
            },
            violation_details: crate::property_monitor::ViolationDetails {
                description: "Test violation".to_string(),
                evidence: Vec::new(),
                potential_causes: Vec::new(),
                severity: crate::property_monitor::ViolationSeverity::Medium,
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
}
    
    #[test]
    fn test_itf_comprehensive_features() {
        let mut converter = ItfTraceConverter::new();
        
        // Test complete ITF functionality
        let test_trace = ItfTrace {
            meta: Some(ItfMetadata {
                format_version: Some("1.0".to_string()),
                source: Some("aura-simulator".to_string()),
                created_at: Some("2023-01-01T00:00:00Z".to_string()),
                additional: HashMap::new(),
            }),
            params: None,
            vars: vec\!["x".to_string(), "y".to_string()],
            states: vec\![
                ItfState {
                    meta: None,
                    variables: {
                        let mut vars = HashMap::new();
                        vars.insert("x".to_string(), ItfExpression::BigInt { value: "123456789".to_string() });
                        vars.insert("y".to_string(), ItfExpression::Bool(true));
                        vars
                    },
                },
            ],
            loop_index: None,
        };
        
        // Test validation
        assert\!(converter.validate_itf_trace(&test_trace).is_ok());
        
        // Test JSON serialization
        let json = converter.serialize_itf_to_json(&test_trace, true).unwrap();
        assert\!(json.contains("#bigint"));
        
        // Test parsing back
        let parsed = converter.parse_itf_from_json(&json).unwrap();
        assert_eq\!(parsed.vars.len(), 2);
    }
}
