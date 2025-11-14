//! Property checking middleware for validating simulation properties and invariants

use super::{
    PropertyViolationType, Result, SimulatorContext, SimulatorError, SimulatorHandler,
    SimulatorMiddleware, SimulatorOperation,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Middleware for checking properties and invariants during simulation
pub struct PropertyCheckingMiddleware {
    /// Registered property checkers
    property_checkers: HashMap<String, PropertyChecker>,
    /// Property violation history
    violations: Vec<PropertyViolation>,
    /// Enable automatic property checking
    auto_check: bool,
    /// Check interval in ticks
    check_interval: u64,
    /// Maximum violations before stopping
    max_violations: Option<usize>,
    /// Property check timeout
    check_timeout: Duration,
    /// Enable detailed violation reporting
    detailed_reporting: bool,
}

impl PropertyCheckingMiddleware {
    /// Create new property checking middleware
    pub fn new() -> Self {
        Self {
            property_checkers: HashMap::new(),
            violations: Vec::new(),
            auto_check: true,
            check_interval: 1, // Check every tick by default
            max_violations: Some(10),
            check_timeout: Duration::from_secs(5),
            detailed_reporting: true,
        }
    }

    /// Configure automatic property checking
    pub fn with_auto_check(mut self, enable: bool, interval: u64) -> Self {
        self.auto_check = enable;
        self.check_interval = interval;
        self
    }

    /// Set maximum violations before stopping
    pub fn with_max_violations(mut self, max: Option<usize>) -> Self {
        self.max_violations = max;
        self
    }

    /// Set property check timeout
    pub fn with_check_timeout(mut self, timeout: Duration) -> Self {
        self.check_timeout = timeout;
        self
    }

    /// Enable detailed violation reporting
    pub fn with_detailed_reporting(mut self, enable: bool) -> Self {
        self.detailed_reporting = enable;
        self
    }

    /// Register a property checker
    pub fn with_property_checker(mut self, checker: PropertyChecker) -> Self {
        self.property_checkers.insert(checker.name.clone(), checker);
        self
    }

    /// Check specific property
    fn check_property(
        &self,
        property_name: &str,
        expected: &Value,
        actual: &Value,
        context: &SimulatorContext,
    ) -> Result<PropertyCheckResult> {
        let start_time = Instant::now();

        // Basic equality check
        let passed = expected == actual;

        let duration = start_time.elapsed();

        if duration > self.check_timeout {
            return Ok(PropertyCheckResult {
                property_name: property_name.to_string(),
                passed: false,
                expected: expected.clone(),
                actual: actual.clone(),
                violation_type: Some(PropertyViolationType::Performance {
                    description: "Property check timeout".to_string(),
                    metric: "check_duration".to_string(),
                    value: duration.as_millis() as f64,
                    threshold: self.check_timeout.as_millis() as f64,
                }),
                check_duration: duration,
                details: json!({
                    "timeout": true,
                    "duration_ms": duration.as_millis()
                }),
            });
        }

        let violation_type = if !passed {
            Some(self.determine_violation_type(property_name, expected, actual))
        } else {
            None
        };

        Ok(PropertyCheckResult {
            property_name: property_name.to_string(),
            passed,
            expected: expected.clone(),
            actual: actual.clone(),
            violation_type,
            check_duration: duration,
            details: json!({
                "comparison": "equality",
                "timestamp": context.timestamp.as_millis(),
                "tick": context.tick
            }),
        })
    }

    /// Determine the type of property violation
    fn determine_violation_type(
        &self,
        property_name: &str,
        expected: &Value,
        actual: &Value,
    ) -> PropertyViolationType {
        // Analyze the property name and values to determine violation type
        if property_name.contains("safety") {
            PropertyViolationType::Safety {
                description: format!("Safety property '{}' violated", property_name),
                evidence: json!({
                    "expected": expected,
                    "actual": actual
                }),
            }
        } else if property_name.contains("liveness") {
            PropertyViolationType::Liveness {
                description: format!("Liveness property '{}' violated", property_name),
                timeout: self.check_timeout,
            }
        } else if property_name.contains("consistency") {
            PropertyViolationType::Consistency {
                description: format!("Consistency property '{}' violated", property_name),
                conflicting_states: vec![expected.clone(), actual.clone()],
            }
        } else if property_name.contains("performance") {
            // Extract numeric values for performance comparison
            let expected_val = expected.as_f64().unwrap_or(0.0);
            let actual_val = actual.as_f64().unwrap_or(0.0);

            PropertyViolationType::Performance {
                description: format!("Performance property '{}' violated", property_name),
                metric: property_name.to_string(),
                value: actual_val,
                threshold: expected_val,
            }
        } else if property_name.contains("security") {
            PropertyViolationType::Security {
                description: format!("Security property '{}' violated", property_name),
                threat: "Unknown security violation".to_string(),
            }
        } else {
            // Default to safety violation
            PropertyViolationType::Safety {
                description: format!("Property '{}' violated", property_name),
                evidence: json!({
                    "expected": expected,
                    "actual": actual
                }),
            }
        }
    }

    /// Run automatic property checks
    fn run_auto_checks(&self, context: &SimulatorContext) -> Vec<PropertyCheckResult> {
        let mut results = Vec::new();

        for checker in self.property_checkers.values() {
            if !checker.enabled {
                continue;
            }

            // Check if this checker should run on this tick
            if context.tick % checker.check_interval != 0 {
                continue;
            }

            // Run the property check
            let result = self.run_property_checker(checker, context);
            results.push(result);
        }

        results
    }

    /// Run individual property checker
    fn run_property_checker(
        &self,
        checker: &PropertyChecker,
        context: &SimulatorContext,
    ) -> PropertyCheckResult {
        let start_time = Instant::now();

        // Simulate property checking logic
        let (passed, actual_value) = match &checker.property_type {
            PropertyType::Safety => {
                // Safety properties should always be true
                let safe = self.check_safety_property(&checker.condition, context);
                (safe, json!(safe))
            }

            PropertyType::Liveness => {
                // Liveness properties should eventually become true
                let live = self.check_liveness_property(&checker.condition, context);
                (live, json!(live))
            }

            PropertyType::Consistency => {
                // Consistency properties check for conflicting states
                let consistent = self.check_consistency_property(&checker.condition, context);
                (consistent, json!(consistent))
            }

            PropertyType::Performance { metric } => {
                // Performance properties check metrics against thresholds
                let (meets_threshold, value) =
                    self.check_performance_property(metric, &checker.condition, context);
                (meets_threshold, json!(value))
            }

            PropertyType::Custom { evaluator } => {
                // Custom property evaluation
                let (result, value) = self.evaluate_custom_property(evaluator, context);
                (result, value)
            }
        };

        let duration = start_time.elapsed();

        let violation_type = if !passed {
            Some(match &checker.property_type {
                PropertyType::Safety => PropertyViolationType::Safety {
                    description: checker.description.clone(),
                    evidence: actual_value.clone(),
                },
                PropertyType::Liveness => PropertyViolationType::Liveness {
                    description: checker.description.clone(),
                    timeout: self.check_timeout,
                },
                PropertyType::Consistency => PropertyViolationType::Consistency {
                    description: checker.description.clone(),
                    conflicting_states: vec![actual_value.clone()],
                },
                PropertyType::Performance { metric } => PropertyViolationType::Performance {
                    description: checker.description.clone(),
                    metric: metric.clone(),
                    value: actual_value.as_f64().unwrap_or(0.0),
                    threshold: checker.condition.as_f64().unwrap_or(0.0),
                },
                PropertyType::Custom { .. } => PropertyViolationType::Safety {
                    description: checker.description.clone(),
                    evidence: actual_value.clone(),
                },
            })
        } else {
            None
        };

        PropertyCheckResult {
            property_name: checker.name.clone(),
            passed,
            expected: checker.condition.clone(),
            actual: actual_value,
            violation_type,
            check_duration: duration,
            details: json!({
                "property_type": format!("{:?}", checker.property_type),
                "description": checker.description,
                "timestamp": context.timestamp.as_millis(),
                "tick": context.tick
            }),
        }
    }

    /// Check safety property
    fn check_safety_property(&self, condition: &Value, context: &SimulatorContext) -> bool {
        // TODO fix - Simplified safety check - in practice this would be more sophisticated
        if let Some(threshold) = condition.get("max_participants") {
            if let Some(max) = threshold.as_u64() {
                return context.participant_count <= max as usize;
            }
        }

        true // Default to safe
    }

    /// Check liveness property
    fn check_liveness_property(&self, condition: &Value, context: &SimulatorContext) -> bool {
        // TODO fix - Simplified liveness check
        if let Some(min_tick) = condition.get("min_progress_tick") {
            if let Some(min) = min_tick.as_u64() {
                return context.tick >= min;
            }
        }

        true // Default to live
    }

    /// Check consistency property
    fn check_consistency_property(&self, condition: &Value, context: &SimulatorContext) -> bool {
        // TODO fix - Simplified consistency check
        if let Some(threshold_check) = condition.get("threshold_consistency") {
            if let Some(required) = threshold_check.as_u64() {
                return context.threshold >= required as usize;
            }
        }

        true // Default to consistent
    }

    /// Check performance property
    fn check_performance_property(
        &self,
        metric: &str,
        condition: &Value,
        context: &SimulatorContext,
    ) -> (bool, f64) {
        // TODO fix - Simplified performance check
        let current_value = match metric {
            "tick_rate" => context.tick as f64,
            "participant_ratio" => {
                (context.participant_count as f64) / (context.threshold as f64).max(1.0)
            }
            _ => 1.0,
        };

        let threshold = condition.as_f64().unwrap_or(0.0);
        (current_value >= threshold, current_value)
    }

    /// Evaluate custom property
    fn evaluate_custom_property(
        &self,
        evaluator: &str,
        context: &SimulatorContext,
    ) -> (bool, Value) {
        // TODO fix - Simplified custom evaluation - in practice this would use a proper expression evaluator
        match evaluator {
            "always_true" => (true, json!(true)),
            "tick_even" => {
                let even = context.tick % 2 == 0;
                (even, json!(even))
            }
            _ => (true, json!({"evaluator": evaluator, "result": true})),
        }
    }

    /// Record property violation
    fn _record_violation(&mut self, result: &PropertyCheckResult, context: &SimulatorContext) {
        if let Some(violation_type) = &result.violation_type {
            let violation = PropertyViolation {
                property_name: result.property_name.clone(),
                violation_type: violation_type.clone(),
                timestamp: context.timestamp,
                tick: context.tick,
                details: result.details.clone(),
                recorded_at: Instant::now(),
            };

            self.violations.push(violation);
        }
    }

    /// Check if simulation should stop due to violations
    fn should_stop_simulation(&self) -> bool {
        if let Some(max) = self.max_violations {
            self.violations.len() >= max
        } else {
            false
        }
    }

    /// Check if auto-check should run
    fn should_auto_check(&self, context: &SimulatorContext) -> bool {
        self.auto_check && (context.tick % self.check_interval == 0)
    }
}

impl Default for PropertyCheckingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatorMiddleware for PropertyCheckingMiddleware {
    fn process(
        &self,
        operation: SimulatorOperation,
        context: &SimulatorContext,
        next: &dyn SimulatorHandler,
    ) -> Result<Value> {
        match &operation {
            SimulatorOperation::CheckProperty {
                property_name,
                expected,
                actual,
            } => {
                // Handle explicit property check
                let check_result = self.check_property(property_name, expected, actual, context)?;

                // Add property check info to context
                let mut enhanced_context = context.clone();
                enhanced_context
                    .metadata
                    .insert("property_checked".to_string(), property_name.clone());
                enhanced_context.metadata.insert(
                    "property_passed".to_string(),
                    check_result.passed.to_string(),
                );

                // Call next handler
                let mut result = next.handle(operation, &enhanced_context)?;

                // Add check results
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("property_check".to_string(), json!({
                        "property_name": check_result.property_name,
                        "passed": check_result.passed,
                        "expected": check_result.expected,
                        "actual": check_result.actual,
                        "violation_type": check_result.violation_type.map(|v| format!("{:?}", v)),
                        "check_duration_ms": check_result.check_duration.as_millis(),
                        "details": check_result.details
                    }));
                }

                Ok(result)
            }

            SimulatorOperation::ExecuteTick { .. } => {
                // Run automatic property checks
                let should_check = self.should_auto_check(context);
                let mut check_results = Vec::new();

                if should_check {
                    check_results = self.run_auto_checks(context);
                }

                // Check if simulation should stop
                let should_stop = self.should_stop_simulation();

                // Add property checking info to context
                let mut enhanced_context = context.clone();
                enhanced_context.metadata.insert(
                    "property_checks_run".to_string(),
                    check_results.len().to_string(),
                );
                enhanced_context.metadata.insert(
                    "violations_count".to_string(),
                    self.violations.len().to_string(),
                );

                if should_stop {
                    enhanced_context.metadata.insert(
                        "simulation_should_stop".to_string(),
                        "property_violations".to_string(),
                    );
                }

                // Call next handler
                let mut result = next.handle(operation, &enhanced_context)?;

                // Add property checking information
                if let Some(obj) = result.as_object_mut() {
                    obj.insert(
                        "property_checking".to_string(),
                        json!({
                            "checks_run": check_results.len(),
                            "violations_total": self.violations.len(),
                            "should_stop": should_stop,
                            "active_checkers": self.property_checkers.len(),
                            "auto_check_enabled": self.auto_check,
                            "check_results": check_results.iter().map(|r| json!({
                                "property": r.property_name,
                                "passed": r.passed,
                                "duration_ms": r.check_duration.as_millis()
                            })).collect::<Vec<_>>()
                        }),
                    );
                }

                // Return error if simulation should stop
                if should_stop {
                    return Err(SimulatorError::PropertyViolation {
                        property: "multiple".to_string(),
                        description: format!(
                            "Too many property violations: {}",
                            self.violations.len()
                        ),
                    });
                }

                Ok(result)
            }

            _ => {
                // For other operations, just add property checking metadata
                let mut enhanced_context = context.clone();
                enhanced_context
                    .metadata
                    .insert("property_checking_enabled".to_string(), "true".to_string());

                next.handle(operation, &enhanced_context)
            }
        }
    }

    fn name(&self) -> &str {
        "property_checking"
    }
}

