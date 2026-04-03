use super::*;

/// Invite a user to join a channel
///
/// **What it does**: Creates a channel invitation for the target user
/// **Returns**: Invitation ID
/// **Signal pattern**: RuntimeBridge handles signal emission
///
/// This delegates to the invitation workflow to create a channel invitation.
/// The target user receives the invitation and can accept to join the channel.
///
/// # Arguments
/// * `app_core` - The application core
/// * `target_user_id` - Target user's authority ID
/// * `channel_id` - Channel to invite user to (required - UI manages selection)
/// * `message` - Optional invitation message
/// * `ttl_ms` - Optional time-to-live for the invitation
pub async fn invite_user_to_channel(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
    channel_id: &str,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationId, AuraError> {
    invite_user_to_channel_with_context(
        app_core,
        target_user_id,
        channel_id,
        None,
        None,
        message,
        ttl_ms,
    )
    .await
}

/// Invite a user to a channel while carrying an already-authoritative context,
/// when the caller has one.
pub async fn invite_user_to_channel_with_context(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
    channel_name_or_id: &str,
    context_id: Option<ContextId>,
    operation_instance_id: Option<OperationInstanceId>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationId, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        operation_instance_id.clone(),
        SemanticOperationKind::InviteActorToChannel,
    );
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    invite_user_to_channel_with_context_owned(
        app_core,
        target_user_id,
        channel_name_or_id,
        context_id,
        operation_instance_id,
        message,
        ttl_ms,
        &owner,
        None,
    )
    .await
}

/// Invite a user to a channel and return the directly-settled terminal status
/// for frontend handoff consumers.
pub async fn invite_user_to_channel_with_context_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
    channel_name_or_id: &str,
    context_id: Option<ContextId>,
    operation_instance_id: Option<OperationInstanceId>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationId> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        operation_instance_id,
        SemanticOperationKind::InviteActorToChannel,
    );
    let result = async {
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        invite_user_to_channel_with_context_owned(
            app_core,
            target_user_id,
            channel_name_or_id,
            context_id,
            None,
            message,
            ttl_ms,
            &owner,
            None,
        )
        .await
    }
    .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner = "invite_user_to_channel_with_context",
    wrapper = "invite_user_to_channel_with_context",
    terminal = "publish_success_with",
    postcondition = "channel_invitation_created",
    proof = crate::workflows::semantic_facts::ChannelInvitationCreatedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "target_authority_resolved,channel_target_resolved",
    child_ops = "",
    category = "move_owned"
)]
async fn invite_user_to_channel_with_context_owned(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
    channel_name_or_id: &str,
    context_id: Option<ContextId>,
    _operation_instance_id: Option<OperationInstanceId>,
    message: Option<String>,
    ttl_ms: Option<u64>,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<InvitationId, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let deadline = workflow_timeout_budget(
        &runtime,
        Duration::from_millis(INVITE_USER_OPERATION_TIMEOUT_MS),
    )
    .await?;
    let stage_tracker = Some(new_workflow_stage_tracker("publish_workflow_dispatched"));

    let result = execute_with_runtime_timeout_budget(&runtime, &deadline, || async {
        update_workflow_stage(&stage_tracker, "resolve_target_authority");
        let receiver = timeout_workflow_stage_with_deadline(
            &runtime,
            "invite_user_to_channel",
            "resolve_target_authority",
            Some(deadline),
            resolve_target_authority_for_invite(app_core, target_user_id),
        )
        .await?;
        update_workflow_stage(&stage_tracker, "resolve_channel_id");
        let channel_id = timeout_workflow_stage_with_deadline(
            &runtime,
            "invite_user_to_channel",
            "resolve_channel_id",
            Some(deadline),
            resolve_chat_channel_id_from_state_or_input(app_core, channel_name_or_id),
        )
        .await?;
        let channel_name_hint =
            canonical_channel_name_hint_for_invite(app_core, channel_id, channel_name_or_id)
                .await?;
        update_workflow_stage(&stage_tracker, "invite_authority_to_channel");

        super::invite_authority_to_channel_with_context(
            app_core,
            receiver,
            channel_id,
            context_id,
            Some(channel_name_hint),
            owner,
            None,
            Some(deadline),
            stage_tracker.clone(),
            message,
            ttl_ms,
        )
        .await
    })
    .await
    .map_err(|error| match error {
        TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. }) => {
            let stage = stage_tracker
                .as_ref()
                .and_then(|tracker| tracker.try_lock().map(|guard| *guard))
                .unwrap_or("operation");
            AuraError::from(super::error::WorkflowError::TimedOut {
                operation: "invite_user_to_channel",
                stage,
                timeout_ms: deadline.timeout_ms(),
            })
        }
        TimeoutRunError::Timeout(timeout_error) => timeout_error.into(),
        TimeoutRunError::Operation(operation_error) => operation_error,
    });

    if let Err(error) = &result {
        let semantic_error = SemanticOperationError::new(
            SemanticFailureDomain::Command,
            SemanticFailureCode::InternalError,
        )
        .with_detail(error.to_string());
        if let Err(_pub_err) = owner.publish_failure(semantic_error).await {
            #[cfg(feature = "instrumented")]
            tracing::error!(
                operation_error = %error,
                publish_error = %_pub_err,
                "invite_authority_to_channel: failed to publish failure fact"
            );
        }
    }

    let invitation_id = result?;
    owner
        .publish_success_with(issue_channel_invitation_created_proof(
            invitation_id.clone(),
        ))
        .await?;
    Ok(invitation_id)
}

/// Invite a canonical authority to a canonical channel.
pub async fn invite_authority_to_channel(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    channel_id: ChannelId,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationId, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        None,
        SemanticOperationKind::InviteActorToChannel,
    );
    super::invite_authority_to_channel_with_context(
        app_core,
        receiver,
        channel_id,
        None,
        None,
        &owner,
        None,
        None,
        None,
        message,
        ttl_ms,
    )
    .await
}
