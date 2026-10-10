#![allow(missing_docs)]

use super::*;
use crate::ui_contract::{SemanticFailureCode, SemanticFailureDomain, SemanticOperationError};
use crate::workflows::error::WorkflowError;
use thiserror::Error;

#[derive(Debug, Error)]
enum GuardianInvitationCompletionError {
    #[error("guardian invitation {invitation_id} cancelled")]
    Cancelled { invitation_id: InvitationId },
    #[error("guardian invitation {invitation_id} ended: {reason:?}")]
    Failed {
        invitation_id: InvitationId,
        reason: crate::runtime_bridge::CeremonyFailureReason,
    },
    #[error("guardian invitation {invitation_id} confirmation timed out")]
    TimedOut { invitation_id: InvitationId },
}

fn guardian_completion_error(error: GuardianInvitationCompletionError) -> AuraError {
    AuraError::from(Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
}

fn emit_contact_accept_probe(stage: &str) {
    let _ = stage;
}

#[derive(Debug, thiserror::Error)]
#[error("guardian acknowledgment is not yet published")]
struct GuardianAcknowledgmentPending;

async fn await_guardian_invitation_completion(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    invitation_id: &InvitationId,
    owner: &SemanticWorkflowOwner,
    budget: &TimeoutBudget,
) -> Result<(), AuraError> {
    use crate::runtime_bridge::{CeremonyFailureReason, CeremonyTerminalOutcome};
    let policy = match workflow_retry_policy(60, Duration::from_secs(1), Duration::from_secs(1)) {
        Ok(policy) => policy,
        Err(error) => {
            return fail_invitation_accept(
                owner,
                AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                    source: Some(accept_failure_source(error)),
                },
            )
            .await
        }
    };
    let observation =
        execute_with_runtime_retry_budget(runtime, budget, &policy, |_attempt, _child| async {
            match runtime
                .get_guardian_invitation_terminal_outcome(invitation_id)
                .await
            {
                Ok(None) => Err(GuardianAcknowledgmentPending),
                // Required provider failures stop retries, retaining their actual cause.
                result => Ok(result),
            }
        })
        .await;
    let outcome = match observation {
        Ok(Ok(Some(outcome))) => outcome,
        Ok(Ok(None)) => unreachable!("pending acknowledgment is retried"),
        Ok(Err(error)) => {
            owner
                .publish_failure(SemanticOperationError::new(
                    SemanticFailureDomain::Ceremony,
                    SemanticFailureCode::CeremonyRuntimeFailed,
                ))
                .await?;
            return Err(super::super::error::runtime_call("guardian acknowledgment", error).into());
        }
        Err(RetryRunError::Timeout(error)) => {
            return fail_invitation_accept(
                owner,
                AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                    source: Some(error.into()),
                },
            )
            .await
        }
        Err(RetryRunError::AttemptsExhausted { .. }) => {
            owner
                .publish_failure(SemanticOperationError::new(
                    SemanticFailureDomain::Ceremony,
                    SemanticFailureCode::OperationTimedOut,
                ))
                .await?;
            return Err(guardian_completion_error(
                GuardianInvitationCompletionError::TimedOut {
                    invitation_id: invitation_id.clone(),
                },
            ));
        }
    };
    match outcome {
        CeremonyTerminalOutcome::Committed => {
            owner
                .publish_success_with(issue_guardian_invitation_confirmed_proof(
                    invitation_id.clone(),
                ))
                .await
        }
        CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Cancelled) => {
            owner
                .publish_phase(SemanticOperationPhase::Cancelled)
                .await?;
            Err(guardian_completion_error(
                GuardianInvitationCompletionError::Cancelled {
                    invitation_id: invitation_id.clone(),
                },
            ))
        }
        CeremonyTerminalOutcome::Failed(reason) => {
            let code = match reason {
                CeremonyFailureReason::Rejected => SemanticFailureCode::CeremonyRejected,
                CeremonyFailureReason::Cancelled => unreachable!("handled above"),
                CeremonyFailureReason::TimedOut => SemanticFailureCode::OperationTimedOut,
                CeremonyFailureReason::ChoreographyFailed => {
                    SemanticFailureCode::CeremonyChoreographyFailed
                }
                CeremonyFailureReason::RuntimeFailed => SemanticFailureCode::CeremonyRuntimeFailed,
                CeremonyFailureReason::Superseded => SemanticFailureCode::CeremonySuperseded,
            };
            owner
                .publish_failure(SemanticOperationError::new(
                    SemanticFailureDomain::Ceremony,
                    code,
                ))
                .await?;
            Err(guardian_completion_error(
                GuardianInvitationCompletionError::Failed {
                    invitation_id: invitation_id.clone(),
                    reason,
                },
            ))
        }
    }
}

/// Select the actual acceptance kind from retained invitation metadata.
pub fn accept_operation_for_imported_invitation(
    invitation: &InvitationHandle,
) -> Result<(OperationId, SemanticOperationKind), AuraError> {
    let kind = semantic_kind_for_bridge_invitation(invitation.info());
    Ok((accept_operation_id(kind)?, kind))
}

#[aura_macros::semantic_owner(
    owner = "accept_imported_invitation_owned",
    wrapper = "accept_invitation_with_terminal_status",
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
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    invitation: &crate::runtime_bridge::InvitationInfo,
    owner: &SemanticWorkflowOwner,
    budget: &TimeoutBudget,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<(), AuraError> {
    match accept_imported_invitation_inner(app_core, runtime, invitation, owner, budget).await? {
        #[cfg(feature = "signals")]
        Some(channel_id) => {
            let membership_proof = prove_channel_membership_ready(app_core, channel_id).await?;
            owner.publish_success_with(membership_proof).await?;
        }
        #[cfg(not(feature = "signals"))]
        Some(_) => {
            debug_assert!(
                false,
                "channel membership proofs are only issued when the `signals` feature is enabled"
            );
            owner
                .publish_success_with(issue_invitation_accepted_or_materialized_proof(
                    invitation.invitation_id.clone(),
                ))
                .await?;
        }
        None => {
            if owner.kind() == SemanticOperationKind::AcceptGuardianInvitation {
                await_guardian_invitation_completion(
                    runtime,
                    &invitation.invitation_id,
                    owner,
                    budget,
                )
                .await?;
            } else {
                owner
                    .publish_success_with(issue_invitation_accepted_or_materialized_proof(
                        invitation.invitation_id.clone(),
                    ))
                    .await?;
            }
        }
    }
    Ok(())
}

