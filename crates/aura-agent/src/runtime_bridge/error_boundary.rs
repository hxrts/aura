use crate::runtime::services::ServiceError;
use aura_app::IntentError;
use std::fmt::Display;

/// Runtime bridge error translation contract for L5 -> L6 -> L7 composition.
///
/// Layer 5 crates may keep crate-local error styles. The runtime bridge is the
/// normalization boundary that classifies failures into stable frontend-visible
/// `IntentError` categories before they cross into `aura-app` and Layer 7.
pub(super) fn bridge_internal(operation: &'static str, error: impl Display) -> IntentError {
    IntentError::internal_error(format!("{operation}: {error}"))
}

pub(super) fn bridge_validation(operation: &'static str, error: impl Display) -> IntentError {
    IntentError::validation_failed(format!("{operation}: {error}"))
}

pub(super) fn bridge_validation_message(reason: impl Into<String>) -> IntentError {
    IntentError::validation_failed(reason)
}

pub(super) fn bridge_network(operation: &'static str, error: impl Display) -> IntentError {
    IntentError::network_error(format!("{operation}: {error}"))
}

pub(super) fn bridge_network_message(reason: impl Into<String>) -> IntentError {
    IntentError::network_error(reason)
}

#[cfg(test)]
pub(super) fn bridge_storage(operation: &'static str, error: impl Display) -> IntentError {
    IntentError::storage_error(format!("{operation}: {error}"))
}

pub(super) fn bridge_service(error: ServiceError) -> IntentError {
    IntentError::service_error(error.to_string())
}

pub(super) fn bridge_service_unavailable(service: &'static str) -> IntentError {
    bridge_service(ServiceError::unavailable(service, "service unavailable"))
}

pub(super) fn bridge_service_unavailable_with_detail(
    service: &'static str,
    detail: impl Display,
) -> IntentError {
    bridge_service(ServiceError::unavailable(service, format!("{detail}")))
}

#[cfg(test)]
mod tests {
    use super::{
        bridge_internal, bridge_network, bridge_service_unavailable, bridge_storage,
        bridge_validation,
    };
    use aura_app::IntentError;

    #[test]
    fn bridge_internal_preserves_operation_context() {
        let error = bridge_internal("Persist authority record failed", "disk full");
        assert!(matches!(error, IntentError::InternalError { .. }));
        assert_eq!(
            error.to_string(),
            "Persist authority record failed: disk full"
        );
    }

    #[test]
    fn bridge_error_categories_remain_distinct() {
        let validation = bridge_validation("Invalid peer id", "bad format");
        let storage = bridge_storage("Read account config failed", "permission denied");
        let network = bridge_network("Trigger discovery failed", "timeout");
        let unavailable = bridge_service_unavailable("sync_service");

        assert!(matches!(validation, IntentError::ValidationFailed { .. }));
        assert!(matches!(storage, IntentError::StorageError { .. }));
        assert!(matches!(network, IntentError::NetworkError { .. }));
        assert!(matches!(unavailable, IntentError::ServiceError { .. }));
    }
}

/// Normalize concrete native causes, retaining wrappers and operation context.
/// Source-less diagnostics cannot establish timeout or authorization evidence.
pub(super) fn bridge_runtime_internal(
    operation: &'static str,
    error: impl std::error::Error + Send + Sync + 'static,
) -> aura_app::runtime_bridge::RuntimeBridgeError {
    use aura_app::runtime_bridge::{RuntimeBridgeError, RuntimeBridgeErrorKind as Kind};
    let kind = native_cause_kind(&error);
    let detail = format!("{operation}: {error}");
    let diagnostic = match kind {
        Kind::Unauthorized => IntentError::unauthorized(detail),
        Kind::Validation | Kind::NotFound => IntentError::validation_failed(detail),
        Kind::Network => IntentError::network_error(detail),
        Kind::Storage => IntentError::storage_error(detail),
        Kind::Service | Kind::TimedOut => IntentError::service_error(detail),
        Kind::Crypto
        | Kind::Serialization
        | Kind::Internal
        | Kind::Journal
        | Kind::Reactive
        | Kind::ContextNotFound
        | Kind::NoAgent => IntentError::internal_error(detail),
    };
    RuntimeBridgeError::with_source(diagnostic, error).with_kind(kind)
}

