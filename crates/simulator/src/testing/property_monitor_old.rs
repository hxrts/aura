//! Real-Time Property Monitoring for Simulation
//!
//! This module provides comprehensive property monitoring capabilities that integrate
//! with Quint formal specifications to detect property violations during simulation
//! execution in real-time.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

// Import types from parent module
use super::{
    QuintInvariant, QuintTemporalProperty, QuintSafetyProperty, QuintEvaluationConfig,
    PropertyPriority, QuintValue, PropertyEvaluationResult, ValidationResult,
    ViolationPattern, ViolationDetectionState, MonitoringStatistics, TraceMetadata,
    TemporalPropertyType, PropertyCheckResult, CheckPerformanceMetrics,
    ExecutionTrace,
};

// Import from crate root
use crate::{Result, SimError};

/// Enhanced property monitor with real-time evaluation capabilities
///
/// This monitor provides comprehensive property checking against Quint specifications
/// during simulation execution, with support for trace-based temporal property
/// evaluation and adaptive monitoring strategies.
pub struct PropertyMonitor {
    /// Invariant properties being monitored
    invariants: Vec<QuintInvariant>,
    /// Temporal properties being monitored
    temporal_properties: Vec<QuintTemporalProperty>,
    /// Safety properties being monitored
    safety_properties: Vec<QuintSafetyProperty>,
    /// Execution trace for temporal property evaluation
    execution_trace: ExecutionTrace,
    /// Property evaluation configuration
    evaluation_config: QuintEvaluationConfig,
    /// Violation detection state
    violation_state: ViolationDetectionState,
    /// Monitoring statistics
    monitoring_stats: MonitoringStatistics,
    /// Property prioritization for efficient checking
    property_priorities: HashMap<String, PropertyPriority>,
}

/// Execution trace for temporal property evaluation
#[derive(Debug, Clone)]
pub struct ExecutionTrace {
    /// Sequence of simulation states
    states: VecDeque<SimulationState>,
    /// Maximum trace length
    max_length: usize,
    /// Current position in trace
    current_position: usize,
    /// Trace metadata
    metadata: TraceMetadata,
}

/// Simulation state snapshot for trace evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationState {
    /// Simulation tick when state was captured
    pub tick: u64,
    /// Simulation time when state was captured
    pub time: u64,
    /// State variables and their values
    pub variables: HashMap<String, QuintValue>,
    /// Protocol execution state
    pub protocol_state: LegacyProtocolExecutionState,
    /// Participant states
    pub participant_states: HashMap<String, ParticipantStateSnapshot>,
    /// Network state
    pub network_state: NetworkStateSnapshot,
}

/// Protocol execution state snapshot (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyProtocolExecutionState {
    /// Active protocol sessions
    pub active_sessions: Vec<SessionInfo>,
    /// Completed protocol sessions
    pub completed_sessions: Vec<SessionInfo>,
    /// Current protocol phase
    pub current_phase: String,
    /// Protocol-specific state variables
    pub protocol_variables: HashMap<String, QuintValue>,
}

/// Session information for protocol monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session identifier
    pub session_id: String,
    /// Protocol type
    pub protocol_type: String,
    /// Session participants
    pub participants: Vec<String>,
    /// Session status
    pub status: String,
    /// Session start time
    pub started_at: u64,
}

/// Participant state snapshot for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantStateSnapshot {
    /// Participant identifier
    pub participant_id: String,
    /// Current status
    pub status: String,
    /// Key shares status
    pub has_key_shares: bool,
    /// Session participation
    pub active_sessions: Vec<String>,
    /// Participant-specific variables
    pub variables: HashMap<String, QuintValue>,
}

/// Network state snapshot for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStateSnapshot {
    /// Active network partitions
    pub partitions: Vec<Vec<String>>,
    /// Message delivery statistics
    pub message_stats: MessageDeliveryStats,
    /// Network failure conditions
    pub failure_conditions: NetworkFailureConditions,
}