pub(in crate::workflows) async fn accept_imported_invitation_inner(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    invitation: &crate::runtime_bridge::InvitationInfo,
    owner: &SemanticWorkflowOwner,
    accept_budget: &TimeoutBudget,
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
                source: None,
            },
        )
        .await;
    }

    if contact_probe {
        emit_contact_accept_probe("runtime_accept");
    }
    let runtime_accept_budget = match crate::workflows::runtime::workflow_child_timeout_budget(
        runtime,
        accept_budget,
        Duration::from_millis(invitation_accept_runtime_stage_timeout_ms(
            Some(invitation),
            None,
        )),
    )
    .await
    {
        Ok(budget) => budget,
        Err(error) => {
            return fail_invitation_accept(
                owner,
                AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                    source: Some(accept_failure_source(error)),
                },
            )
            .await
        }
    };
    let accept_result =
        execute_with_runtime_timeout_budget(runtime, &runtime_accept_budget, || {
            runtime.accept_invitation(invitation.invitation_id.as_str())
        })
        .await;
    if let Err(error) = accept_result {
        let error = match error {
            TimeoutRunError::Timeout(timeout_error @ TimeoutBudgetError::DeadlineExceeded { .. }) => {
                AcceptInvitationError::AcceptFailed {
                    detail: format!(
                        "accept_imported_invitation timed out in stage runtime_accept_invitation after {}ms",
                        accept_budget.timeout_ms()
                    ),
                source: Some(accept_failure_source(timeout_error)),
}
            }
            TimeoutRunError::Timeout(timeout_error) => AcceptInvitationError::AcceptFailed {
                detail: timeout_error.to_string(),
            source: Some(accept_failure_source(timeout_error)),
},
            TimeoutRunError::Operation(operation_error) => AcceptInvitationError::AcceptFailed {
                detail: operation_error.to_string(),
            source: Some(accept_failure_source(operation_error)),
},
        };
        if classify_invitation_accept_error(&error) != InvitationAcceptErrorClass::AlreadyHandled {
            return fail_invitation_accept(
                owner,
                AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                    source: Some(accept_failure_source(error)),
                },
            )
            .await;
        }
    }

    if contact_probe {
        emit_contact_accept_probe("post_accept_discovery");
    }
    if contact_probe {
        emit_contact_accept_probe("post_accept_convergence");
    }
    let accept_peer = matches!(
        invitation.invitation_type,
        crate::runtime_bridge::InvitationBridgeType::Contact { .. }
            | crate::runtime_bridge::InvitationBridgeType::Channel { .. }
    )
    .then_some(invitation.sender_id);
    if !matches!(
        invitation.invitation_type,
        InvitationBridgeType::Contact { .. }
    ) {
        if let Err(error) =
            drive_invitation_accept_convergence(runtime, accept_peer, accept_budget).await
        {
            return fail_invitation_accept(owner, error).await;
        }
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
                        source: Some(accept_failure_source(error)),
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
                    source: Some(accept_failure_source(error)),
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
                        source: None,
},
                    )
                    .await;
                }
            };
            if let Err(error) = reconcile_channel_invitation_acceptance(
                app_core,
                runtime,
                accept_budget,
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
                    crate::workflows::messaging::refresh_authoritative_channel_membership_readiness_with_budget(
                        app_core, accept_budget,
                        )
                    .await
                {
                    return fail_invitation_accept(
                        owner,
                        AcceptInvitationError::AcceptFailed {
                            detail: error.to_string(),
                            source: Some(accept_failure_source(error)),
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
        crate::runtime_bridge::InvitationBridgeType::DeviceEnrollment { .. } => {
            debug_assert!(
                false,
                "device enrollment invitations should have failed before invitation acceptance dispatch"
            );
        }
    }

    Ok(None)
}

fn accept_operation_id(kind: SemanticOperationKind) -> Result<OperationId, AuraError> {
    match kind {
        SemanticOperationKind::AcceptContactInvitation => {
            Ok(OperationId::invitation_accept_contact())
        }
        SemanticOperationKind::AcceptGuardianInvitation => {
            Ok(OperationId::accept_guardian_invitation())
        }
        SemanticOperationKind::AcceptPendingChannelInvitation => {
            Ok(OperationId::invitation_accept_channel())
        }
        _ => Err(AuraError::invalid(
            "submitted invitation acceptance requires an acceptance operation kind",
        )),
    }
}

async fn canonical_invitation_by_id_with_budget(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    invitation_id: &str,
    budget: &TimeoutBudget,
) -> Result<InvitationInfo, AuraError> {
    list_pending_invitations_with_budget(runtime, budget)
        .await
        .map_err(AuraError::from)?
        .into_iter()
        .find(|invitation| invitation.invitation_id.as_str() == invitation_id)
        .ok_or_else(|| AuraError::not_found("canonical pending invitation"))
}

/// Accept retained metadata, a submitted id, a standalone id, or a Contact code
/// through one original semantic owner and runtime deadline.
pub async fn accept_invitation_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    request: InvitationAcceptanceRequest,
) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationHandle> {
    let (kind, instance) = match &request {
        InvitationAcceptanceRequest::RetainedHandle {
            invitation,
            operation_instance_id,
        } => (
            Some(semantic_kind_for_bridge_invitation(invitation.info())),
            operation_instance_id.clone(),
        ),
        InvitationAcceptanceRequest::SubmittedId {
            operation_kind,
            operation_instance_id,
            ..
        } => (Some(*operation_kind), Some(operation_instance_id.clone())),
        InvitationAcceptanceRequest::ContactCode {
            operation_instance_id,
            ..
        } => (
            Some(SemanticOperationKind::AcceptContactInvitation),
            Some(operation_instance_id.clone()),
        ),
        InvitationAcceptanceRequest::UnsubmittedId { .. } => (None, None),
    };
    let mut owner = match kind {
        Some(kind) => match accept_operation_id(kind) {
            Ok(id) => Some(SemanticWorkflowOwner::new(app_core, id, instance, kind)),
            Err(error) => {
                return crate::ui_contract::WorkflowTerminalOutcome {
                    result: Err(error),
                    terminal: None,
                }
            }
        },
        None => None,
    };
    let result = async {
        if let Some(owner) = &owner {
            publish_invitation_owner_status(
                owner,
                None,
                SemanticOperationPhase::WorkflowDispatched,
            )
            .await?;
        }
        let runtime = require_runtime(app_core).await?;
        let lookup_ms = match &request {
            InvitationAcceptanceRequest::RetainedHandle { .. }
            | InvitationAcceptanceRequest::ContactCode { .. } => 0,
            _ => INVITATION_ACCEPT_LOOKUP_TIMEOUT_MS,
        };
        let stage_ms = match kind {
            Some(SemanticOperationKind::AcceptContactInvitation) => {
                CONTACT_INVITATION_ACCEPT_RUNTIME_STAGE_TIMEOUT_MS
            }
            Some(SemanticOperationKind::AcceptGuardianInvitation) => {
                CHOREOGRAPHY_INVITATION_ACCEPT_RUNTIME_STAGE_TIMEOUT_MS
            }
            _ => {
                CHANNEL_INVITATION_ACCEPT_RUNTIME_STAGE_TIMEOUT_MS
                    + CHANNEL_INVITATION_ACCEPT_RECONCILE_TIMEOUT_MS
            }
        };
        let budget =
            workflow_timeout_budget(&runtime, Duration::from_millis(lookup_ms + stage_ms)).await?;
        let invitation = match request {
            InvitationAcceptanceRequest::RetainedHandle { invitation, .. } => *invitation,
            InvitationAcceptanceRequest::SubmittedId { invitation_id, .. }
            | InvitationAcceptanceRequest::UnsubmittedId { invitation_id } => {
                InvitationHandle::new(
                    canonical_invitation_by_id_with_budget(&runtime, &invitation_id, &budget)
                        .await?,
                )
            }
            InvitationAcceptanceRequest::ContactCode { code, .. } => {
                super::import::import_invitation_details_with_budget(&runtime, &code, &budget)
                    .await?
            }
        };
        let canonical_kind = semantic_kind_for_bridge_invitation(invitation.info());
        if let Some(kind) = kind {
            if kind != canonical_kind {
                return Err(AuraError::invalid(
                    "canonical invitation kind does not match original acceptance submission",
                ));
            }
        }
        let acceptance_owner = match &mut owner {
            Some(owner) => owner,
            slot @ None => {
                let id = accept_operation_id(canonical_kind)?;
                let owner = slot.insert(SemanticWorkflowOwner::new(
                    app_core,
                    id,
                    None,
                    canonical_kind,
                ));
                publish_invitation_owner_status(
                    owner,
                    None,
                    SemanticOperationPhase::WorkflowDispatched,
                )
                .await?;
                owner
            }
        };
        accept_imported_invitation_owned(
            app_core,
            &runtime,
            invitation.info(),
            acceptance_owner,
            &budget,
            None,
        )
        .await?;
        Ok(invitation)
    }
    .await;
    match owner {
        Some(owner) => invitation_acceptance_outcome(&owner, result).await,
        None => crate::ui_contract::WorkflowTerminalOutcome {
            result,
            terminal: None,
        },
    }
}

async fn invitation_acceptance_outcome<T>(
    owner: &SemanticWorkflowOwner,
    result: Result<T, AuraError>,
) -> crate::ui_contract::WorkflowTerminalOutcome<T> {
    if let Err(error) = &result {
        if owner.terminal_status().await.is_none() {
            if let Err(publication_error) = owner
                .publish_failure(super::import::invitation_import_failure(error))
                .await
            {
                return crate::ui_contract::WorkflowTerminalOutcome {
                    result: Err(publication_error),
                    terminal: owner.terminal_status().await,
                };
            }
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

#[derive(Debug, Clone, Error)]
pub(in crate::workflows) enum AcceptInvitationError {
    #[error("No pending channel invitation found")]
    PendingInvitationNotFound,
    #[error("pending invitation is not a channel invitation")]
    PendingInvitationKindMismatch,
    #[error("Failed to accept invitation: {detail}")]
    AcceptFailed {
        detail: String,
        #[source]
        source: Option<AuraError>,
    },
    #[error("accepted contact invitation for {contact_id} but the contact never converged")]
    ContactLinkDidNotConverge { contact_id: AuthorityId },
}

fn accept_failure_source(error: impl std::error::Error + Send + Sync + 'static) -> AuraError {
    AuraError::Internal {
        message: error.to_string(),
        source: Some(Arc::new(error)),
    }
}

fn invitation_failure_is_timeout(error: &AcceptInvitationError) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = current {
        if matches!(
            cause.downcast_ref::<WorkflowError>(),
            Some(WorkflowError::TimedOut { .. })
        ) || matches!(
            cause.downcast_ref::<TimeoutBudgetError>(),
            Some(TimeoutBudgetError::DeadlineExceeded { .. })
        ) {
            return true;
        }
        current = cause.source();
    }
    false
}

impl AcceptInvitationError {
    pub(in crate::workflows) fn semantic_error(
        &self,
        kind: SemanticOperationKind,
    ) -> crate::ui_contract::SemanticOperationError {
        use crate::ui_contract::{
            SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
        };

        use crate::workflows::runtime_error_classification::{
            classify_contact_confirmation_error, ContactConfirmationErrorClass,
        };

        match self {
            Self::PendingInvitationNotFound => SemanticOperationError::new(
                SemanticFailureDomain::Invitation,
                SemanticFailureCode::NotFound,
            ),
            Self::PendingInvitationKindMismatch => SemanticOperationError::new(
                SemanticFailureDomain::Invitation,
                SemanticFailureCode::InvalidState,
            ),
            Self::AcceptFailed { detail, .. } => {
                // An inviter that rejected or never confirmed the acceptance is
                // a typed outcome, not an internal failure.
                let code = match classify_contact_confirmation_error(self) {
                    Some(ContactConfirmationErrorClass::Revoked) => {
                        SemanticFailureCode::InvitationRevoked
                    }
                    Some(ContactConfirmationErrorClass::Expired) => {
                        SemanticFailureCode::InvitationExpired
                    }
                    Some(ContactConfirmationErrorClass::AlreadySettled) => {
                        SemanticFailureCode::InvitationAlreadySettled
                    }
                    Some(ContactConfirmationErrorClass::Unconfirmed) => {
                        SemanticFailureCode::InviterDidNotConfirm
                    }
                    None => {
                        use crate::runtime_bridge::InvitationAcceptFailureReason as Reason;
                        use crate::workflows::runtime_error_classification::invitation_accept_failure_reason;
                        match invitation_accept_failure_reason(self) {
                            Some(Reason::AlreadyAccepted | Reason::AlreadySettled) => {
                                SemanticFailureCode::InvitationAlreadySettled
                            }
                            Some(Reason::Revoked) => SemanticFailureCode::InvitationRevoked,
                            Some(Reason::Expired) => SemanticFailureCode::InvitationExpired,
                            Some(Reason::Unconfirmed) => SemanticFailureCode::InviterDidNotConfirm,
                            Some(Reason::NotFound) => SemanticFailureCode::NotFound,
                            Some(Reason::NotPending) => SemanticFailureCode::InvalidState,
                            Some(Reason::PermissionDenied) => SemanticFailureCode::PermissionDenied,
                            None if invitation_failure_is_timeout(self) => {
                                SemanticFailureCode::OperationTimedOut
                            }
                            None => crate::workflows::runtime_error_classification::native_runtime_failure_code(self).unwrap_or(SemanticFailureCode::InternalError),
                        }
                    }
                };
                SemanticOperationError::new(SemanticFailureDomain::Invitation, code)
                    .with_detail(format!("operation_kind={kind:?}; detail={detail}"))
            }
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
        AuraError::Internal {
            message: error.to_string(),
            source: Some(Arc::new(error)),
        }
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
    budget: &TimeoutBudget,
) -> Result<Option<InvitationInfo>, AuraError> {
    Ok(select_authoritative_pending_home_invitation(
        &list_pending_invitations_with_budget(runtime, budget)
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
    budget: &TimeoutBudget,
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
    execute_with_runtime_retry_budget(runtime, budget, &policy, |_attempt, attempt_budget| {
        let runtime = runtime.clone();
        async move {
            if let Some(invitation) =
                authoritative_pending_home_or_channel_invitation(&runtime, &attempt_budget).await?
            {
                return Ok(invitation);
            }
            converge_runtime(&runtime, &attempt_budget).await?;
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
    budget: &TimeoutBudget,
) -> Result<Option<InvitationInfo>, AuraError> {
    if let Some(invitation) =
        authoritative_pending_home_or_channel_invitation(runtime, budget).await?
    {
        return Ok(Some(invitation));
    }
    #[cfg(feature = "signals")]
    {
        return await_authoritative_pending_home_or_channel_invitation_for_accept(
            app_core, runtime, budget,
        )
        .await;
    }
    #[cfg(not(feature = "signals"))]
    {
        let _ = (app_core, budget);
        Ok(None)
    }
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

#[allow(dead_code)]
// Channel acceptance reconciliation threads this metadata through
// target-dependent follow-up stages that strict all-target dead-code analysis
// does not model consistently.
struct AcceptedChannelInvitationTarget {
    channel_id: ChannelId,
    context_hint: Option<ContextId>,
    channel_name_hint: Option<String>,
    /// Authority that sent the invitation (the channel's creator side).
    inviter: Option<AuthorityId>,
}

pub(in crate::workflows) async fn reconcile_channel_invitation_acceptance(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    parent_budget: &TimeoutBudget,
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
        inviter: pending_runtime_invitation
            .map(|invitation| invitation.sender_id)
            .or_else(|| accepted_invitation.map(|invitation| invitation.from_id)),
    };
    let stage_tracker = new_workflow_stage_tracker("reconcile_channel_invitation:start");
    let reconcile_budget = match crate::workflows::runtime::workflow_child_timeout_budget(
        runtime,
        parent_budget,
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
                source: Some(accept_failure_source(error)),
            });
        }
    };

    let reconcile_result = execute_with_runtime_timeout_budget(runtime, &reconcile_budget, || {
        reconcile_accepted_channel_invitation(
            app_core,
            runtime,
            &accepted_channel,
            &stage_tracker,
            &reconcile_budget,
        )
    })
    .await;

    match reconcile_result {
        Ok(()) => Ok(()),
        Err(error) => {
            let detail = match &error {
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
            let source = match error {
                TimeoutRunError::Timeout(error) => accept_failure_source(error),
                TimeoutRunError::Operation(error) => accept_failure_source(error),
            };
            Err(AcceptInvitationError::AcceptFailed {
                detail,
                source: Some(source),
            })
        }
    }
}

async fn list_pending_invitations_with_budget(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    parent: &TimeoutBudget,
) -> Result<Vec<InvitationInfo>, AcceptInvitationError> {
    let budget = crate::workflows::runtime::workflow_child_timeout_budget(
        runtime,
        parent,
        Duration::from_millis(INVITATION_ACCEPT_LOOKUP_TIMEOUT_MS),
    )
    .await
    .map_err(|error| AcceptInvitationError::AcceptFailed {
        detail: error.to_string(),
        source: Some(accept_failure_source(error)),
    })?;
    match execute_with_runtime_timeout_budget(runtime, &budget, || async {
        runtime
            .try_list_pending_invitations()
            .await
            .map_err(|error| AcceptInvitationError::AcceptFailed {
                detail: error.to_string(),
                source: Some(accept_failure_source(error)),
            })
    })
    .await
    {
        Ok(pending) => Ok(pending),
        Err(TimeoutRunError::Timeout(error)) => Err(AcceptInvitationError::AcceptFailed {
            detail: error.to_string(),
            source: Some(accept_failure_source(error)),
        }),
        Err(TimeoutRunError::Operation(error)) => Err(error),
    }
}

pub(in crate::workflows) async fn drive_invitation_accept_convergence(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    peer_hint: Option<AuthorityId>,
    budget: &TimeoutBudget,
) -> Result<(), AcceptInvitationError> {
    let result: Result<(), AuraError> = async {
        if let Some(peer) = peer_hint {
            let peer_id = peer.to_string();
            let child = crate::workflows::runtime::workflow_child_timeout_budget(
                runtime,
                budget,
                Duration::from_millis(INVITATION_ACCEPT_CONVERGENCE_STEP_TIMEOUT_MS),
            )
            .await?;
            // This targeted warmup is optional; canonical sync/state below
            // remains required. A failed timer still terminates this owner.
            match execute_with_runtime_timeout_budget(runtime, &child, || {
                runtime.sync_with_peer(&peer_id)
            })
            .await
            {
                Ok(()) => {}
                Err(TimeoutRunError::Timeout(error)) => return Err(error.into()),
                Err(TimeoutRunError::Operation(error)) => {
                    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&error);
                    while let Some(source) = cause {
                        if let Some(timeout) = source.downcast_ref::<TimeoutBudgetError>() {
                            return Err(timeout.clone().into());
                        }
                        cause = source.source();
                    }
                    tracing::debug!(peer = %peer_id, error = %error, "optional invitation peer warmup failed");
                }
            }
        }
        crate::workflows::runtime::converge_runtime(runtime, budget).await?;
        execute_with_runtime_timeout_budget(runtime, budget, || {
            ensure_runtime_peer_connectivity(runtime, "accept_invitation")
        })
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(error) => AuraError::from(error),
            TimeoutRunError::Operation(error) => error,
        })
    }
    .await;
    result.map_err(|error| AcceptInvitationError::AcceptFailed {
        detail: error.to_string(),
        source: Some(error),
    })
}

#[cfg(feature = "signals")]
async fn reconcile_accepted_channel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    accepted_channel: &AcceptedChannelInvitationTarget,
    stage_tracker: &WorkflowStageTracker,
    budget: &TimeoutBudget,
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
        authoritative_context =
            Some(
                execute_with_runtime_retry_budget(
                    runtime,
                    budget,
                    &policy,
                    |_attempt, attempt_budget| async move {
                        if let Some(context_id) =
                        crate::workflows::messaging::resolve_authoritative_context_id_for_channel(
                            app_core, channel_id, &attempt_budget,
                        )
                        .await?
                    {
                        return Ok(context_id);
                    }
                        converge_runtime(runtime, &attempt_budget).await?;
                        Err(AuraError::from(
                    crate::workflows::error::WorkflowError::Precondition(
                        "Accepted channel invitation but no authoritative context was materialized",
                    ),
                ))
                    },
                )
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
        accepted_channel.inviter,
        stage_tracker,
        budget,
    )
    .await
}

#[cfg(feature = "signals")]
async fn reconcile_accepted_channel_invitation_authoritative(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    authoritative_channel: crate::workflows::messaging::AuthoritativeChannelRef,
    channel_name_hint: Option<&str>,
    inviter: Option<AuthorityId>,
    stage_tracker: &WorkflowStageTracker,
    budget: &TimeoutBudget,
) -> Result<(), AuraError> {
    update_accept_reconcile_stage(
        stage_tracker,
        "reconcile_channel_invitation:resolve_local_channel_id",
    );
    let local_channel_id = authoritative_channel.channel_id();
    let authoritative_context = authoritative_channel.context_id();
    update_accept_reconcile_stage(
        stage_tracker,
        "reconcile_channel_invitation:ensure_runtime_channel_state",
    );
    let runtime_state_ready = crate::workflows::messaging::runtime_channel_state_exists(
        runtime,
        authoritative_channel,
        budget,
    )
    .await?;
    if !runtime_state_ready {
        update_accept_reconcile_stage(
            stage_tracker,
            "reconcile_channel_invitation:amp_join_channel",
        );
        let join_budget = crate::workflows::runtime::workflow_child_timeout_budget(
            runtime,
            budget,
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
        )
        .await?;
        execute_with_runtime_timeout_budget(runtime, &join_budget, || {
            runtime.amp_join_channel(aura_core::effects::amp::ChannelJoinParams {
                context: authoritative_context,
                channel: local_channel_id,
                participant: runtime.authority_id(),
            })
        })
        .await
        .map_err(|error| {
            super::super::error::runtime_call("accept channel invitation join", error)
        })?;
        update_accept_reconcile_stage(
            stage_tracker,
            "reconcile_channel_invitation:wait_for_runtime_channel_state",
        );
        crate::workflows::messaging::wait_for_runtime_channel_state(
            app_core,
            runtime,
            authoritative_channel,
            budget,
        )
        .await?;
    }
    update_accept_reconcile_stage(
        stage_tracker,
        "reconcile_channel_invitation:project_channel_peer_membership",
    );
    crate::workflows::messaging::apply_authoritative_membership_projection_with_budget(
        app_core,
        local_channel_id,
        authoritative_context,
        true,
        channel_name_hint,
        budget,
    )
    .await?;
    // Commit the channel after joining: a rejoin after leaving only lists it
    // once our membership is restored.
    update_accept_reconcile_stage(
        stage_tracker,
        "reconcile_channel_invitation:materialize_channel",
    );
    materialize_accepted_channel(
        app_core,
        runtime,
        authoritative_channel,
        channel_name_hint,
        inviter,
        budget,
    )
    .await?;
    update_accept_reconcile_stage(
        stage_tracker,
        "reconcile_channel_invitation:refresh_channel_membership_readiness",
    );
    crate::workflows::messaging::refresh_authoritative_channel_readiness_for_channel(
        app_core,
        authoritative_channel,
        budget,
    )
    .await?;
    Ok(())
}

#[cfg(not(feature = "signals"))]
async fn reconcile_accepted_channel_invitation(
    _app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    _accepted_channel: &AcceptedChannelInvitationTarget,
    _stage_tracker: &WorkflowStageTracker,
    budget: &TimeoutBudget,
) -> Result<(), AuraError> {
    converge_runtime(runtime, budget).await
}

pub(in crate::workflows) async fn wait_for_contact_link(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    contact_id: AuthorityId,
    budget: &TimeoutBudget,
) -> Result<(), AcceptInvitationError> {
    let policy = workflow_retry_policy(
        CONTACT_LINK_ATTEMPTS as u32,
        Duration::from_millis(CONTACT_LINK_BACKOFF_MS),
        Duration::from_millis(CONTACT_LINK_BACKOFF_MS),
    )
    .map_err(|error| AcceptInvitationError::AcceptFailed {
        detail: error.to_string(),
        source: Some(accept_failure_source(error)),
    })?;
    execute_with_runtime_retry_budget(
        runtime,
        budget,
        &policy,
        |_attempt, attempt_budget| async move {
            let linked = contacts_signal_snapshot(app_core)
                .await
                .map_err(|error| AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                    source: Some(accept_failure_source(error)),
                })?
                .all_contacts()
                .any(|contact| contact.id == contact_id);
            if linked {
                return Ok(());
            }
            converge_runtime(runtime, &attempt_budget)
                .await
                .map_err(|error| AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                    source: Some(error),
                })?;
            Err(AcceptInvitationError::ContactLinkDidNotConverge { contact_id })
        },
    )
    .await
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => AcceptInvitationError::AcceptFailed {
            detail: timeout_error.to_string(),
            source: Some(accept_failure_source(timeout_error)),
        },
        RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
    })
}