pub(super) fn native_cause_kind(
    error: &(dyn std::error::Error + 'static),
) -> aura_app::runtime_bridge::RuntimeBridgeErrorKind {
    use aura_app::runtime_bridge::RuntimeBridgeErrorKind as Kind;
    use aura_core::{AuraError, TimeoutBudgetError};
    let mut current = Some(error);
    while let Some(cause) = current {
        if let Some(agent) = cause.downcast_ref::<crate::core::AgentError>() {
            let kind = match agent {
                crate::core::AgentError::Config(_)
                | crate::core::AgentError::DeviceEnrollmentMessage(_) => Some(Kind::Validation),
                crate::core::AgentError::UnresolvedDeviceBinding { .. } => Some(Kind::Unauthorized),
                crate::core::AgentError::Runtime(_)
                | crate::core::AgentError::Context(_)
                | crate::core::AgentError::Effects(_)
                | crate::core::AgentError::Choreography(_)
                | crate::core::AgentError::Timeout(_)
                | crate::core::AgentError::TimeoutWithSource { .. }
                | crate::core::AgentError::EnrollmentManifest(_)
                | crate::core::AgentError::Aura(_) => None,
            };
            if let Some(kind) = kind {
                return kind;
            }
        }
        if let Some(manifest) =
            cause.downcast_ref::<aura_invitation::enrollment_manifest::EnrollmentManifestError>()
        {
            use aura_invitation::enrollment_manifest::EnrollmentManifestError as Manifest;
            let kind = match manifest {
                Manifest::MissingPin | Manifest::Pin => Some(Kind::Unauthorized),
                Manifest::Unavailable | Manifest::Time(_) => Some(Kind::Service),
                Manifest::Expired | Manifest::Shape => Some(Kind::Validation),
                Manifest::Signature | Manifest::Transcript(_) => Some(Kind::Crypto),
                Manifest::Runtime(_) | Manifest::Boundary(_) | Manifest::Crypto(_) => None,
            };
            if let Some(kind) = kind {
                return kind;
            }
        }
        if cause.is::<aura_core::effects::StorageError>() {
            return Kind::Storage;
        }
        if cause.is::<serde_json::Error>() {
            return Kind::Serialization;
        }
        if let Some(budget) = cause.downcast_ref::<TimeoutBudgetError>() {
            let kind = match budget {
                TimeoutBudgetError::CheckpointFailure { .. } => None,
                TimeoutBudgetError::DeadlineExceeded { .. } => Some(Kind::TimedOut),
                TimeoutBudgetError::InvalidPolicy { .. } => Some(Kind::Validation),
                TimeoutBudgetError::TimeSourceUnavailable { .. }
                | TimeoutBudgetError::ClockRollback { .. }
                | TimeoutBudgetError::AttemptBudgetExhausted { .. } => Some(Kind::Service),
                TimeoutBudgetError::ObservationUnavailable
                | TimeoutBudgetError::CheckpointDiscontinuity { .. } => Some(Kind::Internal),
            };
            if let Some(kind) = kind {
                return kind;
            }
        }
        if let Some(native) = cause.downcast_ref::<AuraError>() {
            let kind = match native {
                AuraError::Invalid { .. } => Some(Kind::Validation),
                AuraError::NotFound { .. } => Some(Kind::NotFound),
                AuraError::PermissionDenied { .. } => Some(Kind::Unauthorized),
                AuraError::Crypto { .. } => Some(Kind::Crypto),
                AuraError::Network { .. } => Some(Kind::Network),
                AuraError::Serialization { .. } => Some(Kind::Serialization),
                AuraError::Storage { .. } => Some(Kind::Storage),
                AuraError::Internal { .. } | AuraError::Terminal(_) => None,
            };
            if let Some(kind) = kind {
                return kind;
            }
        }
        if cause.is::<aura_core::effects::time::TimeError>() {
            return Kind::Service;
        }
        current = cause.source();
    }
    Kind::Internal
}

pub(super) fn bridge_runtime_service_unavailable_with_cause(
    service: &'static str,
    error: impl std::error::Error + Send + Sync + 'static,
) -> aura_app::runtime_bridge::RuntimeBridgeError {
    let diagnostic = IntentError::service_error(
        ServiceError::unavailable(service, error.to_string()).to_string(),
    );
    aura_app::runtime_bridge::RuntimeBridgeError::with_source(diagnostic, error)
}

#[cfg(test)]
mod native_tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn native_required_categories_survive_wrappers_without_diagnostic_inference() {
        use aura_app::runtime_bridge::RuntimeBridgeErrorKind as Kind;
        use aura_core::AuraError;
        let failures = [
            (AuraError::invalid("disk full"), Kind::Validation),
            (AuraError::not_found("permission denied"), Kind::NotFound),
            (
                AuraError::permission_denied("timed out"),
                Kind::Unauthorized,
            ),
            (AuraError::crypto("network failure"), Kind::Crypto),
            (AuraError::network("invalid policy"), Kind::Network),
            (
                AuraError::Serialization {
                    message: "disk failure".into(),
                    source: None,
                },
                Kind::Serialization,
            ),
            (AuraError::storage("not found"), Kind::Storage),
            (AuraError::internal("deadline exceeded"), Kind::Internal),
            (AuraError::Terminal("timeout".into()), Kind::Internal),
        ];
        for (failure, expected) in failures {
            let wrapped = crate::core::AgentError::Aura(failure);
            let native = bridge_runtime_internal("Required read", wrapped);
            assert_eq!(native.kind(), expected);
            assert!(native.source().unwrap().is::<crate::core::AgentError>());
            assert!(native.source().unwrap().source().unwrap().is::<AuraError>());
        }
        let unavailable = aura_core::TimeoutBudgetError::time_source_failure(
            aura_core::effects::TimeError::Timeout { timeout_ms: 1 },
        );
        let native = bridge_runtime_internal(
            "Required clock",
            AuraError::Internal {
                message: "deadline exceeded".into(),
                source: Some(std::sync::Arc::new(unavailable)),
            },
        );
        assert_eq!(
            native.kind(),
            Kind::Service,
            "a failed required clock cannot establish observed budget expiry"
        );
        let native = bridge_runtime_internal(
            "Required wait",
            AuraError::Internal {
                message: "clock unavailable".into(),
                source: Some(std::sync::Arc::new(
                    aura_core::TimeoutBudgetError::DeadlineExceeded {
                        deadline_at_ms: 10,
                        observed_at_ms: 11,
                    },
                )),
            },
        );
        assert_eq!(native.kind(), Kind::TimedOut);
        let config = bridge_runtime_internal(
            "Required config",
            crate::core::AgentError::config("timeout"),
        );
        assert_eq!(config.kind(), Kind::Validation);
        use aura_invitation::enrollment_manifest::EnrollmentManifestError as Manifest;
        for (failure, expected) in [
            (Manifest::MissingPin, Kind::Unauthorized),
            (Manifest::Pin, Kind::Unauthorized),
            (Manifest::Unavailable, Kind::Service),
            (Manifest::Expired, Kind::Validation),
            (Manifest::Shape, Kind::Validation),
            (Manifest::Signature, Kind::Crypto),
        ] {
            let native = bridge_runtime_internal(
                "Required manifest",
                crate::core::AgentError::EnrollmentManifest(failure),
            );
            assert_eq!(native.kind(), expected);
            assert!(native.source().unwrap().source().unwrap().is::<Manifest>());
        }
    }

    #[test]
    fn native_translation_retains_concrete_cause_and_existing_display() {
        let source = aura_core::AuraError::storage("disk denied");
        let expected = bridge_storage("Read authority", source.clone()).to_string();
        let native = bridge_runtime_internal("Read authority", source);
        assert_eq!(native.to_string(), expected);
        assert_eq!(
            native.kind(),
            aura_app::runtime_bridge::RuntimeBridgeErrorKind::Storage
        );
        assert!(native.source().unwrap().is::<aura_core::AuraError>());
        let source = aura_core::effects::time::TimeError::ServiceUnavailable;
        let native = bridge_runtime_service_unavailable_with_cause("physical_time", source);
        assert_eq!(
            native.kind(),
            aura_app::runtime_bridge::RuntimeBridgeErrorKind::Service
        );
        assert!(native
            .source()
            .unwrap()
            .is::<aura_core::effects::time::TimeError>());
    }
}