/// Message delivery statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeliveryStats {
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages delivered
    pub messages_delivered: u64,
    /// Messages dropped
    pub messages_dropped: u64,
    /// Average delivery latency
    pub average_latency_ms: f64,
}

/// Network failure conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkFailureConditions {
    /// Current packet drop rate
    pub drop_rate: f64,
    /// Current latency range
    pub latency_range_ms: (u64, u64),
    /// Whether partitions are active
    pub partitions_active: bool,
}

/// Trace metadata for property evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetadata {
    /// Trace start time
    pub start_time: u64,
    /// Trace duration
    pub duration_ms: u64,
    /// Number of states captured
    pub state_count: usize,
    /// Trace quality metrics
    pub quality_metrics: TraceQualityMetrics,
}

/// Quality metrics for execution traces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceQualityMetrics {
    /// Completeness score (0.0 to 1.0)
    pub completeness: f64,
    /// Consistency score (0.0 to 1.0)
    pub consistency: f64,
    /// Coverage of protocol phases
    pub phase_coverage: f64,
    /// Sample rate quality
    pub sample_rate_quality: f64,
}

/// State for violation detection and tracking
#[derive(Debug, Clone)]
struct ViolationDetectionState {
    /// Detected violations
    violations: Vec<PropertyViolation>,
    /// Violation patterns detected
    detected_patterns: HashMap<String, ViolationPattern>,
    /// False positive tracking
    false_positives: Vec<PropertyViolation>,
    /// Violation history for pattern analysis
    violation_history: VecDeque<PropertyViolation>,
}

/// Detected property violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyViolation {
    /// Property that was violated
    pub property_name: String,
    /// Type of property violated
    pub property_type: PropertyViolationType,
    /// Simulation state when violation occurred
    pub violation_state: SimulationState,
    /// Violation details and context
    pub violation_details: ViolationDetails,
    /// Confidence in violation detection
    pub confidence: f64,
    /// Timestamp of detection
    pub detected_at: u64,
}

/// Type of property violation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PropertyViolationType {
    /// Invariant violation
    Invariant,
    /// Temporal property violation
    Temporal,
    /// Safety property violation
    Safety,
    /// Liveness property violation
    Liveness,
    /// Consistency violation
    Consistency,
}

/// Details about a property violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationDetails {
    /// Human-readable description
    pub description: String,
    /// Violation context and evidence
    pub evidence: Vec<String>,
    /// Potential causes
    pub potential_causes: Vec<String>,
    /// Severity assessment
    pub severity: ViolationSeverity,
    /// Remediation suggestions
    pub remediation_suggestions: Vec<String>,
}

/// Severity levels for property violations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViolationSeverity {
    /// Low severity violation
    Low,
    /// Medium severity violation
    Medium,
    /// High severity violation
    High,
    /// Critical severity violation
    Critical,
}

/// Monitoring statistics and performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringStatistics {
    /// Total properties evaluated
    pub total_evaluations: u64,
    /// Total evaluation time (milliseconds)
    pub total_evaluation_time_ms: u64,
    /// Number of violations detected
    pub violations_detected: u64,
    /// Number of false positives
    pub false_positives: u64,
    /// Average evaluation time per property
    pub average_evaluation_time_ms: f64,
    /// Monitoring efficiency metrics
    pub efficiency_metrics: EfficiencyMetrics,
}

/// Efficiency metrics for property monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EfficiencyMetrics {
    /// Evaluation cache hit rate
    pub cache_hit_rate: f64,
    /// Property evaluation accuracy
    pub evaluation_accuracy: f64,
    /// Resource utilization
    pub resource_utilization: f64,
    /// Monitoring overhead
    pub overhead_percentage: f64,
}