/// Property checker definition
#[derive(Debug, Clone)]
pub struct PropertyChecker {
    /// Property name
    pub name: String,
    /// Property description
    pub description: String,
    /// Type of property
    pub property_type: PropertyType,
    /// Condition to check
    pub condition: Value,
    /// Whether this checker is enabled
    pub enabled: bool,
    /// Check interval in ticks
    pub check_interval: u64,
}

/// Types of properties that can be checked
#[derive(Debug, Clone)]
pub enum PropertyType {
    /// Safety properties (something bad never happens)
    Safety,
    /// Liveness properties (something good eventually happens)
    Liveness,
    /// Consistency properties (state remains consistent)
    Consistency,
    /// Performance properties (metrics meet thresholds)
    Performance { metric: String },
    /// Custom properties with evaluator function
    Custom { evaluator: String },
}

/// Result of a property check
#[derive(Debug, Clone)]
pub struct PropertyCheckResult {
    /// Name of the property that was checked
    pub property_name: String,
    /// Whether the property check passed
    pub passed: bool,
    /// Expected value
    pub expected: Value,
    /// Actual value
    pub actual: Value,
    /// Type of violation if check failed
    pub violation_type: Option<PropertyViolationType>,
    /// Duration of the check
    pub check_duration: Duration,
    /// Additional details
    pub details: Value,
}

