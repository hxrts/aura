//! Unified error handling for simulation components

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unified simulation error with categorization and severity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationError {
    /// Error category
    pub category: ErrorCategory,
    /// Error severity
    pub severity: ErrorSeverity,
    /// Primary error message
    pub message: String,
    /// Detailed error description
    pub details: Option<String>,
    /// Error code for programmatic handling
    pub error_code: Option<String>,
    /// Context where error occurred
    pub context: Option<String>,
    /// Underlying cause if this is a wrapped error
    pub caused_by: Option<Box<SimulationError>>,
    /// Additional error metadata
    pub metadata: std::collections::HashMap<String, String>,
}

/// Categories of simulation errors
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Configuration-related errors
    Configuration,
    /// Property evaluation errors
    PropertyEvaluation,
    /// Network simulation errors
    Network,
    /// Protocol execution errors
    Protocol,
    /// State management errors
    State,
    /// Resource limit errors
    Resource,
    /// I/O and file system errors
    Io,
    /// Parsing and serialization errors
    Parsing,
    /// Timeout errors
    Timeout,
    /// Validation errors
    Validation,
    /// Internal system errors
    Internal,
    /// External dependency errors
    External,
}

/// Error severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorSeverity {
    /// Informational - not actually an error
    Info,
    /// Warning - potential issue but operation can continue
    Warning,
    /// Error - operation failed but system is stable
    Error,
    /// Critical - system stability may be compromised
    Critical,
    /// Fatal - system cannot continue
    Fatal,
}

impl SimulationError {
    /// Create a new simulation error
    pub fn new<S: Into<String>>(
        category: ErrorCategory,
        severity: ErrorSeverity,
        message: S,
    ) -> Self {
        Self {
            category,
            severity,
            message: message.into(),
            details: None,
            error_code: None,
            context: None,
            caused_by: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Create a configuration error
    pub fn configuration<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::Configuration, ErrorSeverity::Error, message)
    }

    /// Create a property evaluation error
    pub fn property_evaluation<S: Into<String>>(message: S) -> Self {
        Self::new(
            ErrorCategory::PropertyEvaluation,
            ErrorSeverity::Error,
            message,
        )
    }

    /// Create a network error
    pub fn network<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::Network, ErrorSeverity::Error, message)
    }

    /// Create a protocol error
    pub fn protocol<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::Protocol, ErrorSeverity::Error, message)
    }

    /// Create a state management error
    pub fn state<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::State, ErrorSeverity::Error, message)
    }

    /// Create a resource limit error
    pub fn resource<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::Resource, ErrorSeverity::Error, message)
    }

    /// Create an I/O error
    pub fn io<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::Io, ErrorSeverity::Error, message)
    }

    /// Create a parsing error
    pub fn parsing<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::Parsing, ErrorSeverity::Error, message)
    }

    /// Create a timeout error
    pub fn timeout<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::Timeout, ErrorSeverity::Error, message)
    }

    /// Create a validation error
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::Validation, ErrorSeverity::Error, message)
    }

    /// Create an internal error
    pub fn internal<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::Internal, ErrorSeverity::Critical, message)
    }

    /// Create an external dependency error
    pub fn external<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCategory::External, ErrorSeverity::Error, message)
    }

    /// Add detailed description
    pub fn with_details<S: Into<String>>(mut self, details: S) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Add error code
    pub fn with_code<S: Into<String>>(mut self, code: S) -> Self {
        self.error_code = Some(code.into());
        self
    }

    /// Add context information
    pub fn with_context<S: Into<String>>(mut self, context: S) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Add underlying cause
    pub fn caused_by(mut self, cause: SimulationError) -> Self {
        self.caused_by = Some(Box::new(cause));
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

    /// Check if error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self.severity,
            ErrorSeverity::Info | ErrorSeverity::Warning | ErrorSeverity::Error
        )
    }

    /// Check if error is critical
    pub fn is_critical(&self) -> bool {
        matches!(
            self.severity,
            ErrorSeverity::Critical | ErrorSeverity::Fatal
        )
    }

    /// Get the full error chain as a string
    pub fn error_chain(&self) -> String {
        let mut chain = vec![self.message.clone()];
        let mut current = self.caused_by.as_ref();

        while let Some(cause) = current {
            chain.push(cause.message.clone());
            current = cause.caused_by.as_ref();
        }

        chain.join(" -> ")
    }

    /// Convert to a standard Error type
    pub fn into_std_error(self) -> Box<dyn std::error::Error + Send + Sync> {
        Box::new(StdSimulationError(self))
    }
}

/// Wrapper to make SimulationError implement std::error::Error
#[derive(Debug)]
struct StdSimulationError(SimulationError);

impl fmt::Display for StdSimulationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StdSimulationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None // We could implement this to return the caused_by error
    }
}

impl fmt::Display for SimulationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.category, self.message)?;

        if let Some(details) = &self.details {
            write!(f, ": {}", details)?;
        }

        if let Some(context) = &self.context {
            write!(f, " (in {})", context)?;
        }

        if let Some(code) = &self.error_code {
            write!(f, " [{}]", code)?;
        }

        Ok(())
    }
}