/// Normalize invitation failure reasons only from concrete runtime sources.
pub(super) fn bridge_runtime_invitation_accept(
    error: crate::core::AgentError,
) -> aura_app::runtime_bridge::RuntimeBridgeError {
    use crate::handlers::invitation::contact_confirmation::{
        ContactConfirmationError, ContactInvitationDecision,
    };
    use crate::handlers::invitation::validation::InvitationValidationError;
    use crate::handlers::invitation::InvitationStatus;
    use aura_app::runtime_bridge::InvitationAcceptFailureReason;
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    let mut reason = None;
    let mut enrollment_terminal = None;
    let mut timed_out = error.is_timeout();
    while let Some(cause) = current {
        if let Some(crate::handlers::invitation::EnrollmentVmAdmissionError::TerminalFailed(
            reason,
        )) = cause.downcast_ref::<crate::handlers::invitation::EnrollmentVmAdmissionError>()
        {
            enrollment_terminal = Some(*reason);
            break;
        }

        if let Some(confirmation) = cause.downcast_ref::<ContactConfirmationError>() {
            reason = Some(match confirmation {
                ContactConfirmationError::Rejected(decision) => match decision {
                    ContactInvitationDecision::Revoked => InvitationAcceptFailureReason::Revoked,
                    ContactInvitationDecision::Expired => InvitationAcceptFailureReason::Expired,
                    ContactInvitationDecision::AlreadySettled
                    | ContactInvitationDecision::Confirmed => {
                        InvitationAcceptFailureReason::AlreadySettled
                    }
                },
                ContactConfirmationError::Unconfirmed(_) => {
                    InvitationAcceptFailureReason::Unconfirmed
                }
            });
            break;
        }
        if let Some(validation) = cause.downcast_ref::<InvitationValidationError>() {
            reason = Some(match validation {
                InvitationValidationError::NotPending { status, .. } => match status {
                    InvitationStatus::Accepted => InvitationAcceptFailureReason::AlreadyAccepted,
                    InvitationStatus::Cancelled => InvitationAcceptFailureReason::Revoked,
                    InvitationStatus::Expired => InvitationAcceptFailureReason::Expired,
                    InvitationStatus::Declined => InvitationAcceptFailureReason::AlreadySettled,
                    InvitationStatus::Pending => InvitationAcceptFailureReason::NotPending,
                },
                InvitationValidationError::Expired { .. } => InvitationAcceptFailureReason::Expired,
                InvitationValidationError::NotFound { .. } => {
                    InvitationAcceptFailureReason::NotFound
                }
            });
            break;
        }
        if matches!(
            cause.downcast_ref::<aura_core::TimeoutBudgetError>(),
            Some(aura_core::TimeoutBudgetError::DeadlineExceeded { .. })
        ) {
            timed_out = true;
        }
        current = cause.source();
    }
    use aura_app::runtime_bridge::{RuntimeBridgeError, RuntimeBridgeErrorKind as Kind};
    let detail = format!("Failed to accept invitation: {error}");
    if let Some(reason) = enrollment_terminal {
        use aura_app::runtime_bridge::CeremonyFailureReason as R;
        let kind = match reason {
            R::Rejected => Kind::Unauthorized,
            R::Cancelled | R::Superseded => Kind::Validation,
            R::TimedOut => Kind::TimedOut,
            R::ChoreographyFailed => Kind::Network,
            R::RuntimeFailed => Kind::Service,
        };
        let diagnostic = match reason {
            R::Rejected => IntentError::unauthorized(detail),
            R::Cancelled | R::Superseded => IntentError::validation_failed(detail),
            R::TimedOut | R::RuntimeFailed => IntentError::service_error(detail),
            R::ChoreographyFailed => IntentError::network_error(detail),
        };
        return RuntimeBridgeError::with_source(diagnostic, error)
            .with_kind(kind)
            .with_enrollment_terminal_reason(reason);
    }
    let (diagnostic, kind) = match reason {
        Some(
            InvitationAcceptFailureReason::Revoked
            | InvitationAcceptFailureReason::PermissionDenied,
        ) => (IntentError::unauthorized(detail), Kind::Unauthorized),
        Some(
            InvitationAcceptFailureReason::AlreadyAccepted
            | InvitationAcceptFailureReason::AlreadySettled
            | InvitationAcceptFailureReason::NotPending
            | InvitationAcceptFailureReason::Expired,
        ) => (IntentError::validation_failed(detail), Kind::Validation),
        Some(InvitationAcceptFailureReason::Unconfirmed) => {
            (IntentError::service_error(detail), Kind::TimedOut)
        }
        Some(InvitationAcceptFailureReason::NotFound) => {
            (IntentError::validation_failed(detail), Kind::NotFound)
        }
        None if timed_out => (IntentError::service_error(detail), Kind::TimedOut),
        None => (IntentError::internal_error(detail), Kind::Internal),
    };
    let native = RuntimeBridgeError::with_source(diagnostic, error).with_kind(kind);
    match reason {
        Some(reason) => native.with_invitation_accept_reason(reason),
        None => native,
    }
}

