#![allow(missing_docs)]

use super::*;

pub async fn accept_device_enrollment_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &InvitationInfo,
) -> Result<(), AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        None,
        SemanticOperationKind::ImportDeviceEnrollmentCode,
    );
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let InvitationBridgeType::DeviceEnrollment { .. } = &invitation.invitation_type else {
        return fail_device_enrollment_accept(
            app_core,
            "accept_device_enrollment_invitation requires a device enrollment invitation",
        )
        .await;
    };

    let runtime = require_runtime(app_core).await?;
    log_device_enrollment_accept_progress(format!(
        "start invitation_id={};authority={}",
        invitation.invitation_id,
        runtime.authority_id()
    ));
    super::prime_device_enrollment_accept_connectivity(app_core, &runtime).await;
    log_device_enrollment_accept_progress(format!(
        "connectivity preflight complete invitation_id={}",
        invitation.invitation_id
    ));
    let accept_result = timeout_runtime_call(
        &runtime,
        "accept_device_enrollment_invitation",
        "accept_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || runtime.accept_invitation(invitation.invitation_id.as_str()),
    )
    .await;
    if let Err(error) = accept_result {
        return fail_device_enrollment_accept(
            app_core,
            format!("accept invitation failed: {error}"),
        )
        .await;
    }
    if let Ok(Err(error)) = accept_result {
        return fail_device_enrollment_accept(
            app_core,
            format!("accept invitation failed: {error}"),
        )
        .await;
    }
    log_device_enrollment_accept_progress(format!(
        "accept_invitation returned invitation_id={}",
        invitation.invitation_id
    ));
    converge_runtime(&runtime).await;
    log_device_enrollment_accept_progress(format!(
        "initial converge_runtime complete invitation_id={}",
        invitation.invitation_id
    ));

    let expected_min_devices = 2_usize;
    let policy = device_enrollment_accept_retry_policy()?;
    let invitation_id = invitation.invitation_id.clone();
    let enrollment_result: Result<(), DeviceEnrollmentAcceptConvergenceError> = {
        let mut attempts = AttemptBudget::new(policy.max_attempts());
        loop {
            let attempt = match attempts.record_attempt() {
                Ok(attempt) => attempt,
                Err(error) => {
                    break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                        AuraError::from(error),
                    ));
                }
            };
            #[cfg(not(feature = "instrumented"))]
            let _ = &invitation_id;
            log_device_enrollment_accept_progress(format!(
                "convergence attempt={attempt} invitation_id={invitation_id}"
            ));

            if let Err(error) = timeout_runtime_call(
                &runtime,
                "accept_device_enrollment_invitation",
                "process_ceremony_messages",
                INVITATION_RUNTIME_OPERATION_TIMEOUT,
                || runtime.process_ceremony_messages(),
            )
            .await
            .unwrap_or_else(|error| {
                Err(crate::core::IntentError::internal_error(error.to_string()))
            }) {
                break Err(DeviceEnrollmentAcceptConvergenceError::Terminal(format!(
                    "device enrollment ceremony processing failed during convergence: {error}"
                )));
            }
            log_device_enrollment_accept_progress(format!(
                "process_ceremony_messages ok attempt={attempt} invitation_id={invitation_id}"
            ));

            converge_runtime(&runtime).await;
            if let Err(error) = settings::refresh_settings_from_runtime(app_core).await {
                break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(error));
            }

            let runtime_device_count = match timeout_runtime_call(
                &runtime,
                "accept_device_enrollment_invitation",
                "try_list_devices",
                INVITATION_RUNTIME_QUERY_TIMEOUT,
                || runtime.try_list_devices(),
            )
            .await
            {
                Ok(Ok(devices)) => devices.len(),
                Ok(Err(error)) => {
                    break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                        AuraError::from(super::super::error::runtime_call("list devices", error)),
                    ));
                }
                Err(error) => {
                    break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                        AuraError::from(super::super::error::runtime_call("list devices", error)),
                    ));
                }
            };
            let settings_device_count = match settings::get_settings(app_core).await {
                Ok(settings) => settings.devices.len(),
                Err(error) => break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(error)),
            };
            log_device_enrollment_accept_progress(format!(
                "counts attempt={attempt} invitation_id={invitation_id} runtime_devices={runtime_device_count} settings_devices={settings_device_count} expected_min_devices={expected_min_devices}"
            ));
            #[cfg(feature = "instrumented")]
            tracing::info!(
                invitation_id = %invitation_id,
                attempt,
                runtime_device_count,
                settings_device_count,
                expected_min_devices,
                "device enrollment convergence poll"
            );
            if runtime_device_count >= expected_min_devices
                || settings_device_count >= expected_min_devices
            {
                settings::refresh_settings_from_runtime(app_core).await?;
                log_device_enrollment_accept_progress(format!(
                    "converged attempt={attempt} invitation_id={invitation_id}"
                ));
                if let Err(_error) =
                    ensure_runtime_peer_connectivity(&runtime, "device_enrollment_accept").await
                {
                    #[cfg(feature = "instrumented")]
                    tracing::warn!(
                        error = %_error,
                        invitation_id = %invitation_id,
                        "device enrollment acceptance completed without reachable peers"
                    );
                }
                break Ok(());
            }

            if !attempts.can_attempt() {
                break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                    AuraError::from(super::super::error::WorkflowError::Precondition(
                        "device enrollment acceptance not yet converged",
                    )),
                ));
            }

            let delay_ms = match u64::try_from(policy.delay_for_attempt(attempt).as_millis()) {
                Ok(delay_ms) => delay_ms,
                Err(_) => {
                    break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                        AuraError::agent("device enrollment retry delay overflow"),
                    ));
                }
            };
            runtime.sleep_ms(delay_ms).await;
        }
    };
    match enrollment_result {
        Ok(()) => {
            log_device_enrollment_accept_progress(format!("success invitation_id={invitation_id}"));
            owner
                .publish_success_with(issue_device_enrollment_imported_proof(invitation_id))
                .await?;
            Ok(())
        }
        Err(DeviceEnrollmentAcceptConvergenceError::Terminal(detail)) => {
            fail_device_enrollment_accept(app_core, detail).await
        }
        Err(DeviceEnrollmentAcceptConvergenceError::Workflow(error)) => {
            #[cfg(feature = "instrumented")]
            tracing::warn!(
                invitation_id = %invitation.invitation_id,
                expected_min_devices,
                error = %error,
                "device enrollment acceptance failed before local device list convergence"
            );
            fail_device_enrollment_accept(
                app_core,
                format!(
                    "device enrollment acceptance did not converge to {expected_min_devices} local devices: {error}"
                ),
            )
            .await
        }
    }
}
