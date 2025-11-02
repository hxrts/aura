//! Unified result framework for simulation components
//!
//! This module provides consistent result types that eliminate duplication
//! across simulation components while providing rich error information
//! and composable result patterns.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod builder;
pub mod conversion;
pub mod error;

pub use builder::ResultBuilder;
pub use conversion::{IntoSimulationResult, ResultConversion};
pub use error::{ErrorCategory, ErrorSeverity, SimulationError};

// Missing export
pub use error::ErrorDetails;

/// Unified simulation result wrapper with rich metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult<T> {
    /// The actual result data
    pub data: T,
    /// Whether the operation was successful
    pub success: bool,
    /// Primary result message
    pub message: String,
    /// Detailed error information if failed
    pub errors: Vec<String>,
    /// Warning messages (non-fatal)
    pub warnings: Vec<String>,
    /// Performance metrics for the operation
    pub performance: PerformanceMetrics,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
    /// Timestamp when result was created
    pub timestamp: u64,
}

/// Performance metrics for operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct PerformanceMetrics {
    /// Duration of the operation in milliseconds
    pub duration_ms: u64,
    /// Memory usage in bytes (if tracked)
    pub memory_usage_bytes: Option<u64>,
    /// CPU utilization percentage (if tracked)
    pub cpu_utilization: Option<f64>,
    /// Number of items processed
    pub items_processed: usize,
    /// Additional performance counters
    pub counters: HashMap<String, u64>,
}

/// Execution status for operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionStatus {
    /// Operation completed successfully
    Success,
    /// Operation completed with warnings
    SuccessWithWarnings,
    /// Operation failed with recoverable error
    Failed,
    /// Operation failed with critical error
    CriticalFailure,
    /// Operation timed out
    Timeout,
    /// Operation was cancelled
    Cancelled,
    /// Operation is still in progress
    InProgress,
}

/// Property check result with detailed violation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCheckResult {
    /// Properties that were checked
    pub checked_properties: Vec<String>,
    /// Detected violations
    pub violations: Vec<PropertyViolation>,
    /// Individual property evaluation results
    pub evaluation_results: Vec<PropertyEvaluationResult>,
    /// Overall validation result
    pub validation_result: ValidationResult,
    /// Performance metrics for the check
    pub performance_metrics: PerformanceMetrics,
}

/// Individual property violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyViolation {
    /// Name of the violated property
    pub property_name: String,
    /// Type of property that was violated
    pub property_type: PropertyViolationType,
    /// Simulation state when violation occurred
    pub violation_state: SimulationStateSnapshot,
    /// Detailed violation information
    pub violation_details: ViolationDetails,
    /// Confidence level (0.0 to 1.0)
    pub confidence: f64,
    /// When violation was detected
    pub detected_at: u64,
}

/// Type of property violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyViolationType {
    /// Invariant property violation
    Invariant,
    /// Temporal property violation
    Temporal,
    /// Safety property violation
    Safety,
    /// Liveness property violation
    Liveness,
    /// Custom property violation
    Custom(String),
}

/// Detailed violation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationDetails {
    /// Human-readable description
    pub description: String,
    /// Evidence supporting the violation
    pub evidence: Vec<String>,
    /// Potential causes of the violation
    pub potential_causes: Vec<String>,
    /// Severity of the violation
    pub severity: ViolationSeverity,
    /// Suggested remediation actions
    pub remediation_suggestions: Vec<String>,
}

/// Severity level of violations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViolationSeverity {
    /// Low severity - informational
    Low,
    /// Medium severity - warning
    Medium,
    /// High severity - error
    High,
    /// Critical severity - system failure
    Critical,
}

/// Individual property evaluation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyEvaluationResult {
    /// Whether the property was satisfied
    pub satisfied: bool,
    /// Detailed evaluation information
    pub details: String,
    /// Evaluation result value
    pub value: PropertyValue,
}

/// Property evaluation value types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyValue {
    /// Boolean result
    Bool(bool),
    /// Numeric result
    Number(f64),
    /// String result
    String(String),
    /// Complex structured result
    Object(HashMap<String, serde_json::Value>),
}

/// Validation result for operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether validation passed
    pub passed: bool,
    /// Validation message
    pub message: String,
    /// Validation errors
    pub errors: Vec<String>,
    /// Validation warnings
    pub warnings: Vec<String>,
}

