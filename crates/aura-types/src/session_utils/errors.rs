//! Session types errors
//!
//! Simple error types for session type infrastructure without dependencies.

use thiserror::Error;

/// Simple session type error
#[derive(Error, Debug, Clone)]
pub enum SessionTypeError {
    #[error("Property evaluation failed for {property_id}: {message}")]
    PropertyEvaluation {
        property_id: String,
        message: String,
    },

    #[error("Trace processing failed: {message}")]
    TraceProcessing {
        trace_id: Option<String>,
        message: String,
    },

    #[error("Simulation error: {message}")]
    Simulation {
        scenario_name: Option<String>,
        message: String,
        error_code: String,
    },

    #[error("WebSocket error: {message}")]
    WebSocket {
        connection_id: Option<String>,
        message: String,
    },

    #[error("Analysis error ({analysis_type}): {message}")]
    Analysis {
        analysis_type: String,
        message: String,
        trace_id: Option<String>,
    },

    #[error("Witness verification failed ({witness_type}): {message}")]
    WitnessVerificationFailed {
        witness_type: String,
        message: String,
    },

    #[error("Evidence validation failed ({evidence_type}): {message}")]
    EvidenceValidationFailed {
        evidence_type: String,
        message: String,
    },
}

/// Result type for session operations
pub type SessionResult<T> = Result<T, SessionTypeError>;
