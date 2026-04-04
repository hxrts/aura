//! Invitation Workflow - Portable Business Logic
//!
//! This module contains invitation operations that are portable across
//! all frontends via the RuntimeBridge abstraction.
//!
//! ## TTL Presets
//!
//! Standard TTL presets for invitation expiration:
//! - 1 hour: Quick invitations
//! - 24 hours (default): Standard invitations
//! - 1 week (168 hours): Extended invitations
//! - 30 days (720 hours): Long-term invitations

use crate::runtime_bridge::{InvitationBridgeType, InvitationInfo};

mod accept;
mod create;
mod device_enrollment;
mod export;
mod followups;
mod import;
mod pending_accept;
mod readiness;
mod utils;

use crate::signal_defs::INVITATIONS_SIGNAL;
use crate::ui::signals::{CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME};
use crate::ui_contract::AuthoritativeSemanticFact;
use crate::ui_contract::{
    InvitationFactKind, OperationId, OperationInstanceId, OperationState, SemanticOperationKind,
    SemanticOperationPhase,
};
use crate::workflows::runtime::{
    converge_runtime, ensure_runtime_peer_connectivity, execute_with_runtime_retry_budget,
    execute_with_runtime_timeout_budget, require_runtime, timeout_runtime_call,
    warn_workflow_timeout, workflow_best_effort, workflow_retry_policy, workflow_timeout_budget,
};
use crate::workflows::runtime_error_classification::{
    classify_amp_channel_error, classify_invitation_accept_error, AmpChannelErrorClass,
    InvitationAcceptErrorClass,
};
#[cfg(feature = "signals")]
use crate::workflows::semantic_facts::prove_channel_membership_ready;
use crate::workflows::semantic_facts::{
    issue_device_enrollment_imported_proof, issue_invitation_accepted_or_materialized_proof,
    issue_invitation_created_proof, issue_invitation_declined_proof,
    issue_invitation_exported_proof, issue_invitation_revoked_proof,
    issue_pending_invitation_consumed_proof, publish_authoritative_semantic_fact,
    replace_authoritative_semantic_facts_of_kind, semantic_readiness_publication_capability,
    update_authoritative_semantic_facts, SemanticWorkflowOwner,
};
use crate::workflows::settings;
use crate::workflows::signals::{read_signal, read_signal_or_default};
#[cfg(feature = "signals")]
use crate::workflows::stage_tracker::update_workflow_stage_direct;
use crate::workflows::stage_tracker::{
    new_workflow_stage_tracker, update_workflow_stage, WorkflowStageTracker,
};
use crate::{views::invitations::InvitationsState, AppCore};
pub use accept::{
    accept_imported_invitation, accept_imported_invitation_with_instance,
    accept_imported_invitation_with_terminal_status, accept_invitation, accept_invitation_by_str,
    accept_invitation_by_str_with_instance, accept_invitation_by_str_with_terminal_status,
    accept_invitation_with_instance, cancel_invitation, cancel_invitation_by_str,
    cancel_invitation_by_str_with_terminal_status, decline_invitation, decline_invitation_by_str,
    decline_invitation_by_str_with_terminal_status,
};
pub(in crate::workflows) use accept::{
    accept_imported_invitation_inner, accept_imported_invitation_owned,
};
use async_lock::RwLock;
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, InvitationId};
use aura_core::{
    AttemptBudget, AuraError, OperationContext, RetryBudgetPolicy, RetryRunError, TimeoutBudget,
    TimeoutBudgetError, TimeoutRunError, TraceContext,
};
#[allow(unused_imports)]
pub(in crate::workflows) use create::create_channel_invitation_owned;
pub use create::{
    create_channel_invitation, create_contact_invitation,
    create_contact_invitation_code_with_terminal_status, create_contact_invitation_with_instance,
    create_generic_contact_invitation_code_terminal_status, create_guardian_invitation,
    create_guardian_invitation_with_instance, create_guardian_invitation_with_terminal_status,
};
pub use device_enrollment::accept_device_enrollment_invitation;
pub(in crate::workflows) use export::export_invitation_runtime;
pub use export::{
    export_invitation, export_invitation_by_str, export_invitation_by_str_with_terminal_status,
};
pub use followups::run_post_contact_accept_followups;
pub(in crate::workflows) use import::pending_invitation_info_by_id;
pub use import::{
    import_invitation, import_invitation_details, list_invitations, list_pending_invitations,
};
#[cfg(feature = "signals")]
pub(in crate::workflows) use pending_accept::run_post_channel_accept_followups;
pub use pending_accept::{
    accept_pending_channel_invitation,
    accept_pending_channel_invitation_with_binding_terminal_status,
    accept_pending_channel_invitation_with_instance,
    accept_pending_channel_invitation_with_terminal_status,
};
pub(in crate::workflows) use readiness::{
    contacts_signal_snapshot, publish_authoritative_contact_invitation_accepted,
    refresh_authoritative_contact_link_readiness, refresh_authoritative_invitation_readiness,
};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
pub use utils::*;

const INVITATION_ACCEPT_LOOKUP_TIMEOUT_MS: u64 = 3_000;
const CONTACT_INVITATION_ACCEPT_RUNTIME_STAGE_TIMEOUT_MS: u64 = 8_000;
const CHANNEL_INVITATION_ACCEPT_RUNTIME_STAGE_TIMEOUT_MS: u64 = 30_000;
const CHANNEL_INVITATION_ACCEPT_RECONCILE_TIMEOUT_MS: u64 = 120_000;
const INVITATION_ACCEPT_CONVERGENCE_ATTEMPTS: usize = 4;
const INVITATION_ACCEPT_CONVERGENCE_STEP_TIMEOUT_MS: u64 = 500;
#[cfg(feature = "signals")]
const PENDING_INVITATION_AUTHORITATIVE_ATTEMPTS: usize = 24;
#[cfg(feature = "signals")]
const PENDING_INVITATION_AUTHORITATIVE_BACKOFF_MS: u64 = 100;
const CONTACT_LINK_ATTEMPTS: usize = 32;
const CONTACT_LINK_BACKOFF_MS: u64 = 100;
const CONTACT_ACCEPT_PROPAGATION_ATTEMPTS: usize = 8;
const CHANNEL_BOOTSTRAP_RETRY_ATTEMPTS: usize = 6;
const CHANNEL_BOOTSTRAP_RETRY_BACKOFF_MS: u64 = 75;
const CHANNEL_INVITATION_CREATE_TIMEOUT_MS: u64 = 5_000;
const INVITATION_RUNTIME_QUERY_TIMEOUT: Duration = Duration::from_millis(5_000);
const INVITATION_RUNTIME_OPERATION_TIMEOUT: Duration = Duration::from_millis(30_000);

/// Move-owned invitation lifecycle handle.
///
/// Frontend and workflow code may inspect invitation metadata through shared
/// borrows, but lifecycle transitions consume the handle so stale owners cannot
/// act twice.
#[aura_macros::strong_reference(domain = "invitation")]
#[derive(Debug)]
pub struct InvitationHandle {
    invitation: InvitationInfo,
}

impl InvitationHandle {
    fn new(invitation: InvitationInfo) -> Self {
        Self { invitation }
    }

    /// Stable invitation identifier for export/display.
    pub fn invitation_id(&self) -> &InvitationId {
        &self.invitation.invitation_id
    }

    /// Borrow the bridge-level invitation metadata.
    pub fn info(&self) -> &InvitationInfo {
        &self.invitation
    }

    fn into_info(self) -> InvitationInfo {
        self.invitation
    }
}

fn update_channel_invitation_stage(tracker: &Option<WorkflowStageTracker>, stage: &'static str) {
    update_workflow_stage(tracker, stage);
}

#[cfg(feature = "signals")]
fn update_accept_reconcile_stage(tracker: &WorkflowStageTracker, stage: &'static str) {
    update_workflow_stage_direct(tracker, stage);
}

