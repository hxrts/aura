#![allow(missing_docs)]

use super::*;
use thiserror::Error;

fn emit_contact_accept_probe(stage: &str) {
    let _ = stage;
}

pub async fn accept_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: InvitationHandle,
) -> Result<(), AuraError> {
    accept_invitation_with_instance(app_core, invitation, None).await
}

#[aura_macros::semantic_owner(
    owner = "invitation_accept_id_owned",
    wrapper = "accept_invitation_with_instance",
    terminal = "publish_success_with",
    postcondition = "invitation_accepted_or_materialized",
    proof = crate::workflows::semantic_facts::InvitationAcceptedOrMaterializedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "runtime_accept_converged",
    child_ops = "",
    category = "move_owned"
)]
async fn accept_invitation_id_owned(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &InvitationId,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<(), AuraError> {
    let accepted_invitation = list_invitations(app_core)
        .await
        .invitation(invitation_id.as_str())
        .cloned();
    let runtime = require_runtime(app_core).await?;
    let pending_runtime_invitation =
        match pending_invitation_by_id_with_timeout(&runtime, invitation_id).await {
            Ok(invitation) => invitation,
            Err(error) => {
                if accepted_invitation.is_none() {
                    return fail_pending_invitation_accept_owned(owner, error).await;
                }
                None
            }
        };

    let accept_budget = match invitation_accept_timeout_budget(
        &runtime,
        pending_runtime_invitation.as_ref(),
        accepted_invitation.as_ref(),
    )
    .await
    {
        Ok(budget) => budget,
        Err(error) => return fail_invitation_accept(owner, error).await,
    };
    let accept_result = execute_with_runtime_timeout_budget(&runtime, &accept_budget, || {
        runtime.accept_invitation(invitation_id.as_str())
    })
    .await;
    if let Err(error) = accept_result {
        let error = match error {
            TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. }) => {
                AcceptInvitationError::AcceptFailed {
                    detail: format!(
                        "accept_invitation timed out in stage runtime_accept_invitation after {}ms",
                        accept_budget.timeout_ms()
                    ),
                }
            }
            TimeoutRunError::Timeout(timeout_error) => AcceptInvitationError::AcceptFailed {
                detail: timeout_error.to_string(),
            },
            TimeoutRunError::Operation(operation_error) => AcceptInvitationError::AcceptFailed {
                detail: operation_error.to_string(),
            },
        };
        if classify_invitation_accept_error(&error) != InvitationAcceptErrorClass::AlreadyHandled {
            return fail_invitation_accept(
                owner,
                AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                },
            )
            .await;
        }
    }

    let contact_peer = (owner.kind() == SemanticOperationKind::AcceptContactInvitation)
        .then(|| {
            accepted_invitation
                .as_ref()
                .map(|invitation| invitation.from_id)
                .or_else(|| {
                    pending_runtime_invitation.as_ref().and_then(|invitation| {
                        if matches!(
                            invitation.invitation_type,
                            InvitationBridgeType::Contact { .. }
                        ) {
                            Some(invitation.sender_id)
                        } else {
                            None
                        }
                    })
                })
        })
        .flatten();
    trigger_runtime_discovery_with_timeout(&runtime).await;
    if let Err(error) = drive_invitation_accept_convergence(app_core, &runtime, contact_peer).await
    {
        return fail_invitation_accept(owner, error).await;
    }

    if owner.kind() == SemanticOperationKind::AcceptContactInvitation {
        let contact_id = accepted_invitation
            .as_ref()
            .map(|invitation| invitation.from_id)
            .or_else(|| {
                pending_runtime_invitation.as_ref().and_then(|invitation| {
                    if matches!(
                        invitation.invitation_type,
                        InvitationBridgeType::Contact { .. }
                    ) {
                        Some(invitation.sender_id)
                    } else {
                        None
                    }
                })
            });
        if let Some(contact_id) = contact_id {
            if let Err(error) = refresh_authoritative_contact_link_readiness(app_core).await {
                return fail_invitation_accept(
                    owner,
                    AcceptInvitationError::AcceptFailed {
                        detail: format!(
                            "contact invitation readiness refresh failed for {contact_id}: {error}"
                        ),
                    },
                )
                .await;
            }
            if let Err(error) =
                publish_authoritative_contact_invitation_accepted(app_core, contact_id).await
            {
                return fail_invitation_accept(
                    owner,
                    AcceptInvitationError::AcceptFailed {
                        detail: format!(
                            "contact invitation authoritative publish failed for {contact_id}: {error}"
                        ),
                    },
                )
                .await;
            }
            owner
                .publish_success_with(issue_invitation_accepted_or_materialized_proof(
                    invitation_id.clone(),
                ))
                .await?;
            return Ok(());
        }
        return fail_invitation_accept(
            owner,
            AcceptInvitationError::AcceptFailed {
                detail: format!(
                    "contact invitation {invitation_id} completed without an authoritative contact id"
                ),
            },
        )
        .await;
    } else if let Some((channel_id, context_hint, channel_name_hint)) = pending_runtime_invitation
        .as_ref()
        .and_then(|invitation| match &invitation.invitation_type {
            InvitationBridgeType::Channel {
                home_id,
                context_id,
                nickname_suggestion,
            } => home_id
                .parse::<ChannelId>()
                .ok()
                .map(|channel_id| (channel_id, *context_id, nickname_suggestion.as_deref())),
            _ => None,
        })
        .or_else(|| {
            accepted_invitation.as_ref().and_then(|invitation| {
                if invitation.invitation_type == crate::views::invitations::InvitationType::Chat {
                    invitation
                        .home_id
                        .map(|channel_id| (channel_id, None, invitation.home_name.as_deref()))
                } else {
                    None
                }
            })
        })
    {
        if let Err(error) = reconcile_channel_invitation_acceptance(
            app_core,
            &runtime,
            pending_runtime_invitation.as_ref(),
            accepted_invitation.as_ref(),
            channel_id,
            context_hint,
            channel_name_hint,
        )
        .await
        {
            return fail_invitation_accept(owner, error).await;
        }
        #[cfg(feature = "signals")]
        {
            if let Err(error) =
                crate::workflows::messaging::refresh_authoritative_channel_membership_readiness(
                    app_core,
                )
                .await
            {
                return fail_invitation_accept(
                    owner,
                    AcceptInvitationError::AcceptFailed {
                        detail: error.to_string(),
                    },
                )
                .await;
            }
            let membership_proof = match prove_channel_membership_ready(app_core, channel_id).await
            {
                Ok(proof) => proof,
                Err(error) => {
                    return fail_invitation_accept(
                        owner,
                        AcceptInvitationError::AcceptFailed {
                            detail: format!(
                                "channel invitation accept missing membership readiness for {channel_id}: {error}"
                            ),
                        },
                    )
                    .await;
                }
            };
            owner.publish_success_with(membership_proof).await?;
            run_post_channel_accept_followups(
                app_core,
                channel_id,
                context_hint,
                channel_name_hint.map(ToOwned::to_owned),
            )
            .await;
        }
        #[cfg(not(feature = "signals"))]
        {
            owner
                .publish_success_with(issue_invitation_accepted_or_materialized_proof(
                    invitation_id.clone(),
                ))
                .await?;
        }
        return Ok(());
    }

    owner
        .publish_success_with(issue_invitation_accepted_or_materialized_proof(
            invitation_id.clone(),
        ))
        .await?;

    Ok(())
}