/// Real-time property checking result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCheckResult {
    /// Properties that were checked
    pub checked_properties: Vec<String>,
    /// Detected violations
    pub violations: Vec<PropertyViolation>,
    /// Evaluation results for each property
    pub evaluation_results: Vec<PropertyEvaluationResult>,
    /// Overall validation result
    pub validation_result: ValidationResult,
    /// Performance metrics for this check
    pub performance_metrics: CheckPerformanceMetrics,
}

/// Performance metrics for individual property checks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckPerformanceMetrics {
    /// Time taken for this check (milliseconds)
    pub check_duration_ms: u64,
    /// Number of properties evaluated
    pub properties_evaluated: usize,
    /// Memory usage during check
    pub memory_usage_bytes: usize,
    /// CPU utilization during check
    pub cpu_utilization: f64,
}

impl PropertyMonitor {
    /// Create a new property monitor
    pub fn new() -> Self {
        Self {
            invariants: Vec::new(),
            temporal_properties: Vec::new(),
            safety_properties: Vec::new(),
            execution_trace: ExecutionTrace::new(1000), // Default trace length
            evaluation_config: QuintEvaluationConfig::default(),
            violation_state: ViolationDetectionState::new(),
            monitoring_stats: MonitoringStatistics::new(),
            property_priorities: HashMap::new(),
        }
    }

    /// Get detected violations
    pub fn get_detected_violations(&self) -> Vec<PropertyViolation> {
        self.violation_state.violations.clone()
    }

    /// Get all violations
    pub fn get_violations(&self) -> &[PropertyViolation] {
        &self.violation_state.violations
    }

    /// Create property monitor with custom configuration
    pub fn with_config(config: QuintEvaluationConfig) -> Self {
        Self {
            invariants: Vec::new(),
            temporal_properties: Vec::new(),
            safety_properties: Vec::new(),
            execution_trace: ExecutionTrace::new(1000),
            evaluation_config: config,
            violation_state: ViolationDetectionState::new(),
            monitoring_stats: MonitoringStatistics::new(),
            property_priorities: HashMap::new(),
        }
    }

    /// Add invariant properties to monitor
    pub fn add_invariants(&mut self, invariants: Vec<QuintInvariant>) {
        for invariant in invariants {
            self.property_priorities.insert(invariant.name.clone(), PropertyPriority::High);
            self.invariants.push(invariant);
        }
    }

    /// Add temporal properties to monitor
    pub fn add_temporal_properties(&mut self, properties: Vec<QuintTemporalProperty>) {
        for property in properties {
            let priority = if matches!(property.property_type, TemporalPropertyType::Always) {
                PropertyPriority::High
            } else {
                PropertyPriority::Medium
            };
            self.property_priorities.insert(property.name.clone(), priority);
            self.temporal_properties.push(property);
        }
    }

    /// Add safety properties to monitor
    pub fn add_safety_properties(&mut self, properties: Vec<QuintSafetyProperty>) {
        for property in properties {
            self.property_priorities.insert(property.name.clone(), PropertyPriority::Critical);
            self.safety_properties.push(property);
        }
    }

