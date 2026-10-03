use super::*;

pub(super) fn handle_invitation_vm_wait_status(
    status: AuraVmHostWaitStatus,
    deferred_completes: bool,
    timeout_message: &'static str,
    cancel_message: &'static str,
) -> AgentResult<Option<()>> {
    match status {
        AuraVmHostWaitStatus::Deferred if deferred_completes => Ok(Some(())),
        AuraVmHostWaitStatus::Idle
        | AuraVmHostWaitStatus::Delivered
        | AuraVmHostWaitStatus::Deferred => Ok(None),
        AuraVmHostWaitStatus::TimedOut => Err(AgentError::timeout(timeout_message)),
        AuraVmHostWaitStatus::Cancelled => Err(AgentError::internal(cancel_message.to_string())),
    }
}

pub(super) fn handle_invitation_vm_step(
    step: StepResult,
    stuck_message: &'static str,
) -> AgentResult<bool> {
    match step {
        StepResult::AllDone => Ok(true),
        StepResult::Continue => Ok(false),
        StepResult::Stuck => Err(AgentError::internal(stuck_message.to_string())),
    }
}

pub(super) fn map_invitation_vm_timeout(
    label: &'static str,
    budget: &TimeoutBudget,
    error: TimeoutRunError<AgentError>,
) -> AgentError {
    match error {
        TimeoutRunError::Timeout(error) => {
            let message = format!("{label}: {error}");
            let deadline = matches!(
                &error,
                aura_core::TimeoutBudgetError::DeadlineExceeded { .. }
            );
            let invalid = match &error {
                aura_core::TimeoutBudgetError::InvalidPolicy { .. }
                | aura_core::TimeoutBudgetError::AttemptBudgetExhausted { .. } => true,
                aura_core::TimeoutBudgetError::DeadlineExceeded { .. }
                | aura_core::TimeoutBudgetError::ClockRollback { .. }
                | aura_core::TimeoutBudgetError::ObservationUnavailable
                | aura_core::TimeoutBudgetError::CheckpointDiscontinuity { .. }
                | aura_core::TimeoutBudgetError::CheckpointFailure { .. }
                | aura_core::TimeoutBudgetError::TimeSourceUnavailable { .. } => false,
            };
            let source =
                Some(std::sync::Arc::new(error)
                    as std::sync::Arc<dyn std::error::Error + Send + Sync>);
            if deadline {
                AgentError::TimeoutWithSource {
                    message: format!("{label} exceeded {}ms overall timeout", budget.timeout_ms()),
                    source: aura_core::AuraError::Internal { message, source },
                }
            } else if invalid {
                AgentError::Aura(aura_core::AuraError::Invalid { message, source })
            } else {
                AgentError::Aura(aura_core::AuraError::Internal { message, source })
            }
        }
        TimeoutRunError::Operation(error) => error,
    }
}

/// Reject a malformed or mismatched peer message with a static reason prefix.
pub(super) fn invitation_invalid_error(
    prefix: &'static str,
    detail: impl std::fmt::Display,
) -> AgentError {
    let mut message = String::from(prefix);
    message.push_str(": ");
    message.push_str(&detail.to_string());
    AgentError::invalid(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_response_wait_preserves_typed_timeout_for_retry_policy() {
        let error = handle_invitation_vm_wait_status(
            AuraVmHostWaitStatus::TimedOut,
            false,
            "invitation no-response deadline",
            "invitation cancelled",
        )
        .unwrap_err();
        assert!(error.is_timeout());
        assert_eq!(
            handle_invitation_vm_wait_status(
                AuraVmHostWaitStatus::Deferred,
                false,
                "invitation no-response deadline",
                "invitation cancelled",
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn budget_failure_classification_retains_original_source() {
        use std::error::Error;
        let budget = TimeoutBudget::from_start_and_timeout(
            &aura_core::time::PhysicalTime {
                ts_ms: 100,
                uncertainty: None,
            },
            std::time::Duration::from_millis(50),
        )
        .unwrap();
        for cause in [
            aura_core::TimeoutBudgetError::deadline_exceeded(150, 151),
            aura_core::TimeoutBudgetError::invalid_policy("bad policy"),
            aura_core::TimeoutBudgetError::time_source_unavailable("required clock unavailable"),
            aura_core::TimeoutBudgetError::attempt_budget_exhausted(2, 2),
        ] {
            let expected_timeout = matches!(
                &cause,
                aura_core::TimeoutBudgetError::DeadlineExceeded { .. }
            );
            let expected = cause.to_string();
            let error =
                map_invitation_vm_timeout("receive", &budget, TimeoutRunError::Timeout(cause));
            assert_eq!(error.is_timeout(), expected_timeout);
            let actual = error
                .source()
                .unwrap()
                .source()
                .unwrap()
                .downcast_ref::<aura_core::TimeoutBudgetError>()
                .unwrap();
            assert_eq!(actual.to_string(), expected);
        }
        let error = map_invitation_vm_timeout(
            "receive",
            &budget,
            TimeoutRunError::Operation(AgentError::invalid("peer rejected")),
        );
        assert!(!error.is_timeout());
        assert_eq!(
            error.to_string(),
            AgentError::invalid("peer rejected").to_string()
        );
    }
}
