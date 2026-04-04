#![allow(missing_docs)]

use super::*;

pub async fn create_contact_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    nickname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationHandle, AuraError> {
    create_contact_invitation_with_instance(app_core, receiver, nickname, message, ttl_ms, None)
        .await
}

pub async fn create_contact_invitation_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    nickname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
    operation_instance_id: Option<OperationInstanceId>,
) -> Result<InvitationHandle, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        operation_instance_id,
        SemanticOperationKind::CreateContactInvitation,
    );
    publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let invitation =
        create_contact_invitation_runtime(app_core, receiver, nickname, message, ttl_ms).await?;
    owner
        .publish_success_with(issue_invitation_created_proof(
            invitation.invitation_id.clone(),
        ))
        .await?;
    Ok(InvitationHandle::new(invitation))
}

async fn create_contact_invitation_runtime(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    nickname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationInfo, AuraError> {
    let runtime = require_runtime(app_core).await?;
    timeout_runtime_call(
        &runtime,
        "create_contact_invitation",
        "create_contact_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || runtime.create_contact_invitation(receiver, nickname, message, ttl_ms),
    )
    .await
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "create contact invitation",
            e,
        ))
    })?
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "create contact invitation",
            e,
        ))
    })
}

pub async fn create_contact_invitation_code_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    nickname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        instance_id,
        SemanticOperationKind::CreateContactInvitation,
    );
    let result: Result<String, AuraError> = async {
        publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
            .await?;
        let invitation =
            create_contact_invitation_runtime(app_core, receiver, nickname, message, ttl_ms)
                .await?;
        let code = super::export_invitation_runtime(app_core, &invitation.invitation_id).await?;
        owner
            .publish_success_with(issue_invitation_created_proof(
                invitation.invitation_id.clone(),
            ))
            .await?;
        Ok(code)
    }
    .await;

    if let Err(error) = &result {
        let _ = owner
            .publish_failure(super::command_terminal_error(error.to_string()))
            .await;
    }

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

pub async fn create_generic_contact_invitation_code_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    nickname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        instance_id,
        SemanticOperationKind::CreateContactInvitation,
    );
    let result: Result<String, AuraError> = async {
        create_generic_contact_invitation_code_owned(
            app_core, nickname, message, ttl_ms, &owner, None,
        )
        .await
    }
    .await;

    if let Err(error) = &result {
        let _ = owner
            .publish_failure(super::command_terminal_error(error.to_string()))
            .await;
    }

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner = "create_generic_contact_invitation_code_owned",
    wrapper = "create_generic_contact_invitation_code_terminal_status",
    terminal = "publish_success_with",
    postcondition = "invitation_created",
    proof = crate::workflows::semantic_facts::InvitationCreatedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "",
    child_ops = "",
    category = "move_owned"
)]
async fn create_generic_contact_invitation_code_owned(
    app_core: &Arc<RwLock<AppCore>>,
    nickname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<String, AuraError> {
    publish_invitation_owner_status(owner, None, SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let receiver = require_runtime(app_core).await?.authority_id();
    let invitation =
        create_contact_invitation_runtime(app_core, receiver, nickname, message, ttl_ms).await?;
    let code = super::export_invitation_runtime(app_core, &invitation.invitation_id).await?;
    owner
        .publish_success_with(issue_invitation_created_proof(
            invitation.invitation_id.clone(),
        ))
        .await?;
    Ok(code)
}

pub async fn create_guardian_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    subject: AuthorityId,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationHandle, AuraError> {
    create_guardian_invitation_with_instance(app_core, receiver, subject, message, ttl_ms, None)
        .await
}

pub async fn create_guardian_invitation_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    subject: AuthorityId,
    message: Option<String>,
    ttl_ms: Option<u64>,
    operation_instance_id: Option<OperationInstanceId>,
) -> Result<InvitationHandle, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        operation_instance_id,
        SemanticOperationKind::CreateContactInvitation,
    );
    publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let runtime = require_runtime(app_core).await?;

    let invitation = timeout_runtime_call(
        &runtime,
        "create_guardian_invitation",
        "create_guardian_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || runtime.create_guardian_invitation(receiver, subject, message, ttl_ms),
    )
    .await
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "create guardian invitation",
            e,
        ))
    })?
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "create guardian invitation",
            e,
        ))
    })?;
    owner
        .publish_success_with(issue_invitation_created_proof(
            invitation.invitation_id.clone(),
        ))
        .await?;
    Ok(InvitationHandle::new(invitation))
}

