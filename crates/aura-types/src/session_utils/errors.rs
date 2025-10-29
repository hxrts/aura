//! Session types errors
//!
//! Simple error types for session type infrastructure without dependencies.

use thiserror::Error;

/// Simple session type error
#[derive(Error, Debug, Clone)]
pub enum SessionTypeError {
    /// Property evaluation failed
    #[error("Property evaluation failed for {property_id}: {message}")]
    PropertyEvaluation {
        /// Property identifier
        property_id: String,
        /// Error message
        message: String,
    },

    /// Trace processing error
    #[error("Trace processing failed: {message}")]
    TraceProcessing {
        /// Trace identifier if available
        trace_id: Option<String>,
        /// Error message
        message: String,
    },

    /// Simulation error
    #[error("Simulation error: {message}")]
    Simulation {
        /// Scenario name if available
        scenario_name: Option<String>,
        /// Error message
        message: String,
        /// Error code
        error_code: String,
    },

    /// WebSocket communication error
    #[error("WebSocket error: {message}")]
    WebSocket {
        /// Connection identifier if available
        connection_id: Option<String>,
        /// Error message
        message: String,
    },

    /// Analysis operation error
    #[error("Analysis error ({analysis_type}): {message}")]
    Analysis {
        /// Type of analysis being performed
        analysis_type: String,
        /// Error message
        message: String,
        /// Trace identifier if available
        trace_id: Option<String>,
    },

    /// Witness verification failed
    #[error("Witness verification failed ({witness_type}): {message}")]
    WitnessVerificationFailed {
        /// Type of witness that failed verification
        witness_type: String,
        /// Error message
        message: String,
    },

    /// Evidence validation failed
    #[error("Evidence validation failed ({evidence_type}): {message}")]
    EvidenceValidationFailed {
        /// Type of evidence that failed validation
        evidence_type: String,
        /// Error message
        message: String,
    },
}

/// Result type for session operations
pub type SessionResult<T> = Result<T, SessionTypeError>;
