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
        AuraVmHostWaitStatus::TimedOut => Err(AgentError::internal(timeout_message.to_string())),
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
        TimeoutRunError::Timeout(_) => AgentError::timeout(format!(
            "{label} exceeded {}ms overall timeout",
            budget.timeout_ms()
        )),
        TimeoutRunError::Operation(error) => error,
    }
}