/// Lightweight simulation state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationStateSnapshot {
    /// Current simulation tick
    pub tick: u64,
    /// Current simulation time
    pub time: u64,
    /// Number of participants
    pub participant_count: usize,
    /// Number of active sessions
    pub active_sessions: usize,
    /// Number of completed sessions
    pub completed_sessions: usize,
    /// State hash for verification
    pub state_hash: String,
}

/// Simulation run result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationRunResult {
    /// Final tick reached
    pub final_tick: u64,
    /// Final simulation time
    pub final_time: u64,
    /// Overall execution status
    pub status: ExecutionStatus,
    /// Reason for stopping
    pub stop_reason: StopReason,
    /// Total events generated
    pub total_events: usize,
    /// Final state snapshot
    pub final_state: SimulationStateSnapshot,
    /// Performance summary
    pub performance_summary: PerformanceMetrics,
    /// Violations detected during run
    pub violations: Vec<PropertyViolation>,
}

/// Reason why simulation stopped
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StopReason {
    /// Maximum ticks reached
    MaxTicksReached,
    /// Maximum time reached
    MaxTimeReached,
    /// Simulation became idle
    BecameIdle,
    /// Manual stop requested
    ManualStop,
    /// Property violation detected
    PropertyViolation(String),
    /// Error occurred
    Error(String),
    /// Resource limit exceeded
    ResourceLimit(String),
}

impl<T> SimulationResult<T> {
    /// Create a successful result
    pub fn success(data: T) -> Self {
        Self {
            data,
            success: true,
            message: "Operation completed successfully".to_string(),
            errors: Vec::new(),
            warnings: Vec::new(),
            performance: PerformanceMetrics::default(),
            metadata: HashMap::new(),
            timestamp: crate::utils::time::current_unix_timestamp_secs(),
        }
    }

    /// Create a failed result
    pub fn failure(data: T, message: String) -> Self {
        Self {
            data,
            success: false,
            message,
            errors: Vec::new(),
            warnings: Vec::new(),
            performance: PerformanceMetrics::default(),
            metadata: HashMap::new(),
            timestamp: crate::utils::time::current_unix_timestamp_secs(),
        }
    }

    /// Add an error message
    pub fn with_error<S: Into<String>>(mut self, error: S) -> Self {
        self.errors.push(error.into());
        self.success = false;
        self
    }

    /// Add a warning message
    pub fn with_warning<S: Into<String>>(mut self, warning: S) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Set performance metrics
    pub fn with_performance(mut self, performance: PerformanceMetrics) -> Self {
        self.performance = performance;
        self
    }

    /// Add metadata
    pub fn with_metadata<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Check if result has warnings
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Check if result has errors
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get execution status
    pub fn execution_status(&self) -> ExecutionStatus {
        if self.success {
            if self.has_warnings() {
                ExecutionStatus::SuccessWithWarnings
            } else {
                ExecutionStatus::Success
            }
        } else {
            ExecutionStatus::Failed
        }
    }
}