#[cfg(test)]
mod invitation_reason_tests {
    use super::*;
    use crate::handlers::invitation::contact_confirmation::{
        ContactConfirmationError as C, ContactInvitationDecision as D,
    };
    use crate::handlers::invitation::validation::InvitationValidationError as V;
    use crate::handlers::invitation::InvitationStatus as S;
    use aura_app::runtime_bridge::{
        InvitationAcceptFailureReason as R, RuntimeBridgeErrorKind as K,
    };
    use std::error::Error;

    fn expected_kind(reason: R) -> K {
        match reason {
            R::AlreadyAccepted | R::Expired | R::AlreadySettled | R::NotPending => K::Validation,
            R::Revoked | R::PermissionDenied => K::Unauthorized,
            R::Unconfirmed => K::TimedOut,
            R::NotFound => K::NotFound,
        }
    }

    #[test]
    fn invitation_normalization_retains_actual_deadline_and_reserves_internal_for_faults() {
        let cause = aura_core::TimeoutBudgetError::DeadlineExceeded {
            deadline_at_ms: 100,
            observed_at_ms: 101,
        };
        let error = crate::core::AgentError::Aura(aura_core::AuraError::Internal {
            message: cause.to_string(),
            source: Some(std::sync::Arc::new(cause)),
        });
        let native = bridge_runtime_invitation_accept(error);
        assert_eq!(native.kind(), K::TimedOut);
        assert_eq!(native.invitation_accept_reason(), None);
        let mut source = native.source();
        let mut found = false;
        while let Some(cause) = source {
            if cause.is::<aura_core::TimeoutBudgetError>() {
                found = true;
                break;
            }
            source = cause.source();
        }
        assert!(found);
        let native =
            bridge_runtime_invitation_accept(crate::core::AgentError::invalid("timed out"));
        assert_eq!(native.kind(), K::Internal);
    }

