#![allow(missing_docs)]

use super::*;
enum DeviceEnrollmentAcceptConvergenceError {
    Terminal(String),
    Workflow(AuraError),
}

fn log_device_enrollment_accept_progress(message: impl Into<String>) {
    let message = message.into();
    #[cfg(feature = "wasm")]
    crate::platform::wasm::console_log(&format!("[device-enrollment-accept] {message}"));
    #[cfg(all(not(feature = "wasm"), feature = "instrumented"))]
    tracing::info!(target: "device-enrollment-accept", "{message}");
    #[cfg(all(not(feature = "wasm"), not(feature = "instrumented")))]
    let _ = message;
}

async fn fail_device_enrollment_accept<T>(
    app_core: &Arc<RwLock<AppCore>>,
    detail: impl Into<String>,
) -> Result<T, AuraError> {
    let error = crate::ui_contract::SemanticOperationError::new(
        crate::ui_contract::SemanticFailureDomain::Invitation,
        crate::ui_contract::SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into());
    super::publish_invitation_operation_failure(
        app_core,
        OperationId::device_enrollment(),
        None,
        None,
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        error.clone(),
    )
    .await?;
    Err(AuraError::agent(error.detail.unwrap_or_else(|| {
        "device enrollment acceptance failed".to_string()
    })))
}

async fn prime_device_enrollment_accept_connectivity(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
) {
    trigger_runtime_discovery_with_timeout(runtime).await;
    let _ = drive_invitation_accept_convergence(app_core, runtime, None).await;
}

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
    prime_device_enrollment_accept_connectivity(app_core, &runtime).await;
    log_device_enrollment_accept_progress(format!(
        "connectivity preflight complete invitation_id={}",
        invitation.invitation_id
    ));
    let accept_result = timeout_runtime_call(
        &runtime,
        "accept_device_enrollment_invitation",
        "accept_invitation",
        DEVICE_ENROLLMENT_ACCEPT_TIMEOUT,
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

    // The accept above is authoritative: the runtime returns success only after
    // the signed enrollment choreography completed and this device adopted the
    // enrolled epoch. What remains is settling local state; device counts are
    // not evidence of success (the device list includes this device even before
    // the tree does), so they no longer decide the outcome.
    let invitation_id = invitation.invitation_id.clone();
    let enrollment_result: Result<(), DeviceEnrollmentAcceptConvergenceError> = async {
        timeout_runtime_call(
            &runtime,
            "accept_device_enrollment_invitation",
            "process_ceremony_messages",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.process_ceremony_messages(),
        )
        .await
        .unwrap_or_else(|error| Err(crate::core::IntentError::internal_error(error.to_string())))
        .map_err(|error| {
            DeviceEnrollmentAcceptConvergenceError::Terminal(format!(
                "device enrollment ceremony processing failed: {error}"
            ))
        })?;
        converge_runtime(&runtime).await;
        settings::refresh_settings_from_runtime(app_core)
            .await
            .map_err(DeviceEnrollmentAcceptConvergenceError::Workflow)?;
        log_device_enrollment_accept_progress(format!("settled invitation_id={invitation_id}"));
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
        Ok(())
    }
    .await;
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
                error = %error,
                "device enrollment acceptance failed while settling the enrolled runtime"
            );
            fail_device_enrollment_accept(
                app_core,
                format!("device enrollment acceptance did not settle: {error}"),
            )
            .await
        }
    }
}
