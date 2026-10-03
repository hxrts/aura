//! Native runtime boundary failures with retained causes.
//! Foreign adapters use the existing diagnostic IntentError contract explicitly.

use crate::core::IntentError;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

/// Stable failure category; diagnostic text never determines this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeBridgeErrorKind {
    Crypto,
    Serialization,
    Unauthorized,
    Validation,
    Journal,
    Internal,
    Reactive,
    ContextNotFound,
    Network,
    Storage,
    NoAgent,
    Service,
    /// The native owner exhausted an actual time budget.
    TimedOut,
    /// An invitation or other identified entity was absent.
    NotFound,
}

/// Structural invitation failure normalized by the runtime from actual
/// protocol/validation evidence. This is a failure reason, not a trust witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationAcceptFailureReason {
    AlreadyAccepted,
    Revoked,
    Expired,
    AlreadySettled,
    Unconfirmed,
    NotFound,
    NotPending,
    PermissionDenied,
}

/// Scoped failure diagnostics, never canonical state or permission evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpFailureReason {
    ChannelStateUnavailable {
        context: aura_core::ContextId,
        channel: aura_core::ChannelId,
    },
    AlreadyExists {
        context: aura_core::ContextId,
        channel: aura_core::ChannelId,
    },
}

/// Native runtime failure retaining its original standard error source.
///
/// Sources cannot be restored from diagnostic bytes:
/// ```compile_fail
/// use aura_app::runtime_bridge::RuntimeBridgeError;
/// fn restore(bytes: &[u8]) -> RuntimeBridgeError {
///     serde_json::from_slice(bytes).unwrap()
/// }
/// ```
/// Native failures cannot implicitly cross into the foreign diagnostic enum:
/// ```compile_fail
/// use aura_app::{IntentError, runtime_bridge::RuntimeBridgeError};
/// fn discard(error: RuntimeBridgeError) -> IntentError { error.into() }
/// ```
#[derive(Debug, Clone)]
pub struct RuntimeBridgeError {
    diagnostic: IntentError,
    source: Arc<dyn Error + Send + Sync>,
    invitation_accept_reason: Option<InvitationAcceptFailureReason>,
    enrollment_terminal_reason: Option<super::CeremonyFailureReason>,
    amp_failure_reason: Option<Box<AmpFailureReason>>,
    normalized_kind: Option<RuntimeBridgeErrorKind>,
}

impl RuntimeBridgeError {
    /// Attach the actual failure to an explicitly selected diagnostic category.
    /// The category communicates failure classification, not authorization.
    /// ```compile_fail
    /// use aura_app::{IntentError, runtime_bridge::RuntimeBridgeError};
    /// RuntimeBridgeError::with_source(IntentError::internal_error("failed"), "lost cause");
    /// ```
    pub fn with_source(
        diagnostic: IntentError,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            diagnostic,
            source: Arc::new(source),
            invitation_accept_reason: None,
            enrollment_terminal_reason: None,
            amp_failure_reason: None,
            normalized_kind: None,
        }
    }

    /// Attach the failure reason selected structurally by the runtime owner.
    #[must_use]
    pub fn with_invitation_accept_reason(mut self, reason: InvitationAcceptFailureReason) -> Self {
        self.invitation_accept_reason = Some(reason);
        self
    }

    /// Read the native invitation failure reason without interpreting text.
    #[must_use]
    pub fn invitation_accept_reason(&self) -> Option<InvitationAcceptFailureReason> {
        self.invitation_accept_reason
    }

    /// Signed terminal outcome reported by the runtime owner. This diagnostic
    /// does not itself prove the signature, authorize activation, or permit retry.
    #[must_use]
    pub fn with_enrollment_terminal_reason(mut self, reason: super::CeremonyFailureReason) -> Self {
        self.enrollment_terminal_reason = Some(reason);
        self
    }
    /// Read a structural enrollment failure without interpreting display text.
    #[must_use]
    pub fn enrollment_terminal_reason(&self) -> Option<super::CeremonyFailureReason> {
        self.enrollment_terminal_reason
    }

    /// Diagnostic scope only; callers independently reconcile canonical state.
    #[must_use]
    pub fn with_amp_failure_reason(mut self, reason: AmpFailureReason) -> Self {
        self.amp_failure_reason = Some(Box::new(reason));
        self
    }

    #[must_use]
    pub fn amp_failure_reason(&self) -> Option<AmpFailureReason> {
        self.amp_failure_reason.as_deref().copied()
    }

    /// Set a native category selected from concrete runtime evidence.
    #[must_use]
    pub fn with_kind(mut self, kind: RuntimeBridgeErrorKind) -> Self {
        self.normalized_kind = Some(kind);
        self
    }

    /// Read the stable category without interpreting display text.
    #[must_use]
    pub fn kind(&self) -> RuntimeBridgeErrorKind {
        if let Some(kind) = self.normalized_kind {
            return kind;
        }
        match &self.diagnostic {
            IntentError::Unauthorized { .. } => RuntimeBridgeErrorKind::Unauthorized,
            IntentError::ValidationFailed { .. } => RuntimeBridgeErrorKind::Validation,
            IntentError::JournalError { .. } => RuntimeBridgeErrorKind::Journal,
            IntentError::InternalError { .. } => RuntimeBridgeErrorKind::Internal,
            IntentError::ReactiveFailure { .. } => RuntimeBridgeErrorKind::Reactive,
            IntentError::ContextNotFound { .. } => RuntimeBridgeErrorKind::ContextNotFound,
            IntentError::NetworkError { .. } => RuntimeBridgeErrorKind::Network,
            IntentError::StorageError { .. } => RuntimeBridgeErrorKind::Storage,
            IntentError::NoAgent { .. } => RuntimeBridgeErrorKind::NoAgent,
            IntentError::ServiceError { .. } => RuntimeBridgeErrorKind::Service,
        }
    }

    /// Explicit terminal diagnostic adapter. This intentionally omits the
    /// process-local cause and must not be used for native policy decisions.
    #[must_use]
    pub fn into_diagnostic(self) -> IntentError {
        self.diagnostic
    }
}