pub async fn create_guardian_invitation_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    subject: AuthorityId,
    message: Option<String>,
    ttl_ms: Option<u64>,
    operation_instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationHandle> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        operation_instance_id.clone(),
        SemanticOperationKind::CreateGuardianInvitation,
    );
    let result: Result<InvitationHandle, AuraError> = async {
        publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
            .await?;
        let runtime = require_runtime(app_core).await?;
        let invitation = timeout_runtime_call(
            &runtime,
            "create_guardian_invitation",
            "create_guardian_invitation",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.create_guardian_invitation(receiver, subject, message, ttl_ms),
        )
        .await
        .map_err(|e| {
            AuraError::from(super::super::error::runtime_call(
                "create guardian invitation",
                e,
            ))
        })?
        .map_err(|e| {
            AuraError::from(super::super::error::runtime_call(
                "create guardian invitation",
                e,
            ))
        })?;
        owner
            .publish_success_with(issue_invitation_created_proof(
                invitation.invitation_id.clone(),
            ))
            .await?;
        Ok(InvitationHandle::new(invitation))
    }
    .await;

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

pub async fn create_channel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    home_id: String,
    context_id: Option<ContextId>,
    channel_name_hint: Option<String>,
    bootstrap: Option<ChannelBootstrapPackage>,
    operation_instance_id: Option<OperationInstanceId>,
    deadline: Option<TimeoutBudget>,
    external_stage_tracker: Option<WorkflowStageTracker>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationHandle, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        operation_instance_id.clone(),
        SemanticOperationKind::InviteActorToChannel,
    );
    create_channel_invitation_owned(
        app_core,
        receiver,
        home_id,
        context_id,
        channel_name_hint,
        bootstrap,
        &owner,
        deadline,
        external_stage_tracker,
        message,
        ttl_ms,
        true,
        None,
    )
    .await
    .map(InvitationHandle::new)
}

