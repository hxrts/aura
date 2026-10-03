#![allow(missing_docs)]

use super::*;
enum DeviceEnrollmentAcceptConvergenceError {
    Terminal(AuraError),
    Workflow(AuraError),
}

#[cfg(all(test, feature = "signals"))]
mod failure_owner_tests {
    use super::*;
    use std::error::Error;

    #[tokio::test]
    async fn unpinned_import_fails_before_runtime_access_and_publishes_terminal_failure() {
        let core = AppCore::new(crate::AppConfig::default()).unwrap();
        crate::signal_defs::register_app_signals(&core)
            .await
            .unwrap();
        let app = Arc::new(RwLock::new(core));
        let outcome = import_device_enrollment_with_terminal_status(
            &app,
            "untrusted payload".to_owned(),
            None,
            Some(crate::ui_contract::OperationInstanceId(
                "missing-manifest-pin".to_owned(),
            )),
        )
        .await;
        let error = outcome.result.unwrap_err();
        assert!(matches!(
            error
                .source()
                .unwrap()
                .downcast_ref::<aura_invitation::enrollment_manifest::EnrollmentManifestError>(),
            Some(aura_invitation::enrollment_manifest::EnrollmentManifestError::MissingPin)
        ));
        assert_eq!(
            outcome.terminal.unwrap().status.phase,
            SemanticOperationPhase::Failed
        );
        assert!(app.read().await.runtime().is_none());
    }

    #[tokio::test]
    async fn device_accept_failure_settles_existing_owner_and_retains_cause() {
        let core = AppCore::new(crate::AppConfig::default()).unwrap();
        crate::signal_defs::register_app_signals(&core)
            .await
            .unwrap();
        let app_core = Arc::new(RwLock::new(core));
        let owner = SemanticWorkflowOwner::new(
            &app_core,
            OperationId::device_enrollment(),
            Some(crate::ui_contract::OperationInstanceId(
                "device-accept-failure-owner".into(),
            )),
            SemanticOperationKind::ImportDeviceEnrollmentCode,
        );
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await
            .unwrap();
        let source = AuraError::Storage {
            message: "enrolled runtime did not settle".into(),
            source: Some(Arc::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "journal denied",
            ))),
        };
        let returned = fail_device_enrollment_accept::<()>(&owner, source)
            .await
            .unwrap_err();
        assert_eq!(returned.category(), "storage");
        assert_eq!(
            returned
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
        let terminal = owner.terminal_status().await.unwrap();
        assert_eq!(terminal.status.phase, SemanticOperationPhase::Failed);
        assert_eq!(
            terminal.status.kind,
            SemanticOperationKind::ImportDeviceEnrollmentCode
        );
        let facts = crate::workflows::signals::read_signal_or_default(
            &app_core,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        crate::workflows::semantic_facts::assert_terminal_failure_or_cancelled(
            &facts,
            &OperationId::device_enrollment(),
            &crate::ui_contract::OperationInstanceId("device-accept-failure-owner".into()),
            SemanticOperationKind::ImportDeviceEnrollmentCode,
        );
    }
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
    owner: &SemanticWorkflowOwner,
    error: AuraError,
) -> Result<T, AuraError> {
    let failure = AcceptInvitationError::AcceptFailed {
        detail: error.to_string(),
        source: Some(error.clone()),
    };
    owner
        .publish_failure(failure.semantic_error(owner.kind()))
        .await?;
    Err(error)
}

async fn prime_device_enrollment_accept_connectivity(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
) {
    trigger_runtime_discovery_with_timeout(runtime).await;
    let _ = drive_invitation_accept_convergence(app_core, runtime, None).await;
}

/// Observed result minted after authenticated enrollment acceptance.
///
/// Frontends cannot construct a completion from received identity fields.
/// ```compile_fail
/// use aura_app::ui::workflows::invitation::DeviceEnrollmentImportCompleted;
/// let fabricated=DeviceEnrollmentImportCompleted {
///     subject_authority:aura_core::AuthorityId::new_from_entropy([1;32]),
///     device_id:aura_core::DeviceId::new_from_entropy([2;32]),
/// };
/// ```

#[derive(Debug, Clone)]
pub struct DeviceEnrollmentImportCompleted {
    subject_authority: AuthorityId,
    device_id: aura_core::DeviceId,
    original_provisional: AuthorityId,
    invitation_id: aura_core::InvitationId,
    ceremony_id: aura_core::CeremonyId,
    pending_epoch: u64,
    manifest_digest: [u8; 32],
    setup_digest: [u8; 32],
}
impl DeviceEnrollmentImportCompleted {
    pub fn subject_authority(&self) -> AuthorityId {
        self.subject_authority
    }
    pub fn device_id(&self) -> aura_core::DeviceId {
        self.device_id
    }
    /// Diagnostic/receipt locator fields identify durable evidence. They never
    /// authorize profile migration without the actual runtime receipt owner.
    pub fn original_provisional(&self) -> AuthorityId {
        self.original_provisional
    }
    pub fn invitation_id(&self) -> &aura_core::InvitationId {
        &self.invitation_id
    }
    pub fn ceremony_id(&self) -> &aura_core::CeremonyId {
        &self.ceremony_id
    }
    pub fn pending_epoch(&self) -> u64 {
        self.pending_epoch
    }
    pub fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }
    pub fn setup_digest(&self) -> [u8; 32] {
        self.setup_digest
    }
}