pub async fn accept_invitation_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: InvitationHandle,
    instance_id: Option<OperationInstanceId>,
) -> Result<(), AuraError> {
    let invitation_id = invitation.invitation_id().clone();
    let accepted_invitation = list_invitations(app_core)
        .await
        .invitation(invitation_id.as_str())
        .cloned();
    let runtime = require_runtime(app_core).await?;
    let pending_runtime_invitation =
        match pending_invitation_by_id_with_timeout(&runtime, &invitation_id).await {
            Ok(invitation) => invitation,
            Err(error) => {
                if accepted_invitation.is_none() {
                    return fail_pending_invitation_accept_unowned(error).await;
                }
                None
            }
        };
    let operation_kind = if pending_runtime_invitation
        .as_ref()
        .is_some_and(|invitation| {
            matches!(
                invitation.invitation_type,
                InvitationBridgeType::Contact { .. }
            )
        })
        || accepted_invitation.as_ref().is_some_and(|invitation| {
            invitation.invitation_type == crate::views::invitations::InvitationType::Home
        }) {
        SemanticOperationKind::AcceptContactInvitation
    } else {
        SemanticOperationKind::AcceptPendingChannelInvitation
    };
    let operation_id = match operation_kind {
        SemanticOperationKind::AcceptPendingChannelInvitation => {
            OperationId::invitation_accept_channel()
        }
        _ => OperationId::invitation_accept_contact(),
    };
    let owner =
        SemanticWorkflowOwner::new(app_core, operation_id, instance_id.clone(), operation_kind);
    publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
        .await?;
    accept_invitation_id_owned(app_core, &invitation_id, &owner, None).await
}

pub async fn accept_imported_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: InvitationHandle,
) -> Result<(), AuraError> {
    accept_imported_invitation_with_instance(app_core, invitation, None).await
}

