use super::*;

pub async fn list_pending_invitations(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<Vec<InvitationInfo>, AuraError> {
    let runtime = require_runtime(app_core).await?;

    timeout_runtime_call(
        &runtime,
        "list_pending_invitations",
        "try_list_pending_invitations",
        INVITATION_RUNTIME_QUERY_TIMEOUT,
        || runtime.try_list_pending_invitations(),
    )
    .await
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "list pending invitations",
            e,
        ))
    })?
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "list pending invitations",
            e,
        ))
    })
}

pub async fn import_invitation_details(
    app_core: &Arc<RwLock<AppCore>>,
    code: &str,
) -> Result<InvitationHandle, AuraError> {
    let runtime = require_runtime(app_core).await?;

    timeout_runtime_call(
        &runtime,
        "import_invitation_details",
        "import_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || runtime.import_invitation(code),
    )
    .await
    .map_err(|e| AuraError::from(super::super::error::runtime_call("import invitation", e)))?
    .map(InvitationHandle::new)
    .map_err(|e| AuraError::from(super::super::error::runtime_call("import invitation", e)))
}

pub(in crate::workflows) async fn pending_invitation_info_by_id(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<InvitationInfo, AuraError> {
    let invitation_id = InvitationId::new(invitation_id);
    let runtime = require_runtime(app_core).await?;
    let invitations = timeout_runtime_call(
        &runtime,
        "pending_invitation_info_by_id",
        "try_list_pending_invitations",
        INVITATION_RUNTIME_QUERY_TIMEOUT,
        || runtime.try_list_pending_invitations(),
    )
    .await
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "list pending invitations",
            e,
        ))
    })?
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "list pending invitations",
            e,
        ))
    })?;
    invitations
        .into_iter()
        .find(|invitation| invitation.invitation_id == invitation_id)
        .ok_or_else(|| AuraError::not_found(invitation_id.to_string()))
}

pub async fn list_invitations(app_core: &Arc<RwLock<AppCore>>) -> InvitationsState {
    read_signal_or_default(app_core, &*INVITATIONS_SIGNAL).await
}

pub async fn import_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    code: &str,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    timeout_runtime_call(
        &runtime,
        "import_invitation",
        "import_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || runtime.import_invitation(code),
    )
    .await
    .map_err(|e| AuraError::from(super::super::error::runtime_call("import invitation", e)))?
    .map_err(|e| AuraError::from(super::super::error::runtime_call("import invitation", e)))?;

    if let Err(_error) = crate::workflows::system::refresh_account(app_core).await {
        #[cfg(feature = "instrumented")]
        tracing::debug!(error = %_error, "refresh_account after invitation import failed");
    }

    refresh_authoritative_invitation_readiness(app_core).await
}
