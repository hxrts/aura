//! Typed workflow errors.
//!
//! Replaces stringly-typed `AuraError::agent(format!(...))` patterns with
//! structured error variants that preserve context without losing type info.

use aura_core::AuraError;
use std::error::Error;
use std::sync::Arc;

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
    #[error("Delivery to {peer} failed after {attempts} attempts: {source}")]
    DeliveryFailed {
        peer: String,
        attempts: usize,
        #[source]
        source: AuraError,
    },

    /// No authoritative recipient peer resolved within the retry budget.
    #[error("No recipient peers resolved for {channel} after {attempts} attempts")]
    DeliveryRecipientsUnresolved { channel: String, attempts: usize },

    /// Delivery prerequisites never converged within the retry budget.
    #[error(
        "Delivery prerequisites never converged for {peer} after {attempts} attempts: {detail}"
    )]
    DeliveryPrerequisitesNeverConverged {
        peer: String,
        attempts: usize,
        detail: String,
    },

    /// Fanout could not reach any recipient within the retry budget.
    #[error("Message fanout unavailable for {peer} after {attempts} attempts: {recipients:?}")]
    DeliveryFanoutUnavailable {
        peer: String,
        attempts: usize,
        recipients: Vec<(aura_core::types::identifiers::AuthorityId, String)>,
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

    /// Authoritative channel context could not be resolved.
    #[error("Missing authoritative context for channel {channel}")]
    MissingAuthoritativeContext { channel: String },

    /// Authoritative home projection for a resolved context is missing.
    #[error("Missing authoritative home projection for context {context}")]
    MissingAuthoritativeHomeProjection { context: String },

    /// Authoritative participant lookup failed.
    #[error(
        "Authoritative participant lookup for channel {channel} in context {context}: {source}"
    )]
    AuthoritativeParticipantsLookup {
        channel: String,
        context: String,
        #[source]
        source: AuraError,
    },

    /// Authoritative participant lookup still failed after an explicit convergence pass.
    #[error(
        "Authoritative participant lookup for channel {channel} in context {context} after convergence: {source}"
    )]
    AuthoritativeParticipantsLookupAfterConvergence {
        channel: String,
        context: String,
        #[source]
        source: AuraError,
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
            other @ WorkflowError::RuntimeCall { .. } => {
                match super::runtime_error_classification::native_runtime_error_kind(&other) {
                    Some(kind) => kind.wrap_source(other.to_string(), other),
                    None => AuraError::Internal {
                        message: other.to_string(),
                        source: Some(Arc::new(other)),
                    },
                }
            }
            other => AuraError::Internal {
                message: other.to_string(),
                source: Some(Arc::new(other)),
            },
        }
    }
}