#[aura_macros::semantic_owner(
    owner = "accept_imported_invitation_owned",
    wrapper = "accept_imported_invitation_with_instance",
    terminal = "publish_success_with",
    postcondition = "invitation_accepted_or_materialized",
    proof = crate::workflows::semantic_facts::InvitationAcceptedOrMaterializedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "runtime_accept_converged",
    child_ops = "",
    category = "move_owned"
)]
pub(in crate::workflows) async fn accept_imported_invitation_owned(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &crate::runtime_bridge::InvitationInfo,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<(), AuraError> {
    match accept_imported_invitation_inner(app_core, invitation, owner).await? {
        #[cfg(feature = "signals")]
        Some(channel_id) => {
            let membership_proof = prove_channel_membership_ready(app_core, channel_id).await?;
            owner.publish_success_with(membership_proof).await?;
        }
        #[cfg(not(feature = "signals"))]
        Some(_) => unreachable!("channel membership proofs are only issued with signals"),
        None => {
            owner
                .publish_success_with(issue_invitation_accepted_or_materialized_proof(
                    invitation.invitation_id.clone(),
                ))
                .await?;
        }
    }
    Ok(())
}

pub(in crate::workflows) async fn accept_imported_invitation_inner(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &crate::runtime_bridge::InvitationInfo,
    owner: &SemanticWorkflowOwner,
) -> Result<Option<ChannelId>, AuraError> {
    let contact_probe = matches!(
        invitation.invitation_type,
        crate::runtime_bridge::InvitationBridgeType::Contact { .. }
    );
    if matches!(
        invitation.invitation_type,
        crate::runtime_bridge::InvitationBridgeType::DeviceEnrollment { .. }
    ) {
        return fail_invitation_accept(
            owner,
            AcceptInvitationError::AcceptFailed {
                detail:
                    "device enrollment invitations must use accept_device_enrollment_invitation"
                        .to_string(),
            },
        )
        .await;
    }

    if contact_probe {
        emit_contact_accept_probe("require_runtime");
    }
    let runtime = require_runtime(app_core).await?;

    if contact_probe {
        emit_contact_accept_probe("accept_budget");
    }
    let accept_budget =
        match invitation_accept_timeout_budget(&runtime, Some(invitation), None).await {
            Ok(budget) => budget,
            Err(error) => return fail_invitation_accept(owner, error).await,
        };
    if contact_probe {
        emit_contact_accept_probe("runtime_accept");
    }
    let accept_result = execute_with_runtime_timeout_budget(&runtime, &accept_budget, || {
        runtime.accept_invitation(invitation.invitation_id.as_str())
    })
    .await;
    if let Err(error) = accept_result {
        let error = match error {
            TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. }) => {
                AcceptInvitationError::AcceptFailed {
                    detail: format!(
                        "accept_imported_invitation timed out in stage runtime_accept_invitation after {}ms",
                        accept_budget.timeout_ms()
                    ),
                }
            }
            TimeoutRunError::Timeout(timeout_error) => AcceptInvitationError::AcceptFailed {
                detail: timeout_error.to_string(),
            },
            TimeoutRunError::Operation(operation_error) => AcceptInvitationError::AcceptFailed {
                detail: operation_error.to_string(),
            },
        };
        if classify_invitation_accept_error(&error) != InvitationAcceptErrorClass::AlreadyHandled {
            return fail_invitation_accept(
                owner,
                AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                },
            )
            .await;
        }
    }

    if contact_probe {
        emit_contact_accept_probe("post_accept_discovery");
    }
    trigger_runtime_discovery_with_timeout(&runtime).await;
    if contact_probe {
        emit_contact_accept_probe("post_accept_convergence");
    }
    let contact_peer = matches!(
        invitation.invitation_type,
        crate::runtime_bridge::InvitationBridgeType::Contact { .. }
    )
    .then_some(invitation.sender_id);
    if let Err(error) = drive_invitation_accept_convergence(app_core, &runtime, contact_peer).await
    {
        return fail_invitation_accept(owner, error).await;
    }

    match &invitation.invitation_type {
        crate::runtime_bridge::InvitationBridgeType::Contact { .. } => {
            emit_contact_accept_probe("refresh_contact_readiness");
            if let Err(error) = refresh_authoritative_contact_link_readiness(app_core).await {
                return fail_invitation_accept(
                    owner,
                    AcceptInvitationError::AcceptFailed {
                        detail: format!(
                            "imported contact invitation readiness refresh failed for {}: {error}",
                            invitation.sender_id
                        ),
                    },
                )
                .await;
            }
            if let Err(error) =
                publish_authoritative_contact_invitation_accepted(app_core, invitation.sender_id)
                    .await
            {
                return fail_invitation_accept(
                    owner,
                    AcceptInvitationError::AcceptFailed {
                        detail: format!(
                            "imported contact invitation authoritative publish failed for {}: {error}",
                            invitation.sender_id
                        ),
                    },
                )
                .await;
            }
            emit_contact_accept_probe("publish_success");
            emit_contact_accept_probe("done");
            return Ok(None);
        }
        crate::runtime_bridge::InvitationBridgeType::Channel {
            home_id,
            context_id,
            nickname_suggestion,
            ..
        } => {
            let channel_id = match home_id.parse::<ChannelId>() {
                Ok(channel_id) => channel_id,
                Err(_) => {
                    return fail_invitation_accept(
                        owner,
                        AcceptInvitationError::AcceptFailed {
                            detail: format!(
                                "channel invitation {} resolved to invalid canonical channel id {home_id}",
                                invitation.invitation_id
                            ),
                        },
                    )
                    .await;
                }
            };
            if let Err(error) = reconcile_channel_invitation_acceptance(
                app_core,
                &runtime,
                Some(invitation),
                None,
                channel_id,
                *context_id,
                nickname_suggestion.as_deref(),
            )
            .await
            {
                return fail_invitation_accept(owner, error).await;
            }
            #[cfg(feature = "signals")]
            {
                if let Err(error) =
                    crate::workflows::messaging::refresh_authoritative_channel_membership_readiness(
                        app_core,
                    )
                    .await
                {
                    return fail_invitation_accept(
                        owner,
                        AcceptInvitationError::AcceptFailed {
                            detail: error.to_string(),
                        },
                    )
                    .await;
                }
                run_post_channel_accept_followups(
                    app_core,
                    channel_id,
                    *context_id,
                    nickname_suggestion.clone(),
                )
                .await;
                return Ok(Some(channel_id));
            }
            #[cfg(not(feature = "signals"))]
            {
                return Ok(None);
            }
        }
        crate::runtime_bridge::InvitationBridgeType::Guardian { .. } => {}
        crate::runtime_bridge::InvitationBridgeType::DeviceEnrollment { .. } => unreachable!(),
    }

    Ok(None)
}

