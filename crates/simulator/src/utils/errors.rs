//! Error handling utilities

use crate::AuraError;
use std::fmt::Display;

/// Convert a time-related error to AuraError
pub fn map_to_time_error<E: Display>(error: E) -> AuraError {
    AuraError::system_time_error(error.to_string())
}

/// Convert a configuration-related error to AuraError with context
pub fn map_to_config_error<E: Display>(error: E, context: &str) -> AuraError {
    AuraError::configuration_error(format!("{}: {}", context, error))
}

/// Convert an IO error to AuraError with context
pub fn map_to_io_error<E: Display>(error: E, context: &str) -> AuraError {
    AuraError::storage_failed(format!("IO error in {}: {}", context, error))
}

/// Convert a serialization error to AuraError with context
pub fn map_to_serialization_error<E: Display>(error: E, context: &str) -> AuraError {
    AuraError::serialization_failed(format!("Serialization error in {}: {}", context, error))
}

/// Convert a parsing error to AuraError with context
pub fn map_to_parsing_error<E: Display>(error: E, context: &str) -> AuraError {
    AuraError::deserialization_failed(format!("Parsing error in {}: {}", context, error))
}

/// Create a timeout error with context
pub fn timeout_error(context: &str, timeout_ms: u64) -> AuraError {
    AuraError::protocol_timeout(format!("Timeout after {}ms in {}", timeout_ms, context))
}

/// Create a capacity exceeded error
pub fn capacity_exceeded_error(context: &str, current: usize, max: usize) -> AuraError {
    AuraError::resource_exhausted(format!(
        "Capacity exceeded in {}: {} > {} (max)",
        context, current, max
    ))
}

/// Create a not found error
pub fn not_found_error(item_type: &str, identifier: &str) -> AuraError {
    AuraError::data_corruption_detected(format!("{} not found: {}", item_type, identifier))
}

/// Create an invalid state error
pub fn invalid_state_error(context: &str, reason: &str) -> AuraError {
    AuraError::agent_invalid_state(format!("Invalid state in {}: {}", context, reason))
}

/// Create a validation error for invalid parameters
pub fn validation_error(field: &str, value: &str, expected: &str) -> AuraError {
    AuraError::configuration_error(format!(
        "Invalid {}: got '{}', expected {}",
        field, value, expected
    ))
}

/// Create a dependency error
pub fn dependency_error(dependent: &str, dependency: &str) -> AuraError {
    AuraError::configuration_error(format!(
        "{} depends on {} which is not available",
        dependent, dependency
    ))
}

/// Create a version mismatch error
pub fn version_mismatch_error(
    component: &str,
    expected_version: &str,
    actual_version: &str,
) -> AuraError {
    AuraError::configuration_error(format!(
        "Version mismatch in {}: expected {}, got {}",
        component, expected_version, actual_version
    ))
}

/// Result mapper for common error types
pub trait ResultExt<T> {
    /// Map error to time error with context
    fn map_time_err(self, context: &str) -> Result<T, AuraError>;

    /// Map error to config error with context
    fn map_config_err(self, context: &str) -> Result<T, AuraError>;

    /// Map error to IO error with context
    fn map_io_err(self, context: &str) -> Result<T, AuraError>;

    /// Map error to serialization error with context
    fn map_serialization_err(self, context: &str) -> Result<T, AuraError>;

    /// Map error to parsing error with context
    fn map_parsing_err(self, context: &str) -> Result<T, AuraError>;

    /// Add context to any error
    fn with_context(self, context: &str) -> Result<T, AuraError>;
}

impl<T, E: Display> ResultExt<T> for Result<T, E> {
    fn map_time_err(self, context: &str) -> Result<T, AuraError> {
        self.map_err(|e| map_to_time_error(format!("{}: {}", context, e)))
    }

    fn map_config_err(self, context: &str) -> Result<T, AuraError> {
        self.map_err(|e| map_to_config_error(e, context))
    }

