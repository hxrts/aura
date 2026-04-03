use super::*;
#[cfg(target_arch = "wasm32")]
use web_sys::js_sys;

fn invitation_stage_runtime_error(
    scope: &'static str,
    stage: &'static str,
    action: &'static str,
    error: impl std::fmt::Display,
) -> AgentError {
    let mut detail = String::from(scope);
    detail.push_str(" `");
    detail.push_str(stage);
    detail.push_str("` ");
    detail.push_str(action);
    detail.push_str(": ");
    detail.push_str(&error.to_string());
    AgentError::runtime(detail)
}

fn invitation_stage_timeout_error(
    scope: &'static str,
    stage: &'static str,
    timeout_ms: u64,
) -> AgentError {
    let mut detail = String::from(scope);
    detail.push_str(" `");
    detail.push_str(stage);
    detail.push_str("` timed out after ");
    detail.push_str(&timeout_ms.to_string());
    detail.push_str("ms");
    AgentError::runtime(detail)
}

fn invitation_stage_effects_error(stage: &'static str, detail: &str) -> AgentError {
    let mut message = String::from(stage);
    message.push_str(": ");
    message.push_str(detail);
    AgentError::effects(message)
}

pub(super) fn invitation_timeout_profile(effects: &AuraEffectSystem) -> TimeoutExecutionProfile {
    if effects.is_testing() {
        TimeoutExecutionProfile::simulation_test()
    } else if effects.harness_mode_enabled() {
        TimeoutExecutionProfile::harness()
    } else {
        TimeoutExecutionProfile::production()
    }
}

pub(super) async fn invitation_timeout_budget(
    effects: &AuraEffectSystem,
    stage: &'static str,
    timeout_ms: u64,
) -> AgentResult<TimeoutBudget> {
    let started_at = effects.physical_time().await.map_err(|error| {
        invitation_stage_runtime_error(
            "invitation stage",
            stage,
            "could not read physical time",
            error,
        )
    })?;
    let scaled_timeout = invitation_timeout_profile(effects)
        .scale_duration(Duration::from_millis(timeout_ms))
        .map_err(|error| {
            invitation_stage_runtime_error(
                "invitation stage",
                stage,
                "could not scale timeout budget",
                error,
            )
        })?;
    TimeoutBudget::from_start_and_timeout(&started_at, scaled_timeout)
        .map_err(|error| AgentError::runtime(error.to_string()))
}

pub(super) async fn timeout_invitation_stage_with_budget<T>(
    effects: &AuraEffectSystem,
    budget: &TimeoutBudget,
    stage: &'static str,
    timeout_ms: u64,
    future: impl Future<Output = AgentResult<T>>,
) -> AgentResult<T> {
    let now = effects.physical_time().await.map_err(|error| {
        invitation_stage_runtime_error(
            "invitation stage",
            stage,
            "could not read physical time",
            error,
        )
    })?;
    let scaled_timeout = invitation_timeout_profile(effects)
        .scale_duration(Duration::from_millis(timeout_ms))
        .map_err(|error| {
            invitation_stage_runtime_error(
                "invitation stage",
                stage,
                "could not scale timeout budget",
                error,
            )
        })?;
    let child_budget = budget.child_budget(&now, scaled_timeout).map_err(|error| {
        AgentError::timeout(format!(
            "invitation stage `{stage}` could not allocate remaining timeout budget: {error}"
        ))
    })?;
    execute_with_timeout_budget(effects, &child_budget, || future)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => AgentError::timeout({
                let mut detail = String::from("invitation stage `");
                detail.push_str(stage);
                detail.push_str("` timed out after ");
                detail.push_str(&child_budget.timeout_ms().to_string());
                detail.push_str("ms");
                detail
            }),
            TimeoutRunError::Operation(error) => error,
        })
}

pub(super) async fn timeout_prepare_invitation_stage<T>(
    effects: &AuraEffectSystem,
    stage: &'static str,
    future: impl Future<Output = AgentResult<T>>,
) -> AgentResult<T> {
    let started_at = effects.physical_time().await.map_err(|error| {
        invitation_stage_runtime_error(
            "invitation.prepare stage",
            stage,
            "could not read physical time",
            error,
        )
    })?;
    let budget = TimeoutBudget::from_start_and_timeout(
        &started_at,
        Duration::from_millis(INVITATION_PREPARE_STAGE_TIMEOUT_MS),
    )
    .map_err(|error| AgentError::runtime(error.to_string()))?;
    execute_with_timeout_budget(effects, &budget, || future)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => invitation_stage_timeout_error(
                "invitation.prepare stage",
                stage,
                INVITATION_PREPARE_STAGE_TIMEOUT_MS,
            ),
            TimeoutRunError::Operation(error) => error,
        })
}

pub(super) async fn timeout_deferred_network_stage<T>(
    effects: &AuraEffectSystem,
    stage: &'static str,
    future: impl Future<Output = AgentResult<T>>,
) -> AgentResult<T> {
    let started_at = effects.physical_time().await.map_err(|error| {
        invitation_stage_runtime_error(
            "invitation best-effort network stage",
            stage,
            "could not read physical time",
            error,
        )
    })?;
    let budget = TimeoutBudget::from_start_and_timeout(
        &started_at,
        Duration::from_millis(INVITATION_BEST_EFFORT_NETWORK_TIMEOUT_MS),
    )
    .map_err(|error| AgentError::runtime(error.to_string()))?;
    execute_with_timeout_budget(effects, &budget, || future)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => invitation_stage_timeout_error(
                "invitation best-effort network stage",
                stage,
                INVITATION_BEST_EFFORT_NETWORK_TIMEOUT_MS,
            ),
            TimeoutRunError::Operation(error) => error,
        })
}

pub(super) async fn attempt_network_send_envelope(
    effects: &AuraEffectSystem,
    stage: &'static str,
    envelope: TransportEnvelope,
) -> AgentResult<()> {
    timeout_deferred_network_stage(effects, stage, async {
        let mut last_error = None;
        for attempt in 0..INVITATION_BEST_EFFORT_NETWORK_SEND_ATTEMPTS {
            match effects.send_envelope(envelope.clone()).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    last_error = Some(error.to_string());
                    if attempt + 1 < INVITATION_BEST_EFFORT_NETWORK_SEND_ATTEMPTS {
                        let _ = effects
                            .sleep_ms(INVITATION_BEST_EFFORT_NETWORK_SEND_BACKOFF_MS)
                            .await;
                    }
                }
            }
        }

        Err(invitation_stage_effects_error(
            stage,
            last_error
                .as_deref()
                .unwrap_or("transport send failed without detail"),
        ))
    })
    .await
}

#[cfg(target_arch = "wasm32")]
pub(super) fn emit_browser_harness_debug_event(event: &str, detail: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(origin) = window.location().origin() else {
        return;
    };
    let event = js_sys::encode_uri_component(event)
        .as_string()
        .unwrap_or_else(|| event.to_string());
    let detail = js_sys::encode_uri_component(detail)
        .as_string()
        .unwrap_or_else(|| detail.to_string());
    let url = format!("{origin}/__aura_harness_debug__/event?event={event}&detail={detail}");
    let _ = window.fetch_with_str(&url);
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn emit_browser_harness_debug_event(_event: &str, _detail: &str) {}
