#![allow(missing_docs)]

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

fn invitation_import_failure(error: &AuraError) -> crate::ui_contract::SemanticOperationError {
    use crate::ui_contract::{SemanticFailureCode, SemanticFailureDomain, SemanticOperationError};
    let code = match error {
        AuraError::Invalid { .. } | AuraError::Serialization { .. } => {
            SemanticFailureCode::InvalidArgument
        }
        AuraError::PermissionDenied { .. } | AuraError::Crypto { .. } => {
            SemanticFailureCode::PermissionDenied
        }
        AuraError::NotFound { .. } => SemanticFailureCode::NotFound,
        AuraError::Network { .. } => SemanticFailureCode::Unavailable,
        _ => SemanticFailureCode::CommandFailed,
    };
    SemanticOperationError::new(SemanticFailureDomain::Invitation, code)
        .with_detail(error.to_string())
}

/// Import and verify a code under the app-owned import lifecycle. The caller
/// passes its already allocated exact instance at the frontend handoff.
pub async fn import_invitation_details_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    code: &str,
    instance_id: OperationInstanceId,
) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationHandle> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_import(),
        Some(instance_id),
        SemanticOperationKind::ImportInvitation,
    );
    let result = import_invitation_details_owned(app_core, code, &owner, None).await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner = "import_invitation_details_owned",
    wrapper = "import_invitation_details_with_terminal_status",
    terminal = "publish_success_with",
    postcondition = "invitation_imported",
    proof = crate::workflows::semantic_facts::InvitationImportedProof,
    authoritative_inputs = "runtime,verified_invitation",
    depends_on = "runtime_import_verified",
    child_ops = "",
    category = "move_owned"
)]
async fn import_invitation_details_owned(
    app_core: &Arc<RwLock<AppCore>>,
    code: &str,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<InvitationHandle, AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    match import_invitation_details(app_core, code).await {
        Ok(invitation) => {
            owner
                .publish_success_with(issue_invitation_imported_proof(
                    invitation.invitation_id().clone(),
                ))
                .await?;
            Ok(invitation)
        }
        Err(error) => {
            owner
                .publish_failure(invitation_import_failure(&error))
                .await?;
            Err(error)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_contract::{SemanticFailureCode, SemanticFailureDomain};

    #[test]
    fn import_errors_keep_stable_failure_classes() {
        let cases = [
            (
                AuraError::invalid("bad code"),
                SemanticFailureCode::InvalidArgument,
            ),
            (
                AuraError::permission_denied("untrusted proof"),
                SemanticFailureCode::PermissionDenied,
            ),
            (
                AuraError::not_found("invitation"),
                SemanticFailureCode::NotFound,
            ),
        ];
        for (error, code) in cases {
            let failure = invitation_import_failure(&error);
            assert_eq!(failure.domain, SemanticFailureDomain::Invitation);
            assert_eq!(failure.code, code);
        }
    }
}
