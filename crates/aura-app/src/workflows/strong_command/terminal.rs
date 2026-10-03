#![allow(missing_docs)]

use super::execution_model::{
    CommandTerminalClassification, CommandTerminalOutcomeStatus, CommandTerminalReasonCode,
};
use aura_core::AuraError;

fn resolver_failure(error: &super::resolve::CommandResolverError) -> CommandTerminalClassification {
    use super::resolve::CommandResolverError as E;
    use CommandTerminalOutcomeStatus as S;
    use CommandTerminalReasonCode as R;
    let (status, reason) = match error {
        E::UnknownTarget { .. } => (S::Invalid, R::NotFound),
        E::AmbiguousTarget { .. } | E::ParseError { .. } => (S::Invalid, R::InvalidArgument),
        E::StaleSnapshot { .. } => (S::Failed, R::InvalidState),
        E::MissingCurrentChannel { .. } => (S::Invalid, R::MissingActiveContext),
    };
    CommandTerminalClassification::new(status, reason)
}

fn command_domain_failure(error: &AuraError) -> Option<CommandTerminalClassification> {
    use super::execution_model::CommandExecutionFailure as E;
    use std::error::Error;
    use CommandTerminalOutcomeStatus as S;
    use CommandTerminalReasonCode as R;
    let mut source = Some(error as &(dyn Error + 'static));
    while let Some(cause) = source {
        if let Some(denial) = cause.downcast_ref::<crate::workflows::moderation::ModerationDenial>()
        {
            use crate::workflows::moderation::ModerationDenial as D;
            let reason = match denial {
                D::NotMember { .. } => R::NotMember,
                D::Muted { .. } => R::Muted,
                D::Banned { .. } => R::Banned,
            };
            return Some(CommandTerminalClassification::new(S::Denied, reason));
        }

        if let Some(domain) = cause.downcast_ref::<E>() {
            return Some(match domain {
                E::Precondition(error) => resolver_failure(error),
                E::MissingChannelScope { .. } => {
                    CommandTerminalClassification::new(S::Invalid, R::MissingActiveContext)
                }
                E::InvalidPlan { .. } => {
                    CommandTerminalClassification::new(S::Failed, R::InvalidState)
                }
                E::FeatureUnavailable => {
                    CommandTerminalClassification::new(S::Failed, R::Unavailable)
                }
            });
        }
        if let Some(resolver) = cause.downcast_ref::<super::resolve::CommandResolverError>() {
            return Some(resolver_failure(resolver));
        }
        source = cause.source();
    }
    None
}

/// Classify strong command execution failures for terminal-facing outcome
/// rendering without requiring Layer 7 string parsing.
#[must_use]
pub fn classify_terminal_execution_error(error: &AuraError) -> CommandTerminalClassification {
    if let Some(kind) =
        crate::workflows::runtime_error_classification::native_runtime_error_kind(error)
    {
        use crate::runtime_bridge::RuntimeBridgeErrorKind as K;
        use CommandTerminalOutcomeStatus as S;
        use CommandTerminalReasonCode as R;
        let (status, reason) = match kind {
            K::Crypto => (S::Failed, R::CryptoFailure),
            K::Serialization => (S::Failed, R::SerializationFailure),
            K::Storage => (S::Failed, R::StorageFailure),
            K::Journal => (S::Failed, R::JournalFailure),
            K::Reactive => (S::Failed, R::ReactiveFailure),
            K::Unauthorized => (S::Denied, R::PermissionDenied),
            K::Validation => (S::Invalid, R::InvalidArgument),
            K::NotFound | K::ContextNotFound => (S::Invalid, R::NotFound),
            K::Network | K::NoAgent | K::Service => (S::Failed, R::Unavailable),
            K::TimedOut => (S::Failed, R::OperationTimedOut),
            K::Internal => (S::Failed, R::Internal),
        };
        return CommandTerminalClassification::new(status, reason);
    }
    if let Some(classification) = command_domain_failure(error) {
        return classification;
    }
    match error {
        AuraError::Invalid { .. } => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::InvalidArgument,
        ),
        AuraError::NotFound { .. } => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::NotFound,
        ),
        AuraError::PermissionDenied { .. } => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Denied,
            CommandTerminalReasonCode::PermissionDenied,
        ),
        AuraError::Crypto { .. } => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Failed,
            CommandTerminalReasonCode::CryptoFailure,
        ),
        AuraError::Serialization { .. } => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Failed,
            CommandTerminalReasonCode::SerializationFailure,
        ),
        AuraError::Storage { .. } => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Failed,
            CommandTerminalReasonCode::StorageFailure,
        ),
        AuraError::Network { .. } => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Failed,
            CommandTerminalReasonCode::Unavailable,
        ),
        AuraError::Internal { .. } | AuraError::Terminal(_) => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Failed,
            CommandTerminalReasonCode::Internal,
        ),
    }
}

