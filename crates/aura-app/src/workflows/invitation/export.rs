use super::*;

pub async fn export_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &InvitationId,
) -> Result<String, AuraError> {
    let code = export_invitation_runtime(app_core, invitation_id).await?;
    SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        None,
        SemanticOperationKind::CreateContactInvitation,
    )
    .publish_success_with(issue_invitation_created_proof(invitation_id.clone()))
    .await?;
    Ok(code)
}

pub async fn export_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<String, AuraError> {
    export_invitation(app_core, &InvitationId::new(invitation_id)).await
}

pub async fn export_invitation_by_str_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_export(),
        instance_id,
        SemanticOperationKind::ExportInvitation,
    );
    let result: Result<String, AuraError> = async {
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        let invitation_id = InvitationId::new(invitation_id);
        let code = export_invitation_runtime(app_core, &invitation_id).await?;
        owner
            .publish_success_with(issue_invitation_exported_proof(invitation_id))
            .await?;
        Ok(code)
    }
    .await;

    if let Err(error) = &result {
        if owner.terminal_status().await.is_none() {
            let _ = owner
                .publish_failure(super::command_terminal_error(error.to_string()))
                .await;
        }
    }

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

pub(in crate::workflows) async fn export_invitation_runtime(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &InvitationId,
) -> Result<String, AuraError> {
    let runtime = require_runtime(app_core).await?;
    timeout_runtime_call(
        &runtime,
        "export_invitation",
        "export_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || runtime.export_invitation(invitation_id.as_str()),
    )
    .await
    .map_err(|e| AuraError::from(super::super::error::runtime_call("export invitation", e)))?
    .map_err(|e| AuraError::from(super::super::error::runtime_call("export invitation", e)))
}
