use aura_app::runtime_bridge::{RuntimeBridgeError, RuntimeBridgeErrorKind};
use aura_app::IntentError;
use aura_core::effects::amp::AmpChannelError;

pub(super) fn map_amp_error(error: AmpChannelError) -> RuntimeBridgeError {
    let scoped_reason = match &error {
        AmpChannelError::AlreadyExists { context, channel } => {
            Some(aura_app::runtime_bridge::AmpFailureReason::AlreadyExists {
                context: *context,
                channel: *channel,
            })
        }
        AmpChannelError::Effect(cause) => aura_protocol::amp::ChannelStateUnavailable::find(cause)
            .map(
                |absence| aura_app::runtime_bridge::AmpFailureReason::ChannelStateUnavailable {
                    context: absence.context(),
                    channel: absence.channel(),
                },
            ),
        _ => None,
    };
    let detail = format!("AMP operation failed: {error}");
    let (diagnostic, kind) = match &error {
        AmpChannelError::NotFound | AmpChannelError::ContextNotFound => (
            IntentError::validation_failed(detail),
            RuntimeBridgeErrorKind::NotFound,
        ),
        AmpChannelError::AlreadyExists { .. }
        | AmpChannelError::InvalidState(_)
        | AmpChannelError::RejoinRequiresMembershipEvidence { .. } => (
            IntentError::validation_failed(detail),
            RuntimeBridgeErrorKind::Validation,
        ),
        AmpChannelError::Unauthorized => (
            IntentError::unauthorized(detail),
            RuntimeBridgeErrorKind::Unauthorized,
        ),
        AmpChannelError::Storage(_) => (
            IntentError::storage_error(detail),
            RuntimeBridgeErrorKind::Storage,
        ),
        AmpChannelError::Crypto(_) => (
            IntentError::internal_error(detail),
            RuntimeBridgeErrorKind::Crypto,
        ),
        AmpChannelError::Internal(_) => (
            IntentError::internal_error(detail),
            RuntimeBridgeErrorKind::Internal,
        ),
        AmpChannelError::Effect(cause) => match cause {
            aura_core::AuraError::Invalid { .. } => (
                IntentError::validation_failed(detail),
                RuntimeBridgeErrorKind::Validation,
            ),
            aura_core::AuraError::NotFound { .. } => (
                IntentError::validation_failed(detail),
                RuntimeBridgeErrorKind::NotFound,
            ),
            aura_core::AuraError::PermissionDenied { .. } => (
                IntentError::unauthorized(detail),
                RuntimeBridgeErrorKind::Unauthorized,
            ),
            aura_core::AuraError::Storage { .. } => (
                IntentError::storage_error(detail),
                RuntimeBridgeErrorKind::Storage,
            ),
            aura_core::AuraError::Network { .. } => (
                IntentError::network_error(detail),
                RuntimeBridgeErrorKind::Network,
            ),
            aura_core::AuraError::Crypto { .. } => (
                IntentError::internal_error(detail),
                RuntimeBridgeErrorKind::Crypto,
            ),
            aura_core::AuraError::Serialization { .. } => (
                IntentError::internal_error(detail),
                RuntimeBridgeErrorKind::Serialization,
            ),
            aura_core::AuraError::Internal { .. } | aura_core::AuraError::Terminal(_) => (
                IntentError::internal_error(detail),
                RuntimeBridgeErrorKind::Internal,
            ),
        },
    };
    let normalized = RuntimeBridgeError::with_source(diagnostic, error).with_kind(kind);
    match scoped_reason {
        Some(reason) => normalized.with_amp_failure_reason(reason),
        None => normalized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn native_amp_mapping_retains_actual_nested_storage_cause() {
        let original = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let cause = aura_core::AuraError::Storage {
            message: "required journal read".into(),
            source: Some(std::sync::Arc::new(original)),
        };
        let error = map_amp_error(AmpChannelError::Effect(cause)).clone();
        assert_eq!(error.kind(), RuntimeBridgeErrorKind::Storage);
        let amp = error
            .source()
            .unwrap()
            .downcast_ref::<AmpChannelError>()
            .unwrap();
        let aura = amp
            .source()
            .unwrap()
            .downcast_ref::<aura_core::AuraError>()
            .unwrap();
        assert_eq!(
            aura.source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn known_amp_effect_categories_are_not_unclassified_internal_faults() {
        for (cause, kind) in [
            (
                aura_core::AuraError::invalid("input"),
                RuntimeBridgeErrorKind::Validation,
            ),
            (
                aura_core::AuraError::not_found("checkpoint"),
                RuntimeBridgeErrorKind::NotFound,
            ),
            (
                aura_core::AuraError::permission_denied("guard"),
                RuntimeBridgeErrorKind::Unauthorized,
            ),
            (
                aura_core::AuraError::crypto("signature"),
                RuntimeBridgeErrorKind::Crypto,
            ),
            (
                aura_core::AuraError::serialization("wire"),
                RuntimeBridgeErrorKind::Serialization,
            ),
            (
                aura_core::AuraError::storage("journal"),
                RuntimeBridgeErrorKind::Storage,
            ),
            (
                aura_core::AuraError::network("transport"),
                RuntimeBridgeErrorKind::Network,
            ),
        ] {
            assert_eq!(map_amp_error(AmpChannelError::Effect(cause)).kind(), kind);
        }
    }
}

pub(super) fn map_amp_budget_error(error: aura_core::TimeoutBudgetError) -> RuntimeBridgeError {
    let kind = match &error {
        aura_core::TimeoutBudgetError::CheckpointFailure { .. } => {
            super::error_boundary::native_cause_kind(&error)
        }
        aura_core::TimeoutBudgetError::ObservationUnavailable
        | aura_core::TimeoutBudgetError::CheckpointDiscontinuity { .. } => {
            RuntimeBridgeErrorKind::Internal
        }
        aura_core::TimeoutBudgetError::DeadlineExceeded { .. } => RuntimeBridgeErrorKind::TimedOut,
        aura_core::TimeoutBudgetError::InvalidPolicy { .. }
        | aura_core::TimeoutBudgetError::AttemptBudgetExhausted { .. } => {
            RuntimeBridgeErrorKind::Validation
        }
        aura_core::TimeoutBudgetError::ClockRollback { .. }
        | aura_core::TimeoutBudgetError::TimeSourceUnavailable { .. } => {
            RuntimeBridgeErrorKind::Service
        }
    };
    let diagnostic = match kind {
        RuntimeBridgeErrorKind::Storage => IntentError::storage_error(error.to_string()),
        RuntimeBridgeErrorKind::Validation => IntentError::validation_failed(error.to_string()),
        RuntimeBridgeErrorKind::Service => IntentError::service_error(error.to_string()),
        _ => IntentError::internal_error(error.to_string()),
    };
    RuntimeBridgeError::with_source(diagnostic, error).with_kind(kind)
}

#[cfg(test)]
mod budget_source_tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn repair_budget_classification_retains_each_concrete_failure() {
        for (budget, kind) in [
            (
                aura_core::TimeoutBudgetError::invalid_policy("invalid"),
                RuntimeBridgeErrorKind::Validation,
            ),
            (
                aura_core::TimeoutBudgetError::AttemptBudgetExhausted {
                    max_attempts: 1,
                    attempts_used: 1,
                },
                RuntimeBridgeErrorKind::Validation,
            ),
            (
                aura_core::TimeoutBudgetError::DeadlineExceeded {
                    deadline_at_ms: 1,
                    observed_at_ms: 2,
                },
                RuntimeBridgeErrorKind::TimedOut,
            ),
            (
                aura_core::TimeoutBudgetError::time_source_failure(std::io::Error::other(
                    "actual clock fault",
                )),
                RuntimeBridgeErrorKind::Service,
            ),
        ] {
            let error = map_amp_budget_error(budget);
            assert_eq!(error.kind(), kind);
            let clone = error.clone();
            assert!(clone
                .source()
                .expect("concrete budget retained")
                .is::<aura_core::TimeoutBudgetError>());
            if kind == RuntimeBridgeErrorKind::Service {
                let mut current = clone.source();
                let mut found = false;
                while let Some(source) = current {
                    found |= source.is::<std::io::Error>();
                    current = source.source();
                }
                assert!(found, "actual clock failure must remain in native chain");
            }
        }
    }
}

#[cfg(test)]
mod checkpoint_category_tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn checkpoint_categories_follow_actual_sources() {
        let codec = serde_json::from_slice::<u64>(b"invalid").expect_err("actual codec failure");
        let failure =
            map_amp_budget_error(aura_core::TimeoutBudgetError::checkpoint_failure(codec));
        assert_eq!(failure.kind(), RuntimeBridgeErrorKind::Serialization);
        let mut cause = failure.source();
        let mut retained = false;
        while let Some(current) = cause {
            retained |= current.is::<serde_json::Error>();
            cause = current.source();
        }
        assert!(retained);
        let storage = map_amp_budget_error(aura_core::TimeoutBudgetError::checkpoint_failure(
            aura_core::AuraError::storage("checkpoint unavailable"),
        ));
        assert_eq!(storage.kind(), RuntimeBridgeErrorKind::Storage);
        let unknown = map_amp_budget_error(aura_core::TimeoutBudgetError::checkpoint_failure(
            std::io::Error::other("unclassified checkpoint"),
        ));
        assert_eq!(unknown.kind(), RuntimeBridgeErrorKind::Internal);
    }
}