/// Commit the accepted channel into the local journal so the chat projection lists it without waiting for the creator's
/// channel fact to arrive. The fact is attributed to the inviter, who created
/// the channel; it carries only metadata from the signed invitation.
#[cfg(feature = "signals")]
async fn materialize_accepted_channel(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    authoritative_channel: crate::workflows::messaging::AuthoritativeChannelRef,
    channel_name_hint: Option<&str>,
    inviter: Option<AuthorityId>,
    budget: &TimeoutBudget,
) -> Result<(), AuraError> {
    let Some(inviter) = inviter else {
        return Ok(());
    };
    let channel_id = authoritative_channel.channel_id();
    // Always commit: the chat signal may already list the channel as an
    // observed-only entry that the runtime view would later drop. The fact is
    // idempotent by channel id.
    let created_at_ms = crate::workflows::time::current_time_ms(app_core).await?;
    let fact = aura_chat::ChatFact::channel_created_ms(
        authoritative_channel.context_id(),
        channel_id,
        channel_name_hint
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("channel")
            .to_string(),
        None,
        false,
        created_at_ms,
        inviter,
    );
    use aura_journal::DomainFact as _;
    let generic = fact.to_generic();
    timeout_runtime_call_with_budget(
        runtime,
        budget,
        "materialize_accepted_channel",
        "commit_relational_facts",
        INVITATION_RUNTIME_QUERY_TIMEOUT,
        || runtime.commit_relational_facts(std::slice::from_ref(&generic)),
    )
    .await
    .map_err(|error| {
        AuraError::from(super::super::error::runtime_call(
            "materialize channel",
            error,
        ))
    })?
    .map_err(|error| {
        AuraError::from(super::super::error::runtime_call(
            "materialize channel",
            error,
        ))
    })?;
    crate::workflows::observed_projection::reduce_chat_fact_observed(app_core, &fact).await
}

