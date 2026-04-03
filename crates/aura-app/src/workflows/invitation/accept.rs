use super::*;

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
        OperationId::invitation_cancel(),
        instance_id,
        SemanticOperationKind::CancelInvitation,
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