/// Record of a property violation
#[derive(Debug, Clone)]
pub struct PropertyViolation {
    /// Name of the violated property
    pub property_name: String,
    /// Type of violation
    pub violation_type: PropertyViolationType,
    /// Simulation timestamp when violation occurred
    pub timestamp: Duration,
    /// Simulation tick when violation occurred
    pub tick: u64,
    /// Additional violation details
    pub details: Value,
    /// Real time when violation was recorded
    pub recorded_at: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpSimulatorHandler;

    #[test]
    fn test_property_checking_creation() {
        let checker = PropertyChecker {
            name: "safety_test".to_string(),
            description: "Test safety property".to_string(),
            property_type: PropertyType::Safety,
            condition: json!({"max_participants": 10}),
            enabled: true,
            check_interval: 1,
        };

        let middleware = PropertyCheckingMiddleware::new()
            .with_auto_check(true, 1)
            .with_max_violations(Some(5))
            .with_property_checker(checker);

        assert!(middleware.auto_check);
        assert_eq!(middleware.check_interval, 1);
        assert_eq!(middleware.max_violations, Some(5));
        assert_eq!(middleware.property_checkers.len(), 1);
    }

    #[test]
    fn test_property_check_operation() {
        let middleware = PropertyCheckingMiddleware::new();
        let handler = NoOpSimulatorHandler;
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let result = middleware.process(
            SimulatorOperation::CheckProperty {
                property_name: "test_property".to_string(),
                expected: json!(true),
                actual: json!(true),
            },
            &context,
            &handler,
        );

        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value.get("property_check").is_some());

