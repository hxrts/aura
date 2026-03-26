use std::future::Future;
use std::time::Duration;

use aura_app::harness_mode_enabled;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::{
    execute_with_timeout_budget, TimeoutBudget, TimeoutExecutionProfile, TimeoutRunError,
};
use aura_effects::time::PhysicalTimeHandler;

pub(crate) enum TerminalTimeoutError<E> {
    Setup {
        context: &'static str,
        detail: String,
    },
    Timeout {
        context: &'static str,
        detail: String,
    },
    Operation(E),
}

fn terminal_timeout_profile() -> TimeoutExecutionProfile {
    if harness_mode_enabled() {
        TimeoutExecutionProfile::harness()
    } else {
        TimeoutExecutionProfile::production()
    }
}

pub(crate) async fn execute_with_terminal_timeout<T, E, F, Fut>(
    context: &'static str,
    duration: Duration,
    operation: F,
) -> Result<T, TerminalTimeoutError<E>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let time = PhysicalTimeHandler::new();
    let started_at = time
        .physical_time()
        .await
        .map_err(|error| TerminalTimeoutError::Setup {
            context,
            detail: format!("failed to read physical time: {error}"),
        })?;
    let scaled = terminal_timeout_profile()
        .scale_duration(duration)
        .map_err(|error| TerminalTimeoutError::Setup {
            context,
            detail: format!("failed to scale timeout: {error}"),
        })?;
    let budget = TimeoutBudget::from_start_and_timeout(&started_at, scaled).map_err(|error| {
        TerminalTimeoutError::Setup {
            context,
            detail: format!("failed to create timeout budget: {error}"),
        }
    })?;

    execute_with_timeout_budget(&time, &budget, operation)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(timeout_error) => TerminalTimeoutError::Timeout {
                context,
                detail: timeout_error.to_string(),
            },
            TimeoutRunError::Operation(error) => TerminalTimeoutError::Operation(error),
        })
}