pub async fn accept_imported_invitation_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: InvitationHandle,
    instance_id: Option<OperationInstanceId>,
) -> Result<(), AuraError> {
    let operation_kind = semantic_kind_for_bridge_invitation(invitation.info());
    let operation_id = match operation_kind {
        SemanticOperationKind::AcceptPendingChannelInvitation => {
            OperationId::invitation_accept_channel()
        }
        _ => OperationId::invitation_accept_contact(),
    };
    let owner =
        SemanticWorkflowOwner::new(app_core, operation_id, instance_id.clone(), operation_kind);
    publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let invitation = invitation.into_info();
    accept_imported_invitation_owned(app_core, &invitation, &owner, None).await
}

pub async fn accept_imported_invitation_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: InvitationHandle,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<()> {
    let operation_kind = semantic_kind_for_bridge_invitation(invitation.info());
    let operation_id = match operation_kind {
        SemanticOperationKind::AcceptPendingChannelInvitation => {
            OperationId::invitation_accept_channel()
        }
        _ => OperationId::invitation_accept_contact(),
    };
    let owner =
        SemanticWorkflowOwner::new(app_core, operation_id, instance_id.clone(), operation_kind);
    let result: Result<(), AuraError> = async {
        if operation_kind == SemanticOperationKind::AcceptContactInvitation {
            emit_contact_accept_probe("publish_workflow_dispatched");
        }
        publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
            .await?;
        let invitation = invitation.into_info();
        if matches!(
            invitation.invitation_type,
            crate::runtime_bridge::InvitationBridgeType::Contact { .. }
        ) {
            emit_contact_accept_probe("owned_start");
        }
        accept_imported_invitation_owned(app_core, &invitation, &owner, None).await
    }
    .await;

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

pub async fn accept_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<InvitationInfo, AuraError> {
    accept_invitation_by_str_with_instance(app_core, invitation_id, None).await
}

pub async fn accept_invitation_by_str_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
    instance_id: Option<OperationInstanceId>,
) -> Result<InvitationInfo, AuraError> {
    let invitation = pending_invitation_info_by_id(app_core, invitation_id).await?;
    accept_invitation_with_instance(
        app_core,
        InvitationHandle::new(invitation.clone()),
        instance_id,
    )
    .await?;
    Ok(invitation)
}

pub async fn accept_invitation_by_str_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationInfo> {
    let prefetched = pending_invitation_info_by_id(app_core, invitation_id).await;
    let kind = prefetched
        .as_ref()
        .map(semantic_kind_for_bridge_invitation)
        .unwrap_or(SemanticOperationKind::AcceptContactInvitation);
    let operation_id = match kind {
        SemanticOperationKind::AcceptPendingChannelInvitation => {
            OperationId::invitation_accept_channel()
        }
        _ => OperationId::invitation_accept_contact(),
    };
    let owner = SemanticWorkflowOwner::new(app_core, operation_id, instance_id, kind);
    let result: Result<InvitationInfo, AuraError> = async {
        publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
            .await?;
        let invitation = prefetched?;
        accept_invitation_id_owned(app_core, &invitation.invitation_id, &owner, None).await?;
        Ok(invitation)
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

pub async fn decline_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: InvitationHandle,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    let _ = timeout_runtime_call(
        &runtime,
        "decline_invitation",
        "decline_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || runtime.decline_invitation(invitation.invitation_id().as_str()),
    )
    .await
    .map_err(|e| AuraError::from(super::super::error::runtime_call("decline invitation", e)))?
    .map_err(|e| AuraError::from(super::super::error::runtime_call("decline invitation", e)))?;
    Ok(())
}

pub async fn decline_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    let invitation = pending_invitation_info_by_id(app_core, invitation_id).await?;
    decline_invitation(app_core, InvitationHandle::new(invitation)).await
}

pub async fn decline_invitation_by_str_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<()> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_decline(),
        instance_id,
        SemanticOperationKind::DeclineInvitation,
    );
    let result: Result<(), AuraError> = async {
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        let invitation_id = InvitationId::new(invitation_id);
        decline_invitation_by_str(app_core, invitation_id.as_str()).await?;
        owner
            .publish_success_with(issue_invitation_declined_proof(invitation_id))
            .await?;
        Ok(())
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

pub async fn cancel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: InvitationHandle,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    let _ = timeout_runtime_call(
        &runtime,
        "cancel_invitation",
        "cancel_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || runtime.cancel_invitation(invitation.invitation_id().as_str()),
    )
    .await
    .map_err(|e| AuraError::from(super::super::error::runtime_call("cancel invitation", e)))?
    .map_err(|e| AuraError::from(super::super::error::runtime_call("cancel invitation", e)))?;
    Ok(())
}

pub async fn cancel_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    let invitation = pending_invitation_info_by_id(app_core, invitation_id).await?;
    cancel_invitation(app_core, InvitationHandle::new(invitation)).await
}