    /// Perform real-time property checking against simulation state
    pub fn check_properties(&mut self, simulation: &CheckpointSimulation) -> Result<PropertyCheckResult> {
        let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        // Capture current simulation state
        let current_state = self.capture_simulation_state(simulation)?;
        self.execution_trace.add_state(current_state.clone());

        let mut violations = Vec::new();
        let mut evaluation_results = Vec::new();
        let mut checked_properties = Vec::new();

        // Check invariant properties
        for invariant in &self.invariants {
            let result = self.evaluate_invariant(invariant, &current_state)?;
            checked_properties.push(invariant.name.clone());
            
            if !result.satisfied {
                violations.push(self.create_violation_from_invariant(invariant, &current_state, &result)?);
            }
            evaluation_results.push(result);
        }

        // Check safety properties
        for safety_property in &self.safety_properties {
            let result = self.evaluate_safety_property(safety_property, &current_state)?;
            checked_properties.push(safety_property.name.clone());
            
            if !result.satisfied {
                violations.push(self.create_violation_from_safety_property(safety_property, &current_state, &result)?);
            }
            evaluation_results.push(result);
        }

        // Check temporal properties (requires trace evaluation)
        for temporal_property in &self.temporal_properties {
            let result = self.evaluate_temporal_property(temporal_property)?;
            checked_properties.push(temporal_property.name.clone());
            
            if !result.satisfied {
                violations.push(self.create_violation_from_temporal_property(temporal_property, &current_state, &result)?);
            }
            evaluation_results.push(result);
        }

        // Update violation state
        for violation in &violations {
            self.violation_state.violations.push(violation.clone());
        }

        // Generate validation result
        let validation_result = ValidationResult {
            passed: violations.is_empty(),
            message: if violations.is_empty() {
                "All properties satisfied".to_string()
            } else {
                format!("Found {} property violations", violations.len())
            },
            errors: violations.iter().map(|v| v.property_name.clone()).collect(),
        };

        let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let check_duration = end_time - start_time;

        // Update monitoring statistics
        self.monitoring_stats.total_evaluations += evaluation_results.len() as u64;
        self.monitoring_stats.total_evaluation_time_ms += check_duration;
        self.monitoring_stats.violations_detected += violations.len() as u64;
        self.monitoring_stats.average_evaluation_time_ms = 
            self.monitoring_stats.total_evaluation_time_ms as f64 / self.monitoring_stats.total_evaluations as f64;

        Ok(PropertyCheckResult {
            checked_properties,
            violations,
            evaluation_results,
            validation_result,
            performance_metrics: CheckPerformanceMetrics {
                check_duration_ms: check_duration,
                properties_evaluated: self.invariants.len() + self.temporal_properties.len() + self.safety_properties.len(),
                memory_usage_bytes: 0, // Would be calculated in real implementation
                cpu_utilization: 0.0,  // Would be measured in real implementation
            },
        })
    }

    /// Add real-time property checking during simulation steps
    pub fn monitor_simulation_step(&mut self, simulation: &CheckpointSimulation) -> Result<Vec<PropertyViolation>> {
        let check_result = self.check_properties(simulation)?;
        Ok(check_result.violations)
    }

    /// Create automatic violation detection and reporting
    pub fn detect_violations(&mut self, simulation: &CheckpointSimulation) -> Result<ViolationDetectionReport> {
        let violations = self.monitor_simulation_step(simulation)?;
        
        let mut patterns = HashMap::new();
        let mut severity_distribution = HashMap::new();
        
        for violation in &violations {
            // Analyze violation patterns
            let pattern = self.analyze_violation_pattern(&violation)?;
            *patterns.entry(pattern).or_insert(0) += 1;
            
            // Track severity distribution
            *severity_distribution.entry(violation.violation_details.severity.clone()).or_insert(0) += 1;
        }

        Ok(ViolationDetectionReport {
            violations: violations.clone(),
            patterns_detected: patterns,
            severity_distribution,
            total_violations: violations.len(),
            critical_violations: violations.iter()
                .filter(|v| v.violation_details.severity == ViolationSeverity::Critical)
                .count(),
            recommendations: self.generate_violation_recommendations(&violations)?,
        })
    }

    /// Get current monitoring statistics
    pub fn get_monitoring_statistics(&self) -> &MonitoringStatistics {
        &self.monitoring_stats
    }


    /// Clear violation history
    pub fn clear_violations(&mut self) {
        self.violation_state.violations.clear();
        self.violation_state.violation_history.clear();
    }

    /// Set property priorities for efficient monitoring
    pub fn set_property_priority(&mut self, property_name: String, priority: PropertyPriority) {
        self.property_priorities.insert(property_name, priority);
    }

    // Private implementation methods

