//! CRDT Coordinator Error Types

use crate::choreography::CrdtType;
use aura_core::AuraError;

/// Error types for CRDT coordination
#[derive(Debug, thiserror::Error)]
pub enum CrdtCoordinatorError {
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error("CRDT type mismatch: expected {expected:?}, got {actual:?}")]
    TypeMismatch {
        expected: CrdtType,
        actual: CrdtType,
    },
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),
    #[error("Handler error: {0}")]
    HandlerError(String),
}

impl From<CrdtCoordinatorError> for AuraError {
    fn from(err: CrdtCoordinatorError) -> Self {
        AuraError::internal(format!("CRDT coordinator error: {err}"))
    }
}

impl aura_core::ProtocolErrorCode for CrdtCoordinatorError {
    fn code(&self) -> &'static str {
        match self {
            CrdtCoordinatorError::Serialization(_) => "crdt_serialization",
            CrdtCoordinatorError::Deserialization(_) => "crdt_deserialization",
            CrdtCoordinatorError::TypeMismatch { .. } => "crdt_type_mismatch",
            CrdtCoordinatorError::UnsupportedOperation(_) => "crdt_unsupported_operation",
            CrdtCoordinatorError::HandlerError(_) => "crdt_handler_error",
        }
    }
}