/// Helper to wrap a runtime bridge call failure.
/// Causes must implement the standard error contract; display-only values lose type information.
/// ```compile_fail
/// use aura_app::workflows::error::runtime_call;
/// runtime_call("query", "string-only cause");
/// ```
pub fn runtime_call(
    operation: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> WorkflowError {
    WorkflowError::RuntimeCall {
        operation,
        source: AuraError::Internal {
            message: source.to_string(),
            source: Some(Arc::new(source)),
        },
    }
}

/// Wrap a required native runtime call without downgrading its category.
/// ```compile_fail
/// use aura_app::{IntentError, workflows::error::native_runtime_call};
/// native_runtime_call("settings", IntentError::internal_error("diagnostic only"));
/// ```
pub fn native_runtime_call(
    operation: &'static str,
    source: crate::runtime_bridge::RuntimeBridgeError,
) -> WorkflowError {
    WorkflowError::RuntimeCall {
        operation,
        source: source.into(),
    }
}

/// Helper to wrap a journal operation failure.
/// Causes must implement the standard error contract; display-only values lose type information.
/// ```compile_fail
/// use aura_app::workflows::error::journal_op;
/// journal_op("load", String::from("string-only cause"));
/// ```
pub fn journal_op(
    operation: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> WorkflowError {
    WorkflowError::Journal {
        operation,
        source: AuraError::Internal {
            message: source.to_string(),
            source: Some(Arc::new(source)),
        },
    }
}

/// Helper to wrap a fact encoding failure.
/// Causes must implement the standard error contract; display-only values lose type information.
/// ```compile_fail
/// use aura_app::workflows::error::fact_encoding;
/// fact_encoding("string-only cause");
/// ```
pub fn fact_encoding(source: impl Error + Send + Sync + 'static) -> WorkflowError {
    WorkflowError::FactEncoding {
        source: AuraError::Serialization {
            message: source.to_string(),
            source: Some(Arc::new(source)),
        },
    }
}

/// Helper to wrap a ceremony operation failure.
/// Causes must implement the standard error contract; display-only values lose type information.
/// ```compile_fail
/// use aura_app::workflows::error::ceremony_op;
/// ceremony_op("start", "string-only cause");
/// ```
pub fn ceremony_op(
    operation: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> WorkflowError {
    WorkflowError::Ceremony {
        operation,
        source: AuraError::Internal {
            message: source.to_string(),
            source: Some(Arc::new(source)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_conversion_preserves_context_and_concrete_io_causes() {
        let constructors: [fn(std::io::Error) -> WorkflowError; 4] = [
            |source| runtime_call("runtime read", source),
            |source| journal_op("journal load", source),
            fact_encoding,
            |source| ceremony_op("ceremony prepare", source),
        ];
        for construct in constructors {
            let workflow = construct(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "denied",
            ));
            let display = workflow.to_string();
            let outer = AuraError::from(workflow);
            assert_eq!(outer.category(), "internal");
            assert_eq!(outer.to_string(), format!("Internal error: {display}"));
            for candidate in [outer.clone(), outer] {
                let workflow = candidate
                    .source()
                    .unwrap()
                    .downcast_ref::<WorkflowError>()
                    .unwrap();
                let context = workflow
                    .source()
                    .unwrap()
                    .downcast_ref::<AuraError>()
                    .unwrap();
                let concrete = context
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap();
                assert_eq!(concrete.kind(), std::io::ErrorKind::PermissionDenied);
                assert_eq!(concrete.to_string(), "denied");
            }
        }
    }

    #[test]
    fn workflow_context_remains_distinguishable_from_same_display_text() {
        let typed = AuraError::from(WorkflowError::TimedOut {
            operation: "enrollment",
            stage: "issuance",
            timeout_ms: 30000,
        });
        let textual = AuraError::agent("enrollment timed out in stage issuance after 30000ms");
        assert_eq!(typed.to_string(), textual.to_string());
        assert!(matches!(
            typed.source().unwrap().downcast_ref::<WorkflowError>(),
            Some(WorkflowError::TimedOut {
                timeout_ms: 30000,
                ..
            })
        ));
        assert!(textual.source().is_none());
        let unavailable = AuraError::from(WorkflowError::RuntimeUnavailable);
        assert!(matches!(
            unavailable
                .source()
                .unwrap()
                .downcast_ref::<WorkflowError>(),
            Some(WorkflowError::RuntimeUnavailable)
        ));
    }

    #[test]
    fn core_passthrough_preserves_category_and_direct_source() {
        let core = AuraError::Serialization {
            message: "wire encoding".into(),
            source: Some(Arc::new(serde_json::from_str::<u8>("invalid").unwrap_err())),
        };
        let before = core.to_string();
        let outer = AuraError::from(WorkflowError::from(core));
        assert_eq!(outer.category(), "serialization");
        assert_eq!(outer.to_string(), before);
        assert!(outer.source().unwrap().is::<serde_json::Error>());
    }

    #[test]
    fn runtime_context_preserves_nested_workflow_timeout() {
        let timeout = AuraError::from(WorkflowError::TimedOut {
            operation: "delivery",
            stage: "send",
            timeout_ms: 10,
        });
        let outer = AuraError::from(runtime_call("dispatch", timeout));
        let workflow = outer
            .source()
            .unwrap()
            .downcast_ref::<WorkflowError>()
            .unwrap();
        let helper = workflow
            .source()
            .unwrap()
            .downcast_ref::<AuraError>()
            .unwrap();
        let inner = helper
            .source()
            .unwrap()
            .downcast_ref::<AuraError>()
            .unwrap();
        assert!(matches!(
            inner.source().unwrap().downcast_ref::<WorkflowError>(),
            Some(WorkflowError::TimedOut { .. })
        ));
    }
}