impl PerformanceMetrics {
    /// Create new performance metrics with duration
    pub fn with_duration_ms(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            ..Default::default()
        }
    }

    /// Add a performance counter
    pub fn with_counter<S: Into<String>>(mut self, name: S, value: u64) -> Self {
        self.counters.insert(name.into(), value);
        self
    }

    /// Set memory usage
    pub fn with_memory_usage(mut self, bytes: u64) -> Self {
        self.memory_usage_bytes = Some(bytes);
        self
    }

    /// Set CPU utilization
    pub fn with_cpu_utilization(mut self, utilization: f64) -> Self {
        self.cpu_utilization = Some(utilization);
        self
    }

    /// Set items processed count
    pub fn with_items_processed(mut self, count: usize) -> Self {
        self.items_processed = count;
        self
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self {
            passed: true,
            message: "Validation passed".to_string(),
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

impl ValidationResult {
    /// Create a successful validation result
    pub fn success() -> Self {
        Self::default()
    }

    /// Create a failed validation result
    pub fn failure<S: Into<String>>(message: S) -> Self {
        Self {
            passed: false,
            message: message.into(),
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Add an error
    pub fn with_error<S: Into<String>>(mut self, error: S) -> Self {
        self.errors.push(error.into());
        self.passed = false;
        self
    }

    /// Add a warning
    pub fn with_warning<S: Into<String>>(mut self, warning: S) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

/// Violation detection report containing comprehensive violation analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationDetectionReport {
    /// List of detected violations
    pub violations: Vec<PropertyViolation>,
    /// Timestamp when report was generated
    pub timestamp: u64,
    /// Confidence level of the detection (0.0 to 1.0)
    pub confidence: f64,
}

impl ViolationDetectionReport {
    /// Create a new violation detection report
    pub fn new(violations: Vec<PropertyViolation>, confidence: f64) -> Self {
        Self {
            violations,
            timestamp: crate::utils::time::current_unix_timestamp_secs(),
            confidence,
        }
    }

    /// Create an empty report
    pub fn empty() -> Self {
        Self {
            violations: Vec::new(),
            timestamp: crate::utils::time::current_unix_timestamp_secs(),
            confidence: 1.0,
        }
    }

    /// Check if any violations were detected
    pub fn has_violations(&self) -> bool {
        !self.violations.is_empty()
    }

    /// Get the number of violations
    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }

    /// Get violations by severity
    pub fn violations_by_severity(&self, severity: ViolationSeverity) -> Vec<&PropertyViolation> {
        self.violations
            .iter()
            .filter(|v| v.violation_details.severity == severity)
            .collect()
    }
}

/// Simulation execution result with detailed information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationExecutionResult {
    /// Whether the simulation succeeded
    pub success: bool,
    /// Final world state after simulation
    pub final_state: crate::WorldState,
    /// Events generated during simulation
    pub events: Vec<aura_console_types::TraceEvent>,
    /// Performance metrics
    pub metrics: crate::metrics::SimulationMetrics,
    /// Error message if simulation failed
    pub error: Option<String>,
}

impl SimulationExecutionResult {
    /// Create a successful simulation result
    pub fn success(
        final_state: crate::WorldState,
        events: Vec<aura_console_types::TraceEvent>,
        metrics: crate::metrics::SimulationMetrics,
    ) -> Self {
        Self {
            success: true,
            final_state,
            events,
            metrics,
            error: None,
        }
    }

    /// Create a failed simulation result
    pub fn failure(final_state: crate::WorldState, error: String) -> Self {
        Self {
            success: false,
            final_state,
            events: Vec::new(),
            metrics: crate::metrics::SimulationMetrics::new(),
            error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_result_creation() {
        let result = SimulationResult::success("test_data".to_string());
        assert!(result.success);
        assert_eq!(result.data, "test_data");
        assert!(!result.has_errors());
        assert!(!result.has_warnings());
        assert_eq!(result.execution_status(), ExecutionStatus::Success);
    }

    #[test]
    fn test_simulation_result_with_warnings() {
        let result =
            SimulationResult::success("test_data".to_string()).with_warning("This is a warning");

        assert!(result.success);
        assert!(result.has_warnings());
        assert_eq!(
            result.execution_status(),
            ExecutionStatus::SuccessWithWarnings
        );
    }

    #[test]
    fn test_simulation_result_failure() {
        let result =
            SimulationResult::failure("test_data".to_string(), "Operation failed".to_string())
                .with_error("Detailed error message");

        assert!(!result.success);
        assert!(result.has_errors());
        assert_eq!(result.execution_status(), ExecutionStatus::Failed);
    }

    #[test]
    fn test_performance_metrics() {
        let metrics = PerformanceMetrics::with_duration_ms(1000)
            .with_memory_usage(1024)
            .with_cpu_utilization(0.5)
            .with_items_processed(10)
            .with_counter("cache_hits", 100);

        assert_eq!(metrics.duration_ms, 1000);
        assert_eq!(metrics.memory_usage_bytes, Some(1024));
        assert_eq!(metrics.cpu_utilization, Some(0.5));
        assert_eq!(metrics.items_processed, 10);
        assert_eq!(metrics.counters.get("cache_hits"), Some(&100));
    }

    #[test]
    fn test_validation_result() {
        let success = ValidationResult::success();
        assert!(success.passed);

        let failure = ValidationResult::failure("Validation failed")
            .with_error("Missing required field")
            .with_warning("Deprecated field used");

        assert!(!failure.passed);
        assert_eq!(failure.errors.len(), 1);
        assert_eq!(failure.warnings.len(), 1);
    }
}
