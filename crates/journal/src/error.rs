//! Journal errors - using unified error system

// Re-export unified error system
pub use aura_errors::{AuraError, ErrorCode, ErrorSeverity, Result};

// Type aliases for backward compatibility
pub type JournalError = AuraError;
pub type LedgerError = AuraError;

// Journal-specific error constructors as free functions to avoid orphan rule issues

/// Create a ledger operation error
pub fn ledger_operation_failed(reason: impl Into<String>) -> AuraError {
    AuraError::ledger_operation_failed(reason)
}

/// Create a CRDT merge error
pub fn crdt_merge_failed(reason: impl Into<String>) -> AuraError {
    AuraError::data_corruption_detected(format!("CRDT merge failed: {}", reason.into()))
}

/// Create an event serialization error
pub fn event_serialization_failed(reason: impl Into<String>) -> AuraError {
    AuraError::serialization_failed(reason)
}

/// Create an event deserialization error
pub fn event_deserialization_failed(reason: impl Into<String>) -> AuraError {
    AuraError::deserialization_failed(reason)
}

/// Create an invalid event error
pub fn invalid_event(reason: impl Into<String>) -> AuraError {
    AuraError::serialization_failed(format!("Invalid event: {}", reason.into()))
}

/// Create a bootstrap error
pub fn bootstrap_failed(reason: impl Into<String>) -> AuraError {
    AuraError::coordination_failed(format!("Bootstrap failed: {}", reason.into()))
}

/// Create a threshold signature error
pub fn threshold_signature_failed(reason: impl Into<String>) -> AuraError {
    AuraError::frost_sign_failed(reason)
}

/// Create an event replay error
pub fn event_replay_failed(reason: impl Into<String>) -> AuraError {
    AuraError::ledger_operation_failed(format!("Event replay failed: {}", reason.into()))
}

/// Create a journal consistency error
pub fn journal_consistency_error(reason: impl Into<String>) -> AuraError {
    AuraError::data_corruption_detected(format!("Journal consistency error: {}", reason.into()))
}

/// Create a persistence error
pub fn persistence_failed(reason: impl Into<String>) -> AuraError {
    AuraError::storage_failed(format!("Journal persistence failed: {}", reason.into()))
}
