//! Result builder for composable result construction

use super::*;

/// Builder for constructing simulation results
pub struct ResultBuilder<T> {
    data: T,
    success: bool,
    message: String,
    errors: Vec<String>,
    warnings: Vec<String>,
    performance: PerformanceMetrics,
    metadata: HashMap<String, String>,
}

impl<T> ResultBuilder<T> {
    /// Create a new result builder with success state
    pub fn success(data: T) -> Self {
        Self {
            data,
            success: true,
            message: "Operation completed successfully".to_string(),
            errors: Vec::new(),
            warnings: Vec::new(),
            performance: PerformanceMetrics::default(),
            metadata: HashMap::new(),
        }
    }

    /// Create a new result builder with failure state
    pub fn failure(data: T, message: String) -> Self {
        Self {
            data,
            success: false,
            message,
            errors: Vec::new(),
            warnings: Vec::new(),
            performance: PerformanceMetrics::default(),
            metadata: HashMap::new(),
        }
    }

    /// Set the result message
    pub fn message<S: Into<String>>(mut self, message: S) -> Self {
        self.message = message.into();
        self
    }

    /// Add an error (automatically sets success to false)
    pub fn error<S: Into<String>>(mut self, error: S) -> Self {
        self.errors.push(error.into());
        self.success = false;
        self
    }

    /// Add multiple errors
    pub fn errors<I, S>(mut self, errors: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for error in errors {
            self.errors.push(error.into());
        }
        self.success = false;
        self
    }

    /// Add a warning
    pub fn warning<S: Into<String>>(mut self, warning: S) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Add multiple warnings
    pub fn warnings<I, S>(mut self, warnings: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for warning in warnings {
            self.warnings.push(warning.into());
        }
        self
    }

    /// Set performance metrics
    pub fn performance(mut self, performance: PerformanceMetrics) -> Self {
        self.performance = performance;
        self
    }

    /// Set operation duration
    pub fn duration_ms(mut self, duration_ms: u64) -> Self {
        self.performance.duration_ms = duration_ms;
        self
    }

    /// Set memory usage
    pub fn memory_usage(mut self, bytes: u64) -> Self {
        self.performance.memory_usage_bytes = Some(bytes);
        self
    }

    /// Set CPU utilization
    pub fn cpu_utilization(mut self, utilization: f64) -> Self {
        self.performance.cpu_utilization = Some(utilization);
        self
    }

    /// Set items processed count
    pub fn items_processed(mut self, count: usize) -> Self {
        self.performance.items_processed = count;
        self
    }

    /// Add a performance counter
    pub fn counter<S: Into<String>>(mut self, name: S, value: u64) -> Self {
        self.performance.counters.insert(name.into(), value);
        self
    }

    /// Add metadata
    pub fn metadata<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Add multiple metadata entries
    pub fn metadata_map<I, K, V>(mut self, metadata: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in metadata {
            self.metadata.insert(key.into(), value.into());
        }
        self
    }

    /// Build the final result
    pub fn build(self) -> SimulationResult<T> {
        SimulationResult {
            data: self.data,
            success: self.success,
            message: self.message,
            errors: self.errors,
            warnings: self.warnings,
            performance: self.performance,
            metadata: self.metadata,
            timestamp: crate::utils::time::current_unix_timestamp_secs(),
        }
    }
}

/// Builder for property check results
pub struct PropertyCheckResultBuilder {
    checked_properties: Vec<String>,
    violations: Vec<PropertyViolation>,
    evaluation_results: Vec<PropertyEvaluationResult>,
    validation_result: ValidationResult,
    performance_metrics: PerformanceMetrics,
}

impl PropertyCheckResultBuilder {
    /// Create a new property check result builder
    pub fn new() -> Self {
        Self {
            checked_properties: Vec::new(),
            violations: Vec::new(),
            evaluation_results: Vec::new(),
            validation_result: ValidationResult::success(),
            performance_metrics: PerformanceMetrics::default(),
        }
    }

    /// Add a checked property
    pub fn checked_property<S: Into<String>>(mut self, property: S) -> Self {
        self.checked_properties.push(property.into());
        self
    }

    /// Add multiple checked properties
    pub fn checked_properties<I, S>(mut self, properties: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for property in properties {
            self.checked_properties.push(property.into());
        }
        self
    }

    /// Add a violation
    pub fn violation(mut self, violation: PropertyViolation) -> Self {
        self.violations.push(violation);
        self.validation_result.passed = false;
        self
    }

    /// Add multiple violations
    pub fn violations<I>(mut self, violations: I) -> Self
    where
        I: IntoIterator<Item = PropertyViolation>,
    {
        for violation in violations {
            self.violations.push(violation);
        }
        if !self.violations.is_empty() {
            self.validation_result.passed = false;
        }
        self
    }

    /// Add an evaluation result
    pub fn evaluation_result(mut self, result: PropertyEvaluationResult) -> Self {
        self.evaluation_results.push(result);
        self
    }

    /// Add multiple evaluation results
    pub fn evaluation_results<I>(mut self, results: I) -> Self
    where
        I: IntoIterator<Item = PropertyEvaluationResult>,
    {
        for result in results {
            self.evaluation_results.push(result);
        }
        self
    }

    /// Set validation result
    pub fn validation_result(mut self, result: ValidationResult) -> Self {
        self.validation_result = result;
        self
    }

    /// Set performance metrics
    pub fn performance_metrics(mut self, metrics: PerformanceMetrics) -> Self {
        self.performance_metrics = metrics;
        self
    }