pub async fn cancel_invitation_by_str_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<()> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_revoke(),
        instance_id,
        SemanticOperationKind::RevokeInvitation,
    );
    let result: Result<(), AuraError> = async {
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        let invitation_id = InvitationId::new(invitation_id);
        cancel_invitation_by_str(app_core, invitation_id.as_str()).await?;
        owner
            .publish_success_with(issue_invitation_revoked_proof(invitation_id))
            .await?;
        Ok(())
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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(in crate::workflows) enum AcceptInvitationError {
    #[error("Failed to accept invitation: {detail}")]
    AcceptFailed { detail: String },
    #[error("accepted contact invitation for {contact_id} but the contact never converged")]
    ContactLinkDidNotConverge { contact_id: AuthorityId },
}

impl AcceptInvitationError {
    pub(in crate::workflows) fn semantic_error(
        &self,
        kind: SemanticOperationKind,
    ) -> crate::ui_contract::SemanticOperationError {
        use crate::ui_contract::{
            SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
        };

        match self {
            Self::AcceptFailed { detail } => SemanticOperationError::new(
                SemanticFailureDomain::Invitation,
                SemanticFailureCode::InternalError,
            )
            .with_detail(format!("operation_kind={kind:?}; detail={detail}")),
            Self::ContactLinkDidNotConverge { contact_id } => SemanticOperationError::new(
                SemanticFailureDomain::Invitation,
                SemanticFailureCode::ContactLinkDidNotConverge,
            )
            .with_detail(format!("contact_id={contact_id}")),
        }
    }
}

impl From<AcceptInvitationError> for AuraError {
    fn from(error: AcceptInvitationError) -> Self {
        AuraError::agent(error.to_string())
    }
}

fn is_authoritative_pending_home_or_channel_invitation(
    invitation: &InvitationInfo,
    our_authority: AuthorityId,
) -> bool {
    matches!(
        invitation.invitation_type,
        InvitationBridgeType::Channel { .. }
    ) && (invitation.sender_id != our_authority || invitation.receiver_id == our_authority)
}

fn select_authoritative_pending_home_invitation(
    invitations: &[InvitationInfo],
    our_authority: AuthorityId,
) -> Option<&InvitationInfo> {
    let pending = invitations.iter().filter(|invitation| {
        invitation.status == crate::runtime_bridge::InvitationBridgeStatus::Pending
            && is_authoritative_pending_home_or_channel_invitation(invitation, our_authority)
    });

    pending
        .clone()
        .find(|invitation| invitation.sender_id != our_authority)
        .or_else(|| pending.into_iter().next())
}

#[aura_macros::authoritative_source(kind = "runtime")]
pub(in crate::workflows) async fn authoritative_pending_home_or_channel_invitation(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
) -> Result<Option<InvitationInfo>, AuraError> {
    Ok(select_authoritative_pending_home_invitation(
        &list_pending_invitations_with_timeout(runtime)
            .await
            .map_err(AuraError::from)?,
        runtime.authority_id(),
    )
    .cloned())
}

#[cfg(feature = "signals")]
pub(super) fn invitations_signal_has_pending_home_or_channel_invitation(
    invitations: &crate::views::invitations::InvitationsState,
) -> bool {
    invitations.all_pending().iter().any(|invitation| {
        invitation.direction == crate::views::invitations::InvitationDirection::Received
            && (invitation.invitation_type == crate::views::invitations::InvitationType::Chat
                || invitation.home_id.is_some())
    })
}

#[cfg(feature = "signals")]
async fn await_authoritative_pending_home_or_channel_invitation_for_accept(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
) -> Result<Option<InvitationInfo>, AuraError> {
    let invitations = read_signal_or_default(app_core, &*INVITATIONS_SIGNAL).await;
    if !invitations_signal_has_pending_home_or_channel_invitation(&invitations) {
        return Ok(None);
    }

    let policy = workflow_retry_policy(
        PENDING_INVITATION_AUTHORITATIVE_ATTEMPTS as u32,
        Duration::from_millis(PENDING_INVITATION_AUTHORITATIVE_BACKOFF_MS),
        Duration::from_millis(PENDING_INVITATION_AUTHORITATIVE_BACKOFF_MS),
    )?;
    let app_core = app_core.clone();
    execute_with_runtime_retry_budget(runtime, &policy, |_attempt| {
        let app_core = app_core.clone();
        let runtime = runtime.clone();
        async move {
            if let Some(invitation) =
                authoritative_pending_home_or_channel_invitation(&runtime).await?
            {
                return Ok(invitation);
            }
            let _ = crate::workflows::system::refresh_account(&app_core).await;
            converge_runtime(&runtime).await;
            Err(AuraError::from(
                crate::workflows::error::WorkflowError::Precondition(
                    "pending channel invitation is not yet authoritative",
                ),
            ))
        }
    })
    .await
    .map(Some)
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => AuraError::from(timeout_error),
        RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
    })
}

pub(in crate::workflows) async fn authoritative_pending_home_or_channel_invitation_for_accept(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
) -> Result<Option<InvitationInfo>, AuraError> {
    if let Some(invitation) = authoritative_pending_home_or_channel_invitation(runtime).await? {
        return Ok(Some(invitation));
    }
    #[cfg(feature = "signals")]
    {
        return await_authoritative_pending_home_or_channel_invitation_for_accept(
            app_core, runtime,
        )
        .await;
    }
    #[cfg(not(feature = "signals"))]
    {
        let _ = app_core;
        Ok(None)
    }
}