    #[test]
    fn invitation_normalization_uses_actual_settled_status() {
        for (status, expected) in [
            (S::Accepted, R::AlreadyAccepted),
            (S::Cancelled, R::Revoked),
            (S::Expired, R::Expired),
            (S::Declined, R::AlreadySettled),
            (S::Pending, R::NotPending),
        ] {
            let native = bridge_runtime_invitation_accept(
                V::NotPending {
                    invitation_id: aura_core::InvitationId::new("typed-status"),
                    status,
                }
                .into(),
            );
            assert_eq!(native.invitation_accept_reason(), Some(expected));
            assert_eq!(native.kind(), expected_kind(expected));
            let mut source = native.source();
            let mut found = false;
            while let Some(cause) = source {
                if cause.is::<V>() {
                    found = true;
                    break;
                }
                source = cause.source();
            }
            assert!(found, "actual validation cause must remain available");
        }
    }

    #[test]
    fn confirmation_normalization_preserves_concrete_decision_and_rejects_text() {
        for (cause, expected) in [
            (C::Rejected(D::Revoked), R::Revoked),
            (C::Rejected(D::Expired), R::Expired),
            (C::Rejected(D::AlreadySettled), R::AlreadySettled),
            (C::Rejected(D::Confirmed), R::AlreadySettled),
            (C::Unconfirmed(30_000), R::Unconfirmed),
        ] {
            let native = bridge_runtime_invitation_accept(cause.into());
            assert_eq!(native.invitation_accept_reason(), Some(expected));
            assert_eq!(native.kind(), expected_kind(expected));
            let mut source = native.source();
            let mut found = false;
            while let Some(cause) = source {
                if cause.is::<C>() {
                    found = true;
                    break;
                }
                source = cause.source();
            }
            assert!(found, "actual confirmation cause must remain available");
        }
        for text in [
            "invitation already accepted",
            "invitation not pending",
            "The inviter revoked this contact invitation",
        ] {
            let native = bridge_runtime_invitation_accept(crate::core::AgentError::invalid(text));
            assert_eq!(native.invitation_accept_reason(), None);
        }
    }
    #[test]
    fn typed_enrollment_terminal_failures_keep_reason_category_and_original_source() {
        use crate::handlers::invitation::EnrollmentVmAdmissionError as E;
        use aura_app::runtime_bridge::{CeremonyFailureReason as R, RuntimeBridgeErrorKind as K};
        use std::error::Error;
        for (reason, kind) in [
            (R::Rejected, K::Unauthorized),
            (R::Cancelled, K::Validation),
            (R::TimedOut, K::TimedOut),
            (R::ChoreographyFailed, K::Network),
            (R::RuntimeFailed, K::Service),
            (R::Superseded, K::Validation),
        ] {
            let cause = aura_core::AuraError::crypto_with_source(
                "context before diagnostic boundary",
                std::sync::Arc::new(E::TerminalFailed(reason)),
            );
            let native = bridge_runtime_invitation_accept(cause.into()).clone();
            assert_eq!(native.enrollment_terminal_reason(), Some(reason));
            assert_eq!(native.kind(), kind);
            let mut source = native.source();
            let mut original = None;
            while let Some(error) = source {
                if let Some(E::TerminalFailed(actual)) = error.downcast_ref::<E>() {
                    original = Some(*actual);
                    break;
                }
                source = error.source();
            }
            assert_eq!(
                original,
                Some(reason),
                "standard source must retain exact domain reason"
            );
        }
        let diagnostic = bridge_runtime_invitation_accept(crate::core::AgentError::internal(
            "enrollment activation failed: Cancelled",
        ));
        assert_eq!(diagnostic.enrollment_terminal_reason(), None);
        assert_eq!(
            diagnostic.kind(),
            K::Internal,
            "text cannot claim a signed terminal outcome"
        );
    }
}