async fn timeout_channel_invitation_stage_with_deadline<T>(
    runtime: Option<&Arc<dyn crate::runtime_bridge::RuntimeBridge>>,
    stage: &'static str,
    deadline: Option<TimeoutBudget>,
    future: impl std::future::Future<Output = Result<T, AuraError>>,
) -> Result<T, AuraError> {
    let Some(runtime) = runtime else {
        return future.await;
    };
    let requested = deadline
        .map(|deadline| {
            Duration::from_millis(deadline.timeout_ms())
                .min(Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS))
        })
        .unwrap_or(Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS));
    let budget = match workflow_timeout_budget(runtime, requested).await {
        Ok(budget) => budget,
        Err(TimeoutBudgetError::DeadlineExceeded { .. }) => {
            warn_workflow_timeout("create_channel_invitation", stage, 0);
            return Err(AuraError::from(
                crate::workflows::error::WorkflowError::TimedOut {
                    operation: "create_channel_invitation",
                    stage,
                    timeout_ms: 0,
                },
            ));
        }
        Err(error) => return Err(error.into()),
    };
    match execute_with_runtime_timeout_budget(runtime, &budget, || future).await {
        Ok(value) => Ok(value),
        Err(TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. })) => {
            warn_workflow_timeout("create_channel_invitation", stage, budget.timeout_ms());
            Err(AuraError::from(
                crate::workflows::error::WorkflowError::TimedOut {
                    operation: "create_channel_invitation",
                    stage,
                    timeout_ms: budget.timeout_ms(),
                },
            ))
        }
        Err(TimeoutRunError::Timeout(error)) => Err(error.into()),
        Err(TimeoutRunError::Operation(error)) => Err(error),
    }
}