pub(in crate::workflows) async fn invitation_accept_timeout_budget(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    pending_runtime_invitation: Option<&crate::runtime_bridge::InvitationInfo>,
    accepted_invitation: Option<&crate::views::invitations::Invitation>,
) -> Result<TimeoutBudget, AcceptInvitationError> {
    workflow_timeout_budget(
        runtime,
        Duration::from_millis(invitation_accept_runtime_stage_timeout_ms(
            pending_runtime_invitation,
            accepted_invitation,
        )),
    )
    .await
    .map_err(|error| AcceptInvitationError::AcceptFailed {
        detail: error.to_string(),
    })
}

pub(in crate::workflows) async fn fail_invitation_accept<T>(
    owner: &SemanticWorkflowOwner,
    error: AcceptInvitationError,
) -> Result<T, AuraError> {
    publish_invitation_owner_failure(owner, None, error.semantic_error(owner.kind())).await?;
    Err(error.into())
}

pub(in crate::workflows) async fn fail_pending_invitation_accept_owned<T>(
    owner: &SemanticWorkflowOwner,
    error: AcceptInvitationError,
) -> Result<T, AuraError> {
    fail_invitation_accept(owner, error).await
}

pub(in crate::workflows) async fn fail_pending_invitation_accept_unowned<T>(
    error: AcceptInvitationError,
) -> Result<T, AuraError> {
    Err(error.into())
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
struct AcceptedChannelInvitationTarget {
    channel_id: ChannelId,
    context_hint: Option<ContextId>,
    channel_name_hint: Option<String>,
}

pub(in crate::workflows) async fn reconcile_channel_invitation_acceptance(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    pending_runtime_invitation: Option<&InvitationInfo>,
    accepted_invitation: Option<&crate::views::invitations::Invitation>,
    channel_id: ChannelId,
    context_hint: Option<ContextId>,
    channel_name_hint: Option<&str>,
) -> Result<(), AcceptInvitationError> {
    let accepted_channel = AcceptedChannelInvitationTarget {
        channel_id,
        context_hint,
        channel_name_hint: channel_name_hint.map(ToOwned::to_owned),
    };
    let stage_tracker = new_workflow_stage_tracker("reconcile_channel_invitation:start");
    let reconcile_budget = match workflow_timeout_budget(
        runtime,
        Duration::from_millis(invitation_accept_reconcile_timeout_ms(
            pending_runtime_invitation,
            accepted_invitation,
        )),
    )
    .await
    {
        Ok(budget) => budget,
        Err(error) => {
            return Err(AcceptInvitationError::AcceptFailed {
                detail: error.to_string(),
            });
        }
    };

    let reconcile_result = execute_with_runtime_timeout_budget(runtime, &reconcile_budget, || {
        reconcile_accepted_channel_invitation(app_core, runtime, &accepted_channel, &stage_tracker)
    })
    .await;

    match reconcile_result {
        Ok(()) => Ok(()),
        Err(error) => {
            let detail = match error {
                TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. }) => {
                    let stage = stage_tracker
                        .try_lock()
                        .map(|guard| *guard)
                        .unwrap_or("reconcile_channel_invitation:unknown");
                    format!(
                        "accept_invitation timed out in stage reconcile_channel_invitation after {}ms (last_stage={stage})",
                        reconcile_budget.timeout_ms()
                    )
                }
                TimeoutRunError::Timeout(timeout_error) => timeout_error.to_string(),
                TimeoutRunError::Operation(operation_error) => operation_error.to_string(),
            };
            Err(AcceptInvitationError::AcceptFailed { detail })
        }
    }
}

pub(in crate::workflows) async fn list_pending_invitations_with_timeout(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
) -> Result<Vec<InvitationInfo>, AcceptInvitationError> {
    let budget = workflow_timeout_budget(
        runtime,
        Duration::from_millis(INVITATION_ACCEPT_LOOKUP_TIMEOUT_MS),
    )
    .await
    .map_err(|error| AcceptInvitationError::AcceptFailed {
        detail: error.to_string(),
    })?;

    match execute_with_runtime_timeout_budget(runtime, &budget, || async {
        runtime
            .try_list_pending_invitations()
            .await
            .map_err(|error| AcceptInvitationError::AcceptFailed {
                detail: error.to_string(),
            })
    })
    .await
    {
        Ok(pending) => Ok(pending),
        Err(TimeoutRunError::Timeout(error)) => Err(AcceptInvitationError::AcceptFailed {
            detail: error.to_string(),
        }),
        Err(TimeoutRunError::Operation(error)) => Err(error),
    }
}

pub(in crate::workflows) async fn pending_invitation_by_id_with_timeout(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    invitation_id: &InvitationId,
) -> Result<Option<InvitationInfo>, AcceptInvitationError> {
    Ok(list_pending_invitations_with_timeout(runtime)
        .await?
        .into_iter()
        .find(|invitation| invitation.invitation_id == *invitation_id))
}

pub(in crate::workflows) async fn trigger_runtime_discovery_with_timeout(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
) {
    let budget = match workflow_timeout_budget(
        runtime,
        Duration::from_millis(INVITATION_ACCEPT_LOOKUP_TIMEOUT_MS),
    )
    .await
    {
        Ok(budget) => budget,
        Err(_) => return,
    };

    let _ =
        execute_with_runtime_timeout_budget(runtime, &budget, || runtime.trigger_discovery()).await;
}

