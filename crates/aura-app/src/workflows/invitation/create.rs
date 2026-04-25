#![allow(missing_docs)]

use super::*;
use thiserror::Error;

pub async fn create_contact_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    nickname: Option<String>,
    receiver_nickname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationHandle, AuraError> {
    create_contact_invitation_with_instance(
        app_core,
        receiver,
        nickname,
        receiver_nickname,
        message,
        ttl_ms,
        None,
    )
    .await
}

pub async fn create_contact_invitation_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    nickname: Option<String>,
    receiver_nickname: Option<String>,
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
    let invitation = create_contact_invitation_runtime(
        app_core,
        receiver,
        nickname,
        receiver_nickname,
        message,
        ttl_ms,
    )
    .await?;
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
    receiver_nickname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationInfo, AuraError> {
    let runtime = require_runtime(app_core).await?;
    timeout_runtime_call(
        &runtime,
        "create_contact_invitation",
        "create_contact_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || {
            runtime.create_contact_invitation(
                receiver,
                nickname,
                receiver_nickname,
                message,
                ttl_ms,
            )
        },
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
    receiver_nickname: Option<String>,
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
        let invitation = create_contact_invitation_runtime(
            app_core,
            receiver,
            nickname,
            receiver_nickname,
            message,
            ttl_ms,
        )
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
    receiver_nickname: Option<String>,
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
            app_core,
            nickname,
            receiver_nickname,
            message,
            ttl_ms,
            &owner,
            None,
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
    receiver_nickname: Option<String>,
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
    let invitation = create_contact_invitation_runtime(
        app_core,
        receiver,
        nickname,
        receiver_nickname,
        message,
        ttl_ms,
    )
    .await?;
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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(super) enum ChannelInvitationBootstrapError {
    #[error("InviteActorToChannel requires a canonical channel id, got {raw}")]
    InvalidCanonicalChannelId { raw: String },
    #[error("InviteActorToChannel requires an authoritative context for channel {channel_id}")]
    MissingAuthoritativeContext { channel_id: ChannelId },
    #[error(
        "Failed to bootstrap channel invitation for channel {channel_id} in context {context_id}"
    )]
    BootstrapUnavailable {
        channel_id: ChannelId,
        context_id: ContextId,
    },
    #[error("Failed to bootstrap channel invitation for channel {channel_id}: {detail}")]
    BootstrapTransport {
        channel_id: ChannelId,
        detail: String,
    },
    #[error(
        "Failed to create channel invitation for channel {channel_id} and receiver {receiver_id}: {detail}"
    )]
    CreateFailed {
        channel_id: ChannelId,
        receiver_id: AuthorityId,
        detail: String,
    },
    #[error(
        "Timed out creating channel invitation for channel {channel_id} and receiver {receiver_id} after {timeout_ms}ms"
    )]
    CreateTimedOut {
        channel_id: ChannelId,
        receiver_id: AuthorityId,
        timeout_ms: u64,
    },
}

impl ChannelInvitationBootstrapError {
    pub(super) fn semantic_error(&self) -> crate::ui_contract::SemanticOperationError {
        use crate::ui_contract::{
            SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
        };

        match self {
            Self::InvalidCanonicalChannelId { raw } => SemanticOperationError::new(
                SemanticFailureDomain::Command,
                SemanticFailureCode::MissingAuthoritativeContext,
            )
            .with_detail(format!("invalid_channel_id={raw}")),
            Self::MissingAuthoritativeContext { channel_id } => SemanticOperationError::new(
                SemanticFailureDomain::ChannelContext,
                SemanticFailureCode::MissingAuthoritativeContext,
            )
            .with_detail(format!("channel_id={channel_id}")),
            Self::BootstrapUnavailable {
                channel_id,
                context_id,
            } => SemanticOperationError::new(
                SemanticFailureDomain::Transport,
                SemanticFailureCode::ChannelBootstrapUnavailable,
            )
            .with_detail(format!("channel_id={channel_id}; context_id={context_id}")),
            Self::BootstrapTransport { channel_id, detail } => SemanticOperationError::new(
                SemanticFailureDomain::Transport,
                SemanticFailureCode::ChannelBootstrapUnavailable,
            )
            .with_detail(format!("channel_id={channel_id}; detail={detail}")),
            Self::CreateFailed {
                channel_id,
                receiver_id,
                detail,
            } => SemanticOperationError::new(
                SemanticFailureDomain::Invitation,
                SemanticFailureCode::InternalError,
            )
            .with_detail(format!(
                "channel_id={channel_id}; receiver_id={receiver_id}; detail={detail}"
            )),
            Self::CreateTimedOut {
                channel_id,
                receiver_id,
                timeout_ms,
            } => SemanticOperationError::new(
                SemanticFailureDomain::Invitation,
                SemanticFailureCode::OperationTimedOut,
            )
            .with_detail(format!(
                "channel_id={channel_id}; receiver_id={receiver_id}; timeout_ms={timeout_ms}"
            )),
        }
    }
}