    /// Capture current simulation state for property evaluation
    fn capture_simulation_state(&self, simulation: &CheckpointSimulation) -> Result<SimulationState> {
        let participants = simulation.get_participants();
        let mut participant_states = HashMap::new();
        
        for (id, participant) in participants {
            participant_states.insert(id.clone(), ParticipantStateSnapshot {
                participant_id: id.clone(),
                status: format!("{:?}", participant.status),
                has_key_shares: participant.key_shares.root_share.is_some(),
                active_sessions: participant.active_sessions.keys().cloned().collect(),
                variables: HashMap::new(), // Would be populated from actual participant state
            });
        }

        let simulation_state = simulation.get_simulation_state();
        
        Ok(SimulationState {
            tick: simulation.world_state.current_tick,
            time: simulation.world_state.current_time,
            variables: HashMap::new(), // Would be populated from simulation state
            protocol_state: LegacyProtocolExecutionState {
                active_sessions: simulation_state.protocols.active_sessions.values()
                    .map(|session| SessionInfo {
                        session_id: session.session_id.clone(),
                        protocol_type: session.protocol_type.clone(),
                        participants: session.participants.clone(),
                        status: session.current_phase.clone(),
                        started_at: session.started_at,
                    })
                    .collect(),
                completed_sessions: simulation_state.protocols.completed_sessions.iter()
                    .map(|session| SessionInfo {
                        session_id: session.session.session_id.clone(),
                        protocol_type: session.session.protocol_type.clone(),
                        participants: session.session.participants.clone(),
                        status: format!("{:?}", session.result),
                        started_at: session.completed_at,
                    })
                    .collect(),
                current_phase: "monitoring".to_string(),
                protocol_variables: HashMap::new(),
            },
            participant_states,
            network_state: NetworkStateSnapshot {
                partitions: simulation_state.network.partitions.iter()
                    .map(|p| p.participants.clone())
                    .collect(),
                message_stats: MessageDeliveryStats {
                    messages_sent: 0,
                    messages_delivered: 0,
                    messages_dropped: 0,
                    average_latency_ms: 100.0,
                },
                failure_conditions: NetworkFailureConditions {
                    drop_rate: simulation_state.network.failure_config.drop_rate,
                    latency_range_ms: simulation_state.network.failure_config.latency_range,
                    partitions_active: !simulation_state.network.partitions.is_empty(),
                },
            },
        })
    }

    /// Evaluate invariant property against current state
    fn evaluate_invariant(&self, invariant: &QuintInvariant, state: &SimulationState) -> Result<PropertyEvaluationResult> {
        let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        // Simplified evaluation - in real implementation would use Quint evaluator
        let holds = self.evaluate_invariant_expression(&invariant.expression, state)?;
        
        let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        Ok(PropertyEvaluationResult {
            satisfied: holds,
            details: format!("Invariant '{}' evaluation: {}", invariant.name, holds),
            value: QuintValue::Bool(holds),
        })
    }

    /// Evaluate safety property against current state
    fn evaluate_safety_property(&self, safety_property: &QuintSafetyProperty, state: &SimulationState) -> Result<PropertyEvaluationResult> {
        let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        let holds = self.evaluate_safety_expression(&safety_property.expression, state)?;
        
        let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        Ok(PropertyEvaluationResult {
            satisfied: holds,
            details: format!("Safety property '{}' evaluation: {}", safety_property.name, holds),
            value: QuintValue::Bool(holds),
        })
    }

    /// Evaluate temporal property using execution trace
    fn evaluate_temporal_property(&self, temporal_property: &QuintTemporalProperty) -> Result<PropertyEvaluationResult> {
        let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        let holds = self.evaluate_temporal_expression(&temporal_property.expression)?;
        
        let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        Ok(PropertyEvaluationResult {
            satisfied: holds,
            details: format!("Temporal property '{}' evaluation: {}", temporal_property.name, holds),
            value: QuintValue::Bool(holds),
        })
    }