impl From<IntentError> for RuntimeBridgeError {
    fn from(error: IntentError) -> Self {
        Self {
            diagnostic: error.clone(),
            source: Arc::new(error),
            invitation_accept_reason: None,
            enrollment_terminal_reason: None,
            amp_failure_reason: None,
            normalized_kind: None,
        }
    }
}

impl fmt::Display for RuntimeBridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.diagnostic, formatter)
    }
}

impl Error for RuntimeBridgeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl RuntimeBridgeErrorKind {
    /// Preserve this native category and the full concrete source chain.
    pub(crate) fn wrap_source(
        self,
        message: String,
        cause: impl Error + Send + Sync + 'static,
    ) -> aura_core::AuraError {
        use aura_core::AuraError as A;
        let source = Some(Arc::new(cause) as Arc<dyn Error + Send + Sync>);
        match self {
            Self::Crypto => A::Crypto { message, source },
            Self::Serialization => A::Serialization { message, source },
            Self::Unauthorized => A::PermissionDenied { message, source },
            Self::Validation => A::Invalid { message, source },
            Self::NotFound | Self::ContextNotFound => A::NotFound { message, source },
            Self::Network => A::Network { message, source },
            Self::Storage => A::Storage { message, source },
            Self::Journal
            | Self::Internal
            | Self::Reactive
            | Self::NoAgent
            | Self::Service
            | Self::TimedOut => A::Internal { message, source },
        }
    }
}

impl From<RuntimeBridgeError> for aura_core::AuraError {
    fn from(error: RuntimeBridgeError) -> Self {
        error.kind().wrap_source(error.to_string(), error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AuraError;

    #[test]
    fn native_boundary_error_stays_within_caller_value_budget() {
        assert!(
            std::mem::size_of::<RuntimeBridgeError>() <= 96,
            "native errors must keep scoped diagnostics out of the caller value",
        );
    }

    #[tokio::test]
    async fn acceptance_boundary_retains_injected_original_failure() {
        use crate::runtime_bridge::{OfflineRuntimeBridge, RuntimeBridge};
        let runtime =
            OfflineRuntimeBridge::new(aura_core::AuthorityId::new_from_entropy([211; 32]));
        let original = AuraError::Storage {
            message: "acceptance journal unavailable".into(),
            source: Some(Arc::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "journal denied",
            ))),
        };
        runtime.set_accept_invitation_result(Err(RuntimeBridgeError::with_source(
            IntentError::internal_error("Failed to accept invitation"),
            original,
        )));
        let native = runtime.accept_invitation("invitation").await.unwrap_err();
        assert_eq!(native.kind(), RuntimeBridgeErrorKind::Internal);
        let outer = AuraError::from(native);
        let boundary = outer
            .source()
            .unwrap()
            .downcast_ref::<RuntimeBridgeError>()
            .unwrap();
        let actual = boundary
            .source()
            .unwrap()
            .downcast_ref::<AuraError>()
            .unwrap();
        assert_eq!(
            actual
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn native_bridge_preserves_actual_core_cause_after_clone() {
        let core = AuraError::Storage {
            message: "read canonical state".into(),
            source: Some(Arc::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "denied",
            ))),
        };
        let native = RuntimeBridgeError::with_source(
            IntentError::storage_error("Read canonical state failed"),
            core,
        );
        for native in [native.clone(), native] {
            assert_eq!(native.kind(), RuntimeBridgeErrorKind::Storage);
            let original = native
                .source()
                .unwrap()
                .downcast_ref::<AuraError>()
                .unwrap();
            assert_eq!(original.category(), "storage");
            assert_eq!(
                original
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
    }

    #[test]
    fn native_kind_never_comes_from_source_or_diagnostic_text() {
        let error = RuntimeBridgeError::with_source(
            IntentError::internal_error("Unauthorized: invalid capability"),
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Service error"),
        );
        assert_eq!(error.kind(), RuntimeBridgeErrorKind::Internal);
        let diagnostic = error.into_diagnostic();
        assert!(matches!(diagnostic, IntentError::InternalError { .. }));
        assert_eq!(diagnostic.to_string(), "Unauthorized: invalid capability");
    }

    #[test]
    fn source_free_domain_failure_retains_original_typed_diagnostic() {
        let error = RuntimeBridgeError::from(IntentError::ContextNotFound {
            context_id: "missing".into(),
        });
        assert_eq!(error.kind(), RuntimeBridgeErrorKind::ContextNotFound);
        assert!(
            matches!(error.source().unwrap().downcast_ref::<IntentError>(), Some(IntentError::ContextNotFound { context_id }) if context_id == "missing")
        );
    }
}
