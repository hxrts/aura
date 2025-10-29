//! Result conversion traits for backward compatibility

use super::*;
use crate::Result;

/// Trait for converting results into SimulationResult
pub trait IntoSimulationResult<T> {
    /// Convert into a SimulationResult
    fn into_simulation_result(self) -> SimulationResult<T>;
}

/// Trait for converting between different result types
pub trait ResultConversion<T, U> {
    /// Convert from one result type to another
    fn convert(self) -> U;
}

/// Convert standard Result into SimulationResult
impl<T, E> IntoSimulationResult<T> for std::result::Result<T, E>
where
    E: std::fmt::Display,
{
    fn into_simulation_result(self) -> SimulationResult<T> {
        match self {
            Ok(data) => SimulationResult::success(data),
            Err(e) => {
                // For error cases, we need a default value for T
                // This is a limitation - we'll need to handle this differently
                panic!(
                    "Cannot convert Err to SimulationResult without default value. Error: {}",
                    e
                );
            }
        }
    }
}

/// Convert standard Result into SimulationResult with default value
pub fn result_to_simulation_result<T, E>(
    result: std::result::Result<T, E>,
    default_value: T,
) -> SimulationResult<T>
where
    E: std::fmt::Display,
{
    match result {
        Ok(data) => SimulationResult::success(data),
        Err(e) => SimulationResult::failure(default_value, e.to_string()),
    }
}

/// Convert Option into SimulationResult
impl<T> IntoSimulationResult<Option<T>> for Option<T> {
    fn into_simulation_result(self) -> SimulationResult<Option<T>> {
        match self {
            Some(data) => SimulationResult::success(Some(data)),
            None => SimulationResult::success(None),
        }
    }
}

/// Helper function to convert Option with error message
pub fn option_to_simulation_result<T>(
    option: Option<T>,
    error_message: &str,
    default_value: T,
) -> SimulationResult<T> {
    match option {
        Some(data) => SimulationResult::success(data),
        None => SimulationResult::failure(default_value, error_message.to_string()),
    }
}

/// Convert between different result types with transformation
impl<T, U> ResultConversion<SimulationResult<T>, SimulationResult<U>> for SimulationResult<T>
where
    U: From<T>,
{
    fn convert(self) -> SimulationResult<U> {
        SimulationResult {
            data: U::from(self.data),
            success: self.success,
            message: self.message,
            errors: self.errors,
            warnings: self.warnings,
            performance: self.performance,
            metadata: self.metadata,
            timestamp: self.timestamp,
        }
    }
}

/// Extension trait for chaining result operations
pub trait SimulationResultExt<T> {
    /// Map the data while preserving the result structure
    fn map_data<U, F>(self, f: F) -> SimulationResult<U>
    where
        F: FnOnce(T) -> U;

    /// Map the data with a fallible operation
    fn and_then<U, F>(self, f: F) -> SimulationResult<U>
    where
        F: FnOnce(T) -> SimulationResult<U>;

    /// Add context to the result
    fn with_context<S: Into<String>>(self, context: S) -> Self;

    /// Convert to standard Result
    fn to_result(self) -> Result<T>;
}

impl<T> SimulationResultExt<T> for SimulationResult<T> {
    fn map_data<U, F>(self, f: F) -> SimulationResult<U>
    where
        F: FnOnce(T) -> U,
    {
        SimulationResult {
            data: f(self.data),
            success: self.success,
            message: self.message,
            errors: self.errors,
            warnings: self.warnings,
            performance: self.performance,
            metadata: self.metadata,
            timestamp: self.timestamp,
        }
    }

    fn and_then<U, F>(self, f: F) -> SimulationResult<U>
    where
        F: FnOnce(T) -> SimulationResult<U>,
    {
        if !self.success {
            return SimulationResult {
                data: f(self.data).data, // We need the data from f, but keep our error state
                success: false,
                message: self.message,
                errors: self.errors,
                warnings: self.warnings,
                performance: self.performance,
                metadata: self.metadata,
                timestamp: self.timestamp,
            };
        }

        let mut new_result = f(self.data);

        // Merge warnings from the original result
        new_result.warnings.extend(self.warnings);

        // Merge metadata
        for (k, v) in self.metadata {
            new_result.metadata.entry(k).or_insert(v);
        }

        new_result
    }