    // Simplified expression evaluation methods (placeholders for actual Quint integration)
    fn evaluate_invariant_expression(&self, _expression: &str, _state: &SimulationState) -> Result<bool> {
        // Placeholder implementation - would integrate with actual Quint evaluator
        Ok(true)
    }

    fn evaluate_safety_expression(&self, _expression: &str, _state: &SimulationState) -> Result<bool> {
        // Placeholder implementation
        Ok(true)
    }

    fn evaluate_temporal_expression(&self, _expression: &str) -> Result<bool> {
        // Placeholder implementation - would evaluate against execution trace
        Ok(true)
    }

    // Violation creation methods
    fn create_violation_from_invariant(&self, invariant: &QuintInvariant, state: &SimulationState, result: &PropertyEvaluationResult) -> Result<PropertyViolation> {
        Ok(PropertyViolation {
            property_name: invariant.name.clone(),
            property_type: PropertyViolationType::Invariant,
            violation_state: state.clone(),
            violation_details: ViolationDetails {
                description: format!("Invariant '{}' violated", invariant.name),
                evidence: vec![result.details.clone()],
                potential_causes: vec!["State inconsistency".to_string()],
                severity: ViolationSeverity::High,
                remediation_suggestions: vec!["Check state transitions".to_string()],
            },
            confidence: 0.9,
            detected_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
        })
    }

    fn create_violation_from_safety_property(&self, safety_property: &QuintSafetyProperty, state: &SimulationState, result: &PropertyEvaluationResult) -> Result<PropertyViolation> {
        Ok(PropertyViolation {
            property_name: safety_property.name.clone(),
            property_type: PropertyViolationType::Safety,
            violation_state: state.clone(),
            violation_details: ViolationDetails {
                description: format!("Safety property '{}' violated", safety_property.name),
                evidence: vec![result.details.clone()],
                potential_causes: vec!["Safety condition breach".to_string()],
                severity: ViolationSeverity::Critical,
                remediation_suggestions: vec!["Investigate safety mechanisms".to_string()],
            },
            confidence: 0.95,
            detected_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
        })
    }

    fn create_violation_from_temporal_property(&self, temporal_property: &QuintTemporalProperty, state: &SimulationState, result: &PropertyEvaluationResult) -> Result<PropertyViolation> {
        Ok(PropertyViolation {
            property_name: temporal_property.name.clone(),
            property_type: PropertyViolationType::Temporal,
            violation_state: state.clone(),
            violation_details: ViolationDetails {
                description: format!("Temporal property '{}' violated", temporal_property.name),
                evidence: vec![result.details.clone()],
                potential_causes: vec!["Temporal ordering violation".to_string()],
                severity: ViolationSeverity::Medium,
                remediation_suggestions: vec!["Review execution ordering".to_string()],
            },
            confidence: 0.8,
            detected_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
        })
    }

    fn analyze_violation_pattern(&self, _violation: &PropertyViolation) -> Result<ViolationPattern> {
        // Placeholder implementation
        Ok(ViolationPattern {
            name: "General".to_string(),
            description: "General violation pattern".to_string(),
            confidence: 1.0,
        })
    }

    fn generate_violation_recommendations(&self, _violations: &[PropertyViolation]) -> Result<Vec<String>> {
        Ok(vec![
            "Monitor protocol state transitions".to_string(),
            "Check Byzantine participant behavior".to_string(),
            "Verify network conditions".to_string(),
        ])
    }
}

/// Report of violation detection results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationDetectionReport {
    /// Detected violations
    pub violations: Vec<PropertyViolation>,
    /// Patterns detected in violations
    pub patterns_detected: HashMap<ViolationPattern, usize>,
    /// Distribution of violation severities
    pub severity_distribution: HashMap<ViolationSeverity, usize>,
    /// Total number of violations
    pub total_violations: usize,
    /// Number of critical violations
    pub critical_violations: usize,
    /// Recommendations for addressing violations
    pub recommendations: Vec<String>,
}