impl From<ChannelInvitationBootstrapError> for AuraError {
    fn from(error: ChannelInvitationBootstrapError) -> Self {
        AuraError::agent(error.to_string())
    }
}

fn channel_invitation_bootstrap_timeout(
    deadline: Option<TimeoutBudget>,
    channel_id: ChannelId,
    stage: &'static str,
    context_id: Option<ContextId>,
) -> Result<Duration, ChannelInvitationBootstrapError> {
    let stage_timeout = channel_invitation_create_timeout();
    match deadline {
        Some(deadline) => {
            if deadline.timeout_ms() == 0 {
                let context_detail =
                    context_id.map_or_else(String::new, |context| format!(" in context {context}"));
                return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: format!(
                        "create_channel_invitation deadline exhausted before {stage}{context_detail}"
                    ),
                });
            }
            Ok(std::cmp::min(
                Duration::from_millis(deadline.timeout_ms()),
                stage_timeout,
            ))
        }
        None => Ok(stage_timeout),
    }
}

fn channel_invitation_create_timeout() -> Duration {
    let timeout_ms = if crate::workflows::harness_determinism::harness_mode_enabled() {
        CHANNEL_INVITATION_CREATE_TIMEOUT_MS.saturating_mul(4)
    } else {
        CHANNEL_INVITATION_CREATE_TIMEOUT_MS
    };
    Duration::from_millis(timeout_ms)
}

pub(super) async fn fail_channel_invitation<T>(
    owner: &SemanticWorkflowOwner,
    _deadline: Option<TimeoutBudget>,
    error: ChannelInvitationBootstrapError,
) -> Result<T, AuraError> {
    publish_invitation_owner_failure(owner, None, error.semantic_error()).await?;
    Err(error.into())
}

