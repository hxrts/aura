//! Negative-only transport authority borrowed from the actual original terminal.
use super::*;

pub(crate) struct OriginalCancelledNoticeWindowCapability<'a> {
    child: &'a EnrollmentExecutionChild,
    terminal: &'a Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
    destination: &'a crate::handlers::invitation_service::OriginalIssuerTerminalDestination,
    cleanup: TimeoutBudget,
}
impl OriginalCancelledNoticeWindowCapability<'_> {
    #[cfg(test)]
    pub(crate) fn assert_original_deadline_for_test(&self) {
        let WindowCheckpoint::Registered { capability } = self.child.checkpoint.as_ref() else {
            panic!("actual registered original");
        };
        assert_eq!(
            self.cleanup.started_at_ms(),
            capability.negative_cleanup_budget().started_at_ms()
        );
        assert_eq!(
            self.cleanup.deadline_at_ms(),
            capability
                .negative_cleanup_budget()
                .deadline_at_ms()
                .min(self.terminal.issued().manifest().expires_at_ms)
        );
    }
    pub(crate) fn require_owner(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        effects: &AuraEffectSystem,
    ) -> Result<(), AuraError> {
        self.child.require_execution_effects(effects)?;
        self.child.require_issued_notice_owner(issued)?;
        if !std::ptr::eq(issued, self.terminal.issued()) {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        let WindowCheckpoint::Registered { capability } = self.child.checkpoint.as_ref() else {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        };
        capability.require_original_cancellation(self.terminal)?;
        if !self
            .destination
            .original_cancelled_retirement_acknowledged(capability, self.terminal)?
        {
            return Err(AuraError::permission_denied(
                "original cancelled generation retirement is pending",
            ));
        }
        Ok(())
    }
    pub(crate) async fn execute<F, Fut, T>(
        &self,
        effects: &AuraEffectSystem,
        operation: F,
    ) -> Result<T, TimeoutRunError<crate::core::AgentError>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = crate::core::AgentResult<T>>,
    {
        self.require_owner(self.terminal.issued(), effects)
            .map_err(|source| TimeoutRunError::Operation(source.into()))?;
        let _work = self
            .child
            ._lease
            .borrow_execution()
            .map_err(|source| TimeoutRunError::Operation(source.into()))?;
        let actual = aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
            effects,
            &self.cleanup,
            self.terminal,
            operation,
        )
        .await;
        match (actual.operation, actual.validation) {
            (Some(Err(primary)), Err(secondary)) => Err(TimeoutRunError::Operation(
                crate::core::AgentError::from(AuraError::Internal {
                    message: "original negative notice operation and validation failed".into(),
                    source: Some(Arc::new(
                        crate::handlers::invitation_service::EnrollmentInitiatorTaskFailure::new(
                            primary,
                            Some(secondary.into()),
                        ),
                    )),
                }),
            )),
            (Some(Err(source)), Ok(_)) => Err(TimeoutRunError::Operation(source)),
            (_, Err(source)) => Err(TimeoutRunError::Timeout(source)),
            (Some(Ok(value)), Ok(_)) => Ok(value),
            (None, Ok(_)) => Err(TimeoutRunError::Operation(
                crate::core::AgentError::invalid(
                    "original negative notice operation did not execute",
                ),
            )),
        }
    }
    pub(crate) async fn remaining_ms(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<u64, TimeoutBudgetError> {
        self.require_owner(self.terminal.issued(), effects)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let actual = aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
            effects,
            &self.cleanup,
            self.terminal,
            || async { Ok::<(), AuraError>(()) },
        )
        .await;
        if let Some(Err(source)) = actual.operation {
            return Err(TimeoutBudgetError::checkpoint_failure(source));
        }
        let now = actual.validation?;
        Ok(self.cleanup.deadline_at_ms().saturating_sub(now.ts_ms))
    }
    pub(crate) fn map_run_error(
        &self,
        stage: &'static str,
        source: TimeoutRunError<crate::core::AgentError>,
    ) -> crate::core::AgentError {
        map_enrollment_run_error(stage, &self.cleanup, source)
    }
    pub(crate) async fn retry_delay(
        &self,
        effects: &AuraEffectSystem,
        delay_ms: u64,
    ) -> crate::core::AgentResult<()> {
        let remaining = self.remaining_ms(effects).await.map_err(AuraError::from)?;
        self.execute(effects, || async {
            effects
                .sleep_ms(delay_ms.min(remaining))
                .await
                .map_err(AuraError::from)
                .map_err(crate::core::AgentError::from)
        })
        .await
        .map_err(|source| self.map_run_error("original negative notice retry", source))
    }
}
impl EnrollmentExecutionChild {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuerCancellationTerminal", receiver_type = EnrollmentExecutionChild, family = "runtime_helper")]
    pub(crate) fn original_cancelled_notice_window<'a>(
        &'a self,
        effects: &AuraEffectSystem,
        terminal: &'a Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
        notice: &Arc<crate::handlers::invitation::SignedCancelledNotice>,
        destination: &'a crate::handlers::invitation_service::OriginalIssuerTerminalDestination,
    ) -> Result<OriginalCancelledNoticeWindowCapability<'a>, AuraError> {
        let WindowCheckpoint::Registered { capability } = self.checkpoint.as_ref() else {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        };
        let actual = destination
            .original_cancelled_notice(terminal)?
            .ok_or_else(|| {
                AuraError::permission_denied("original signed cancellation notice missing")
            })?;
        if !Arc::ptr_eq(&actual, notice) {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        let window = OriginalCancelledNoticeWindowCapability {
            child: self,
            terminal,
            destination,
            cleanup: capability
                .negative_cleanup_budget()
                .attenuate_deadline_at(aura_core::types::window::WindowPosition::new(
                    terminal.issued().manifest().expires_at_ms,
                ))
                .map_err(AuraError::from)?,
        };
        window.require_owner(terminal.issued(), effects)?;
        Ok(window)
    }
}