#[cfg(test)]
mod typed_command_domain_tests {
    use super::super::{CommandExecutionFailure, CommandResolverError, ResolveTarget};
    use super::*;
    #[test]
    fn resolver_precondition_retains_actual_domain_reason_and_source() {
        use std::error::Error;
        let error = AuraError::from(CommandExecutionFailure::Precondition(
            CommandResolverError::UnknownTarget {
                target: ResolveTarget::Authority,
                input: "stale snapshot permission denied timeout".into(),
            },
        ));
        let classification = classify_terminal_execution_error(&error);
        assert_eq!(classification.status, CommandTerminalOutcomeStatus::Invalid);
        assert_eq!(classification.reason, CommandTerminalReasonCode::NotFound);
        let domain = error
            .source()
            .unwrap()
            .downcast_ref::<CommandExecutionFailure>()
            .unwrap();
        assert!(domain.source().unwrap().is::<CommandResolverError>());
    }
    #[test]
    fn display_text_cannot_forge_strong_command_subreason() {
        for text in [
            "missing channel scope",
            "not found",
            "stale snapshot",
            "precondition failed",
            "parse error",
        ] {
            let classified = classify_terminal_execution_error(&AuraError::invalid(text));
            assert_eq!(
                classified.reason,
                CommandTerminalReasonCode::InvalidArgument
            );
        }
        for text in ["not a member", "muted", "banned"] {
            let classified = classify_terminal_execution_error(&AuraError::permission_denied(text));
            assert_eq!(
                classified.reason,
                CommandTerminalReasonCode::PermissionDenied
            );
        }
    }
}

#[cfg(test)]
mod native_terminal_category_tests {
    use super::*;
    use crate::{
        runtime_bridge::{RuntimeBridgeError, RuntimeBridgeErrorKind},
        IntentError,
    };
    #[test]
    fn direct_structural_faults_do_not_become_internal_or_unauthorized() {
        for (error, reason) in [
            (
                AuraError::crypto("permission denied"),
                CommandTerminalReasonCode::CryptoFailure,
            ),
            (
                AuraError::serialization("invalid argument"),
                CommandTerminalReasonCode::SerializationFailure,
            ),
            (
                AuraError::storage("timeout"),
                CommandTerminalReasonCode::StorageFailure,
            ),
            (
                AuraError::network("internal"),
                CommandTerminalReasonCode::Unavailable,
            ),
        ] {
            let classified = classify_terminal_execution_error(&error);
            assert_eq!(classified.status, CommandTerminalOutcomeStatus::Failed);
            assert_eq!(classified.reason, reason);
        }
    }
    #[test]
    fn retained_native_fault_controls_classification_through_workflow_context() {
        for (kind, reason) in [
            (
                RuntimeBridgeErrorKind::Crypto,
                CommandTerminalReasonCode::CryptoFailure,
            ),
            (
                RuntimeBridgeErrorKind::Serialization,
                CommandTerminalReasonCode::SerializationFailure,
            ),
            (
                RuntimeBridgeErrorKind::Storage,
                CommandTerminalReasonCode::StorageFailure,
            ),
            (
                RuntimeBridgeErrorKind::Journal,
                CommandTerminalReasonCode::JournalFailure,
            ),
            (
                RuntimeBridgeErrorKind::Reactive,
                CommandTerminalReasonCode::ReactiveFailure,
            ),
        ] {
            let native = RuntimeBridgeError::with_source(
                IntentError::internal_error("permission denied timeout"),
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            )
            .with_kind(kind);
            let error = AuraError::from(crate::workflows::error::native_runtime_call(
                "required read",
                native,
            ));
            let classified = classify_terminal_execution_error(&error);
            assert_eq!(classified.status, CommandTerminalOutcomeStatus::Failed);
            assert_eq!(classified.reason, reason);
        }
    }
}