pub async fn import_device_enrollment_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    code: String,
    transfer: Option<crate::ui_contract::EnrollmentManifestTransferInput>,
    instance_id: Option<crate::ui_contract::OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<DeviceEnrollmentImportCompleted> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        instance_id,
        SemanticOperationKind::ImportDeviceEnrollmentCode,
    );
    let result = import_device_enrollment_owned(app_core, code, transfer, &owner).await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner="import_device_enrollment_owned",
    wrapper="import_device_enrollment_with_terminal_status",
    terminal="accept_device_enrollment_invitation_owned",postcondition="device_enrollment_imported",
    proof=crate::workflows::semantic_facts::DeviceEnrollmentImportedProof,
    authoritative_inputs="selected_manifest,canonical_invitation",depends_on="runtime_verified_enrollment_admission",
    child_ops="",category="move_owned"
)]
async fn import_device_enrollment_owned(
    app_core: &Arc<RwLock<AppCore>>,
    code: String,
    transfer: Option<crate::ui_contract::EnrollmentManifestTransferInput>,
    owner: &SemanticWorkflowOwner<OperationContext<OperationId, OperationInstanceId, TraceContext>>,
) -> Result<DeviceEnrollmentImportCompleted, AuraError> {
    use aura_invitation::enrollment_manifest::EnrollmentManifestError;
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let prepare = async {
        let transfer = transfer.ok_or(EnrollmentManifestError::MissingPin)?;
        let pin = crate::workflows::ceremonies::pin_user_transferred_enrollment_manifest(
            app_core,
            transfer.manifest_code,
            transfer.initiator_verifier_code,
        )
        .await?;
        let subject_authority = pin.manifest().subject;
        let device_id = pin.manifest().invitee_device;
        let original_provisional = pin.manifest().invitee_authority;
        let invitation_id = pin.manifest().invitation.clone();
        let ceremony_id = pin.manifest().ceremony.clone();
        let pending_epoch = pin.manifest().pending_epoch;
        let manifest_digest = pin.digest();
        let setup_digest = pin.manifest().setup.digest;

        let runtime = app_core
            .read()
            .await
            .runtime()
            .cloned()
            .ok_or(EnrollmentManifestError::Unavailable)?;
        let import_runtime = runtime.clone();
        let info = timeout_runtime_call(
            &runtime,
            "import_device_enrollment_owned",
            "import_enrollment_invitation",
            DEVICE_ENROLLMENT_ACCEPT_TIMEOUT,
            move || async move {
                import_runtime
                    .import_enrollment_invitation(&code, pin)
                    .await
            },
        )
        .await
        .map_err(EnrollmentManifestError::Boundary)??;
        Ok::<_, EnrollmentManifestError>((
            info,
            DeviceEnrollmentImportCompleted {
                subject_authority,
                device_id,
                original_provisional,
                invitation_id,
                ceremony_id,
                pending_epoch,
                manifest_digest,
                setup_digest,
            },
        ))
    }
    .await;
    let (invitation, completion) = match prepare {
        Ok(result) => result,
        Err(error) => {
            return fail_device_enrollment_accept(
                owner,
                AuraError::PermissionDenied {
                    message: error.to_string(),
                    source: Some(Arc::new(error)),
                },
            )
            .await
        }
    };
    accept_device_enrollment_invitation_owned(app_core, &invitation, owner).await?;
    Ok(completion)
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
    accept_device_enrollment_invitation_owned(app_core, invitation, &owner).await?;
    Ok(())
}

#[aura_macros::semantic_owner(
    owner = "accept_device_enrollment_invitation_owned",
    wrapper = "accept_device_enrollment_invitation",
    terminal = "publish_success_with",
    postcondition = "device_enrollment_imported",
    proof = crate::workflows::semantic_facts::DeviceEnrollmentImportedProof,
    authoritative_inputs = "canonical_invitation,runtime",
    depends_on = "runtime_verified_enrollment_confirmation",
    child_ops = "",
    category = "move_owned"
)]
async fn accept_device_enrollment_invitation_owned(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &InvitationInfo,
    owner: &SemanticWorkflowOwner<OperationContext<OperationId, OperationInstanceId, TraceContext>>,
) -> Result<(), AuraError> {
    let InvitationBridgeType::DeviceEnrollment { .. } = &invitation.invitation_type else {
        return fail_device_enrollment_accept(
            owner,
            crate::workflows::error::WorkflowError::Precondition(
                "accept_device_enrollment_invitation requires a device enrollment invitation",
            )
            .into(),
        )
        .await;
    };

    let runtime = match require_runtime(app_core).await {
        Ok(runtime) => runtime,
        Err(error) => return fail_device_enrollment_accept(owner, error).await,
    };
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
            owner,
            crate::workflows::error::runtime_call("accept invitation failed", error).into(),
        )
        .await;
    }
    if let Ok(Err(error)) = accept_result {
        return fail_device_enrollment_accept(
            owner,
            crate::workflows::error::runtime_call("accept invitation failed", error).into(),
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
        .map_err(DeviceEnrollmentAcceptConvergenceError::Workflow)?
        .map_err(|error| {
            DeviceEnrollmentAcceptConvergenceError::Terminal(
                crate::workflows::error::runtime_call(
                    "device enrollment ceremony processing failed",
                    error,
                )
                .into(),
            )
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
                .await
        }
        Err(DeviceEnrollmentAcceptConvergenceError::Terminal(error)) => {
            fail_device_enrollment_accept(owner, error).await
        }
        Err(DeviceEnrollmentAcceptConvergenceError::Workflow(error)) => {
            #[cfg(feature = "instrumented")]
            tracing::warn!(
                invitation_id = %invitation.invitation_id,
                error = %error,
                "device enrollment acceptance failed while settling the enrolled runtime"
            );
            fail_device_enrollment_accept(
                owner,
                crate::workflows::error::runtime_call(
                    "device enrollment acceptance did not settle",
                    error,
                )
                .into(),
            )
            .await
        }
    }
}