    fn map_io_err(self, context: &str) -> Result<T, AuraError> {
        self.map_err(|e| map_to_io_error(e, context))
    }

    fn map_serialization_err(self, context: &str) -> Result<T, AuraError> {
        self.map_err(|e| map_to_serialization_error(e, context))
    }

    fn map_parsing_err(self, context: &str) -> Result<T, AuraError> {
        self.map_err(|e| map_to_parsing_error(e, context))
    }

    fn with_context(self, context: &str) -> Result<T, AuraError> {
        self.map_err(|e| AuraError::configuration_error(format!("{}: {}", context, e)))
    }
}

/// Utility for collecting multiple validation errors
#[derive(Debug, Default)]
pub struct ErrorCollector {
    errors: Vec<AuraError>,
}

impl ErrorCollector {
    /// Create a new error collector
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an error to the collection
    pub fn add_error(&mut self, error: AuraError) {
        self.errors.push(error);
    }

    /// Add a validation error
    pub fn add_validation_error(&mut self, field: &str, message: &str) {
        self.add_error(AuraError::configuration_error(format!(
            "Validation failed for {}: {}",
            field, message
        )));
    }

    /// Check if there are any errors
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get the number of errors
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Convert to result - Ok if no errors, Err with combined message if errors exist
    pub fn into_result<T>(self, success_value: T) -> Result<T, AuraError> {
        if self.errors.is_empty() {
            Ok(success_value)
        } else {
            let combined_message = self
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            Err(AuraError::configuration_error(format!(
                "Multiple errors occurred: {}",
                combined_message
            )))
        }
    }

    /// Get all errors
    pub fn errors(&self) -> &[AuraError] {
        &self.errors
    }
}

/// Macro for creating validation errors with field context
#[macro_export]
macro_rules! validation_error {
    ($field:expr, $message:expr) => {
        crate::utils::errors::validation_error($field, "", $message)
    };
    ($field:expr, $value:expr, $expected:expr) => {
        crate::utils::errors::validation_error($field, $value, $expected)
    };
}

/// Macro for creating context-aware errors
#[macro_export]
macro_rules! context_error {
    ($context:expr, $error:expr) => {
        AuraError::generic_error(format!("{}: {}", $context, $error))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_mapping_functions() {
        let time_err = map_to_time_error("test time error");
        assert!(time_err.to_string().contains("test time error"));

        let config_err = map_to_config_error("test error", "test context");
        assert!(config_err.to_string().contains("test context"));
        assert!(config_err.to_string().contains("test error"));
    }

    #[test]
    fn test_specific_error_constructors() {
        let timeout_err = timeout_error("test operation", 1000);
        assert!(timeout_err.to_string().contains("1000ms"));
        assert!(timeout_err.to_string().contains("test operation"));

        let capacity_err = capacity_exceeded_error("test", 10, 5);
        assert!(capacity_err.to_string().contains("10 > 5"));

        let not_found_err = not_found_error("item", "id123");
        assert!(not_found_err.to_string().contains("item not found: id123"));
    }

    #[test]
    fn test_result_ext() {
        let result: Result<i32, &str> = Err("test error");
        let mapped = result.with_context("test context");

        assert!(mapped.is_err());
        let error_string = mapped.unwrap_err().to_string();
        assert!(error_string.contains("test context"));
        assert!(error_string.contains("test error"));
    }

    #[test]
    fn test_error_collector() {
        let mut collector = ErrorCollector::new();
        assert!(!collector.has_errors());

        collector.add_validation_error("field1", "invalid value");
        collector.add_validation_error("field2", "missing required");

        assert!(collector.has_errors());
        assert_eq!(collector.error_count(), 2);

        let result = collector.into_result(42);
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(error_message.contains("field1"));
        assert!(error_message.contains("field2"));
    }

    #[test]
    fn test_error_collector_success() {
        let collector = ErrorCollector::new();
        let result = collector.into_result(42);
        assert_eq!(result.unwrap(), 42);
    }
}