#[cfg(all(test, feature = "signals", not(target_arch = "wasm32")))]
#[allow(clippy::expect_used)]
mod contact_code_owner_tests {
    use super::*;
    use crate::core::IntentError;
    use crate::runtime_bridge::{OfflineRuntimeBridge, RuntimeBridge};
    use crate::ui_contract::SemanticFailureCode;
    use crate::workflows::signals::emit_signal;
    use futures::FutureExt;

    async fn fixture() -> (
        Arc<aura_testkit::time::ManualPhysicalClock>,
        Arc<OfflineRuntimeBridge>,
        Arc<RwLock<AppCore>>,
    ) {
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(100));
        let mut runtime = OfflineRuntimeBridge::new(AuthorityId::new_from_entropy([71; 32]));
        runtime.use_time_provider(clock.clone());
        let runtime = Arc::new(runtime);
        let core = AppCore::with_runtime(crate::AppConfig::default(), runtime.clone())
            .expect("fixture runtime attaches");
        crate::signal_defs::register_app_signals(&core)
            .await
            .expect("fixture signals register");
        (clock, runtime, Arc::new(RwLock::new(core)))
    }

    fn invitation(kind: InvitationBridgeType) -> InvitationInfo {
        InvitationInfo {
            invitation_id: InvitationId::new("verified-contact-code"),
            sender_id: AuthorityId::new_from_entropy([72; 32]),
            receiver_id: AuthorityId::new_from_entropy([71; 32]),
            invitation_type: kind,
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 100,
            expires_at_ms: None,
            message: None,
            receiver_nickname: None,
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("original acceptance clock fault")]
    struct AcceptanceClockFault;

    fn source_contains<T: std::error::Error + 'static>(error: &AuraError) -> bool {
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
        while let Some(error) = source {
            if error.downcast_ref::<T>().is_some() {
                return true;
            }
            source = error.source();
        }
        false
    }

    async fn assert_original_failed_instance(
        app: &Arc<RwLock<AppCore>>,
        instance: &OperationInstanceId,
        kind: SemanticOperationKind,
    ) {
        let facts = read_signal_or_default(
            app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        assert!(facts.iter().any(|fact| matches!(fact,
            AuthoritativeSemanticFact::OperationStatus { instance_id: Some(id), causality: Some(_), status, .. }
                if id == instance && status.kind == kind && status.phase == SemanticOperationPhase::Failed)));
    }

    #[tokio::test]
    async fn retained_handle_initial_clock_fault_settles_original_instance_without_accepting() {
        let (_, runtime, app) = fixture().await;
        runtime.queue_clock_answers(vec![Err(
            crate::runtime_bridge::RuntimeBridgeError::with_source(
                IntentError::service_error("acceptance clock unavailable"),
                AcceptanceClockFault,
            ),
        )]);
        let instance = OperationInstanceId("retained-handle-clock-fault".into());
        let outcome = accept_invitation_with_terminal_status(
            &app,
            InvitationAcceptanceRequest::RetainedHandle {
                invitation: Box::new(InvitationHandle::new(invitation(
                    InvitationBridgeType::Contact { nickname: None },
                ))),
                operation_instance_id: Some(instance.clone()),
            },
        )
        .await;
        let error = outcome.result.unwrap_err();
        assert!(source_contains::<AcceptanceClockFault>(&error), "{error}");
        let terminal = outcome
            .terminal
            .expect("original owner must settle clock failure");
        assert_original_failed_instance(&app, &instance, terminal.status.kind).await;
        assert_eq!(
            terminal.status.kind,
            SemanticOperationKind::AcceptContactInvitation
        );
        assert!(terminal.status.error.is_some());
        assert_eq!(runtime.accept_invitation_call_count(), 0);
    }

    #[tokio::test]
    async fn submitted_id_lookup_fault_preserves_original_owner_and_provider_cause() {
        let (_, runtime, app) = fixture().await;
        let instance = OperationInstanceId("submitted-id-query-fault".into());
        let outcome = accept_invitation_with_terminal_status(
            &app,
            InvitationAcceptanceRequest::SubmittedId {
                invitation_id: "unavailable".into(),
                operation_instance_id: instance.clone(),
                operation_kind: SemanticOperationKind::AcceptGuardianInvitation,
            },
        )
        .await;
        let error = outcome.result.unwrap_err();
        assert!(source_contains::<IntentError>(&error), "{error}");
        let terminal = outcome
            .terminal
            .expect("original submitted owner must fail");
        assert_original_failed_instance(&app, &instance, terminal.status.kind).await;
        assert_eq!(
            terminal.status.kind,
            SemanticOperationKind::AcceptGuardianInvitation
        );
        assert!(terminal.status.error.is_some());
        assert_eq!(runtime.accept_invitation_call_count(), 0);
    }

    #[tokio::test]
    async fn submitted_id_kind_mismatch_fails_original_owner_before_mutation() {
        let (_, runtime, app) = fixture().await;
        runtime.set_pending_invitations(vec![invitation(InvitationBridgeType::Guardian {
            subject_authority: AuthorityId::new_from_entropy([72; 32]),
        })]);
        let instance = OperationInstanceId("submitted-id-wrong-kind".into());
        let outcome = accept_invitation_with_terminal_status(
            &app,
            InvitationAcceptanceRequest::SubmittedId {
                invitation_id: "verified-contact-code".into(),
                operation_instance_id: instance.clone(),
                operation_kind: SemanticOperationKind::AcceptContactInvitation,
            },
        )
        .await;
        assert!(outcome.result.is_err());
        let terminal = outcome
            .terminal
            .expect("original submitted owner must fail");
        assert_original_failed_instance(&app, &instance, terminal.status.kind).await;
        assert_eq!(
            terminal.status.kind,
            SemanticOperationKind::AcceptContactInvitation
        );
        assert_eq!(
            terminal.status.error.unwrap().code,
            SemanticFailureCode::InvalidArgument
        );
        assert_eq!(runtime.accept_invitation_call_count(), 0);
    }

    #[tokio::test]
    async fn accepted_signal_history_cannot_authorize_submitted_acceptance() {
        let (_, runtime, app) = fixture().await;
        runtime.set_pending_invitations(Vec::new());
        emit_signal(
            &app,
            &*crate::signal_defs::INVITATIONS_SIGNAL,
            crate::views::invitations::InvitationsState::from_parts(
                Vec::new(),
                Vec::new(),
                vec![crate::views::invitations::Invitation {
                    id: "accepted-history-only".into(),
                    invitation_type: crate::views::invitations::InvitationType::Contact,
                    status: crate::views::invitations::InvitationStatus::Accepted,
                    direction: crate::views::invitations::InvitationDirection::Received,
                    from_id: AuthorityId::new_from_entropy([72; 32]),
                    from_name: "sender".into(),
                    to_id: None,
                    to_name: None,
                    created_at: 100,
                    expires_at: None,
                    message: None,
                    home_id: None,
                    home_name: None,
                }],
            ),
            "invitations",
        )
        .await
        .unwrap();
        let instance = OperationInstanceId("history-is-not-authority".into());
        let outcome = accept_invitation_with_terminal_status(
            &app,
            InvitationAcceptanceRequest::SubmittedId {
                invitation_id: "accepted-history-only".into(),
                operation_instance_id: instance.clone(),
                operation_kind: SemanticOperationKind::AcceptContactInvitation,
            },
        )
        .await;
        assert!(outcome.result.is_err());
        assert_original_failed_instance(
            &app,
            &instance,
            SemanticOperationKind::AcceptContactInvitation,
        )
        .await;
        assert_eq!(runtime.accept_invitation_call_count(), 0);
        assert_eq!(list_invitations(&app).await.all_history().len(), 1);
    }

    #[tokio::test]
    async fn guardian_acknowledgment_retries_on_original_virtual_endpoint() {
        let (clock, bridge, app) = fixture().await;
        bridge.queue_guardian_outcome_answers(vec![
            futures::future::ready(Ok(None)).boxed(),
            futures::future::ready(Ok(Some(
                crate::runtime_bridge::CeremonyTerminalOutcome::Committed,
            )))
            .boxed(),
        ]);
        let runtime: Arc<dyn RuntimeBridge> = bridge.clone();
        let owner = SemanticWorkflowOwner::new(
            &app,
            OperationId::accept_guardian_invitation(),
            Some(OperationInstanceId("guardian-original-window".into())),
            SemanticOperationKind::AcceptGuardianInvitation,
        );
        let budget = TimeoutBudget::from_start_and_timeout(
            &aura_core::time::PhysicalTime::exact(100),
            Duration::from_secs(2),
        )
        .unwrap();
        let id = InvitationId::new("guardian-ack");
        let workflow = await_guardian_invitation_completion(&runtime, &id, &owner, &budget);
        futures::pin_mut!(workflow);
        assert!(futures::poll!(workflow.as_mut()).is_pending());
        assert_eq!(bridge.guardian_outcome_call_count(), 1);
        clock.advance(1_000);
        workflow.await.unwrap();
        assert_eq!(bridge.guardian_outcome_call_count(), 2);
        assert_eq!(clock.now_ms(), 1_100);
        assert_eq!(
            owner.terminal_status().await.unwrap().status.phase,
            SemanticOperationPhase::Succeeded
        );
    }

    #[tokio::test]
    async fn guardian_acknowledgment_cannot_restart_exhausted_owner_window() {
        let (clock, bridge, app) = fixture().await;
        let runtime: Arc<dyn RuntimeBridge> = bridge.clone();
        let owner = SemanticWorkflowOwner::new(
            &app,
            OperationId::accept_guardian_invitation(),
            Some(OperationInstanceId("guardian-exhausted-window".into())),
            SemanticOperationKind::AcceptGuardianInvitation,
        );
        let budget = TimeoutBudget::from_start_and_timeout(
            &aura_core::time::PhysicalTime::exact(100),
            Duration::from_secs(2),
        )
        .unwrap();
        clock.advance(2_000);
        assert!(await_guardian_invitation_completion(
            &runtime,
            &InvitationId::new("guardian-ack"),
            &owner,
            &budget
        )
        .await
        .is_err());
        assert_eq!(bridge.guardian_outcome_call_count(), 0);
        assert_eq!(
            owner.terminal_status().await.unwrap().status.phase,
            SemanticOperationPhase::Failed
        );
    }

    #[tokio::test]
    async fn guardian_acknowledgment_provider_fault_is_not_retried() {
        let (_, bridge, app) = fixture().await;
        bridge.queue_guardian_outcome_answers(vec![
            futures::future::ready(Err(IntentError::no_agent(
                "original guardian provider fault",
            )))
            .boxed(),
            futures::future::ready(Ok(Some(
                crate::runtime_bridge::CeremonyTerminalOutcome::Committed,
            )))
            .boxed(),
        ]);
        let runtime: Arc<dyn RuntimeBridge> = bridge.clone();
        let owner = SemanticWorkflowOwner::new(
            &app,
            OperationId::accept_guardian_invitation(),
            Some(OperationInstanceId("guardian-provider-fault".into())),
            SemanticOperationKind::AcceptGuardianInvitation,
        );
        let budget = TimeoutBudget::from_start_and_timeout(
            &aura_core::time::PhysicalTime::exact(100),
            Duration::from_secs(2),
        )
        .unwrap();
        let error = await_guardian_invitation_completion(
            &runtime,
            &InvitationId::new("guardian-ack"),
            &owner,
            &budget,
        )
        .await
        .unwrap_err();
        assert!(source_contains::<IntentError>(&error), "{error}");
        assert_eq!(bridge.guardian_outcome_call_count(), 1);
        assert_eq!(
            owner.terminal_status().await.unwrap().status.phase,
            SemanticOperationPhase::Failed
        );
    }

    #[tokio::test]
    async fn matching_context_cannot_replace_missing_amp_state_or_hide_join_failure() {
        let (clock, runtime, app) = fixture().await;
        let channel_id = ChannelId::from_bytes([91; 32]);
        let context_id = ContextId::new_from_entropy([92; 32]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_canonical_channel_created_fact(aura_chat::ChatFact::channel_created_ms(
            context_id,
            channel_id,
            "canonical-state-required".into(),
            None,
            false,
            100,
            runtime.authority_id(),
        ));
        runtime.set_amp_channel_state_exists_without_resolution(context_id, channel_id, false);
        runtime.set_amp_channel_participants_without_resolution(
            context_id,
            channel_id,
            vec![runtime.authority_id()],
        );
        runtime.queue_amp_join_results(vec![Err(IntentError::validation_failed(
            "required canonical join provider rejected admission",
        )
        .into())]);
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime.clone();
        let budget = TimeoutBudget::from_start_and_timeout(
            &aura_core::time::PhysicalTime::exact(100),
            Duration::from_secs(30),
        )
        .unwrap();
        let result = reconcile_accepted_channel_invitation_authoritative(
            &app,
            &runtime_bridge,
            crate::workflows::messaging::AuthoritativeChannelRef::new(channel_id, context_id),
            Some("canonical-state-required"),
            None,
            &new_workflow_stage_tracker("test"),
            &budget,
        )
        .await;
        let error = result.expect_err("matching context is not an AMP-state witness");
        assert_eq!(runtime.amp_join_call_count(), 1);
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        let mut retained_provider = false;
        while let Some(error) = source {
            retained_provider |= error.downcast_ref::<IntentError>().is_some();
            source = error.source();
        }
        assert!(
            retained_provider,
            "original join provider error must survive: {error}"
        );
        let facts = read_signal_or_default(
            &app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        assert!(!facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::ChannelMembershipReady { .. }
        )));
        assert_eq!(clock.now_ms(), 100);
    }

    #[tokio::test]
    async fn pending_code_import_retains_original_acceptance_owner_and_typed_failure() {
        let (clock, runtime, app) = fixture().await;
        let (entered, mut started) = futures::channel::oneshot::channel();
        let (complete, result) = futures::channel::oneshot::channel();
        runtime.queue_import_answers(vec![async move {
            entered.send(()).unwrap();
            result.await.unwrap()
        }
        .boxed()]);
        let instance = OperationInstanceId("original-contact-acceptance".into());
        let workflow = super::super::handoff::accept_contact_invitation_from_code(
            &app,
            super::super::handoff::AcceptContactInvitationFromCodeRequest {
                code: "untrusted-code".into(),
                operation_instance_id: instance.clone(),
            },
        );
        futures::pin_mut!(workflow);
        assert!(futures::poll!(workflow.as_mut()).is_pending());
        assert_eq!(started.try_recv().unwrap(), Some(()));
        let facts = read_signal_or_default(
            &app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        assert!(facts.iter().any(|fact| matches!(fact,
            AuthoritativeSemanticFact::OperationStatus { operation_id, instance_id: Some(id), causality: Some(_), status }
            if *operation_id == OperationId::invitation_accept_contact() && *id == instance
                && status.kind == SemanticOperationKind::AcceptContactInvitation
                && status.phase == SemanticOperationPhase::WorkflowDispatched
        )));
        assert!(!facts.iter().any(|fact| matches!(fact,
            AuthoritativeSemanticFact::OperationStatus { operation_id, .. }
            if *operation_id == OperationId::invitation_import()
        )));
        complete
            .send(Err(IntentError::ValidationFailed {
                reason: "invalid Contact code".into(),
            }))
            .unwrap();
        let outcome = workflow.await;
        let error = outcome.result.unwrap_err();
        assert!(error.to_string().contains("invalid Contact code"));
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        let mut original_validation = false;
        while let Some(error) = source {
            original_validation |= matches!(
                error.downcast_ref::<IntentError>(),
                Some(IntentError::ValidationFailed { .. })
            );
            source = error.source();
        }
        assert!(
            original_validation,
            "import lost its concrete validation cause"
        );
        let terminal = outcome.terminal.unwrap();
        assert!(terminal.causality.is_some());
        assert_eq!(
            terminal.status.kind,
            SemanticOperationKind::AcceptContactInvitation
        );
        assert_eq!(terminal.status.phase, SemanticOperationPhase::Failed);
        assert_eq!(
            terminal.status.error.unwrap().code,
            SemanticFailureCode::InvalidArgument
        );
        assert_eq!(runtime.accept_invitation_call_count(), 0);
        assert_eq!(clock.now_ms(), 100);
    }

    #[tokio::test]
    async fn verified_non_contact_codes_fail_before_any_acceptance() {
        let kinds = [
            InvitationBridgeType::Guardian {
                subject_authority: AuthorityId::new_from_entropy([72; 32]),
            },
            InvitationBridgeType::Channel {
                home_id: "home".into(),
                context_id: None,
                nickname_suggestion: None,
            },
            InvitationBridgeType::DeviceEnrollment {
                subject_authority: AuthorityId::new_from_entropy([72; 32]),
                initiator_device_id: aura_core::DeviceId::new_from_entropy([73; 32]),
                device_id: aura_core::DeviceId::new_from_entropy([74; 32]),
                nickname_suggestion: None,
                ceremony_id: aura_core::CeremonyId::new("wrong-kind-enrollment"),
                pending_epoch: aura_core::Epoch(1),
            },
        ];
        for kind in kinds {
            let (_, runtime, app) = fixture().await;
            runtime.queue_import_answers(vec![async move { Ok(invitation(kind)) }.boxed()]);
            let outcome = accept_invitation_with_terminal_status(
                &app,
                InvitationAcceptanceRequest::ContactCode {
                    code: ("verified-non-contact").to_owned(),
                    operation_instance_id: OperationInstanceId("wrong-kind".into()),
                },
            )
            .await;
            assert!(matches!(outcome.result, Err(AuraError::Invalid { .. })));
            let terminal = outcome.terminal.unwrap();
            assert_eq!(
                terminal.status.kind,
                SemanticOperationKind::AcceptContactInvitation
            );
            assert_eq!(
                terminal.status.error.unwrap().code,
                SemanticFailureCode::InvalidArgument
            );
            assert_eq!(runtime.accept_invitation_call_count(), 0);
        }
    }

    #[tokio::test]
    async fn acceptance_after_pending_import_uses_original_remaining_endpoint() {
        let (clock, bridge, app) = fixture().await;
        let (complete, imported) = futures::channel::oneshot::channel();
        bridge.queue_import_answers(vec![async move {
            imported.await.unwrap();
            Ok(invitation(InvitationBridgeType::Contact { nickname: None }))
        }
        .boxed()]);
        bridge.queue_accept_answers(vec![futures::future::pending().boxed()]);
        let workflow = accept_invitation_with_terminal_status(
            &app,
            InvitationAcceptanceRequest::ContactCode {
                code: "pending-contact".into(),
                operation_instance_id: OperationInstanceId("original-window".into()),
            },
        );
        futures::pin_mut!(workflow);
        assert!(futures::poll!(workflow.as_mut()).is_pending());
        clock.advance(25_000);
        complete.send(()).unwrap();
        assert!(futures::poll!(workflow.as_mut()).is_pending());
        assert_eq!(bridge.accept_invitation_call_count(), 1);
        clock.advance(14_999);
        assert!(futures::poll!(workflow.as_mut()).is_pending());
        clock.advance(1);
        let outcome = workflow.await;
        let error = outcome.result.unwrap_err();
        let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        let mut original_deadline = None;
        while let Some(source) = cause {
            if let Some(TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms,
                observed_at_ms,
            }) = source.downcast_ref::<TimeoutBudgetError>()
            {
                original_deadline = Some((*deadline_at_ms, *observed_at_ms));
                break;
            }
            cause = source.source();
        }
        assert_eq!(original_deadline, Some((40_100, 40_100)));
        assert_eq!(clock.now_ms(), 40_100);
        let terminal = outcome.terminal.unwrap();
        assert_eq!(
            terminal.status.error.unwrap().code,
            SemanticFailureCode::OperationTimedOut
        );
    }
}