    /// Build the final result
    pub fn build(mut self) -> PropertyCheckResult {
        // Update validation result based on violations
        if self.violations.is_empty() {
            self.validation_result = ValidationResult {
                passed: true,
                message: "All properties satisfied".to_string(),
                errors: Vec::new(),
                warnings: Vec::new(),
            };
        } else {
            self.validation_result = ValidationResult {
                passed: false,
                message: format!("{} property violations detected", self.violations.len()),
                errors: self
                    .violations
                    .iter()
                    .map(|v| v.violation_details.description.clone())
                    .collect(),
                warnings: Vec::new(),
            };
        }

        PropertyCheckResult {
            checked_properties: self.checked_properties,
            violations: self.violations,
            evaluation_results: self.evaluation_results,
            validation_result: self.validation_result,
            performance_metrics: self.performance_metrics,
        }
    }
}

impl Default for PropertyCheckResultBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for simulation run results
pub struct SimulationRunResultBuilder {
    final_tick: u64,
    final_time: u64,
    status: ExecutionStatus,
    stop_reason: StopReason,
    total_events: usize,
    final_state: SimulationStateSnapshot,
    performance_summary: PerformanceMetrics,
    violations: Vec<PropertyViolation>,
}

impl SimulationRunResultBuilder {
    /// Create a new simulation run result builder
    pub fn new(final_tick: u64, final_time: u64) -> Self {
        Self {
            final_tick,
            final_time,
            status: ExecutionStatus::Success,
            stop_reason: StopReason::BecameIdle,
            total_events: 0,
            final_state: SimulationStateSnapshot {
                tick: final_tick,
                time: final_time,
                participant_count: 0,
                active_sessions: 0,
                completed_sessions: 0,
                state_hash: "unknown".to_string(),
            },
            performance_summary: PerformanceMetrics::default(),
            violations: Vec::new(),
        }
    }

    /// Set execution status
    pub fn status(mut self, status: ExecutionStatus) -> Self {
        self.status = status;
        self
    }

    /// Set stop reason
    pub fn stop_reason(mut self, reason: StopReason) -> Self {
        self.stop_reason = reason;
        self
    }

    /// Set total events count
    pub fn total_events(mut self, count: usize) -> Self {
        self.total_events = count;
        self
    }

    /// Set final state
    pub fn final_state(mut self, state: SimulationStateSnapshot) -> Self {
        self.final_state = state;
        self
    }

    /// Set performance summary
    pub fn performance_summary(mut self, performance: PerformanceMetrics) -> Self {
        self.performance_summary = performance;
        self
    }

    /// Add violations
    pub fn violations<I>(mut self, violations: I) -> Self
    where
        I: IntoIterator<Item = PropertyViolation>,
    {
        self.violations.extend(violations);
        self
    }

    /// Build the final result
    pub fn build(self) -> SimulationRunResult {
        SimulationRunResult {
            final_tick: self.final_tick,
            final_time: self.final_time,
            status: self.status,
            stop_reason: self.stop_reason,
            total_events: self.total_events,
            final_state: self.final_state,
            performance_summary: self.performance_summary,
            violations: self.violations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_builder_success() {
        let result = ResultBuilder::success("test_data".to_string())
            .message("Custom success message")
            .warning("Minor warning")
            .duration_ms(1000)
            .metadata("key", "value")
            .build();

        assert!(result.success);
        assert_eq!(result.data, "test_data");
        assert_eq!(result.message, "Custom success message");
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.performance.duration_ms, 1000);
        assert_eq!(result.metadata.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_result_builder_failure() {
        let result =
            ResultBuilder::failure("test_data".to_string(), "Operation failed".to_string())
                .error("First error")
                .error("Second error")
                .build();

        assert!(!result.success);
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.execution_status(), ExecutionStatus::Failed);
    }

    #[test]
    fn test_property_check_result_builder() {
        let violation = PropertyViolation {
            property_name: "test_property".to_string(),
            property_type: PropertyViolationType::Invariant,
            violation_state: SimulationStateSnapshot {
                tick: 100,
                time: 10000,
                participant_count: 3,
                active_sessions: 1,
                completed_sessions: 0,
                state_hash: "abc123".to_string(),
            },
            violation_details: ViolationDetails {
                description: "Test violation".to_string(),
                evidence: vec!["Evidence 1".to_string()],
                potential_causes: vec!["Cause 1".to_string()],
                severity: ViolationSeverity::High,
                remediation_suggestions: vec!["Fix 1".to_string()],
            },
            confidence: 0.9,
            detected_at: 12345,
        };

        let result = PropertyCheckResultBuilder::new()
            .checked_property("property1")
            .checked_property("property2")
            .violation(violation)
            .build();

        assert_eq!(result.checked_properties.len(), 2);
        assert_eq!(result.violations.len(), 1);
        assert!(!result.validation_result.passed);
    }

    #[test]
    fn test_simulation_run_result_builder() {
        let result = SimulationRunResultBuilder::new(1000, 100000)
            .status(ExecutionStatus::Success)
            .stop_reason(StopReason::MaxTicksReached)
            .total_events(500)
            .build();

        assert_eq!(result.final_tick, 1000);
        assert_eq!(result.final_time, 100000);
        assert_eq!(result.status, ExecutionStatus::Success);
        assert_eq!(result.total_events, 500);
        assert!(matches!(result.stop_reason, StopReason::MaxTicksReached));
    }
}