#[aura_macros::semantic_owner(
    owner = "create_channel_invitation",
    wrapper = "create_channel_invitation",
    terminal = "publish_success_with",
    postcondition = "invitation_created",
    proof = crate::workflows::semantic_facts::InvitationCreatedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "context_and_bootstrap_ready",
    child_ops = "",
    category = "move_owned"
)]
pub(in crate::workflows) async fn create_channel_invitation_owned(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    home_id: String,
    context_id: Option<ContextId>,
    channel_name_hint: Option<String>,
    bootstrap: Option<ChannelBootstrapPackage>,
    owner: &SemanticWorkflowOwner,
    deadline: Option<TimeoutBudget>,
    external_stage_tracker: Option<WorkflowStageTracker>,
    message: Option<String>,
    ttl_ms: Option<u64>,
    publish_terminal: bool,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<InvitationInfo, AuraError> {
    let stage_tracker =
        external_stage_tracker.or_else(|| Some(new_workflow_stage_tracker("require_runtime")));
    let channel_id = home_id.parse::<ChannelId>().map_err(|_| {
        AuraError::from(ChannelInvitationBootstrapError::InvalidCanonicalChannelId {
            raw: home_id.clone(),
        })
    })?;
    let runtime = require_runtime(app_core).await.map_err(|error| {
        ChannelInvitationBootstrapError::BootstrapTransport {
            channel_id,
            detail: error.to_string(),
        }
    })?;
    let operation_budget = workflow_timeout_budget(
        &runtime,
        Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS),
    )
    .await
    .map_err(
        |error| ChannelInvitationBootstrapError::BootstrapTransport {
            channel_id,
            detail: error.to_string(),
        },
    )?;
    let invitation_result =
        execute_with_runtime_timeout_budget(&runtime, &operation_budget, || async {
            update_channel_invitation_stage(&stage_tracker, "publish_workflow_dispatched");
            publish_invitation_owner_status(
                owner,
                deadline,
                SemanticOperationPhase::WorkflowDispatched,
            )
            .await
            .map_err(
                |error| ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: error.to_string(),
                },
            )?;
            update_channel_invitation_stage(&stage_tracker, "parse_channel_id");
            update_channel_invitation_stage(&stage_tracker, "ensure_context_and_bootstrap");
            let (context_id, bootstrap) = ensure_channel_invitation_context_and_bootstrap(
                app_core,
                &runtime,
                receiver,
                channel_id,
                context_id,
                bootstrap,
                &stage_tracker,
                deadline,
            )
            .await?;
            update_channel_invitation_stage(&stage_tracker, "publish_authoritative_context_ready");
            publish_invitation_owner_status(
                owner,
                deadline,
                SemanticOperationPhase::AuthoritativeContextReady,
            )
            .await
            .map_err(
                |error| ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: error.to_string(),
                },
            )?;

            update_channel_invitation_stage(&stage_tracker, "runtime.create_channel_invitation");
            let invitation_budget = workflow_timeout_budget(
                &runtime,
                channel_invitation_bootstrap_timeout(
                    deadline,
                    channel_id,
                    "runtime.create_channel_invitation",
                    Some(context_id),
                )?,
            )
            .await
            .map_err(
                |error| ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: error.to_string(),
                },
            )?;
            let invitation =
                match execute_with_runtime_timeout_budget(&runtime, &invitation_budget, || {
                    runtime.create_channel_invitation(
                        receiver,
                        home_id,
                        Some(context_id),
                        channel_name_hint.clone(),
                        Some(bootstrap),
                        message,
                        ttl_ms,
                    )
                })
                .await
                {
                    Ok(invitation) => invitation,
                    Err(TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded {
                        ..
                    })) => {
                        return Err(ChannelInvitationBootstrapError::CreateTimedOut {
                            channel_id,
                            receiver_id: receiver,
                            timeout_ms: invitation_budget.timeout_ms(),
                        });
                    }
                    Err(TimeoutRunError::Timeout(error)) => {
                        return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                            channel_id,
                            detail: error.to_string(),
                        });
                    }
                    Err(TimeoutRunError::Operation(error)) => {
                        return Err(ChannelInvitationBootstrapError::CreateFailed {
                            channel_id,
                            receiver_id: receiver,
                            detail: error.to_string(),
                        });
                    }
                };

            Ok((channel_id, context_id, invitation))
        })
        .await;

    let (_channel_id, _context_id, invitation) = match invitation_result {
        Ok(value) => value,
        Err(TimeoutRunError::Timeout(_)) => {
            let detail = stage_tracker
                .as_ref()
                .and_then(|tracker| tracker.try_lock().map(|guard| *guard))
                .unwrap_or("operation");
            let error = ChannelInvitationBootstrapError::BootstrapTransport {
                channel_id,
                detail: format!(
                    "create_channel_invitation timed out in stage {detail} after {}ms",
                    operation_budget.timeout_ms()
                ),
            };
            return if publish_terminal {
                fail_channel_invitation(owner, deadline, error).await
            } else {
                Err(error.into())
            };
        }
        Err(TimeoutRunError::Operation(error)) => {
            return if publish_terminal {
                fail_channel_invitation(owner, deadline, error).await
            } else {
                Err(error.into())
            };
        }
    };
    if publish_terminal {
        owner
            .publish_success_with(issue_invitation_created_proof(
                invitation.invitation_id.clone(),
            ))
            .await?;
    }
    Ok(invitation)
}
