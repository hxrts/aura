#![allow(missing_docs)]

use super::*;

pub async fn accept_pending_channel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<InvitationId, AuraError> {
    accept_pending_channel_invitation_with_instance(app_core, None).await
}

#[aura_macros::semantic_owner(
    owner = "accept_pending_channel_invitation_id_owned",
    wrapper = "accept_pending_channel_invitation_with_instance",
    terminal = "publish_success_with",
    postcondition = "pending_invitation_consumed",
    proof = crate::workflows::semantic_facts::PendingInvitationConsumedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "runtime_accept_converged,invitation_accepted_or_materialized",
    child_ops = "accept_imported_invitation",
    category = "move_owned"
)]
async fn accept_pending_channel_invitation_id_owned(
    app_core: &Arc<RwLock<AppCore>>,
    owner: &SemanticWorkflowOwner,
    _instance_id: Option<OperationInstanceId>,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<InvitationId, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let pending_invitation =
        match authoritative_pending_home_or_channel_invitation_for_accept(app_core, &runtime).await
        {
            Ok(invitation) => invitation,
            Err(error) => {
                return fail_pending_invitation_accept_owned(
                    owner,
                    AcceptInvitationError::AcceptFailed {
                        detail: error.to_string(),
                    },
                )
                .await;
            }
        };

    let Some(invitation) = pending_invitation else {
        return fail_pending_invitation_accept_owned(
            owner,
            AcceptInvitationError::AcceptFailed {
                detail: "No pending channel invitation found".to_string(),
            },
        )
        .await;
    };

    let invitation_id = invitation.invitation_id.clone();
    let _ = super::accept_imported_invitation_inner(app_core, &invitation, owner).await?;
    owner
        .publish_success_with(issue_pending_invitation_consumed_proof(
            invitation_id.clone(),
        ))
        .await?;
    Ok(invitation_id)
}

pub async fn accept_pending_channel_invitation_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    instance_id: Option<OperationInstanceId>,
) -> Result<InvitationId, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_accept_channel(),
        instance_id.clone(),
        SemanticOperationKind::AcceptPendingChannelInvitation,
    );
    publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
        .await?;
    accept_pending_channel_invitation_id_owned(app_core, &owner, instance_id, None).await
}

pub async fn accept_pending_channel_invitation_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationId> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_accept_channel(),
        instance_id.clone(),
        SemanticOperationKind::AcceptPendingChannelInvitation,
    );
    let result = async {
        publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
            .await?;
        accept_pending_channel_invitation_id_owned(app_core, &owner, instance_id, None).await
    }
    .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

async fn pending_channel_binding_witness(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &InvitationInfo,
) -> Result<crate::ui_contract::ChannelBindingWitness, AuraError> {
    let crate::runtime_bridge::InvitationBridgeType::Channel {
        home_id,
        context_id,
        ..
    } = &invitation.invitation_type
    else {
        return Err(AuraError::invalid(
            "pending invitation does not materialize a channel binding",
        ));
    };

    let channel_id = home_id.parse::<ChannelId>().map_err(|error| {
        AuraError::invalid(format!(
            "pending channel invitation {} resolved to invalid canonical channel id {home_id}: {error}",
            invitation.invitation_id
        ))
    })?;
    let authoritative_context = match context_id {
        Some(context_id) => Some(*context_id),
        None => {
            #[cfg(feature = "signals")]
            {
                crate::workflows::messaging::resolve_authoritative_context_id_for_channel(
                    app_core, channel_id,
                )
                .await
            }
            #[cfg(not(feature = "signals"))]
            {
                let _ = app_core;
                None
            }
        }
    };

    Ok(crate::ui_contract::ChannelBindingWitness::new(
        channel_id.to_string(),
        authoritative_context.map(|context_id| context_id.to_string()),
    ))
}

#[cfg(feature = "signals")]
pub(in crate::workflows) async fn run_post_channel_accept_followups(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    context_hint: Option<ContextId>,
    channel_name_hint: Option<String>,
) {
    let authoritative_context = match context_hint {
        Some(context_id) => Some(context_id),
        None => {
            crate::workflows::messaging::resolve_authoritative_context_id_for_channel(
                app_core, channel_id,
            )
            .await
        }
    };
    let Some(context_id) = authoritative_context else {
        return;
    };

    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(crate::workflows::messaging::post_terminal_join_followups(
            app_core,
            crate::workflows::messaging::authoritative_channel_ref(channel_id, context_id),
            channel_name_hint.as_deref(),
        ))
        .await;
    let _ = best_effort.finish();
}

pub async fn accept_pending_channel_invitation_with_binding_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<crate::ui_contract::AcceptedPendingChannelBinding>
{
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_accept_channel(),
        instance_id.clone(),
        SemanticOperationKind::AcceptPendingChannelInvitation,
    );
    let result = async {
        publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
            .await?;

        let runtime = require_runtime(app_core).await?;
        let pending_invitation =
            match authoritative_pending_home_or_channel_invitation_for_accept(app_core, &runtime)
                .await
            {
                Ok(invitation) => invitation,
                Err(error) => {
                    return fail_pending_invitation_accept_owned(
                        &owner,
                        AcceptInvitationError::AcceptFailed {
                            detail: error.to_string(),
                        },
                    )
                    .await;
                }
            };
        let Some(pending_invitation) = pending_invitation else {
            return fail_pending_invitation_accept_owned(
                &owner,
                AcceptInvitationError::AcceptFailed {
                    detail: "No pending channel invitation found".to_string(),
                },
            )
            .await;
        };

        if !matches!(
            pending_invitation.invitation_type,
            crate::runtime_bridge::InvitationBridgeType::Channel { .. }
        ) {
            return fail_pending_invitation_accept_owned(
                &owner,
                AcceptInvitationError::AcceptFailed {
                    detail: "pending invitation is not a channel invitation".to_string(),
                },
            )
            .await;
        }

        let invitation_id = pending_invitation.invitation_id.clone();
        super::accept_imported_invitation_owned(app_core, &pending_invitation, &owner, None)
            .await?;
        let binding = match pending_channel_binding_witness(app_core, &pending_invitation).await {
            Ok(binding) => binding,
            Err(error) => {
                return fail_invitation_accept(
                    &owner,
                    AcceptInvitationError::AcceptFailed {
                        detail: error.to_string(),
                    },
                )
                .await;
            }
        };

        Ok(crate::ui_contract::AcceptedPendingChannelBinding {
            invitation_id: invitation_id.to_string(),
            binding,
            channel_name: match &pending_invitation.invitation_type {
                crate::runtime_bridge::InvitationBridgeType::Channel {
                    nickname_suggestion,
                    ..
                } => nickname_suggestion.clone(),
                _ => None,
            },
        })
    }
    .await;

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}