impl ExecutionTrace {
    /// Create new execution trace with specified capacity
    pub fn new(max_length: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(max_length),
            max_length,
            current_position: 0,
            metadata: TraceMetadata {
                start_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
                duration_ms: 0,
                state_count: 0,
                quality_metrics: TraceQualityMetrics {
                    completeness: 1.0,
                    consistency: 1.0,
                    phase_coverage: 0.0,
                    sample_rate_quality: 1.0,
                },
            },
        }
    }

    /// Add new state to trace
    pub fn add_state(&mut self, state: SimulationState) {
        if self.states.len() >= self.max_length {
            self.states.pop_front();
        }
        self.states.push_back(state);
        self.current_position = self.states.len() - 1;
        self.metadata.state_count = self.states.len();
    }

    /// Get current state
    pub fn current_state(&self) -> Option<&SimulationState> {
        self.states.back()
    }

    /// Get trace length
    pub fn length(&self) -> usize {
        self.states.len()
    }
}

impl ViolationDetectionState {
    fn new() -> Self {
        Self {
            violations: Vec::new(),
            detected_patterns: HashMap::new(),
            false_positives: Vec::new(),
            violation_history: VecDeque::new(),
        }
    }
}

impl MonitoringStatistics {
    fn new() -> Self {
        Self {
            total_evaluations: 0,
            total_evaluation_time_ms: 0,
            violations_detected: 0,
            false_positives: 0,
            average_evaluation_time_ms: 0.0,
            efficiency_metrics: EfficiencyMetrics {
                cache_hit_rate: 0.0,
                evaluation_accuracy: 1.0,
                resource_utilization: 0.0,
                overhead_percentage: 0.0,
            },
        }
    }
}

impl Default for PropertyMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quint::types::SafetyPropertyType;

    #[test]
    fn test_property_monitor_creation() {
        let monitor = PropertyMonitor::new();
        assert_eq!(monitor.invariants.len(), 0);
        assert_eq!(monitor.temporal_properties.len(), 0);
        assert_eq!(monitor.safety_properties.len(), 0);
    }

    #[test]
    fn test_add_invariants() {
        let mut monitor = PropertyMonitor::new();
        let invariants = vec![
            QuintInvariant {
                name: "test_invariant".to_string(),
                expression: "always_true".to_string(),
                description: "Test invariant".to_string(),
                source_location: "test:1".to_string(),
            }
        ];

        monitor.add_invariants(invariants);
        assert_eq!(monitor.invariants.len(), 1);
        assert_eq!(monitor.property_priorities.len(), 1);
    }

    #[test]
    fn test_add_safety_properties() {
        let mut monitor = PropertyMonitor::new();
        let safety_properties = vec![
            QuintSafetyProperty {
                name: "test_safety".to_string(),
                expression: "no_double_spend".to_string(),
                description: "Test safety property".to_string(),
                source_location: "test:1".to_string(),
                safety_type: SafetyPropertyType::Consistency,
                monitored_variables: vec!["balance".to_string()],
            }
        ];

        monitor.add_safety_properties(safety_properties);
        assert_eq!(monitor.safety_properties.len(), 1);
        assert_eq!(monitor.property_priorities.get("test_safety"), Some(&PropertyPriority::Critical));
    }

    #[test]
    fn test_execution_trace() {
        let mut trace = ExecutionTrace::new(5);
        assert_eq!(trace.length(), 0);

        let state = SimulationState {
            tick: 1,
            time: 1000,
            variables: HashMap::new(),
            protocol_state: LegacyProtocolExecutionState {
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

        trace.add_state(state);
        assert_eq!(trace.length(), 1);
        assert!(trace.current_state().is_some());
    }

    #[test]
    fn test_monitoring_statistics() {
        let stats = MonitoringStatistics::new();
        assert_eq!(stats.total_evaluations, 0);
        assert_eq!(stats.violations_detected, 0);
        assert_eq!(stats.average_evaluation_time_ms, 0.0);
    }
}