        let check_result = &value["property_check"];
        assert_eq!(check_result["passed"], true);
        assert_eq!(check_result["property_name"], "test_property");
    }

    #[test]
    fn test_property_violation_detection() {
        let middleware = PropertyCheckingMiddleware::new();
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let result = middleware
            .check_property("safety_test", &json!(true), &json!(false), &context)
            .unwrap();

        assert!(!result.passed);
        assert!(result.violation_type.is_some());

        if let Some(PropertyViolationType::Safety { description, .. }) = result.violation_type {
            assert!(description.contains("safety_test"));
        }
    }

    #[test]
    fn test_performance_property() {
        let middleware = PropertyCheckingMiddleware::new();

        let (meets_threshold, value) = middleware.check_performance_property(
            "tick_rate",
            &json!(10.0),
            &SimulatorContext::new("test".to_string(), "run1".to_string())
                .with_timestamp(Duration::from_secs(0)),
        );

        // tick is 0, threshold is 10.0, so should not meet threshold
        assert!(!meets_threshold);
        assert_eq!(value, 0.0);
    }

    #[test]
    fn test_custom_property_evaluation() {
        let middleware = PropertyCheckingMiddleware::new();
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let (result, value) = middleware.evaluate_custom_property("always_true", &context);
        assert!(result);
        assert_eq!(value, json!(true));

        let (result, value) = middleware.evaluate_custom_property("tick_even", &context);
        assert!(result); // tick 0 is even
        assert_eq!(value, json!(true));
    }
}