async fn invitation_accept_timeout_budget(
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

fn device_enrollment_accept_retry_policy() -> Result<RetryBudgetPolicy, AuraError> {
    workflow_retry_policy(80, Duration::from_millis(250), Duration::from_millis(500))
        .map_err(AuraError::from)
}

enum DeviceEnrollmentAcceptConvergenceError {
    Terminal(String),
    Workflow(AuraError),
}

fn log_device_enrollment_accept_progress(message: impl Into<String>) {
    let message = message.into();
    #[cfg(feature = "wasm")]
    crate::platform::wasm::console_log(&format!("[device-enrollment-accept] {message}"));
    #[cfg(all(not(feature = "wasm"), feature = "instrumented"))]
    tracing::info!(target: "device-enrollment-accept", "{message}");
    #[cfg(all(not(feature = "wasm"), not(feature = "instrumented")))]
    let _ = message;
}

fn emit_contact_accept_probe(stage: &str) {
    let _ = stage;
}

// OWNERSHIP: first-run-default
fn channel_invitation_bootstrap_timeout(
    deadline: Option<TimeoutBudget>,
    channel_id: ChannelId,
    stage: &'static str,
    context_id: Option<ContextId>,
) -> Result<Duration, ChannelInvitationBootstrapError> {
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
                Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS),
            ))
        }
        None => Ok(Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS)),
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
async fn authoritative_pending_home_or_channel_invitation(
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
fn invitations_signal_has_pending_home_or_channel_invitation(
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

async fn authoritative_pending_home_or_channel_invitation_for_accept(
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

async fn publish_invitation_owner_status(
    owner: &SemanticWorkflowOwner,
    deadline: Option<TimeoutBudget>,
    phase: SemanticOperationPhase,
) -> Result<(), AuraError> {
    let stage = match phase {
        SemanticOperationPhase::Submitted => "publish_submitted",
        SemanticOperationPhase::WorkflowDispatched => "publish_workflow_dispatched",
        SemanticOperationPhase::AuthoritativeContextReady => "publish_authoritative_context_ready",
        SemanticOperationPhase::ContactLinkReady => "publish_contact_link_ready",
        SemanticOperationPhase::MembershipReady => "publish_membership_ready",
        SemanticOperationPhase::RecipientResolutionReady => "publish_recipient_resolution_ready",
        SemanticOperationPhase::PeerChannelReady => "publish_peer_channel_ready",
        SemanticOperationPhase::DeliveryReady => "publish_delivery_ready",
        SemanticOperationPhase::Succeeded => "publish_succeeded",
        SemanticOperationPhase::Failed => "publish_failed",
        SemanticOperationPhase::Cancelled => "publish_cancelled",
    };
    timeout_channel_invitation_stage_with_deadline(
        None,
        stage,
        deadline,
        owner.publish_phase(phase),
    )
    .await
}

fn semantic_kind_for_bridge_invitation(
    invitation: &crate::runtime_bridge::InvitationInfo,
) -> SemanticOperationKind {
    match invitation.invitation_type {
        crate::runtime_bridge::InvitationBridgeType::Contact { .. } => {
            SemanticOperationKind::AcceptContactInvitation
        }
        crate::runtime_bridge::InvitationBridgeType::Channel { .. }
        | crate::runtime_bridge::InvitationBridgeType::Guardian { .. } => {
            SemanticOperationKind::AcceptPendingChannelInvitation
        }
        crate::runtime_bridge::InvitationBridgeType::DeviceEnrollment { .. } => {
            SemanticOperationKind::ImportDeviceEnrollmentCode
        }
    }
}

fn invitation_accept_runtime_stage_timeout_ms(
    pending_runtime_invitation: Option<&InvitationInfo>,
    accepted_invitation: Option<&crate::views::invitations::Invitation>,
) -> u64 {
    if pending_runtime_invitation.is_some_and(|invitation| {
        matches!(
            invitation.invitation_type,
            InvitationBridgeType::Channel { .. }
        )
    }) || accepted_invitation.is_some_and(|invitation| {
        invitation.invitation_type == crate::views::invitations::InvitationType::Chat
    }) {
        CHANNEL_INVITATION_ACCEPT_RUNTIME_STAGE_TIMEOUT_MS
    } else {
        CONTACT_INVITATION_ACCEPT_RUNTIME_STAGE_TIMEOUT_MS
    }
}

fn invitation_accept_reconcile_timeout_ms(
    pending_runtime_invitation: Option<&InvitationInfo>,
    accepted_invitation: Option<&crate::views::invitations::Invitation>,
) -> u64 {
    if pending_runtime_invitation.is_some_and(|invitation| {
        matches!(
            invitation.invitation_type,
            InvitationBridgeType::Channel { .. }
        )
    }) || accepted_invitation.is_some_and(|invitation| {
        invitation.invitation_type == crate::views::invitations::InvitationType::Chat
    }) {
        CHANNEL_INVITATION_ACCEPT_RECONCILE_TIMEOUT_MS
    } else {
        INVITATION_ACCEPT_LOOKUP_TIMEOUT_MS
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
enum ChannelInvitationBootstrapError {
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
    fn semantic_error(&self) -> crate::ui_contract::SemanticOperationError {
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

async fn publish_invitation_operation_failure(
    app_core: &Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    deadline: Option<TimeoutBudget>,
    kind: SemanticOperationKind,
    error: crate::ui_contract::SemanticOperationError,
) -> Result<(), AuraError> {
    let owner = SemanticWorkflowOwner::new(app_core, operation_id, instance_id, kind);
    publish_invitation_owner_failure(&owner, deadline, error).await
}

async fn publish_invitation_owner_failure(
    owner: &SemanticWorkflowOwner,
    deadline: Option<TimeoutBudget>,
    error: crate::ui_contract::SemanticOperationError,
) -> Result<(), AuraError> {
    timeout_channel_invitation_stage_with_deadline(
        None,
        "publish_failure",
        deadline,
        owner.publish_failure(error),
    )
    .await
}

async fn fail_channel_invitation<T>(
    owner: &SemanticWorkflowOwner,
    _deadline: Option<TimeoutBudget>,
    error: ChannelInvitationBootstrapError,
) -> Result<T, AuraError> {
    publish_invitation_owner_failure(owner, None, error.semantic_error()).await?;
    Err(error.into())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
enum AcceptInvitationError {
    #[error("Failed to accept invitation: {detail}")]
    AcceptFailed { detail: String },
    #[error("accepted contact invitation for {contact_id} but the contact never converged")]
    ContactLinkDidNotConverge { contact_id: AuthorityId },
}

impl AcceptInvitationError {
    fn semantic_error(
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

async fn fail_invitation_accept<T>(
    owner: &SemanticWorkflowOwner,
    error: AcceptInvitationError,
) -> Result<T, AuraError> {
    publish_invitation_owner_failure(owner, None, error.semantic_error(owner.kind())).await?;
    Err(error.into())
}

async fn fail_device_enrollment_accept<T>(
    app_core: &Arc<RwLock<AppCore>>,
    detail: impl Into<String>,
) -> Result<T, AuraError> {
    let error = crate::ui_contract::SemanticOperationError::new(
        crate::ui_contract::SemanticFailureDomain::Invitation,
        crate::ui_contract::SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into());
    publish_invitation_operation_failure(
        app_core,
        OperationId::device_enrollment(),
        None,
        None,
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        error.clone(),
    )
    .await?;
    Err(AuraError::agent(error.detail.unwrap_or_else(|| {
        "device enrollment acceptance failed".to_string()
    })))
}

async fn fail_pending_invitation_accept_owned<T>(
    owner: &SemanticWorkflowOwner,
    error: AcceptInvitationError,
) -> Result<T, AuraError> {
    fail_invitation_accept(owner, error).await
}

async fn fail_pending_invitation_accept_unowned<T>(
    error: AcceptInvitationError,
) -> Result<T, AuraError> {
    Err(error.into())
}

async fn reconcile_channel_invitation_acceptance(
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

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
struct AcceptedChannelInvitationTarget {
    channel_id: ChannelId,
    context_hint: Option<ContextId>,
    channel_name_hint: Option<String>,
}

async fn list_pending_invitations_with_timeout(
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

async fn pending_invitation_by_id_with_timeout(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    invitation_id: &InvitationId,
) -> Result<Option<InvitationInfo>, AcceptInvitationError> {
    Ok(list_pending_invitations_with_timeout(runtime)
        .await?
        .into_iter()
        .find(|invitation| invitation.invitation_id == *invitation_id))
}

async fn trigger_runtime_discovery_with_timeout(
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

async fn drive_invitation_accept_convergence(
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

    // Return Ok even when convergence didn't reach peer connectivity: the
    // invitation acceptance itself succeeded and sync will complete once
    // peers become reachable.  The warning above provides diagnostics.
    Ok(())
}

async fn prime_device_enrollment_accept_connectivity(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
) {
    trigger_runtime_discovery_with_timeout(runtime).await;
    let _ = drive_invitation_accept_convergence(app_core, runtime, None).await;
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
                // A channel that already satisfied the authoritative
                // runtime-state gate for create/join must not be silently
                // "repaired" here by re-running channel creation. The
                // invite path is allowed to wait for convergence and retry
                // bootstrap lookup, but a missing checkpoint after that is
                // a real inconsistency that should fail explicitly.
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
                Err(AuraError::from(super::error::WorkflowError::Precondition(
                    "Accepted channel invitation but no authoritative context was materialized",
                )))
            })
            .await
            .map_err(|error| match error {
                RetryRunError::Timeout(timeout_error) => AuraError::from(timeout_error),
                RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
            })?,
        );
    }
    let authoritative_context = authoritative_context.ok_or_else(|| {
        AuraError::from(super::error::WorkflowError::Precondition(
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
// OWNERSHIP: authoritative-ref-only
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
        .map_err(|error| super::error::runtime_call("resolve channel context", error))
        .map(|result| {
            result.map_err(|error| super::error::runtime_call("resolve channel context", error))
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
        .map_err(|error| super::error::runtime_call("accept channel invitation join", error))?
        {
            if classify_amp_channel_error(&error) != AmpChannelErrorClass::AlreadyExists {
                return Err(
                    super::error::runtime_call("accept channel invitation join", error).into(),
                );
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
            .map_err(|error| super::error::runtime_call("resolve channel context", error))
            .map(|result| {
                result.map_err(|error| super::error::runtime_call("resolve channel context", error))
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
            .map_err(|error| super::error::runtime_call("resolve channel context", error))
            .map(|result| {
                result.map_err(|error| super::error::runtime_call("resolve channel context", error))
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

// ============================================================================
// Invitation Creation via RuntimeBridge
// ============================================================================

/// Create a contact invitation
///
/// **What it does**: Creates an invitation to become a contact
/// **Returns**: InvitationInfo with the created invitation details
/// **Signal pattern**: RuntimeBridge handles state updates
fn command_terminal_error(detail: impl Into<String>) -> crate::ui_contract::SemanticOperationError {
    crate::ui_contract::SemanticOperationError::new(
        crate::ui_contract::SemanticFailureDomain::Command,
        crate::ui_contract::SemanticFailureCode::InternalError,
    )
    .with_detail(detail.into())
}

/// Typed frontend handoff facades for invitation workflows.
pub mod handoff {
    use super::*;

    /// Inputs for the create-contact-invitation handoff workflow.
    #[derive(Debug, Clone)]
    pub struct CreateContactInvitationRequest {
        /// The receiver authority for the invitation.
        pub receiver: AuthorityId,
        /// Optional nickname carried in the invitation payload.
        pub nickname: Option<String>,
        /// Optional invitation message.
        pub message: Option<String>,
        /// Optional invitation TTL in milliseconds.
        pub ttl_ms: Option<u64>,
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
    }

    /// Inputs for the generic out-of-band create-contact-invitation workflow.
    #[derive(Debug, Clone)]
    pub struct CreateGenericContactInvitationRequest {
        /// Optional nickname carried in the invitation payload.
        pub nickname: Option<String>,
        /// Optional invitation message.
        pub message: Option<String>,
        /// Optional invitation TTL in milliseconds.
        pub ttl_ms: Option<u64>,
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
    }

    /// Inputs for the create-guardian-invitation handoff workflow.
    #[derive(Debug, Clone)]
    pub struct CreateGuardianInvitationRequest {
        /// The receiver authority for the invitation.
        pub receiver: AuthorityId,
        /// The subject authority the guardian protects.
        pub subject: AuthorityId,
        /// Optional invitation message.
        pub message: Option<String>,
        /// Optional invitation TTL in milliseconds.
        pub ttl_ms: Option<u64>,
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
    }

    /// Inputs for accepting a freshly imported invitation.
    #[derive(Debug)]
    pub struct AcceptImportedInvitationRequest {
        /// Imported invitation handle returned by the runtime.
        pub invitation: InvitationHandle,
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
    }

    /// Inputs for invitation actions that address an existing invitation id.
    #[derive(Debug, Clone)]
    pub struct InvitationByIdRequest {
        /// Canonical invitation identifier.
        pub invitation_id: String,
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
    }

    /// Inputs for accepting the current pending channel invitation.
    #[derive(Debug, Clone, Default)]
    pub struct PendingChannelInvitationRequest {
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
    }

    /// Create and export a contact invitation as one typed handoff workflow.
    pub async fn create_contact_invitation(
        app_core: &Arc<RwLock<AppCore>>,
        request: CreateContactInvitationRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
        super::create_contact_invitation_code_with_terminal_status(
            app_core,
            request.receiver,
            request.nickname,
            request.message,
            request.ttl_ms,
            request.operation_instance_id,
        )
        .await
    }

    /// Create and export a generic contact invitation as one typed handoff workflow.
    pub async fn create_generic_contact_invitation(
        app_core: &Arc<RwLock<AppCore>>,
        request: CreateGenericContactInvitationRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
        super::create_generic_contact_invitation_code_terminal_status(
            app_core,
            request.nickname,
            request.message,
            request.ttl_ms,
            request.operation_instance_id,
        )
        .await
    }

    /// Create a guardian invitation as one typed handoff workflow.
    pub async fn create_guardian_invitation(
        app_core: &Arc<RwLock<AppCore>>,
        request: CreateGuardianInvitationRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationHandle> {
        super::create_guardian_invitation_with_terminal_status(
            app_core,
            request.receiver,
            request.subject,
            request.message,
            request.ttl_ms,
            request.operation_instance_id,
        )
        .await
    }

    /// Accept a previously imported invitation handle.
    pub async fn accept_imported_invitation(
        app_core: &Arc<RwLock<AppCore>>,
        request: AcceptImportedInvitationRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<()> {
        super::accept_imported_invitation_with_terminal_status(
            app_core,
            request.invitation,
            request.operation_instance_id,
        )
        .await
    }

    /// Accept a pending invitation by its canonical id.
    pub async fn accept_invitation_by_id(
        app_core: &Arc<RwLock<AppCore>>,
        request: InvitationByIdRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationInfo> {
        super::accept_invitation_by_str_with_terminal_status(
            app_core,
            &request.invitation_id,
            request.operation_instance_id,
        )
        .await
    }

    /// Decline a pending invitation by its canonical id.
    pub async fn decline_invitation_by_id(
        app_core: &Arc<RwLock<AppCore>>,
        request: InvitationByIdRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<()> {
        super::decline_invitation_by_str_with_terminal_status(
            app_core,
            &request.invitation_id,
            request.operation_instance_id,
        )
        .await
    }

    /// Export an existing invitation code by canonical id.
    pub async fn export_invitation_by_id(
        app_core: &Arc<RwLock<AppCore>>,
        request: InvitationByIdRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
        super::export_invitation_by_str_with_terminal_status(
            app_core,
            &request.invitation_id,
            request.operation_instance_id,
        )
        .await
    }

    /// Revoke an existing invitation by canonical id.
    pub async fn cancel_invitation_by_id(
        app_core: &Arc<RwLock<AppCore>>,
        request: InvitationByIdRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<()> {
        super::cancel_invitation_by_str_with_terminal_status(
            app_core,
            &request.invitation_id,
            request.operation_instance_id,
        )
        .await
    }

    /// Accept the current pending channel invitation.
    pub async fn accept_pending_channel_invitation(
        app_core: &Arc<RwLock<AppCore>>,
        request: PendingChannelInvitationRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationId> {
        super::accept_pending_channel_invitation_with_terminal_status(
            app_core,
            request.operation_instance_id,
        )
        .await
    }
}

// ============================================================================
// Invitation Queries via RuntimeBridge
// ============================================================================

// ============================================================================
// Invitation Operations via RuntimeBridge
// ============================================================================

// Accept a device-enrollment invitation and wait for the local device view to
// converge.
async fn wait_for_contact_link(
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

// ============================================================================
// Invitation Role Parsing and Formatting
// ============================================================================

// ============================================================================
// Additional Invitation Operations
// ============================================================================

/// Accept the first pending home/channel invitation
///
/// **What it does**: Finds and accepts the first pending channel invitation
/// **Returns**: Invitation ID that was accepted
/// **Signal pattern**: RuntimeBridge handles signal emission
///
/// This is used by UI to quickly accept a pending channel invitation without
/// requiring the user to select a specific invitation ID.
/// Returns the typed InvitationId of the accepted invitation.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL;
    use crate::ui_contract::{
        SemanticFailureCode, SemanticFailureDomain, SemanticOperationKind, SemanticOperationPhase,
    };
    use crate::views::invitations::InvitationType;
    #[cfg(feature = "signals")]
    use crate::workflows::messaging::apply_authoritative_membership_projection;
    use crate::workflows::semantic_facts::assert_terminal_failure_status;
    use crate::workflows::signals::emit_signal;
    use crate::AppConfig;
    use std::{fs, path::PathBuf};

    fn read_invitation_workflow_source(relative_path: &str) -> String {
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative_path))
            .expect("invitation workflow source should be readable")
    }

    // === Invitation Role Parsing Tests ===

    #[test]
    fn accept_pending_channel_invitation_owned_boundary_is_declared_and_wrapped() {
        let source = read_invitation_workflow_source("src/workflows/invitation/pending_accept.rs");

        assert!(source.contains("owner = \"accept_pending_channel_invitation_id_owned\""));
        assert!(source.contains("async fn accept_pending_channel_invitation_id_owned("));
        assert!(source.contains(
            "let _ = super::accept_imported_invitation_inner(app_core, &invitation, owner).await?;"
        ));
        assert!(source.contains("issue_pending_invitation_consumed_proof("));
    }

    #[test]
    fn accept_imported_invitation_owned_boundary_preserves_wrapper_and_inner_split() {
        let source = read_invitation_workflow_source("src/workflows/invitation/accept.rs");

        assert!(source.contains("owner = \"accept_imported_invitation_owned\""));
        assert!(source.contains("async fn accept_imported_invitation_owned("));
        assert!(source.contains("async fn accept_imported_invitation_inner("));
        assert!(source.contains(
            "match accept_imported_invitation_inner(app_core, invitation, owner).await? {"
        ));
        assert!(source.contains(
            "accept_imported_invitation_owned(app_core, &invitation, &owner, None).await"
        ));
    }

    #[test]
    fn create_channel_invitation_owned_boundary_is_declared() {
        let source = read_invitation_workflow_source("src/workflows/invitation/create.rs");

        assert!(source.contains("owner = \"create_channel_invitation\""));
        assert!(source.contains("async fn create_channel_invitation_owned("));
        assert!(source
            .contains("owner\n            .publish_success_with(issue_invitation_created_proof("));
    }

    #[test]
    fn test_parse_invitation_role_guardian() -> Result<(), InvitationRoleParseError> {
        let result = parse_invitation_role("guardian")?;
        assert_eq!(result, InvitationRoleValue::Guardian);
        Ok(())
    }

    #[test]
    fn test_parse_invitation_role_guardian_case_insensitive() -> Result<(), InvitationRoleParseError>
    {
        assert_eq!(
            parse_invitation_role("GUARDIAN")?,
            InvitationRoleValue::Guardian
        );
        assert_eq!(
            parse_invitation_role("Guardian")?,
            InvitationRoleValue::Guardian
        );
        Ok(())
    }

    #[test]
    fn test_parse_invitation_role_channel() -> Result<(), InvitationRoleParseError> {
        let result = parse_invitation_role("channel")?;
        assert_eq!(result, InvitationRoleValue::Channel);
        Ok(())
    }

    #[test]
    fn test_parse_invitation_role_channel_case_insensitive() -> Result<(), InvitationRoleParseError>
    {
        assert_eq!(
            parse_invitation_role("CHANNEL")?,
            InvitationRoleValue::Channel
        );
        assert_eq!(
            parse_invitation_role("Channel")?,
            InvitationRoleValue::Channel
        );
        Ok(())
    }

    #[test]
    fn test_parse_invitation_role_contact() -> Result<(), InvitationRoleParseError> {
        let result = parse_invitation_role("contact")?;
        assert_eq!(result, InvitationRoleValue::Contact);
        Ok(())
    }

    #[test]
    fn test_parse_invitation_role_rejects_invalid_role() {
        let result = parse_invitation_role("friend");
        assert!(matches!(
            result,
            Err(InvitationRoleParseError::InvalidRole(role)) if role == "friend"
        ));
    }

    #[test]
    fn test_parse_invitation_role_rejects_empty_role() {
        let result = parse_invitation_role("");
        assert_eq!(result, Err(InvitationRoleParseError::Empty));
    }

    #[test]
    fn test_invitation_role_as_str() {
        assert_eq!(InvitationRoleValue::Guardian.as_str(), "guardian");
        assert_eq!(InvitationRoleValue::Channel.as_str(), "channel");
        assert_eq!(InvitationRoleValue::Contact.as_str(), "contact");
    }

    #[test]
    fn test_invitation_role_display() {
        assert_eq!(format!("{}", InvitationRoleValue::Guardian), "guardian");
        assert_eq!(format!("{}", InvitationRoleValue::Channel), "channel");
        assert_eq!(format!("{}", InvitationRoleValue::Contact), "contact");
    }

    #[test]
    fn test_invitation_role_to_invitation_type() {
        assert_eq!(
            InvitationRoleValue::Guardian.to_invitation_type(),
            InvitationType::Guardian
        );
        assert_eq!(
            InvitationRoleValue::Channel.to_invitation_type(),
            InvitationType::Chat
        );
        assert_eq!(
            InvitationRoleValue::Contact.to_invitation_type(),
            InvitationType::Home
        );
    }

    #[test]
    fn test_format_invitation_type() {
        assert_eq!(format_invitation_type(InvitationType::Home), "Home");
        assert_eq!(format_invitation_type(InvitationType::Guardian), "Guardian");
        assert_eq!(format_invitation_type(InvitationType::Chat), "Channel");
    }

    #[test]
    fn test_format_invitation_type_detailed() {
        assert_eq!(
            format_invitation_type_detailed(InvitationType::Home, None),
            "Home"
        );
        assert_eq!(
            format_invitation_type_detailed(InvitationType::Home, Some("living room")),
            "Home (living room)"
        );
        assert_eq!(
            format_invitation_type_detailed(InvitationType::Guardian, Some("alice-authority")),
            "Guardian (for: alice-authority)"
        );
        assert_eq!(
            format_invitation_type_detailed(InvitationType::Chat, Some("general")),
            "Channel (general)"
        );
    }

    // === TTL Tests ===

    #[test]
    fn test_ttl_constants() {
        assert_eq!(INVITATION_TTL_1_HOUR, 1);
        assert_eq!(INVITATION_TTL_1_DAY, 24);
        assert_eq!(INVITATION_TTL_1_WEEK, 168);
        assert_eq!(INVITATION_TTL_30_DAYS, 720);
        assert_eq!(DEFAULT_INVITATION_TTL_HOURS, 24);
    }

    #[test]
    fn test_ttl_presets_array() {
        assert_eq!(INVITATION_TTL_PRESETS.len(), 4);
        assert_eq!(INVITATION_TTL_PRESETS[0], 1);
        assert_eq!(INVITATION_TTL_PRESETS[1], 24);
        assert_eq!(INVITATION_TTL_PRESETS[2], 168);
        assert_eq!(INVITATION_TTL_PRESETS[3], 720);
    }

    #[test]
    fn test_ttl_hours_to_ms() {
        assert_eq!(ttl_hours_to_ms(1), 3_600_000);
        assert_eq!(ttl_hours_to_ms(24), 86_400_000);
        assert_eq!(ttl_hours_to_ms(168), 604_800_000);
        assert_eq!(ttl_hours_to_ms(720), 2_592_000_000);
    }

    #[test]
    fn test_format_ttl_display_presets() {
        assert_eq!(format_ttl_display(1), "1 hour");
        assert_eq!(format_ttl_display(24), "1 day");
        assert_eq!(format_ttl_display(168), "1 week");
        assert_eq!(format_ttl_display(720), "30 days");
    }

    #[test]
    fn test_format_ttl_display_other_values() {
        assert_eq!(format_ttl_display(0), "No expiration");
        assert_eq!(format_ttl_display(2), "2 hours");
        assert_eq!(format_ttl_display(12), "12 hours");
        assert_eq!(format_ttl_display(48), "2 days");
        assert_eq!(format_ttl_display(336), "2 weeks");
        assert_eq!(format_ttl_display(1000), "41 days");
    }

    #[test]
    fn test_ttl_preset_index() {
        assert_eq!(ttl_preset_index(1), 0);
        assert_eq!(ttl_preset_index(24), 1);
        assert_eq!(ttl_preset_index(168), 2);
        assert_eq!(ttl_preset_index(720), 3);
        // Unknown value defaults to index 1 (24h)
        assert_eq!(ttl_preset_index(100), 1);
    }

    #[test]
    fn test_next_ttl_preset() {
        assert_eq!(next_ttl_preset(1), 24);
        assert_eq!(next_ttl_preset(24), 168);
        assert_eq!(next_ttl_preset(168), 720);
        assert_eq!(next_ttl_preset(720), 1); // Wraps around
    }

    #[test]
    fn test_prev_ttl_preset() {
        assert_eq!(prev_ttl_preset(1), 720); // Wraps around
        assert_eq!(prev_ttl_preset(24), 1);
        assert_eq!(prev_ttl_preset(168), 24);
        assert_eq!(prev_ttl_preset(720), 168);
    }

    // === Workflow Tests ===

    #[tokio::test]
    async fn test_list_invitations_default() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);

        let invitations = list_invitations(&app_core).await;
        assert_eq!(invitations.sent_count(), 0);
        assert_eq!(invitations.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_refresh_authoritative_invitation_readiness_tracks_pending_home_invitations() {
        let authority = AuthorityId::new_from_entropy([40u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        runtime.set_pending_invitations(vec![InvitationInfo {
            invitation_id: InvitationId::new("pending-home"),
            sender_id: AuthorityId::new_from_entropy([41u8; 32]),
            receiver_id: authority,
            invitation_type: InvitationBridgeType::Channel {
                home_id: ChannelId::from_bytes([42u8; 32]).to_string(),
                context_id: None,
                nickname_suggestion: Some("shared".to_string()),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        }]);
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        refresh_authoritative_invitation_readiness(&app_core)
            .await
            .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts
            .iter()
            .any(|fact| matches!(fact, AuthoritativeSemanticFact::PendingHomeInvitationReady)));

        runtime.set_pending_invitations(Vec::new());

        refresh_authoritative_invitation_readiness(&app_core)
            .await
            .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(!facts
            .iter()
            .any(|fact| matches!(fact, AuthoritativeSemanticFact::PendingHomeInvitationReady)));
    }

    #[tokio::test]
    async fn refresh_authoritative_invitation_readiness_ignores_sent_channel_invites_for_current_authority(
    ) {
        let authority = AuthorityId::new_from_entropy([73u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        runtime.set_pending_invitations(vec![InvitationInfo {
            invitation_id: InvitationId::new("sent-channel-pending"),
            sender_id: authority,
            receiver_id: AuthorityId::new_from_entropy([74u8; 32]),
            invitation_type: InvitationBridgeType::Channel {
                home_id: ChannelId::from_bytes([75u8; 32]).to_string(),
                context_id: None,
                nickname_suggestion: Some("shared".to_string()),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        }]);
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        refresh_authoritative_invitation_readiness(&app_core)
            .await
            .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(
            !facts
                .iter()
                .any(|fact| matches!(fact, AuthoritativeSemanticFact::PendingHomeInvitationReady)),
            "sent channel invites for the current authority must not advertise accept-pending readiness"
        );
    }

    #[tokio::test]
    async fn refresh_authoritative_invitation_readiness_requires_runtime() {
        let app_core = crate::testing::default_test_app_core();
        let error = refresh_authoritative_invitation_readiness(&app_core)
            .await
            .expect_err("authoritative invitation readiness requires runtime");
        assert!(matches!(error, AuraError::Internal { .. }));
        assert!(
            error.to_string().contains("Runtime bridge not available"),
            "expected explicit missing-runtime failure, got: {error}"
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn refresh_authoritative_invitation_readiness_uses_signal_pending_channel_invitation_when_runtime_snapshot_is_empty(
    ) {
        let authority = AuthorityId::new_from_entropy([165u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        runtime.set_pending_invitations(Vec::new());
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }
        emit_signal(
            &app_core,
            &*INVITATIONS_SIGNAL,
            crate::views::invitations::InvitationsState::from_parts(
                vec![crate::views::invitations::Invitation {
                    id: "signal-pending-channel".to_string(),
                    invitation_type: crate::views::invitations::InvitationType::Chat,
                    status: crate::views::invitations::InvitationStatus::Pending,
                    direction: crate::views::invitations::InvitationDirection::Received,
                    from_id: AuthorityId::new_from_entropy([164u8; 32]),
                    from_name: "Alice".to_string(),
                    to_id: None,
                    to_name: None,
                    created_at: 1,
                    expires_at: None,
                    message: None,
                    home_id: Some(ChannelId::from_bytes([163u8; 32])),
                    home_name: Some("shared-parity-lab".to_string()),
                }],
                Vec::new(),
                Vec::new(),
            ),
            "invitations",
        )
        .await
        .unwrap();

        refresh_authoritative_invitation_readiness(&app_core)
            .await
            .expect("signal-backed pending invitation should publish readiness");

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts
            .iter()
            .any(|fact| matches!(fact, AuthoritativeSemanticFact::PendingHomeInvitationReady)));
    }

    #[tokio::test]
    async fn test_refresh_authoritative_contact_link_readiness_tracks_contacts_signal() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }
        let contact_id = AuthorityId::new_from_entropy([50u8; 32]);
        let contact = crate::views::contacts::Contact {
            id: contact_id,
            nickname: "Bob".to_string(),
            nickname_suggestion: Some("Bob".to_string()),
            is_guardian: false,
            is_member: false,
            last_interaction: None,
            is_online: false,
            read_receipt_policy: crate::views::contacts::ReadReceiptPolicy::Disabled,
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        emit_signal(
            &app_core,
            &*crate::signal_defs::CONTACTS_SIGNAL,
            crate::views::contacts::ContactsState::from_contacts(vec![contact]),
            crate::signal_defs::CONTACTS_SIGNAL_NAME,
        )
        .await
        .unwrap();

        refresh_authoritative_contact_link_readiness(&app_core)
            .await
            .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::ContactLinkReady {
                authority_id,
                contact_count
            } if *authority_id == contact_id.to_string() && *contact_count == 1
        )));
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Contact,
                authority_id: Some(authority_id),
                operation_state: Some(OperationState::Succeeded),
            } if authority_id == &contact_id.to_string()
        )));
    }

    #[tokio::test]
    async fn refresh_authoritative_contact_link_readiness_preserves_generic_acceptance_facts() {
        let app_core = crate::testing::default_test_app_core();
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }
        let contact_id = AuthorityId::new_from_entropy([167u8; 32]);
        emit_signal(
            &app_core,
            &*crate::signal_defs::CONTACTS_SIGNAL,
            crate::views::contacts::ContactsState::from_contacts(vec![
                crate::views::contacts::Contact {
                    id: contact_id,
                    nickname: "Bob".to_string(),
                    nickname_suggestion: Some("Bob".to_string()),
                    is_guardian: false,
                    is_member: false,
                    last_interaction: None,
                    is_online: false,
                    read_receipt_policy: crate::views::contacts::ReadReceiptPolicy::Disabled,
                    relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
                },
            ]),
            crate::signal_defs::CONTACTS_SIGNAL_NAME,
        )
        .await
        .unwrap();
        {
            let mut core = app_core.write().await;
            core.set_authoritative_semantic_facts(vec![
                AuthoritativeSemanticFact::InvitationAccepted {
                    invitation_kind: InvitationFactKind::Generic,
                    authority_id: None,
                    operation_state: Some(OperationState::Succeeded),
                },
            ]);
        }

        refresh_authoritative_contact_link_readiness(&app_core)
            .await
            .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Generic,
                authority_id: None,
                operation_state: Some(OperationState::Succeeded),
            }
        )));
    }

    #[tokio::test]
    async fn refresh_authoritative_contact_link_readiness_requires_contacts_signal() {
        let app_core = crate::testing::default_test_app_core();

        let error = refresh_authoritative_contact_link_readiness(&app_core)
            .await
            .expect_err("contact-link readiness should require the contacts signal");
        assert!(matches!(error, AuraError::Internal { .. }));
    }

    #[tokio::test]
    async fn wait_for_contact_link_fails_explicitly_when_contacts_signal_is_unavailable() {
        let authority = AuthorityId::new_from_entropy([70u8; 32]);
        let runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge> =
            Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));

        let error = wait_for_contact_link(
            &app_core,
            &runtime,
            AuthorityId::new_from_entropy([71u8; 32]),
        )
        .await
        .expect_err("contact-link wait should fail when the contacts signal is unavailable");
        assert!(matches!(error, AcceptInvitationError::AcceptFailed { .. }));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn accept_pending_channel_invitation_without_pending_invites_publishes_terminal_failure()
    {
        let our_authority = AuthorityId::new_from_entropy([69u8; 32]);
        let runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge> = Arc::new(
            crate::runtime_bridge::OfflineRuntimeBridge::new(our_authority),
        );
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let instance_id = OperationInstanceId("pending-accept-1".to_string());
        let result =
            accept_pending_channel_invitation_with_instance(&app_core, Some(instance_id.clone()))
                .await;

        assert!(result.is_err());

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        crate::workflows::semantic_facts::assert_terminal_failure_or_cancelled(
            &facts,
            &OperationId::invitation_accept_channel(),
            &instance_id,
            SemanticOperationKind::AcceptPendingChannelInvitation,
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn channel_reconcile_materialization_preserves_terminal_success() {
        let our_authority = AuthorityId::new_from_entropy([81u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let channel_id = ChannelId::from_bytes([82u8; 32]);
        let context_id = ContextId::new_from_entropy([83u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([84u8; 32]);
        let instance_id = OperationInstanceId("invitation-accept-reconcile-1".to_string());

        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(
            context_id,
            channel_id,
            vec![our_authority, sender_id],
        );
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);

        apply_authoritative_membership_projection(
            &app_core,
            channel_id,
            context_id,
            true,
            Some("shared-parity-lab"),
        )
        .await
        .unwrap();

        crate::workflows::semantic_facts::SemanticWorkflowOwner::new(
            &app_core,
            OperationId::invitation_accept_channel(),
            Some(instance_id.clone()),
            SemanticOperationKind::AcceptPendingChannelInvitation,
        )
        .publish_success_with(
            crate::workflows::semantic_facts::issue_invitation_accepted_or_materialized_proof(
                InvitationId::new("accepted-from-test-history"),
            ),
        )
        .await
        .unwrap();

        reconcile_channel_invitation_acceptance(
            &app_core,
            &runtime_bridge,
            None,
            None,
            channel_id,
            Some(context_id),
            Some("shared-parity-lab"),
        )
        .await
        .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        crate::workflows::semantic_facts::assert_succeeded_with_postcondition(
            &facts,
            &OperationId::invitation_accept_channel(),
            &instance_id,
            SemanticOperationKind::AcceptPendingChannelInvitation,
            |facts| {
                facts.iter().any(|fact| {
                    matches!(
                        fact,
                        AuthoritativeSemanticFact::ChannelMembershipReady { channel, .. }
                            if channel.id.as_deref() == Some(channel_id.to_string().as_str())
                    )
                })
            },
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn channel_reconcile_requires_materialized_channel_state() {
        let our_authority = AuthorityId::new_from_entropy([85u8; 32]);
        let runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge> = Arc::new(
            crate::runtime_bridge::OfflineRuntimeBridge::new(our_authority),
        );
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let channel_id = ChannelId::from_bytes([86u8; 32]);
        let error = reconcile_channel_invitation_acceptance(
            &app_core,
            &runtime,
            None,
            None,
            channel_id,
            None,
            Some("shared-parity-lab"),
        )
        .await
        .expect_err("unmaterialized channel must fail reconciliation");
        assert!(matches!(error, AcceptInvitationError::AcceptFailed { .. }));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn channel_reconcile_uses_known_authoritative_context_without_reresolving_it() {
        let our_authority = AuthorityId::new_from_entropy([88u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let channel_id = ChannelId::from_bytes([89u8; 32]);
        let context_id = ContextId::new_from_entropy([90u8; 32]);
        runtime.set_amp_channel_state_exists_without_resolution(context_id, channel_id, true);
        runtime.set_amp_channel_participants_without_resolution(
            context_id,
            channel_id,
            vec![our_authority],
        );

        reconcile_channel_invitation_acceptance(
            &app_core,
            &runtime_bridge,
            None,
            None,
            channel_id,
            Some(context_id),
            Some("shared-parity-lab"),
        )
        .await
        .expect("known authoritative context should avoid re-resolution");
    }

    #[tokio::test]
    async fn accept_pending_channel_invitation_with_terminal_status_returns_direct_failure_status()
    {
        let our_authority = AuthorityId::new_from_entropy([111u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let outcome = accept_pending_channel_invitation_with_terminal_status(
            &app_core,
            Some(OperationInstanceId("accept-pending-direct-1".to_string())),
        )
        .await;

        assert!(outcome.result.is_err());
        let terminal = outcome
            .terminal
            .as_ref()
            .expect("owner-settled failure must produce a direct terminal status");
        assert_terminal_failure_status(
            terminal,
            SemanticOperationKind::AcceptPendingChannelInvitation,
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn accept_pending_channel_invitation_with_binding_terminal_status_returns_binding_witness(
    ) {
        let our_authority = AuthorityId::new_from_entropy([112u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        runtime.set_accept_invitation_result(Ok(
            crate::runtime_bridge::InvitationMutationOutcome {
                invitation_id: InvitationId::new("pending-channel-binding"),
                new_status: crate::runtime_bridge::InvitationBridgeStatus::Accepted,
            },
        ));
        let channel_id = ChannelId::from_bytes([113u8; 32]);
        let context_id = ContextId::new_from_entropy([114u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([115u8; 32]);
        runtime.set_pending_invitations(vec![InvitationInfo {
            invitation_id: InvitationId::new("pending-channel-binding"),
            sender_id,
            receiver_id: our_authority,
            invitation_type: InvitationBridgeType::Channel {
                home_id: channel_id.to_string(),
                context_id: Some(context_id),
                nickname_suggestion: Some("shared-room".to_string()),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        }]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(
            context_id,
            channel_id,
            vec![our_authority, sender_id],
        );
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);

        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let outcome = accept_pending_channel_invitation_with_binding_terminal_status(
            &app_core,
            Some(OperationInstanceId("accept-pending-binding-1".to_string())),
        )
        .await;

        let accepted = outcome
            .result
            .expect("accepted channel invitation should return a binding witness");
        assert_eq!(accepted.invitation_id, "pending-channel-binding");
        assert_eq!(accepted.binding.channel_id, channel_id.to_string());
        assert_eq!(accepted.binding.context_id, Some(context_id.to_string()));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn accept_pending_channel_invitation_with_binding_terminal_status_waits_for_authoritative_runtime_pending_snapshot_when_signal_indicates_pending(
    ) {
        let our_authority = AuthorityId::new_from_entropy([154u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([155u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        runtime.set_pending_invitations(Vec::new());
        let channel_id = ChannelId::from_bytes([156u8; 32]);
        let context_id = ContextId::new_from_entropy([157u8; 32]);

        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }
        emit_signal(
            &app_core,
            &*INVITATIONS_SIGNAL,
            crate::views::invitations::InvitationsState::from_parts(
                vec![crate::views::invitations::Invitation {
                    id: "pending-channel-signal-fallback".to_string(),
                    invitation_type: crate::views::invitations::InvitationType::Chat,
                    status: crate::views::invitations::InvitationStatus::Pending,
                    direction: crate::views::invitations::InvitationDirection::Received,
                    from_id: sender_id,
                    from_name: "Alice".to_string(),
                    to_id: None,
                    to_name: None,
                    created_at: 1,
                    expires_at: None,
                    message: None,
                    home_id: Some(channel_id),
                    home_name: Some("shared-parity-lab".to_string()),
                }],
                Vec::new(),
                Vec::new(),
            ),
            "invitations",
        )
        .await
        .unwrap();

        let runtime_for_pending_publish = runtime.clone();
        let delayed_pending_publish = async move {
            for _ in 0..4 {
                crate::workflows::runtime::cooperative_yield().await;
            }
            runtime_for_pending_publish.set_pending_invitations(vec![InvitationInfo {
                invitation_id: InvitationId::new("pending-channel-signal-fallback"),
                sender_id,
                receiver_id: our_authority,
                invitation_type: InvitationBridgeType::Channel {
                    home_id: channel_id.to_string(),
                    context_id: Some(context_id),
                    nickname_suggestion: Some("shared-parity-lab".to_string()),
                },
                status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
                created_at_ms: 1,
                expires_at_ms: None,
                message: None,
            }]);
        };
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime;
        let await_pending =
            authoritative_pending_home_or_channel_invitation_for_accept(&app_core, &runtime_bridge);
        let ((), pending) = tokio::join!(delayed_pending_publish, await_pending);

        let accepted = pending
            .expect("signal-indicated pending channel invitation should wait for authoritative runtime data");
        let accepted = accepted.expect("expected authoritative pending channel invitation");
        assert_eq!(
            accepted.invitation_id,
            InvitationId::new("pending-channel-signal-fallback")
        );
        assert_eq!(accepted.sender_id, sender_id);
        assert_eq!(accepted.receiver_id, our_authority);
        assert!(matches!(
            accepted.invitation_type,
            InvitationBridgeType::Channel {
                home_id,
                context_id: Some(found_context),
                nickname_suggestion: Some(_),
            } if home_id == channel_id.to_string() && found_context == context_id
        ));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn accept_contact_invitation_publishes_authoritative_invitation_accepted_fact() {
        let our_authority = AuthorityId::new_from_entropy([150u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([151u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        runtime.set_accept_invitation_result(Ok(
            crate::runtime_bridge::InvitationMutationOutcome {
                invitation_id: InvitationId::new("pending-contact-accepted"),
                new_status: crate::runtime_bridge::InvitationBridgeStatus::Accepted,
            },
        ));
        runtime.set_pending_invitations(vec![InvitationInfo {
            invitation_id: InvitationId::new("pending-contact-accepted"),
            sender_id,
            receiver_id: our_authority,
            invitation_type: InvitationBridgeType::Contact {
                nickname: Some("BobUser".to_string()),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        }]);

        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }
        emit_signal(
            &app_core,
            &*CONTACTS_SIGNAL,
            crate::views::contacts::ContactsState::from_contacts(vec![
                crate::views::contacts::Contact {
                    id: sender_id,
                    nickname: "BobUser".to_string(),
                    nickname_suggestion: Some("BobUser".to_string()),
                    is_guardian: false,
                    is_member: false,
                    last_interaction: None,
                    is_online: false,
                    read_receipt_policy: crate::views::contacts::ReadReceiptPolicy::Disabled,
                    relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
                },
            ]),
            CONTACTS_SIGNAL_NAME,
        )
        .await
        .unwrap();

        let accepted = accept_invitation_by_str_with_instance(
            &app_core,
            "pending-contact-accepted",
            Some(OperationInstanceId(
                "accept-contact-authoritative-fact-1".to_string(),
            )),
        )
        .await
        .expect("contact invitation acceptance should succeed");
        assert_eq!(
            accepted.invitation_id,
            InvitationId::new("pending-contact-accepted")
        );

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Contact,
                authority_id: Some(authority_id),
                operation_state: Some(OperationState::Succeeded),
            } if authority_id == &sender_id.to_string()
        )));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn accept_imported_contact_invitation_succeeds_before_contact_link_converges() {
        let our_authority = AuthorityId::new_from_entropy([152u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([153u8; 32]);
        let invitation = InvitationInfo {
            invitation_id: InvitationId::new("imported-contact-terminal"),
            sender_id,
            receiver_id: our_authority,
            invitation_type: InvitationBridgeType::Contact {
                nickname: Some("BobUser".to_string()),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        };
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        runtime.set_pending_invitations(vec![invitation.clone()]);
        runtime.set_accept_invitation_result(Ok(
            crate::runtime_bridge::InvitationMutationOutcome {
                invitation_id: invitation.invitation_id.clone(),
                new_status: crate::runtime_bridge::InvitationBridgeStatus::Accepted,
            },
        ));

        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let outcome = accept_imported_invitation_with_terminal_status(
            &app_core,
            InvitationHandle::new(invitation),
            Some(OperationInstanceId(
                "accept-imported-contact-terminal-1".to_string(),
            )),
        )
        .await;

        assert!(
            outcome.result.is_ok(),
            "terminal success should not wait for contact-link convergence"
        );
        assert!(matches!(
            outcome.terminal,
            Some(crate::ui_contract::WorkflowTerminalStatus {
                status: crate::ui_contract::SemanticOperationStatus {
                    kind: SemanticOperationKind::AcceptContactInvitation,
                    phase: SemanticOperationPhase::Succeeded,
                    ..
                },
                ..
            })
        ));

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Contact,
                authority_id: Some(authority_id),
                operation_state: Some(OperationState::Succeeded),
            } if authority_id == &sender_id.to_string()
        )));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn accept_pending_channel_invitation_refreshes_recipient_resolution_readiness() {
        let our_authority = AuthorityId::new_from_entropy([116u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([117u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        runtime.set_accept_invitation_result(Ok(
            crate::runtime_bridge::InvitationMutationOutcome {
                invitation_id: InvitationId::new("pending-channel-recipient-readiness"),
                new_status: crate::runtime_bridge::InvitationBridgeStatus::Accepted,
            },
        ));
        let channel_id = ChannelId::from_bytes([118u8; 32]);
        let context_id = ContextId::new_from_entropy([119u8; 32]);
        runtime.set_pending_invitations(vec![InvitationInfo {
            invitation_id: InvitationId::new("pending-channel-recipient-readiness"),
            sender_id,
            receiver_id: our_authority,
            invitation_type: InvitationBridgeType::Channel {
                home_id: channel_id.to_string(),
                context_id: Some(context_id),
                nickname_suggestion: Some("shared-parity-lab".to_string()),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        }]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(
            context_id,
            channel_id,
            vec![our_authority, sender_id],
        );
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);

        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let outcome = accept_pending_channel_invitation_with_binding_terminal_status(
            &app_core,
            Some(OperationInstanceId(
                "accept-pending-recipient-readiness-1".to_string(),
            )),
        )
        .await;
        outcome
            .result
            .expect("accepted channel invitation should refresh readiness facts");

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id.to_string().as_str())
                        && *member_count == 2
            )
        }));
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::RecipientPeersResolved { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id.to_string().as_str())
                        && *member_count == 2
            )
        }));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn accept_pending_channel_invitation_succeeds_when_participant_lookup_is_unavailable() {
        let our_authority = AuthorityId::new_from_entropy([120u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([121u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        runtime.set_accept_invitation_result(Ok(
            crate::runtime_bridge::InvitationMutationOutcome {
                invitation_id: InvitationId::new("pending-channel-membership-only"),
                new_status: crate::runtime_bridge::InvitationBridgeStatus::Accepted,
            },
        ));
        let channel_id = ChannelId::from_bytes([122u8; 32]);
        let context_id = ContextId::new_from_entropy([123u8; 32]);
        runtime.set_pending_invitations(vec![InvitationInfo {
            invitation_id: InvitationId::new("pending-channel-membership-only"),
            sender_id,
            receiver_id: our_authority,
            invitation_type: InvitationBridgeType::Channel {
                home_id: channel_id.to_string(),
                context_id: Some(context_id),
                nickname_suggestion: Some("shared-parity-lab".to_string()),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        }]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);

        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let outcome = accept_pending_channel_invitation_with_binding_terminal_status(
            &app_core,
            Some(OperationInstanceId(
                "accept-pending-membership-only-1".to_string(),
            )),
        )
        .await;
        outcome
            .result
            .expect("accepted channel invitation should succeed with membership readiness");

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id.to_string().as_str())
                        && *member_count == 1
            )
        }));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn accept_device_enrollment_invitation_fails_closed_on_ceremony_processing_error() {
        let authority = AuthorityId::new_from_entropy([121u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        runtime.set_accept_invitation_result(Ok(
            crate::runtime_bridge::InvitationMutationOutcome {
                invitation_id: InvitationId::new("device-enrollment-fail-closed"),
                new_status: crate::runtime_bridge::InvitationBridgeStatus::Accepted,
            },
        ));
        runtime.set_process_ceremony_result(Err(crate::core::IntentError::service_error(
            "ceremony inbox unavailable",
        )));
        let runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime;
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let invitation = InvitationInfo {
            invitation_id: InvitationId::new("device-enrollment-fail-closed"),
            sender_id: AuthorityId::new_from_entropy([122u8; 32]),
            receiver_id: authority,
            invitation_type: InvitationBridgeType::DeviceEnrollment {
                subject_authority: authority,
                initiator_device_id: aura_core::DeviceId::new_from_entropy([123u8; 32]),
                device_id: aura_core::DeviceId::new_from_entropy([124u8; 32]),
                nickname_suggestion: Some("Laptop".to_string()),
                ceremony_id: aura_core::CeremonyId::new("device-enrollment-ceremony"),
                pending_epoch: aura_core::Epoch(1),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        };

        let error = accept_device_enrollment_invitation(&app_core, &invitation)
            .await
            .expect_err("ceremony processing failure must terminate device enrollment acceptance");
        assert!(matches!(error, AuraError::Internal { .. }));

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::OperationStatus {
                    operation_id,
                    instance_id: None,
                    status,
                    ..
                } if operation_id == &OperationId::device_enrollment()
                    && status.kind == SemanticOperationKind::ImportDeviceEnrollmentCode
                    && status.phase == SemanticOperationPhase::Failed
            )
        }));
    }

    #[tokio::test]
    async fn authoritative_pending_home_invitation_prefers_received_pending_channel_invite() {
        let our_authority = AuthorityId::new_from_entropy([64u8; 32]);
        let sender = AuthorityId::new_from_entropy([65u8; 32]);
        let channel_id = ChannelId::from_bytes([66u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        runtime.set_pending_invitations(vec![
            InvitationInfo {
                invitation_id: InvitationId::new("sent-channel"),
                sender_id: our_authority,
                receiver_id: sender,
                invitation_type: InvitationBridgeType::Channel {
                    home_id: channel_id.to_string(),
                    context_id: None,
                    nickname_suggestion: Some("shared".to_string()),
                },
                status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
                created_at_ms: 1,
                expires_at_ms: None,
                message: Some("sent".to_string()),
            },
            InvitationInfo {
                invitation_id: InvitationId::new("received-channel"),
                sender_id: sender,
                receiver_id: our_authority,
                invitation_type: InvitationBridgeType::Channel {
                    home_id: channel_id.to_string(),
                    context_id: None,
                    nickname_suggestion: Some("shared".to_string()),
                },
                status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
                created_at_ms: 2,
                expires_at_ms: None,
                message: Some("join".to_string()),
            },
        ]);
        let runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime;

        let invitation = authoritative_pending_home_or_channel_invitation(&runtime)
            .await
            .expect("authoritative pending invitation should resolve")
            .expect("pending invitation should exist");
        assert_eq!(
            invitation.invitation_id,
            InvitationId::new("received-channel")
        );
        assert_eq!(invitation.sender_id, sender);
        assert_eq!(invitation.receiver_id, our_authority);
    }

    #[tokio::test]
    async fn authoritative_pending_home_invitation_ignores_contact_style_pending_invites() {
        let our_authority = AuthorityId::new_from_entropy([67u8; 32]);
        let sender = AuthorityId::new_from_entropy([68u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(
            our_authority,
        ));
        runtime.set_pending_invitations(vec![InvitationInfo {
            invitation_id: InvitationId::new("contact-style-home"),
            sender_id: sender,
            receiver_id: our_authority,
            invitation_type: InvitationBridgeType::Contact {
                nickname: Some("Alice".to_string()),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        }]);
        let runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime;

        assert!(authoritative_pending_home_or_channel_invitation(&runtime)
            .await
            .expect("authoritative pending lookup should succeed")
            .is_none());
    }

    #[test]
    fn test_channel_invitation_bootstrap_error_maps_to_typed_semantic_failure() {
        let channel_id = ChannelId::from_bytes([44u8; 32]);
        let error = ChannelInvitationBootstrapError::BootstrapUnavailable {
            channel_id,
            context_id: ContextId::new_from_entropy([45u8; 32]),
        };
        let semantic = error.semantic_error();
        assert_eq!(semantic.domain, SemanticFailureDomain::Transport);
        assert_eq!(
            semantic.code,
            SemanticFailureCode::ChannelBootstrapUnavailable
        );
        assert!(semantic
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(&channel_id.to_string())));
    }

    #[test]
    fn test_channel_invitation_create_failure_maps_to_typed_semantic_failure() {
        let channel_id = ChannelId::from_bytes([49u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([50u8; 32]);
        let error = ChannelInvitationBootstrapError::CreateFailed {
            channel_id,
            receiver_id,
            detail: "bridge create failed".to_string(),
        };
        let semantic = error.semantic_error();
        assert_eq!(semantic.domain, SemanticFailureDomain::Invitation);
        assert_eq!(semantic.code, SemanticFailureCode::InternalError);
        assert!(semantic
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(&channel_id.to_string())));
        assert!(semantic
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(&receiver_id.to_string())));
        assert!(semantic
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("bridge create failed")));
    }

    #[test]
    fn test_channel_invitation_timeout_maps_to_typed_semantic_failure() {
        let channel_id = ChannelId::from_bytes([51u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([52u8; 32]);
        let error = ChannelInvitationBootstrapError::CreateTimedOut {
            channel_id,
            receiver_id,
            timeout_ms: CHANNEL_INVITATION_CREATE_TIMEOUT_MS,
        };
        let semantic = error.semantic_error();
        assert_eq!(semantic.domain, SemanticFailureDomain::Invitation);
        assert_eq!(semantic.code, SemanticFailureCode::OperationTimedOut);
        assert!(semantic
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(&channel_id.to_string())));
        assert!(semantic
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(&receiver_id.to_string())));
        assert!(semantic.detail.as_deref().is_some_and(|detail| {
            detail.contains(&CHANNEL_INVITATION_CREATE_TIMEOUT_MS.to_string())
        }));
    }

    #[tokio::test]
    async fn test_fail_channel_invitation_publishes_terminal_failure_fact() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let channel_id = ChannelId::from_bytes([51u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([52u8; 32]);
        let owner = SemanticWorkflowOwner::new(
            &app_core,
            OperationId::invitation_create(),
            None,
            SemanticOperationKind::InviteActorToChannel,
        );
        let result = fail_channel_invitation::<()>(
            &owner,
            None,
            ChannelInvitationBootstrapError::CreateFailed {
                channel_id,
                receiver_id,
                detail: "typed create failure".to_string(),
            },
        )
        .await;

        assert!(result.is_err());

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                status,
                ..
            } if *operation_id == OperationId::invitation_create()
                && status.kind == SemanticOperationKind::InviteActorToChannel
                && status.phase == SemanticOperationPhase::Failed
                && status.error.as_ref().is_some_and(|error|
                    error.domain == SemanticFailureDomain::Invitation
                        && error.code == SemanticFailureCode::InternalError
                        && error.detail.as_deref().is_some_and(|detail| detail.contains("typed create failure"))
                )
        )));
    }

    #[test]
    fn test_accept_invitation_contact_link_failure_maps_to_typed_semantic_failure() {
        let contact_id = AuthorityId::new_from_entropy([46u8; 32]);
        let error = AcceptInvitationError::ContactLinkDidNotConverge { contact_id };
        let semantic = error.semantic_error(SemanticOperationKind::AcceptContactInvitation);
        assert_eq!(semantic.domain, SemanticFailureDomain::Invitation);
        assert_eq!(
            semantic.code,
            SemanticFailureCode::ContactLinkDidNotConverge
        );
        assert!(semantic
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(&contact_id.to_string())));
    }

    #[test]
    fn test_semantic_kind_for_bridge_invitation_uses_imported_type() {
        let sender = AuthorityId::new_from_entropy([47u8; 32]);
        let receiver = AuthorityId::new_from_entropy([48u8; 32]);

        let contact = crate::runtime_bridge::InvitationInfo {
            invitation_id: InvitationId::new("contact"),
            sender_id: sender,
            receiver_id: receiver,
            invitation_type: crate::runtime_bridge::InvitationBridgeType::Contact {
                nickname: None,
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        };
        assert_eq!(
            semantic_kind_for_bridge_invitation(&contact),
            SemanticOperationKind::AcceptContactInvitation
        );

        let channel = crate::runtime_bridge::InvitationInfo {
            invitation_id: InvitationId::new("channel"),
            sender_id: sender,
            receiver_id: receiver,
            invitation_type: crate::runtime_bridge::InvitationBridgeType::Channel {
                home_id: ChannelId::from_bytes([49u8; 32]).to_string(),
                context_id: None,
                nickname_suggestion: None,
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 1,
            expires_at_ms: None,
            message: None,
        };
        assert_eq!(
            semantic_kind_for_bridge_invitation(&channel),
            SemanticOperationKind::AcceptPendingChannelInvitation
        );
    }
}