#[cfg(test)]
mod guardian_operation_tests {
    use super::*;

    #[test]
    fn guardian_invitation_has_a_distinct_accept_owner() {
        let sender = AuthorityId::new_from_entropy([31; 32]);
        let receiver = AuthorityId::new_from_entropy([32; 32]);
        let guardian = InvitationInfo {
            invitation_id: InvitationId::new("guardian-operation"),
            sender_id: sender,
            receiver_id: receiver,
            invitation_type: InvitationBridgeType::Guardian {
                subject_authority: sender,
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
            receiver_nickname: None,
        };
        assert_eq!(
            semantic_kind_for_bridge_invitation(&guardian),
            SemanticOperationKind::AcceptGuardianInvitation
        );
        let imported = InvitationHandle::new(guardian);
        assert_eq!(
            accept_operation_for_imported_invitation(&imported).unwrap(),
            (
                OperationId::accept_guardian_invitation(),
                SemanticOperationKind::AcceptGuardianInvitation,
            )
        );
    }
}

#[cfg(test)]
mod native_failure_tests {
    use super::*;
    use crate::ui_contract::SemanticFailureCode;

    #[test]
    fn pending_selection_failures_have_explicit_semantic_codes_and_typed_causes() {
        use std::error::Error;
        for (failure, expected) in [
            (
                AcceptInvitationError::PendingInvitationNotFound,
                SemanticFailureCode::NotFound,
            ),
            (
                AcceptInvitationError::PendingInvitationKindMismatch,
                SemanticFailureCode::InvalidState,
            ),
        ] {
            assert_eq!(
                failure
                    .semantic_error(SemanticOperationKind::AcceptPendingChannelInvitation)
                    .code,
                expected
            );
            let returned = AuraError::from(failure);
            let cause = returned
                .source()
                .unwrap()
                .downcast_ref::<AcceptInvitationError>()
                .unwrap();
            assert_eq!(
                cause
                    .semantic_error(SemanticOperationKind::AcceptPendingChannelInvitation)
                    .code,
                expected
            );
        }
    }