async fn ensure_channel_invitation_context_and_bootstrap(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    receiver: AuthorityId,
    channel_id: ChannelId,
    context_id: Option<ContextId>,
    bootstrap: Option<ChannelBootstrapPackage>,
    stage_tracker: &Option<WorkflowStageTracker>,
    deadline: Option<TimeoutBudget>,
) -> Result<(ContextId, ChannelBootstrapPackage), ChannelInvitationBootstrapError> {
    let requested_context = context_id;
    #[allow(unused_mut)]
    let mut resolved_context = match context_id {
        Some(context_id) => context_id,
        None => {
            update_channel_invitation_stage(stage_tracker, "resolve_context");
            #[cfg(feature = "signals")]
            {
                crate::workflows::messaging::context_id_for_channel(
                    app_core,
                    channel_id,
                    Some(runtime.authority_id()),
                )
                .await
                .map_err(|_| {
                    ChannelInvitationBootstrapError::MissingAuthoritativeContext { channel_id }
                })?
            }
            #[cfg(not(feature = "signals"))]
            {
                let _ = app_core;
                return Err(
                    ChannelInvitationBootstrapError::MissingAuthoritativeContext { channel_id },
                );
            }
        }
    };

    if let Some(bootstrap) = bootstrap {
        return Ok((resolved_context, bootstrap));
    }

    let mut runtime_resolved_context = None;
    update_channel_invitation_stage(stage_tracker, "resolve_runtime_channel_context");
    if let Some(runtime_context) = timeout_channel_invitation_stage_with_deadline(
        Some(runtime),
        "resolve_runtime_channel_context",
        deadline,
        async {
            timeout_runtime_call(
                runtime,
                "ensure_channel_invitation_context_and_bootstrap",
                "resolve_amp_channel_context",
                INVITATION_RUNTIME_QUERY_TIMEOUT,
                || runtime.resolve_amp_channel_context(channel_id),
            )
            .await
            .map_err(|error| AuraError::internal(error.to_string()))?
            .map_err(|error| AuraError::internal(error.to_string()))
        },
    )
    .await
    .map_err(|error| ChannelInvitationBootstrapError::BootstrapTransport {
        channel_id,
        detail: format!(
            "{error}; requested_context={requested_context:?}; resolved_context_before_runtime={resolved_context}"
        ),
    })? {
        runtime_resolved_context = Some(runtime_context);
        resolved_context = runtime_context;
    }

    let invitees = vec![receiver];
    let retry_policy = workflow_retry_policy(
        (CHANNEL_BOOTSTRAP_RETRY_ATTEMPTS + 1) as u32,
        Duration::from_millis(CHANNEL_BOOTSTRAP_RETRY_BACKOFF_MS),
        Duration::from_millis(
            CHANNEL_BOOTSTRAP_RETRY_BACKOFF_MS * (CHANNEL_BOOTSTRAP_RETRY_ATTEMPTS as u64 + 1),
        ),
    )
    .map_err(
        |error| ChannelInvitationBootstrapError::BootstrapTransport {
            channel_id,
            detail: error.to_string(),
        },
    )?;
    let mut attempts = retry_policy.attempt_budget();
    loop {
        let attempt = attempts.record_attempt().map_err(|error| {
            ChannelInvitationBootstrapError::BootstrapTransport {
                channel_id,
                detail: error.to_string(),
            }
        })?;
        update_channel_invitation_stage(stage_tracker, "amp_create_channel_bootstrap");
        let bootstrap_timeout = channel_invitation_bootstrap_timeout(
            deadline,
            channel_id,
            "amp_create_channel_bootstrap",
            Some(resolved_context),
        )?;
        let bootstrap_budget = workflow_timeout_budget(runtime, bootstrap_timeout)
            .await
            .map_err(
                |error| ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: error.to_string(),
                },
            )?;
        let bootstrap_attempt =
            execute_with_runtime_timeout_budget(runtime, &bootstrap_budget, || {
                runtime.amp_create_channel_bootstrap(resolved_context, channel_id, invitees.clone())
            })
            .await;
        match bootstrap_attempt {
            Err(TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. })) => {
                return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: format!(
                        "amp_create_channel_bootstrap timed out after {}ms in context {resolved_context}",
                        bootstrap_budget.timeout_ms()
                    ),
                });
            }
            Err(TimeoutRunError::Timeout(error)) => {
                return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: error.to_string(),
                });
            }
            Ok(bootstrap) => return Ok((resolved_context, bootstrap)),
            Err(TimeoutRunError::Operation(error))
                if classify_amp_channel_error(&error)
                    == AmpChannelErrorClass::ChannelStateUnavailable =>
            {
                if !attempts.can_attempt() {
                    break;
                }
                converge_runtime(runtime).await;
                runtime
                    .sleep_ms(retry_policy.delay_for_attempt(attempt).as_millis() as u64)
                    .await;
                update_channel_invitation_stage(stage_tracker, "amp_channel_state_exists");
                let exists_timeout = channel_invitation_bootstrap_timeout(
                    deadline,
                    channel_id,
                    "amp_channel_state_exists",
                    Some(resolved_context),
                )?;
                let exists_budget = workflow_timeout_budget(runtime, exists_timeout)
                    .await
                    .map_err(
                        |error| ChannelInvitationBootstrapError::BootstrapTransport {
                            channel_id,
                            detail: error.to_string(),
                        },
                    )?;
                let state_exists = match execute_with_runtime_timeout_budget(
                    runtime,
                    &exists_budget,
                    || runtime.amp_channel_state_exists(resolved_context, channel_id),
                )
                .await
                {
                    Err(TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded {
                        ..
                    })) => {
                        return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                            channel_id,
                            detail: format!(
                                "amp_channel_state_exists timed out after {}ms in context {resolved_context}",
                                exists_budget.timeout_ms()
                            ),
                        });
                    }
                    Err(TimeoutRunError::Timeout(error)) => {
                        return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                            channel_id,
                            detail: error.to_string(),
                        });
                    }
                    Ok(state_exists) => state_exists,
                    Err(TimeoutRunError::Operation(state_error)) => {
                        return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                            channel_id,
                            detail: format!(
                                "failed to verify repaired channel state in context {resolved_context}: {state_error}"
                            ),
                        });
                    }
                };
                #[cfg(feature = "signals")]
                {
                    if !state_exists {
                        if let Ok(authoritative_context) =
                            crate::workflows::messaging::context_id_for_channel(
                                app_core,
                                channel_id,
                                Some(runtime.authority_id()),
                            )
                            .await
                        {
                            if authoritative_context != resolved_context {
                                resolved_context = authoritative_context;
                                continue;
                            }
                        }
                    }
                }
                if !state_exists {
                    continue;
                }
            }
            Err(TimeoutRunError::Operation(error)) => {
                return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: format!(
                        "{error}; requested_context={requested_context:?}; runtime_resolved_context={runtime_resolved_context:?}; bootstrap_context={resolved_context}"
                    ),
                });
            }
        }
    }

    Err(ChannelInvitationBootstrapError::BootstrapUnavailable {
        channel_id,
        context_id: resolved_context,
    })
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
    let operation_budget = workflow_timeout_budget(&runtime, channel_invitation_create_timeout())
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
