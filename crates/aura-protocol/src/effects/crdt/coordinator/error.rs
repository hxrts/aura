//! CRDT Coordinator Error Types

use crate::choreography::CrdtType;
use aura_core::AuraError;

/// Error types for CRDT coordination
#[derive(Debug, thiserror::Error)]
pub enum CrdtCoordinatorError {
    #[error("serialization failed for {target}: {detail}")]
    SerializationFailed { target: &'static str, detail: String },
    #[error("deserialization failed for {target}: {detail}")]
    DeserializationFailed { target: &'static str, detail: String },
    #[error("CRDT type mismatch: expected {expected:?}, got {actual:?}")]
    TypeMismatch {
        expected: CrdtType,
        actual: CrdtType,
    },
    #[error("missing handler for {crdt_type:?}")]
    MissingHandler { crdt_type: CrdtType },
}

impl From<CrdtCoordinatorError> for AuraError {
    fn from(err: CrdtCoordinatorError) -> Self {
        AuraError::internal(format!("CRDT coordinator error: {err}"))
    }
}

impl aura_core::ProtocolErrorCode for CrdtCoordinatorError {
    fn code(&self) -> &'static str {
        match self {
            CrdtCoordinatorError::SerializationFailed { .. } => "crdt_serialization",
            CrdtCoordinatorError::DeserializationFailed { .. } => "crdt_deserialization",
            CrdtCoordinatorError::TypeMismatch { .. } => "crdt_type_mismatch",
            CrdtCoordinatorError::MissingHandler { .. } => "crdt_missing_handler",
        }
    }
}
