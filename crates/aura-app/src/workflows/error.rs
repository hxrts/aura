//! Typed workflow errors.
//!
//! Replaces stringly-typed `AuraError::agent(format!(...))` patterns with
//! structured error variants that preserve context without losing type info.

use aura_core::AuraError;

/// Typed errors for workflow operations.
///
/// Each variant captures the operation context structurally rather than
/// through format strings. The `From<WorkflowError> for AuraError` impl
/// lets callers keep `Result<T, AuraError>` signatures during migration.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    /// Runtime bridge is not available (not initialized or disconnected).
    #[error("Runtime bridge not available")]
    RuntimeUnavailable,

    /// A runtime bridge call failed.
    #[error("{operation}: {source}")]
    RuntimeCall {
        operation: &'static str,
        #[source]
        source: AuraError,
    },

    /// Connectivity prerequisite not met.
    #[error("Connectivity prerequisite not met for {flow}: connected_peers={connected_peers} sync_peers={sync_peers} discovered_peers={discovered_peers} lan_peers={lan_peers}")]
    ConnectivityRequired {
        flow: String,
        connected_peers: usize,
        sync_peers: usize,
        discovered_peers: usize,
        lan_peers: usize,
    },

    /// Journal operation failed (load, merge, persist).
    #[error("Journal {operation}: {source}")]
    Journal {
        operation: &'static str,
        #[source]
        source: AuraError,
    },

    /// Fact serialization or encoding failed.
    #[error("Fact encoding failed: {source}")]
    FactEncoding {
        #[source]
        source: AuraError,
    },

    /// Ceremony lifecycle operation failed.
    #[error("Ceremony {operation}: {source}")]
    Ceremony {
        operation: &'static str,
        #[source]
        source: AuraError,
    },

    /// Transport delivery failed after retries.
    #[error("Delivery to {peer} failed after {attempts} attempts: {detail}")]
    DeliveryFailed {
        peer: String,
        attempts: usize,
        detail: String,
    },

    /// A precondition was not met.
    #[error("{0}")]
    Precondition(&'static str),

    /// A bounded workflow stage did not complete in time.
    #[error("{operation} timed out in stage {stage} after {timeout_ms}ms")]
    TimedOut {
        operation: &'static str,
        stage: &'static str,
        timeout_ms: u64,
    },

    /// Passthrough for an underlying AuraError.
    #[error(transparent)]
    Core(AuraError),
}

impl From<AuraError> for WorkflowError {
    fn from(error: AuraError) -> Self {
        Self::Core(error)
    }
}

impl From<WorkflowError> for AuraError {
    fn from(error: WorkflowError) -> Self {
        match error {
            WorkflowError::Core(inner) => inner,
            other => AuraError::agent(other.to_string()),
        }
    }
}

/// Helper to wrap a runtime bridge call failure.
pub fn runtime_call(operation: &'static str, source: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::RuntimeCall {
        operation,
        source: AuraError::agent(source.to_string()),
    }
}

/// Helper to wrap a journal operation failure.
pub fn journal_op(operation: &'static str, source: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::Journal {
        operation,
        source: AuraError::agent(source.to_string()),
    }
}

/// Helper to wrap a fact encoding failure.
pub fn fact_encoding(source: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::FactEncoding {
        source: AuraError::serialization(source.to_string()),
    }
}

/// Helper to wrap a ceremony operation failure.
pub fn ceremony_op(operation: &'static str, source: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::Ceremony {
        operation,
        source: AuraError::agent(source.to_string()),
    }
}