pub(in crate::workflows) async fn drive_invitation_accept_convergence(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    peer_hint: Option<AuthorityId>,
) -> Result<(), AcceptInvitationError> {
    let peer_id = peer_hint.map(|peer| peer.to_string());
    let mut converged = false;
    for _ in 0..INVITATION_ACCEPT_CONVERGENCE_ATTEMPTS {
        let step_budget = workflow_timeout_budget(
            runtime,
            Duration::from_millis(INVITATION_ACCEPT_CONVERGENCE_STEP_TIMEOUT_MS),
        )
        .await
        .map_err(|error| AcceptInvitationError::AcceptFailed {
            detail: error.to_string(),
        })?;

        let _ = execute_with_runtime_timeout_budget(runtime, &step_budget, || {
            runtime.process_ceremony_messages()
        })
        .await;
        if let Some(peer_id) = peer_id.as_deref() {
            let _ = execute_with_runtime_timeout_budget(runtime, &step_budget, || {
                runtime.sync_with_peer(peer_id)
            })
            .await;
        }
        let _ =
            execute_with_runtime_timeout_budget(runtime, &step_budget, || runtime.trigger_sync())
                .await;
        converge_runtime(runtime).await;
        let _ = execute_with_runtime_timeout_budget(runtime, &step_budget, || {
            crate::workflows::system::refresh_account(app_core)
        })
        .await;

        if ensure_runtime_peer_connectivity(runtime, "accept_invitation")
            .await
            .is_ok()
        {
            converged = true;
            break;
        }
    }

    if !converged {
        #[cfg(feature = "instrumented")]
        tracing::warn!(
            attempts = INVITATION_ACCEPT_CONVERGENCE_ATTEMPTS,
            "invitation accept convergence exhausted without peer connectivity"
        );
    }

    Ok(())
}

#[cfg(feature = "signals")]
async fn reconcile_accepted_channel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    accepted_channel: &AcceptedChannelInvitationTarget,
    stage_tracker: &WorkflowStageTracker,
) -> Result<(), AuraError> {
    const CHANNEL_CONTEXT_ATTEMPTS: usize = 60;
    const CHANNEL_CONTEXT_BACKOFF_MS: u64 = 100;

    let channel_id = accepted_channel.channel_id;
    let mut authoritative_context = accepted_channel.context_hint;
    if authoritative_context.is_none() {
        update_accept_reconcile_stage(
            stage_tracker,
            "reconcile_channel_invitation:resolve_context",
        );
        let policy = workflow_retry_policy(
            CHANNEL_CONTEXT_ATTEMPTS as u32,
            Duration::from_millis(CHANNEL_CONTEXT_BACKOFF_MS),
            Duration::from_millis(CHANNEL_CONTEXT_BACKOFF_MS),
        )?;
        authoritative_context = Some(
            execute_with_runtime_retry_budget(runtime, &policy, |_attempt| async {
                if let Some(context_id) =
                    crate::workflows::messaging::resolve_authoritative_context_id_for_channel(
                        app_core, channel_id,
                    )
                    .await
                {
                    return Ok(context_id);
                }
                converge_runtime(runtime).await;
                Err(AuraError::from(
                    crate::workflows::error::WorkflowError::Precondition(
                        "Accepted channel invitation but no authoritative context was materialized",
                    ),
                ))
            })
            .await
            .map_err(|error| match error {
                RetryRunError::Timeout(timeout_error) => AuraError::from(timeout_error),
                RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
            })?,
        );
    }
    let authoritative_context = authoritative_context.ok_or_else(|| {
        AuraError::from(crate::workflows::error::WorkflowError::Precondition(
            "Accepted channel invitation but no authoritative context was materialized",
        ))
    })?;
    let authoritative_channel = crate::workflows::messaging::AuthoritativeChannelRef::new(
        channel_id,
        authoritative_context,
    );
    reconcile_accepted_channel_invitation_authoritative(
        app_core,
        runtime,
        authoritative_channel,
        accepted_channel.channel_name_hint.as_deref(),
        stage_tracker,
    )
    .await
}

