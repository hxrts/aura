//! Session types errors
//!
//! This module provides session-specific constructors.

// Re-export unified error system
pub use aura_errors::{AuraError, ErrorCode, ErrorSeverity, Result};

// Type aliases for backward compatibility during transition
/// Session-specific error alias for unified error system
pub type SessionError = AuraError;
/// Generic result type alias for unified error system
pub type AuraResult<T> = Result<T>;
/// Session-specific result type alias
pub type SessionResult<T> = Result<T>;

/// Session-specific error constructors
pub struct SessionErrorBuilder;

impl SessionErrorBuilder {
    /// Create a property evaluation error
    pub fn property_evaluation(
        property_id: impl Into<String>,
        message: impl Into<String>,
    ) -> AuraError {
        let property_id_str = property_id.into();
        let message_str = message.into();
        AuraError::Session(aura_errors::SessionError::ProtocolViolation {
            message: format!(
                "Property evaluation failed for {}: {}",
                property_id_str, message_str
            ),
            context: aura_errors::ErrorContext::new()
                .with_code(ErrorCode::SessionProtocolViolation)
                .with_context("property_id", property_id_str),
        })
    }

    /// Create a trace processing error
    pub fn trace_processing(trace_id: Option<String>, message: impl Into<String>) -> AuraError {
        let mut error = AuraError::Data(aura_errors::DataError::LedgerOperationFailed {
            message: format!("Trace processing failed: {}", message.into()),
            context: aura_errors::ErrorContext::new()
                .with_code(ErrorCode::DataLedgerOperationFailed),
        });

        if let Some(trace_id) = trace_id {
            error = error.with_context("trace_id", trace_id);
        }

        error
    }

    /// Create a simulation error
    pub fn simulation(
        scenario_name: Option<String>,
        message: impl Into<String>,
        error_code: impl Into<String>,
    ) -> AuraError {
        let mut error = AuraError::System(aura_errors::SystemError::ConfigurationError {
            message: format!("Simulation error: {}", message.into()),
            context: aura_errors::ErrorContext::new()
                .with_code(ErrorCode::SystemConfigurationError)
                .with_context("error_code", error_code.into()),
        });

        if let Some(scenario_name) = scenario_name {
            error = error.with_context("scenario_name", scenario_name);
        }

        error
    }

    /// Create a WebSocket error
    pub fn websocket(connection_id: Option<String>, message: impl Into<String>) -> AuraError {
        let mut error = AuraError::transport_failed(format!("WebSocket error: {}", message.into()));

        if let Some(connection_id) = connection_id {
            error = error.with_context("connection_id", connection_id);
        }

        error
    }

    /// Create an analysis error
    pub fn analysis(
        analysis_type: impl Into<String>,
        message: impl Into<String>,
        trace_id: Option<String>,
    ) -> AuraError {
        let analysis_type_str = analysis_type.into();
        let message_str = message.into();
        let mut error = AuraError::Data(aura_errors::DataError::LedgerOperationFailed {
            message: format!("Analysis error ({}): {}", analysis_type_str, message_str),
            context: aura_errors::ErrorContext::new()
                .with_code(ErrorCode::DataLedgerOperationFailed)
                .with_context("analysis_type", analysis_type_str),
        });

        if let Some(trace_id) = trace_id {
            error = error.with_context("trace_id", trace_id);
        }

        error
    }

    /// Create a witness verification failed error
    pub fn witness_verification_failed(
        witness_type: impl Into<String>,
        message: impl Into<String>,
    ) -> AuraError {
        let witness_type_str = witness_type.into();
        let message_str = message.into();
        AuraError::Session(aura_errors::SessionError::ProtocolViolation {
            message: format!(
                "Witness verification failed ({}): {}",
                witness_type_str, message_str
            ),
            context: aura_errors::ErrorContext::new()
                .with_code(ErrorCode::SessionProtocolViolation)
                .with_context("witness_type", witness_type_str),
        })
    }

    /// Create an evidence validation failed error
    pub fn evidence_validation_failed(
        evidence_type: impl Into<String>,
        message: impl Into<String>,
    ) -> AuraError {
        let evidence_type_str = evidence_type.into();
        let message_str = message.into();
        AuraError::Session(aura_errors::SessionError::ProtocolViolation {
            message: format!(
                "Evidence validation failed ({}): {}",
                evidence_type_str, message_str
            ),
            context: aura_errors::ErrorContext::new()
                .with_code(ErrorCode::SessionProtocolViolation)
                .with_context("evidence_type", evidence_type_str),
        })
    }
}
