use super::*;
use crate::runtime::transport_boundary::send_guarded_transport_envelope;
#[cfg(target_arch = "wasm32")]
use web_sys::js_sys;

fn invitation_stage_runtime_error(
    scope: &'static str,
    stage: &'static str,
    action: &'static str,
    error: impl std::error::Error + Send + Sync + 'static,
) -> AgentError {
    let mut detail = String::from(scope);
    detail.push_str(" `");
    detail.push_str(stage);
    detail.push_str("` ");
    detail.push_str(action);
    detail.push_str(": ");
    detail.push_str(&error.to_string());
    AgentError::Aura(aura_core::AuraError::Internal {
        message: detail,
        source: Some(Arc::new(error)),
    })
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
    TimeoutBudget::from_start_and_timeout(&started_at, scaled_timeout).map_err(|error| {
        invitation_stage_runtime_error(
            "invitation stage",
            stage,
            "could not construct timeout budget",
            error,
        )
    })
}

/// Boxed like [`timeout_prepare_invitation_stage`]: the accept chain awaits
/// several budgeted stages whose inlined state machines overflowed the
/// debug stack budget.
pub(super) fn timeout_invitation_stage_with_budget<'a, T: 'a>(
    effects: &'a AuraEffectSystem,
    budget: &'a TimeoutBudget,
    stage: &'static str,
    timeout_ms: u64,
    future: impl Future<Output = AgentResult<T>> + 'a,
) -> std::pin::Pin<Box<impl Future<Output = AgentResult<T>> + 'a>> {
    Box::pin(async move {
        // One shared observation owner orders the required physical read and child
        // allocation; no competing branch can capture a stale time then publish it.
        let child_budget = {
            let _observation = budget.acquire_observation().await;
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
            budget
                .child_budget(&now, scaled_timeout)
                .map_err(|source| {
                    super::vm_loop::map_invitation_vm_timeout(
                        stage,
                        budget,
                        TimeoutRunError::Timeout(source),
                    )
                })?
        };
        execute_with_timeout_budget(effects, &child_budget, || future)
            .await
            .map_err(|error| super::vm_loop::map_invitation_vm_timeout(stage, &child_budget, error))
    })
}

/// Boxed so each stage's state machine lives on the heap: the reserved
/// preparation path awaits many stages and inlining them overflowed
/// the debug stack budget (`just ci-accept-chain-stack`).
pub(super) fn timeout_prepare_invitation_stage<'a, T: 'a>(
    effects: &'a AuraEffectSystem,
    stage: &'static str,
    future: impl Future<Output = AgentResult<T>> + 'a,
) -> std::pin::Pin<Box<impl Future<Output = AgentResult<T>> + 'a>> {
    Box::pin(async move {
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
        .map_err(|error| {
            invitation_stage_runtime_error(
                "invitation stage",
                stage,
                "could not construct timeout budget",
                error,
            )
        })?;
        execute_with_timeout_budget(effects, &budget, || future)
            .await
            .map_err(|error| super::vm_loop::map_invitation_vm_timeout(stage, &budget, error))
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
    .map_err(|error| {
        invitation_stage_runtime_error(
            "invitation stage",
            stage,
            "could not construct timeout budget",
            error,
        )
    })?;
    execute_with_timeout_budget(effects, &budget, || future)
        .await
        .map_err(|error| super::vm_loop::map_invitation_vm_timeout(stage, &budget, error))
}

pub(super) async fn attempt_network_send_envelope(
    effects: &AuraEffectSystem,
    stage: &'static str,
    envelope: TransportEnvelope,
) -> AgentResult<()> {
    timeout_deferred_network_stage(effects, stage, async {
        let mut last_error = None;
        for attempt in 0..INVITATION_BEST_EFFORT_NETWORK_SEND_ATTEMPTS {
            match send_guarded_transport_envelope(effects, envelope.clone()).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    last_error = Some(error);
                    let retryable = matches!(last_error.as_ref(),
                        Some(TransportError::DestinationUnreachable { destination })
                            if *destination == envelope.destination);
                    if !retryable {
                        break;
                    }
                    if attempt + 1 < INVITATION_BEST_EFFORT_NETWORK_SEND_ATTEMPTS {
                        effects
                            .sleep_ms(INVITATION_BEST_EFFORT_NETWORK_SEND_BACKOFF_MS)
                            .await
                            .map_err(|source| {
                                invitation_stage_runtime_error(
                                    "invitation network stage",
                                    stage,
                                    "required retry timer failed",
                                    source,
                                )
                            })?;
                    }
                }
            }
        }

        match last_error {
            Some(source) => Err(AgentError::Aura(aura_core::AuraError::Network {
                message: format!("{stage}: {source}"),
                source: Some(Arc::new(source)),
            })),
            None => Err(AgentError::invalid(
                "invitation send requires at least one attempt",
            )),
        }
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

#[cfg(test)]
mod required_stage_source_tests {
    use super::*;

    #[test]
    fn required_stage_budget_failure_preserves_generated_cause_and_timeout_kind() {
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 100,
                uncertainty: None,
            },
            Duration::from_millis(20),
        )
        .expect("actual original budget");
        budget
            .remaining_at(&PhysicalTime {
                ts_ms: 110,
                uncertainty: None,
            })
            .expect("actual progress observation");
        let rollback = budget
            .child_budget(
                &PhysicalTime {
                    ts_ms: 109,
                    uncertainty: None,
                },
                Duration::from_millis(5),
            )
            .expect_err("actual child allocation must reject rollback above original start");
        let error = super::super::vm_loop::map_invitation_vm_timeout(
            "required-stage",
            &budget,
            TimeoutRunError::Timeout(rollback),
        );
        assert!(!error.is_timeout(), "rollback is not a deadline");
        let cause = std::error::Error::source(&error)
            .and_then(|source| source.source())
            .and_then(|source| source.downcast_ref::<aura_core::TimeoutBudgetError>())
            .expect("actual generated rollback survives standard source chain");
        assert!(matches!(
            cause,
            aura_core::TimeoutBudgetError::ClockRollback { .. }
        ));
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 100,
                uncertainty: None,
            },
            Duration::from_millis(20),
        )
        .expect("distinct actual deadline owner");
        let deadline = budget
            .child_budget(
                &PhysicalTime {
                    ts_ms: 120,
                    uncertainty: None,
                },
                Duration::from_millis(5),
            )
            .expect_err("actual original deadline is exhausted");
        let error = super::super::vm_loop::map_invitation_vm_timeout(
            "required-stage",
            &budget,
            TimeoutRunError::Timeout(deadline),
        );
        assert!(error.is_timeout());
        let invalid = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 100,
                uncertainty: None,
            },
            Duration::ZERO,
        )
        .expect_err("actual physical timeout policy rejects zero");
        let error = invitation_stage_runtime_error(
            "invitation stage",
            "required-stage",
            "construct budget",
            invalid,
        );
        assert!(!error.is_timeout());
        assert!(std::error::Error::source(&error)
            .and_then(|source| source.source())
            .is_some_and(|source| matches!(
                source.downcast_ref::<aura_core::TimeoutBudgetError>(),
                Some(aura_core::TimeoutBudgetError::InvalidPolicy { .. })
            )));
    }
}