impl From<std::io::Error> for SimulationError {
    fn from(err: std::io::Error) -> Self {
        SimulationError::io(err.to_string())
            .with_code("IO_ERROR")
            .with_details(format!("std::io::Error: {}", err))
    }
}

impl From<serde_json::Error> for SimulationError {
    fn from(err: serde_json::Error) -> Self {
        SimulationError::parsing(format!("JSON parsing error: {}", err))
            .with_code("JSON_PARSE_ERROR")
    }
}

impl From<toml::de::Error> for SimulationError {
    fn from(err: toml::de::Error) -> Self {
        SimulationError::parsing(format!("TOML parsing error: {}", err))
            .with_code("TOML_PARSE_ERROR")
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for SimulationError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        SimulationError::external(err.to_string()).with_code("EXTERNAL_ERROR")
    }
}

/// Error details structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetails {
    /// Detailed error description
    pub description: String,
    /// Stack trace if available
    pub stack_trace: Option<String>,
    /// Error occurred in module
    pub module: Option<String>,
    /// Error occurred in function
    pub function: Option<String>,
    /// Line number where error occurred
    pub line_number: Option<u32>,
}

impl ErrorDetails {
    /// Create new error details
    pub fn new<S: Into<String>>(description: S) -> Self {
        Self {
            description: description.into(),
            stack_trace: None,
            module: None,
            function: None,
            line_number: None,
        }
    }

    /// Add stack trace
    pub fn with_stack_trace<S: Into<String>>(mut self, stack_trace: S) -> Self {
        self.stack_trace = Some(stack_trace.into());
        self
    }

    /// Add module information
    pub fn with_module<S: Into<String>>(mut self, module: S) -> Self {
        self.module = Some(module.into());
        self
    }

    /// Add function information
    pub fn with_function<S: Into<String>>(mut self, function: S) -> Self {
        self.function = Some(function.into());
        self
    }

    /// Add line number
    pub fn with_line_number(mut self, line_number: u32) -> Self {
        self.line_number = Some(line_number);
        self
    }
}

/// Convenience type alias for simulation results
pub type SimulationResult<T> = std::result::Result<T, SimulationError>;

/// Extension trait for adding context to results
pub trait SimulationResultContext<T> {
    /// Add context to the error if present
    fn with_simulation_context<S: Into<String>>(self, context: S) -> SimulationResult<T>;
}

impl<T> SimulationResultContext<T> for SimulationResult<T> {
    fn with_simulation_context<S: Into<String>>(self, context: S) -> SimulationResult<T> {
        self.map_err(|err| err.with_context(context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = SimulationError::configuration("Invalid timeout value");
        assert_eq!(error.category, ErrorCategory::Configuration);
        assert_eq!(error.severity, ErrorSeverity::Error);
        assert_eq!(error.message, "Invalid timeout value");
    }

    #[test]
    fn test_error_with_details() {
        let error = SimulationError::validation("Field validation failed")
            .with_details("The 'max_ticks' field must be greater than 0")
            .with_code("VALIDATION_001")
            .with_context("configuration_parsing");

        assert_eq!(
            error.details,
            Some("The 'max_ticks' field must be greater than 0".to_string())
        );
        assert_eq!(error.error_code, Some("VALIDATION_001".to_string()));
        assert_eq!(error.context, Some("configuration_parsing".to_string()));
    }

    #[test]
    fn test_error_chain() {
        let root_cause = SimulationError::io("File not found");
        let wrapper = SimulationError::configuration("Failed to load config").caused_by(root_cause);

        let chain = wrapper.error_chain();
        assert!(chain.contains("Failed to load config"));
        assert!(chain.contains("File not found"));
        assert!(chain.contains(" -> "));
    }

    #[test]
    fn test_error_severity_checks() {
        let warning = SimulationError::new(
            ErrorCategory::Network,
            ErrorSeverity::Warning,
            "Minor issue",
        );
        assert!(warning.is_recoverable());
        assert!(!warning.is_critical());

        let critical = SimulationError::new(
            ErrorCategory::Internal,
            ErrorSeverity::Critical,
            "System failure",
        );
        assert!(!critical.is_recoverable());
        assert!(critical.is_critical());
    }

    #[test]
    fn test_error_conversion_from_std() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let sim_error = SimulationError::from(io_error);

        assert_eq!(sim_error.category, ErrorCategory::Io);
        assert!(sim_error.message.contains("File not found"));
        assert_eq!(sim_error.error_code, Some("IO_ERROR".to_string()));
    }

    #[test]
    fn test_result_context_extension() {
        let result: SimulationResult<String> = Err(SimulationError::validation("Invalid input"));
        let with_context = result.with_simulation_context("user_registration");

        assert!(with_context.is_err());
        let error = with_context.unwrap_err();
        assert_eq!(error.context, Some("user_registration".to_string()));
    }
}
