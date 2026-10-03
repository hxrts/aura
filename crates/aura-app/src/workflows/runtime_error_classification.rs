/// Read native evidence through context wrappers, without examining diagnostic text.
pub(crate) fn native_runtime_error_kind(
    error: &(impl std::error::Error + 'static),
) -> Option<crate::runtime_bridge::RuntimeBridgeErrorKind> {
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = cause {
        if let Some(native) = error.downcast_ref::<crate::runtime_bridge::RuntimeBridgeError>() {
            return Some(native.kind());
        }
        cause = error.source();
    }
    None
}

pub(crate) fn native_runtime_failure_code(
    error: &(impl std::error::Error + 'static),
) -> Option<crate::ui_contract::SemanticFailureCode> {
    use crate::runtime_bridge::RuntimeBridgeErrorKind as K;
    use crate::ui_contract::SemanticFailureCode as C;
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(source) = cause {
        if let Some(native) = source.downcast_ref::<crate::runtime_bridge::RuntimeBridgeError>() {
            if let Some(reason) = native.enrollment_terminal_reason() {
                use crate::runtime_bridge::CeremonyFailureReason as R;
                return Some(match reason {
                    R::Rejected => C::CeremonyRejected,
                    R::Cancelled => C::CeremonyCancelled,
                    R::TimedOut => C::OperationTimedOut,
                    R::ChoreographyFailed => C::CeremonyChoreographyFailed,
                    R::RuntimeFailed => C::CeremonyRuntimeFailed,
                    R::Superseded => C::CeremonySuperseded,
                });
            }
        }
        cause = source.source();
    }
    native_runtime_error_kind(error).map(|kind| match kind {
        K::Crypto => C::CryptoFailure,
        K::Serialization => C::SerializationFailure,
        K::Storage => C::StorageFailure,
        K::Journal => C::JournalFailure,
        K::Reactive => C::ReactiveFailure,
        K::Unauthorized => C::PermissionDenied,
        K::Validation => C::InvalidArgument,
        K::NotFound | K::ContextNotFound => C::NotFound,
        K::Network | K::NoAgent | K::Service => C::Unavailable,
        K::TimedOut => C::OperationTimedOut,
        K::Internal => C::InternalError,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvitationAcceptErrorClass {
    AlreadyHandled,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AmpChannelErrorClass {
    ChannelStateUnavailable,
    AlreadyExists,
    Other,
}

pub(crate) fn invitation_accept_failure_reason(
    error: &(impl std::error::Error + 'static),
) -> Option<crate::runtime_bridge::InvitationAcceptFailureReason> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = current {
        if let Some(native) = cause.downcast_ref::<crate::runtime_bridge::RuntimeBridgeError>() {
            if let Some(reason) = native.invitation_accept_reason() {
                return Some(reason);
            }
        }
        current = cause.source();
    }
    None
}

pub(crate) fn classify_invitation_accept_error(
    error: &(impl std::error::Error + 'static),
) -> InvitationAcceptErrorClass {
    use crate::runtime_bridge::InvitationAcceptFailureReason as Reason;
    match invitation_accept_failure_reason(error) {
        Some(Reason::AlreadyAccepted) => InvitationAcceptErrorClass::AlreadyHandled,
        Some(
            Reason::Revoked
            | Reason::Expired
            | Reason::AlreadySettled
            | Reason::Unconfirmed
            | Reason::NotFound
            | Reason::NotPending
            | Reason::PermissionDenied,
        )
        | None => InvitationAcceptErrorClass::Other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactConfirmationErrorClass {
    Revoked,
    Expired,
    AlreadySettled,
    Unconfirmed,
}

pub(crate) fn classify_contact_confirmation_error(
    error: &(impl std::error::Error + 'static),
) -> Option<ContactConfirmationErrorClass> {
    use crate::runtime_bridge::InvitationAcceptFailureReason as Reason;
    match invitation_accept_failure_reason(error) {
        Some(Reason::Revoked) => Some(ContactConfirmationErrorClass::Revoked),
        Some(Reason::Expired) => Some(ContactConfirmationErrorClass::Expired),
        Some(Reason::AlreadySettled) => Some(ContactConfirmationErrorClass::AlreadySettled),
        Some(Reason::Unconfirmed) => Some(ContactConfirmationErrorClass::Unconfirmed),
        Some(
            Reason::AlreadyAccepted
            | Reason::NotFound
            | Reason::NotPending
            | Reason::PermissionDenied,
        )
        | None => None,
    }
}

pub(crate) fn classify_amp_channel_error(
    error: &(impl std::error::Error + 'static),
    expected_context: aura_core::ContextId,
    expected_channel: aura_core::ChannelId,
) -> AmpChannelErrorClass {
    use crate::runtime_bridge::AmpFailureReason;
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = cause {
        if let Some(native) = error.downcast_ref::<crate::runtime_bridge::RuntimeBridgeError>() {
            match native.amp_failure_reason() {
                Some(AmpFailureReason::ChannelStateUnavailable { context, channel })
                    if (context, channel) == (expected_context, expected_channel) =>
                {
                    return AmpChannelErrorClass::ChannelStateUnavailable
                }
                Some(AmpFailureReason::AlreadyExists { context, channel })
                    if (context, channel) == (expected_context, expected_channel) =>
                {
                    return AmpChannelErrorClass::AlreadyExists
                }
                Some(
                    AmpFailureReason::ChannelStateUnavailable { .. }
                    | AmpFailureReason::AlreadyExists { .. },
                )
                | None => {}
            }
        }
        if matches!(error.downcast_ref::<aura_core::effects::amp::AmpChannelError>(),
            Some(aura_core::effects::amp::AmpChannelError::AlreadyExists { context, channel })
                if (*context, *channel) == (expected_context, expected_channel))
        {
            return AmpChannelErrorClass::AlreadyExists;
        }
        cause = error.source();
    }
    AmpChannelErrorClass::Other
}

/// Follow actual lower-owner causes when an infrastructure checkpoint adds context.
/// Unknown IO alone does not establish which runtime service failed.
pub(crate) fn runtime_source_failure_code(
    error: &(impl std::error::Error + 'static),
) -> Option<crate::ui_contract::SemanticFailureCode> {
    use crate::runtime_bridge::{RuntimeBridgeError, RuntimeBridgeErrorKind};
    use crate::ui_contract::SemanticFailureCode as C;
    use aura_core::AuraError as A;
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = source {
        if let Some(native) = cause.downcast_ref::<RuntimeBridgeError>() {
            if native.kind() != RuntimeBridgeErrorKind::Internal {
                return native_runtime_failure_code(native);
            }
        }
        if let Some(budget) = cause.downcast_ref::<aura_core::TimeoutBudgetError>() {
            return Some(timeout_budget_failure_code(budget));
        }
        if let Some(core) = cause.downcast_ref::<A>() {
            let code = match core {
                A::Crypto { .. } => Some(C::CryptoFailure),
                A::Serialization { .. } => Some(C::SerializationFailure),
                A::Storage { .. } => Some(C::StorageFailure),
                A::Invalid { .. } => Some(C::InvalidArgument),
                A::NotFound { .. } => Some(C::NotFound),
                A::PermissionDenied { .. } => Some(C::PermissionDenied),
                A::Network { .. } => Some(C::Unavailable),
                A::Internal { .. } | A::Terminal(_) => None,
            };
            if code.is_some() {
                return code;
            }
        }
        if cause.is::<serde_json::Error>()
            || cause.is::<aura_core::util::serialization::SerializationError>()
        {
            return Some(C::SerializationFailure);
        }
        if cause.is::<aura_core::effects::StorageError>() {
            return Some(C::StorageFailure);
        }
        if cause.is::<aura_core::effects::reactive::ReactiveError>() {
            return Some(C::ReactiveFailure);
        }
        if cause.is::<aura_core::effects::time::TimeError>() {
            return Some(C::Unavailable);
        }
        source = cause.source();
    }
    None
}

/// Exhaustive semantic classification of an actual required budget failure.
pub(crate) fn timeout_budget_failure_code(
    error: &aura_core::TimeoutBudgetError,
) -> crate::ui_contract::SemanticFailureCode {
    use crate::ui_contract::SemanticFailureCode as C;
    use aura_core::TimeoutBudgetError as B;
    match error {
        B::ClockRollback { .. } | B::TimeSourceUnavailable { .. } => C::Unavailable,
        B::ObservationUnavailable | B::CheckpointDiscontinuity { .. } => C::InternalError,
        B::CheckpointFailure { source, .. } => source
            .as_ref()
            .and_then(runtime_source_failure_code)
            .unwrap_or(C::InternalError),
        B::DeadlineExceeded { .. } => C::OperationTimedOut,
        B::InvalidPolicy { .. } | B::AttemptBudgetExhausted { .. } => C::InvalidArgument,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::IntentError,
        runtime_bridge::{InvitationAcceptFailureReason as Reason, RuntimeBridgeError},
    };
    use aura_core::AuraError;

    #[test]
    fn checkpoint_failure_uses_actual_storage_codec_or_clock_source() {
        use crate::ui_contract::SemanticFailureCode as C;
        use aura_core::TimeoutBudgetError as B;
        let storage = B::CheckpointFailure {
            detail: "clock unavailable timeout".into(),
            source: Some(AuraError::Internal {
                message: "checkpoint context".into(),
                source: Some(std::sync::Arc::new(
                    aura_core::effects::StorageError::ReadFailed("actual read fault".into()),
                )),
            }),
        };
        assert_eq!(timeout_budget_failure_code(&storage), C::StorageFailure);
        let json = serde_json::from_slice::<serde_json::Value>(b"not-json").unwrap_err();
        let codec = B::CheckpointFailure {
            detail: "storage".into(),
            source: Some(AuraError::Internal {
                message: "checkpoint context".into(),
                source: Some(std::sync::Arc::new(json)),
            }),
        };
        assert_eq!(timeout_budget_failure_code(&codec), C::SerializationFailure);
        let clock =
            B::time_source_failure(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(timeout_budget_failure_code(&clock), C::Unavailable);
        let unknown = B::CheckpointFailure {
            detail: "storage serialization timeout".into(),
            source: Some(AuraError::Internal {
                message: "unknown operation".into(),
                source: Some(std::sync::Arc::new(std::io::Error::from(
                    std::io::ErrorKind::Other,
                ))),
            }),
        };
        assert_eq!(timeout_budget_failure_code(&unknown), C::InternalError);
        assert_eq!(
            timeout_budget_failure_code(&B::CheckpointFailure {
                detail: "clock unavailable".into(),
                source: None
            }),
            C::InternalError
        );
    }

    #[test]
    fn native_categories_survive_settings_context_and_foreign_code_projection() {
        use crate::runtime_bridge::RuntimeBridgeErrorKind as K;
        use crate::ui_contract::SemanticFailureCode as C;
        use std::error::Error;
        let cases = [
            (K::Crypto, "crypto", C::CryptoFailure, "crypto_error", false),
            (
                K::Serialization,
                "serialization",
                C::SerializationFailure,
                "serialization_error",
                false,
            ),
            (
                K::Unauthorized,
                "permission_denied",
                C::PermissionDenied,
                "unauthorized",
                false,
            ),
            (
                K::Validation,
                "invalid",
                C::InvalidArgument,
                "validation_failed",
                true,
            ),
            (
                K::Journal,
                "internal",
                C::JournalFailure,
                "journal_error",
                false,
            ),
            (
                K::Internal,
                "internal",
                C::InternalError,
                "internal_error",
                false,
            ),
            (
                K::Reactive,
                "internal",
                C::ReactiveFailure,
                "reactive_failure",
                true,
            ),
            (
                K::ContextNotFound,
                "not_found",
                C::NotFound,
                "context_not_found",
                false,
            ),
            (K::NotFound, "not_found", C::NotFound, "not_found", false),
            (K::Network, "network", C::Unavailable, "network_error", true),
            (
                K::Storage,
                "storage",
                C::StorageFailure,
                "storage_error",
                false,
            ),
            (K::NoAgent, "internal", C::Unavailable, "no_agent", false),
            (
                K::Service,
                "internal",
                C::Unavailable,
                "service_error",
                true,
            ),
            (
                K::TimedOut,
                "internal",
                C::OperationTimedOut,
                "timed_out",
                true,
            ),
        ];
        for (kind, category, code, _foreign_code, _recoverable) in cases {
            let native = RuntimeBridgeError::with_source(
                IntentError::internal_error(
                    "identical misleading display: timeout invalid permission",
                ),
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            )
            .with_kind(kind);
            #[cfg(feature = "callbacks")]
            {
                let callback = crate::bridge::callback::CallbackError::from(native.clone());
                assert_eq!(callback.code, _foreign_code);
                assert_eq!(callback.recoverable, _recoverable);
            }
            let workflow =
                crate::workflows::error::native_runtime_call("required settings read", native);
            let outer = AuraError::from(workflow);
            assert_eq!(outer.category(), category);
            assert_eq!(native_runtime_failure_code(&outer), Some(code));
            let mut current = Some(&outer as &(dyn Error + 'static));
            let mut found = false;
            while let Some(cause) = current {
                found |= cause.is::<std::io::Error>();
                current = cause.source();
            }
            assert!(found, "concrete IO cause lost for {kind:?}");
        }
    }

    #[test]
    fn native_projection_does_not_authorize_from_diagnostic_words() {
        for message in [
            "crypto_error",
            "serialization_error",
            "permission denied",
            "timed out",
        ] {
            let error = AuraError::internal(message);
            assert_eq!(native_runtime_error_kind(&error), None);
            assert_eq!(native_runtime_failure_code(&error), None);
        }
    }

    #[test]
    fn actual_json_fault_survives_required_settings_workflow_context() {
        use crate::runtime_bridge::RuntimeBridgeErrorKind as K;
        use std::error::Error;
        let native = RuntimeBridgeError::with_source(
            IntentError::internal_error("account corrupt"),
            serde_json::from_slice::<serde_json::Value>(b"not-json").unwrap_err(),
        )
        .with_kind(K::Serialization);
        let outer = AuraError::from(crate::workflows::error::native_runtime_call(
            "required account read",
            native,
        ));
        assert_eq!(outer.category(), "serialization");
        let mut source = outer.source();
        let mut found = false;
        while let Some(cause) = source {
            found |= cause.is::<serde_json::Error>();
            source = cause.source();
        }
        assert!(found);
    }

    #[test]
    fn invitation_classification_requires_typed_runtime_reason() {
        for reason in [
            Reason::AlreadyAccepted,
            Reason::Revoked,
            Reason::Expired,
            Reason::AlreadySettled,
            Reason::Unconfirmed,
            Reason::NotFound,
            Reason::NotPending,
            Reason::PermissionDenied,
        ] {
            let native = RuntimeBridgeError::with_source(
                IntentError::internal_error("display is deliberately unrelated"),
                std::io::Error::other("original"),
            )
            .with_invitation_accept_reason(reason);
            let outer = AuraError::from(native);
            let expected = match reason {
                Reason::AlreadyAccepted => InvitationAcceptErrorClass::AlreadyHandled,
                Reason::Revoked
                | Reason::Expired
                | Reason::AlreadySettled
                | Reason::Unconfirmed
                | Reason::NotFound
                | Reason::NotPending
                | Reason::PermissionDenied => InvitationAcceptErrorClass::Other,
            };
            assert_eq!(classify_invitation_accept_error(&outer), expected);
            assert_eq!(invitation_accept_failure_reason(&outer), Some(reason));
        }
    }

    #[test]
    fn invitation_display_lookalikes_cannot_suppress_failure() {
        for message in [
            "invitation already accepted",
            "invitation not pending",
            "The inviter revoked this contact invitation",
            "This contact invitation has expired",
            "This contact invitation was already used",
            "The inviter did not confirm this contact invitation",
        ] {
            let error = AuraError::agent(message);
            assert_eq!(
                classify_invitation_accept_error(&error),
                InvitationAcceptErrorClass::Other
            );
            assert_eq!(classify_contact_confirmation_error(&error), None);
        }
    }

    #[test]
    fn contact_classification_uses_source_chain_reasons() {
        for (reason, expected) in [
            (Reason::Revoked, ContactConfirmationErrorClass::Revoked),
            (Reason::Expired, ContactConfirmationErrorClass::Expired),
            (
                Reason::AlreadySettled,
                ContactConfirmationErrorClass::AlreadySettled,
            ),
            (
                Reason::Unconfirmed,
                ContactConfirmationErrorClass::Unconfirmed,
            ),
        ] {
            let native = RuntimeBridgeError::from(IntentError::internal_error("storage failed"))
                .with_invitation_accept_reason(reason);
            assert_eq!(
                classify_contact_confirmation_error(&AuraError::from(native)),
                Some(expected)
            );
        }
    }

    #[test]
    fn amp_channel_classifier_requires_matching_entity_and_structural_causes() {
        use crate::runtime_bridge::AmpFailureReason;
        use aura_core::effects::amp::AmpChannelError;
        let context = aura_core::ContextId::new_from_entropy([1; 32]);
        let channel = aura_core::ChannelId::from_bytes([2; 32]);
        let wrong_context = aura_core::ContextId::new_from_entropy([3; 32]);
        let wrong_channel = aura_core::ChannelId::from_bytes([4; 32]);
        for (reason, expected) in [
            (
                AmpFailureReason::ChannelStateUnavailable { context, channel },
                AmpChannelErrorClass::ChannelStateUnavailable,
            ),
            (
                AmpFailureReason::AlreadyExists { context, channel },
                AmpChannelErrorClass::AlreadyExists,
            ),
        ] {
            let native = RuntimeBridgeError::with_source(
                IntentError::storage_error("wording changed"),
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            )
            .with_amp_failure_reason(reason);
            let outer = AuraError::from(native);
            assert_eq!(
                classify_amp_channel_error(&outer, context, channel),
                expected
            );
            assert_eq!(
                classify_amp_channel_error(&outer, wrong_context, channel),
                AmpChannelErrorClass::Other
            );
            assert_eq!(
                classify_amp_channel_error(&outer, context, wrong_channel),
                AmpChannelErrorClass::Other
            );
        }
        let duplicate = AmpChannelError::AlreadyExists { context, channel };
        assert_eq!(
            classify_amp_channel_error(&duplicate, context, channel),
            AmpChannelErrorClass::AlreadyExists
        );
        assert_eq!(
            classify_amp_channel_error(&duplicate, context, wrong_channel),
            AmpChannelErrorClass::Other
        );
        for error in [
            AuraError::not_found("channel state not found"),
            AuraError::internal("channel already exists"),
            AuraError::invalid("already exists"),
        ] {
            assert_eq!(
                classify_amp_channel_error(&error, context, channel),
                AmpChannelErrorClass::Other
            );
        }
    }
    #[test]
    fn native_enrollment_terminal_projection_is_exhaustive_and_not_text_based() {
        use crate::runtime_bridge::{CeremonyFailureReason as R, RuntimeBridgeError};
        use crate::ui_contract::SemanticFailureCode as C;
        for (reason, code) in [
            (R::Rejected, C::CeremonyRejected),
            (R::Cancelled, C::CeremonyCancelled),
            (R::TimedOut, C::OperationTimedOut),
            (R::ChoreographyFailed, C::CeremonyChoreographyFailed),
            (R::RuntimeFailed, C::CeremonyRuntimeFailed),
            (R::Superseded, C::CeremonySuperseded),
        ] {
            let native = RuntimeBridgeError::with_source(
                crate::IntentError::internal_error("diagnostic"),
                std::io::Error::from(std::io::ErrorKind::BrokenPipe),
            )
            .with_enrollment_terminal_reason(reason);
            let outer = aura_core::AuraError::Internal {
                message: "owned workflow context".into(),
                source: Some(std::sync::Arc::new(native)),
            };
            assert_eq!(native_runtime_failure_code(&outer), Some(code));
        }
        let text = RuntimeBridgeError::with_source(
            crate::IntentError::internal_error("enrollment activation failed: Rejected"),
            std::io::Error::from(std::io::ErrorKind::BrokenPipe),
        );
        assert_eq!(native_runtime_failure_code(&text), Some(C::InternalError));
    }
}