    #[test]
    fn acceptance_owner_retains_native_reason_and_original_source() {
        use crate::runtime_bridge::{InvitationAcceptFailureReason as R, RuntimeBridgeError};
        use std::error::Error;
        for (reason, code) in [
            (
                R::AlreadyAccepted,
                SemanticFailureCode::InvitationAlreadySettled,
            ),
            (R::Revoked, SemanticFailureCode::InvitationRevoked),
            (R::Expired, SemanticFailureCode::InvitationExpired),
            (
                R::AlreadySettled,
                SemanticFailureCode::InvitationAlreadySettled,
            ),
            (R::Unconfirmed, SemanticFailureCode::InviterDidNotConfirm),
            (R::NotFound, SemanticFailureCode::NotFound),
            (R::NotPending, SemanticFailureCode::InvalidState),
            (R::PermissionDenied, SemanticFailureCode::PermissionDenied),
        ] {
            let native = RuntimeBridgeError::with_source(
                crate::core::IntentError::internal_error("unrelated diagnostic"),
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "actual original"),
            )
            .with_invitation_accept_reason(reason);
            let failure = AcceptInvitationError::AcceptFailed {
                detail: native.to_string(),
                source: Some(accept_failure_source(native)),
            };
            assert_eq!(
                failure
                    .semantic_error(SemanticOperationKind::AcceptContactInvitation)
                    .code,
                code
            );
            let returned = AuraError::from(failure);
            let original = returned
                .source()
                .unwrap()
                .downcast_ref::<AcceptInvitationError>()
                .unwrap();
            let context = original
                .source()
                .unwrap()
                .downcast_ref::<AuraError>()
                .unwrap();
            let native = context
                .source()
                .unwrap()
                .downcast_ref::<RuntimeBridgeError>()
                .unwrap();
            assert_eq!(native.invitation_accept_reason(), Some(reason));
            assert_eq!(
                native
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
    }

    #[test]
    fn acceptance_owner_rejects_textual_lookalikes_and_maps_real_deadlines() {
        let failure = AcceptInvitationError::AcceptFailed {
            detail: "invitation already accepted; inviter revoked this contact invitation".into(),
            source: None,
        };
        assert_eq!(
            classify_invitation_accept_error(&failure),
            InvitationAcceptErrorClass::Other
        );
        assert_eq!(
            failure
                .semantic_error(SemanticOperationKind::AcceptContactInvitation)
                .code,
            SemanticFailureCode::InternalError
        );
        for source in [
            accept_failure_source(WorkflowError::TimedOut {
                operation: "accept",
                stage: "runtime",
                timeout_ms: 30_000,
            }),
            accept_failure_source(TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms: 30_000,
                observed_at_ms: 30_001,
            }),
        ] {
            let timeout = AcceptInvitationError::AcceptFailed {
                detail: "unrelated diagnostic".into(),
                source: Some(source),
            };
            assert_eq!(
                timeout
                    .semantic_error(SemanticOperationKind::AcceptContactInvitation)
                    .code,
                SemanticFailureCode::OperationTimedOut
            );
        }
    }
}