#[cfg(feature = "signals")]
async fn reconcile_accepted_channel_invitation_authoritative(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    authoritative_channel: crate::workflows::messaging::AuthoritativeChannelRef,
    channel_name_hint: Option<&str>,
    stage_tracker: &WorkflowStageTracker,
) -> Result<(), AuraError> {
    update_accept_reconcile_stage(
        stage_tracker,
        "reconcile_channel_invitation:resolve_local_channel_id",
    );
    let local_channel_id = authoritative_channel.channel_id();
    let authoritative_context = authoritative_channel.context_id();
    update_accept_reconcile_stage(
        stage_tracker,
        "reconcile_channel_invitation:project_channel_peer_membership",
    );
    crate::workflows::messaging::apply_authoritative_membership_projection(
        app_core,
        local_channel_id,
        authoritative_context,
        true,
        channel_name_hint,
    )
    .await?;
    update_accept_reconcile_stage(
        stage_tracker,
        "reconcile_channel_invitation:ensure_runtime_channel_state",
    );
    let mut resolved_runtime_context = None;
    let mut runtime_state_ready =
        crate::workflows::messaging::runtime_channel_state_exists(runtime, authoritative_channel)
            .await?;
    if !runtime_state_ready {
        resolved_runtime_context = timeout_runtime_call(
            runtime,
            "reconcile_accepted_channel_invitation_authoritative",
            "resolve_amp_channel_context",
            INVITATION_RUNTIME_QUERY_TIMEOUT,
            || runtime.resolve_amp_channel_context(local_channel_id),
        )
        .await
        .map_err(|error| super::super::error::runtime_call("resolve channel context", error))
        .map(|result| {
            result.map_err(|error| {
                super::super::error::runtime_call("resolve channel context", error)
            })
        })
        .unwrap_or_else(|_| Ok(None))?;
        runtime_state_ready = resolved_runtime_context == Some(authoritative_context);
    }

    if !runtime_state_ready {
        update_accept_reconcile_stage(
            stage_tracker,
            "reconcile_channel_invitation:amp_join_channel",
        );
        if let Err(error) = timeout_runtime_call(
            runtime,
            "reconcile_accepted_channel_invitation_authoritative",
            "amp_join_channel",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || {
                runtime.amp_join_channel(aura_core::effects::amp::ChannelJoinParams {
                    context: authoritative_context,
                    channel: local_channel_id,
                    participant: runtime.authority_id(),
                })
            },
        )
        .await
        .map_err(|error| {
            super::super::error::runtime_call("accept channel invitation join", error)
        })? {
            if classify_amp_channel_error(&error) != AmpChannelErrorClass::AlreadyExists {
                return Err(super::super::error::runtime_call(
                    "accept channel invitation join",
                    error,
                )
                .into());
            }
        }
        runtime_state_ready = crate::workflows::messaging::runtime_channel_state_exists(
            runtime,
            authoritative_channel,
        )
        .await?;
        if !runtime_state_ready {
            resolved_runtime_context = timeout_runtime_call(
                runtime,
                "reconcile_accepted_channel_invitation_authoritative",
                "resolve_amp_channel_context",
                INVITATION_RUNTIME_QUERY_TIMEOUT,
                || runtime.resolve_amp_channel_context(local_channel_id),
            )
            .await
            .map_err(|error| super::super::error::runtime_call("resolve channel context", error))
            .map(|result| {
                result.map_err(|error| {
                    super::super::error::runtime_call("resolve channel context", error)
                })
            })
            .unwrap_or_else(|_| Ok(None))?;
            runtime_state_ready = resolved_runtime_context == Some(authoritative_context);
        }
    }
    update_accept_reconcile_stage(
        stage_tracker,
        "reconcile_channel_invitation:wait_for_runtime_channel_state",
    );
    if !runtime_state_ready {
        crate::workflows::messaging::wait_for_runtime_channel_state(
            app_core,
            runtime,
            authoritative_channel,
        )
        .await?;
        runtime_state_ready = crate::workflows::messaging::runtime_channel_state_exists(
            runtime,
            authoritative_channel,
        )
        .await?;
        if !runtime_state_ready {
            resolved_runtime_context = timeout_runtime_call(
                runtime,
                "reconcile_accepted_channel_invitation_authoritative",
                "resolve_amp_channel_context",
                INVITATION_RUNTIME_QUERY_TIMEOUT,
                || runtime.resolve_amp_channel_context(local_channel_id),
            )
            .await
            .map_err(|error| super::super::error::runtime_call("resolve channel context", error))
            .map(|result| {
                result.map_err(|error| {
                    super::super::error::runtime_call("resolve channel context", error)
                })
            })
            .unwrap_or_else(|_| Ok(None))?;
        }
    }
    crate::workflows::messaging::publish_authoritative_channel_membership_ready(
        app_core,
        local_channel_id,
        channel_name_hint,
        1,
    )
    .await?;
    if resolved_runtime_context == Some(authoritative_context) {
        update_accept_reconcile_stage(
            stage_tracker,
            "reconcile_channel_invitation:refresh_channel_membership_readiness",
        );
        crate::workflows::messaging::refresh_authoritative_channel_membership_readiness(app_core)
            .await?;
    }
    Ok(())
}

#[cfg(not(feature = "signals"))]
async fn reconcile_accepted_channel_invitation(
    _app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    _accepted_channel: &AcceptedChannelInvitationTarget,
    _stage_tracker: &WorkflowStageTracker,
) -> Result<(), AuraError> {
    converge_runtime(runtime).await;
    Ok(())
}

pub(in crate::workflows) async fn wait_for_contact_link(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    contact_id: AuthorityId,
) -> Result<(), AcceptInvitationError> {
    let policy = workflow_retry_policy(
        CONTACT_LINK_ATTEMPTS as u32,
        Duration::from_millis(CONTACT_LINK_BACKOFF_MS),
        Duration::from_millis(CONTACT_LINK_BACKOFF_MS),
    )
    .map_err(|error| AcceptInvitationError::AcceptFailed {
        detail: error.to_string(),
    })?;
    execute_with_runtime_retry_budget(runtime, &policy, |_attempt| async {
        let linked = contacts_signal_snapshot(app_core)
            .await
            .map_err(|error| AcceptInvitationError::AcceptFailed {
                detail: error.to_string(),
            })?
            .all_contacts()
            .any(|contact| contact.id == contact_id);
        if linked {
            return Ok(());
        }
        converge_runtime(runtime).await;
        Err(AcceptInvitationError::ContactLinkDidNotConverge { contact_id })
    })
    .await
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => AcceptInvitationError::AcceptFailed {
            detail: timeout_error.to_string(),
        },
        RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
    })
}