    fn with_context<S: Into<String>>(mut self, context: S) -> Self {
        let context_str = context.into();
        self.message = format!("{}: {}", context_str, self.message);
        self.metadata.insert("context".to_string(), context_str);
        self
    }

    fn to_result(self) -> Result<T> {
        if self.success {
            Ok(self.data)
        } else {
            let error_msg = if self.errors.is_empty() {
                self.message
            } else {
                format!("{}: {}", self.message, self.errors.join(", "))
            };
            Err(crate::AuraError::protocol_execution_failed(error_msg))
        }
    }
}

/// Conversion helpers for legacy result types
pub mod legacy {
    use super::*;

    /// Convert legacy PropertyCheckResult if it exists
    pub fn convert_property_check_result(
        checked_properties: Vec<String>,
        violations: Vec<PropertyViolation>,
        evaluation_results: Vec<PropertyEvaluationResult>,
        validation_result: ValidationResult,
        performance_metrics: PerformanceMetrics,
    ) -> PropertyCheckResult {
        PropertyCheckResult {
            checked_properties,
            violations,
            evaluation_results,
            validation_result,
            performance_metrics,
        }
    }

    /// Convert legacy RunResult to SimulationRunResult
    pub fn convert_run_result(
        final_tick: u64,
        final_time: u64,
        success: bool,
        stop_reason: StopReason,
        event_trace: Vec<String>, // Simplified for conversion
        final_state: SimulationStateSnapshot,
    ) -> SimulationRunResult {
        let status = if success {
            ExecutionStatus::Success
        } else {
            ExecutionStatus::Failed
        };

        SimulationRunResult {
            final_tick,
            final_time,
            status,
            stop_reason,
            total_events: event_trace.len(),
            final_state,
            performance_summary: PerformanceMetrics::default(),
            violations: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_conversion() {
        let some_value = Some("test".to_string());
        let result = some_value.into_simulation_result();
        assert!(result.success);
        assert_eq!(result.data, Some("test".to_string()));

        let none_value: Option<String> = None;
        let result = none_value.into_simulation_result();
        assert!(result.success);
        assert_eq!(result.data, None);
    }

    #[test]
    fn test_option_to_simulation_result_with_error() {
        let some_value = Some("test".to_string());
        let result = option_to_simulation_result(some_value, "Not found", "default".to_string());
        assert!(result.success);
        assert_eq!(result.data, "test");

        let none_value: Option<String> = None;
        let result = option_to_simulation_result(none_value, "Not found", "default".to_string());
        assert!(!result.success);
        assert_eq!(result.data, "default");
        assert_eq!(result.message, "Not found");
    }

    #[test]
    fn test_result_extension_map_data() {
        let result = SimulationResult::success(5);
        let mapped = result.map_data(|x| x * 2);
        assert!(mapped.success);
        assert_eq!(mapped.data, 10);
    }

    #[test]
    fn test_result_extension_and_then() {
        let result = SimulationResult::success(5);
        let chained = result.and_then(|x| SimulationResult::success(x * 2));
        assert!(chained.success);
        assert_eq!(chained.data, 10);

        let failed_result = SimulationResult::failure(5, "Initial failure".to_string());
        let chained_failed = failed_result.and_then(|x| SimulationResult::success(x * 2));
        assert!(!chained_failed.success);
        assert_eq!(chained_failed.message, "Initial failure");
    }

    #[test]
    fn test_result_extension_with_context() {
        let result = SimulationResult::success("data".to_string()).with_context("test_operation");

        assert!(result.message.contains("test_operation"));
        assert_eq!(
            result.metadata.get("context"),
            Some(&"test_operation".to_string())
        );
    }

    #[test]
    fn test_result_to_standard_result() {
        let success_result = SimulationResult::success("data".to_string());
        let std_result = success_result.to_result();
        assert!(std_result.is_ok());
        assert_eq!(std_result.unwrap(), "data");

        let failed_result = SimulationResult::failure("data".to_string(), "Failed".to_string());
        let std_result = failed_result.to_result();
        assert!(std_result.is_err());
    }
}
