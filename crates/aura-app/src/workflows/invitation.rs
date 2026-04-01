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

// ============================================================================
// TTL Constants
// ============================================================================

/// 1 hour TTL preset in hours
pub const INVITATION_TTL_1_HOUR: u64 = 1;

/// 1 day (24 hours) TTL preset in hours
pub const INVITATION_TTL_1_DAY: u64 = 24;

/// 1 week (168 hours) TTL preset in hours
pub const INVITATION_TTL_1_WEEK: u64 = 168;

/// 30 days (720 hours) TTL preset in hours
pub const INVITATION_TTL_30_DAYS: u64 = 720;

/// Standard TTL presets in hours: 1h, 1d, 1w, 30d
pub const INVITATION_TTL_PRESETS: [u64; 4] = [
    INVITATION_TTL_1_HOUR,
    INVITATION_TTL_1_DAY,
    INVITATION_TTL_1_WEEK,
    INVITATION_TTL_30_DAYS,
];

/// Default TTL for invitations (24 hours)
pub const DEFAULT_INVITATION_TTL_HOURS: u64 = INVITATION_TTL_1_DAY;

/// Convert TTL from hours to milliseconds.
///
/// # Examples
///
/// ```ignore
/// use aura_app::workflows::invitation::ttl_hours_to_ms;
///
/// assert_eq!(ttl_hours_to_ms(1), 3_600_000);   // 1 hour
/// assert_eq!(ttl_hours_to_ms(24), 86_400_000); // 24 hours
/// ```
#[inline]
#[must_use]
pub const fn ttl_hours_to_ms(hours: u64) -> u64 {
    hours * 60 * 60 * 1000
}

/// Format TTL for human-readable display.
///
/// Returns a user-friendly string representation of the TTL duration.
///
/// # Examples
///
/// ```ignore
/// use aura_app::workflows::invitation::format_ttl_display;
///
/// assert_eq!(format_ttl_display(1), "1 hour");
/// assert_eq!(format_ttl_display(24), "1 day");
/// assert_eq!(format_ttl_display(168), "1 week");
/// assert_eq!(format_ttl_display(720), "30 days");
/// ```
#[must_use]
pub fn format_ttl_display(hours: u64) -> String {
    match hours {
        0 => "No expiration".to_string(),
        1 => "1 hour".to_string(),
        h if h < 24 => format!("{h} hours"),
        24 => "1 day".to_string(),
        h if h < 168 => {
            let days = h / 24;
            if days == 1 {
                "1 day".to_string()
            } else {
                format!("{days} days")
            }
        }
        168 => "1 week".to_string(),
        h if h < 720 => {
            let weeks = h / 168;
            if weeks == 1 {
                "1 week".to_string()
            } else {
                format!("{weeks} weeks")
            }
        }
        720 => "30 days".to_string(),
        h => {
            let days = h / 24;
            format!("{days} days")
        }
    }
}

/// Get the TTL preset index for a given hours value.
///
/// Returns the index in `INVITATION_TTL_PRESETS` that matches or is closest
/// to the given hours value.
#[must_use]
pub fn ttl_preset_index(hours: u64) -> usize {
    INVITATION_TTL_PRESETS
        .iter()
        .position(|&preset| preset == hours)
        .unwrap_or(1) // Default to 24h (index 1)
}

/// Get the next TTL preset from the current hours value.
///
/// Cycles through presets: 1h -> 24h -> 1w -> 30d -> 1h
#[must_use]
pub fn next_ttl_preset(current_hours: u64) -> u64 {
    let current_index = ttl_preset_index(current_hours);
    let next_index = (current_index + 1) % INVITATION_TTL_PRESETS.len();
    INVITATION_TTL_PRESETS[next_index]
}

/// Get the previous TTL preset from the current hours value.
///
/// Cycles through presets: 1h <- 24h <- 1w <- 30d <- 1h
#[must_use]
pub fn prev_ttl_preset(current_hours: u64) -> u64 {
    let current_index = ttl_preset_index(current_hours);
    let prev_index = if current_index == 0 {
        INVITATION_TTL_PRESETS.len() - 1
    } else {
        current_index - 1
    };
    INVITATION_TTL_PRESETS[prev_index]
}
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
use async_lock::RwLock;
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, InvitationId};
use aura_core::{
    AttemptBudget, AuraError, OperationContext, RetryBudgetPolicy, RetryRunError, TimeoutBudget,
    TimeoutBudgetError, TimeoutRunError, TraceContext,
};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

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
    #[cfg(not(feature = "wasm"))]
    eprintln!("[device-enrollment-accept] {message}");
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

async fn propagate_contact_acceptance_followup(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: AuthorityId,
) -> Result<(), AuraError> {
    let Ok(runtime) = require_runtime(app_core).await else {
        return Ok(());
    };
    let contact_peer_id = contact_id.to_string();
    let _ = wait_for_contact_link(app_core, &runtime, contact_id).await;
    for _ in 0..CONTACT_ACCEPT_PROPAGATION_ATTEMPTS {
        trigger_runtime_discovery_with_timeout(&runtime).await;
        let _ = crate::workflows::network::refresh_discovered_peers(app_core).await;
        let _ = timeout_runtime_call(
            &runtime,
            "accept_contact_invitation",
            "process_ceremony_messages",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.process_ceremony_messages(),
        )
        .await;
        let _ = timeout_runtime_call(
            &runtime,
            "accept_contact_invitation",
            "sync_with_peer",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.sync_with_peer(&contact_peer_id),
        )
        .await;
        let _ = timeout_runtime_call(
            &runtime,
            "accept_contact_invitation",
            "trigger_sync",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.trigger_sync(),
        )
        .await;
        converge_runtime(&runtime).await;
        let _ = crate::workflows::system::refresh_account(app_core).await;
        let _ = crate::workflows::network::refresh_discovered_peers(app_core).await;
        let _ = refresh_authoritative_contact_link_readiness(app_core).await;
        let linked = contacts_signal_snapshot(app_core)
            .await
            .map(|contacts| {
                contacts
                    .all_contacts()
                    .any(|contact| contact.id == contact_id)
            })
            .unwrap_or(false);
        let peer_online = timeout_runtime_call(
            &runtime,
            "accept_contact_invitation",
            "is_peer_online",
            INVITATION_RUNTIME_OPERATION_TIMEOUT,
            || runtime.is_peer_online(contact_id),
        )
        .await
        .unwrap_or(false);
        if linked && peer_online {
            break;
        }
    }
    Ok(())
}

/// Run bounded post-terminal contact-accept convergence without changing the
/// already-published terminal outcome.
pub async fn run_post_contact_accept_followups(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: AuthorityId,
) {
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(propagate_contact_acceptance_followup(app_core, contact_id))
        .await;
    let _ = best_effort.finish();
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

/// Refresh authoritative invitation readiness facts from the current invitation state.
pub(in crate::workflows) async fn refresh_authoritative_invitation_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    #[cfg(feature = "signals")]
    let signal_has_pending = invitations_signal_has_pending_home_or_channel_invitation(
        &read_signal_or_default(app_core, &*INVITATIONS_SIGNAL).await,
    );
    #[cfg(not(feature = "signals"))]
    let signal_has_pending = false;

    let runtime_has_pending = authoritative_pending_home_or_channel_invitation(&runtime)
        .await?
        .is_some();
    let replacements = if signal_has_pending || runtime_has_pending {
        vec![AuthoritativeSemanticFact::PendingHomeInvitationReady]
    } else {
        Vec::new()
    };
    replace_authoritative_semantic_facts_of_kind(
        app_core,
        aura_core::AuthorizedReadinessPublication::authorize(
            semantic_readiness_publication_capability(),
            (
                crate::ui_contract::AuthoritativeSemanticFactKind::PendingHomeInvitationReady,
                replacements,
            ),
        ),
    )
    .await
}

/// Refresh authoritative contact-link readiness facts from the current contacts state.
pub(in crate::workflows) async fn refresh_authoritative_contact_link_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let contacts = contacts_signal_snapshot(app_core).await?;
    let contact_count = contacts.contact_count() as u32;
    let contact_link_facts = contacts
        .all_contacts()
        .map(|contact| AuthoritativeSemanticFact::ContactLinkReady {
            authority_id: contact.id.to_string(),
            contact_count,
        })
        .collect::<Vec<_>>();
    let invitation_accepted_facts = contacts
        .all_contacts()
        .map(|contact| AuthoritativeSemanticFact::InvitationAccepted {
            invitation_kind: InvitationFactKind::Contact,
            authority_id: Some(contact.id.to_string()),
            operation_state: Some(OperationState::Succeeded),
        })
        .collect::<Vec<_>>();
    update_authoritative_semantic_facts(app_core, move |facts| {
        facts.retain(|existing| {
            !matches!(
                existing,
                AuthoritativeSemanticFact::ContactLinkReady { .. }
                    | AuthoritativeSemanticFact::InvitationAccepted {
                        invitation_kind: InvitationFactKind::Contact,
                        ..
                    }
            )
        });
        facts.extend(contact_link_facts.clone());
        facts.extend(invitation_accepted_facts);
    })
    .await
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_readiness",
    family = "authorizer"
)]
async fn publish_authoritative_contact_invitation_accepted(
    app_core: &Arc<RwLock<AppCore>>,
    authority_id: AuthorityId,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        aura_core::AuthorizedReadinessPublication::authorize(
            semantic_readiness_publication_capability(),
            AuthoritativeSemanticFact::InvitationAccepted {
                invitation_kind: InvitationFactKind::Contact,
                authority_id: Some(authority_id.to_string()),
                operation_state: Some(OperationState::Succeeded),
            },
        ),
    )
    .await
}

#[aura_macros::authoritative_source(kind = "signal")]
async fn contacts_signal_snapshot(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<crate::views::contacts::ContactsState, AuraError> {
    read_signal(app_core, &*CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME).await
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

/// Create a contact invitation while preserving a caller-owned semantic
/// operation instance identity.
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
    .map_err(|e| AuraError::from(super::error::runtime_call("create contact invitation", e)))?
    .map_err(|e| AuraError::from(super::error::runtime_call("create contact invitation", e)))
}

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

/// Create and export a contact invitation code as one typed terminal operation.
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
        let code = export_invitation_runtime(app_core, &invitation.invitation_id).await?;
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
            .publish_failure(command_terminal_error(error.to_string()))
            .await;
    }

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

/// Create a guardian invitation
///
/// **What it does**: Creates an invitation to become a guardian
/// **Returns**: InvitationInfo with the created invitation details
/// **Signal pattern**: RuntimeBridge handles state updates
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

/// Create a guardian invitation while preserving a caller-owned semantic
/// operation instance identity.
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
    .map_err(|e| AuraError::from(super::error::runtime_call("create guardian invitation", e)))?
    .map_err(|e| AuraError::from(super::error::runtime_call("create guardian invitation", e)))?;
    owner
        .publish_success_with(issue_invitation_created_proof(
            invitation.invitation_id.clone(),
        ))
        .await?;
    Ok(InvitationHandle::new(invitation))
}

/// Create a guardian invitation and return the directly-settled terminal
/// status for frontend handoff consumers.
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
        .map_err(|e| AuraError::from(super::error::runtime_call("create guardian invitation", e)))?
        .map_err(|e| {
            AuraError::from(super::error::runtime_call("create guardian invitation", e))
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

/// Create a channel invitation
///
/// **What it does**: Creates an invitation to join a channel
/// **Returns**: InvitationInfo with the created invitation details
/// **Signal pattern**: RuntimeBridge handles state updates
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

// ============================================================================
// Invitation Queries via RuntimeBridge
// ============================================================================

/// List pending invitations via RuntimeBridge
///
/// **What it does**: Gets all pending invitations from the RuntimeBridge
/// **Returns**: Vector of InvitationInfo
/// **Signal pattern**: Read-only operation (no emission)
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
    .map_err(|e| AuraError::from(super::error::runtime_call("list pending invitations", e)))?
    .map_err(|e| AuraError::from(super::error::runtime_call("list pending invitations", e)))
}

/// Import and get invitation details from a shareable code
///
/// **What it does**: Parses invite code and returns the details
/// **Returns**: InvitationInfo with parsed details
/// **Signal pattern**: Read-only until acceptance
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
    .map_err(|e| AuraError::from(super::error::runtime_call("import invitation", e)))?
    .map(InvitationHandle::new)
    .map_err(|e| AuraError::from(super::error::runtime_call("import invitation", e)))
}

async fn pending_invitation_info_by_id(
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
    .map_err(|e| AuraError::from(super::error::runtime_call("list pending invitations", e)))?
    .map_err(|e| AuraError::from(super::error::runtime_call("list pending invitations", e)))?;
    invitations
        .into_iter()
        .find(|invitation| invitation.invitation_id == invitation_id)
        .ok_or_else(|| AuraError::not_found(invitation_id.to_string()))
}

// ============================================================================
// Export Operations via RuntimeBridge
// ============================================================================

/// Export an invite code for sharing
///
/// **What it does**: Generates shareable invite code
/// **Returns**: Base64-encoded invite code
/// **Signal pattern**: Read-only operation (no emission)
///
/// This method is implemented via RuntimeBridge.export_invitation().
/// Takes a typed InvitationId, returns the shareable invite code as String.
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

/// Export an invitation by string ID for callers that only carry string keys.
pub async fn export_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<String, AuraError> {
    export_invitation(app_core, &InvitationId::new(invitation_id)).await
}

/// Export an invitation by string ID with typed terminal status publication.
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
                .publish_failure(command_terminal_error(error.to_string()))
                .await;
        }
    }

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

async fn export_invitation_runtime(
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
    .map_err(|e| AuraError::from(super::error::runtime_call("export invitation", e)))?
    .map_err(|e| AuraError::from(super::error::runtime_call("export invitation", e)))
}

/// Get current invitations state
///
/// **What it does**: Reads invitation state from INVITATIONS_SIGNAL
/// **Returns**: Current invitations (sent and received)
/// **Signal pattern**: Read-only operation (no emission)
pub async fn list_invitations(app_core: &Arc<RwLock<AppCore>>) -> InvitationsState {
    read_signal_or_default(app_core, &*INVITATIONS_SIGNAL).await
}

// ============================================================================
// Invitation Operations via RuntimeBridge
// ============================================================================

/// Accept an invitation
///
/// **What it does**: Accepts a received invitation via RuntimeBridge using typed InvitationId
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
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
            // Terminal success reflects accepted invitation ownership.
            // Contact-link materialization can lag and is observed separately
            // via readiness/runtime signals and refresh hooks.
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

/// Accept an invitation and attribute the semantic operation to a specific UI instance.
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
        SemanticOperationKind::AcceptPendingChannelInvitation => OperationId::invitation_accept_channel(),
        _ => OperationId::invitation_accept_contact(),
    };
    let owner = SemanticWorkflowOwner::new(
        app_core,
        operation_id,
        instance_id.clone(),
        operation_kind,
    );
    publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
        .await?;
    accept_invitation_id_owned(app_core, &invitation_id, &owner, None).await
}

/// Accept an imported invitation using the invitation metadata returned by the runtime bridge.
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
async fn accept_imported_invitation_owned(
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

async fn accept_imported_invitation_inner(
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
            // Imported contact acceptance should settle terminal status once
            // the accept itself succeeds. Contact-link materialization follows
            // via normal convergence and authoritative readiness hooks.
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

/// Accept an imported invitation and attribute the semantic operation to a specific UI instance.
pub async fn accept_imported_invitation_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: InvitationHandle,
    instance_id: Option<OperationInstanceId>,
) -> Result<(), AuraError> {
    let operation_kind = semantic_kind_for_bridge_invitation(invitation.info());
    let operation_id = match operation_kind {
        SemanticOperationKind::AcceptPendingChannelInvitation => OperationId::invitation_accept_channel(),
        _ => OperationId::invitation_accept_contact(),
    };
    let owner = SemanticWorkflowOwner::new(
        app_core,
        operation_id,
        instance_id.clone(),
        operation_kind,
    );
    publish_invitation_owner_status(&owner, None, SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let invitation = invitation.into_info();
    accept_imported_invitation_owned(app_core, &invitation, &owner, None).await
}

/// Accept an imported invitation and return the directly-settled terminal
/// status for frontend handoff consumers.
pub async fn accept_imported_invitation_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: InvitationHandle,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<()> {
    let operation_kind = semantic_kind_for_bridge_invitation(invitation.info());
    let operation_id = match operation_kind {
        SemanticOperationKind::AcceptPendingChannelInvitation => OperationId::invitation_accept_channel(),
        _ => OperationId::invitation_accept_contact(),
    };
    let owner = SemanticWorkflowOwner::new(
        app_core,
        operation_id,
        instance_id.clone(),
        operation_kind,
    );
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

/// Accept a device-enrollment invitation and wait for the local device view to converge.
pub async fn accept_device_enrollment_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &InvitationInfo,
) -> Result<(), AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        None,
        SemanticOperationKind::ImportDeviceEnrollmentCode,
    );
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let InvitationBridgeType::DeviceEnrollment { .. } = &invitation.invitation_type else {
        return fail_device_enrollment_accept(
            app_core,
            "accept_device_enrollment_invitation requires a device enrollment invitation",
        )
        .await;
    };

    let runtime = require_runtime(app_core).await?;
    log_device_enrollment_accept_progress(format!(
        "start invitation_id={};authority={}",
        invitation.invitation_id,
        runtime.authority_id()
    ));
    prime_device_enrollment_accept_connectivity(app_core, &runtime).await;
    log_device_enrollment_accept_progress(format!(
        "connectivity preflight complete invitation_id={}",
        invitation.invitation_id
    ));
    let accept_result = timeout_runtime_call(
        &runtime,
        "accept_device_enrollment_invitation",
        "accept_invitation",
        INVITATION_RUNTIME_OPERATION_TIMEOUT,
        || runtime.accept_invitation(invitation.invitation_id.as_str()),
    )
    .await;
    if let Err(error) = accept_result {
        return fail_device_enrollment_accept(
            app_core,
            format!("accept invitation failed: {error}"),
        )
        .await;
    }
    if let Ok(Err(error)) = accept_result {
        return fail_device_enrollment_accept(
            app_core,
            format!("accept invitation failed: {error}"),
        )
        .await;
    }
    log_device_enrollment_accept_progress(format!(
        "accept_invitation returned invitation_id={}",
        invitation.invitation_id
    ));
    converge_runtime(&runtime).await;
    log_device_enrollment_accept_progress(format!(
        "initial converge_runtime complete invitation_id={}",
        invitation.invitation_id
    ));

    let expected_min_devices = 2_usize;
    let policy = device_enrollment_accept_retry_policy()?;
    let invitation_id = invitation.invitation_id.clone();
    let enrollment_result: Result<(), DeviceEnrollmentAcceptConvergenceError> = {
        let mut attempts = AttemptBudget::new(policy.max_attempts());
        loop {
            let attempt = match attempts.record_attempt() {
                Ok(attempt) => attempt,
                Err(error) => {
                    break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                        AuraError::from(error),
                    ));
                }
            };
            #[cfg(not(feature = "instrumented"))]
            let _ = &invitation_id;
            log_device_enrollment_accept_progress(format!(
                "convergence attempt={attempt} invitation_id={invitation_id}"
            ));

            if let Err(error) = timeout_runtime_call(
                &runtime,
                "accept_device_enrollment_invitation",
                "process_ceremony_messages",
                INVITATION_RUNTIME_OPERATION_TIMEOUT,
                || runtime.process_ceremony_messages(),
            )
            .await
            .unwrap_or_else(|error| {
                Err(crate::core::IntentError::internal_error(error.to_string()))
            }) {
                break Err(DeviceEnrollmentAcceptConvergenceError::Terminal(format!(
                    "device enrollment ceremony processing failed during convergence: {error}"
                )));
            }
            log_device_enrollment_accept_progress(format!(
                "process_ceremony_messages ok attempt={attempt} invitation_id={invitation_id}"
            ));

            converge_runtime(&runtime).await;
            if let Err(error) = settings::refresh_settings_from_runtime(app_core).await {
                break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(error));
            }

            let runtime_device_count = match timeout_runtime_call(
                &runtime,
                "accept_device_enrollment_invitation",
                "try_list_devices",
                INVITATION_RUNTIME_QUERY_TIMEOUT,
                || runtime.try_list_devices(),
            )
            .await
            {
                Ok(Ok(devices)) => devices.len(),
                Ok(Err(error)) => {
                    break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                        AuraError::from(super::error::runtime_call("list devices", error)),
                    ));
                }
                Err(error) => {
                    break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                        AuraError::from(super::error::runtime_call("list devices", error)),
                    ));
                }
            };
            let settings_device_count = match settings::get_settings(app_core).await {
                Ok(settings) => settings.devices.len(),
                Err(error) => break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(error)),
            };
            log_device_enrollment_accept_progress(format!(
                "counts attempt={attempt} invitation_id={invitation_id} runtime_devices={runtime_device_count} settings_devices={settings_device_count} expected_min_devices={expected_min_devices}"
            ));
            #[cfg(feature = "instrumented")]
            tracing::info!(
                invitation_id = %invitation_id,
                attempt,
                runtime_device_count,
                settings_device_count,
                expected_min_devices,
                "device enrollment convergence poll"
            );
            if runtime_device_count >= expected_min_devices
                || settings_device_count >= expected_min_devices
            {
                settings::refresh_settings_from_runtime(app_core).await?;
                log_device_enrollment_accept_progress(format!(
                    "converged attempt={attempt} invitation_id={invitation_id}"
                ));
                if let Err(_error) =
                    ensure_runtime_peer_connectivity(&runtime, "device_enrollment_accept").await
                {
                    #[cfg(feature = "instrumented")]
                    tracing::warn!(
                        error = %_error,
                        invitation_id = %invitation_id,
                        "device enrollment acceptance completed without reachable peers"
                    );
                }
                break Ok(());
            }

            if !attempts.can_attempt() {
                break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                    AuraError::from(super::error::WorkflowError::Precondition(
                        "device enrollment acceptance not yet converged",
                    )),
                ));
            }

            let delay_ms = match u64::try_from(policy.delay_for_attempt(attempt).as_millis()) {
                Ok(delay_ms) => delay_ms,
                Err(_) => {
                    break Err(DeviceEnrollmentAcceptConvergenceError::Workflow(
                        AuraError::agent("device enrollment retry delay overflow"),
                    ));
                }
            };
            runtime.sleep_ms(delay_ms).await;
        }
    };
    match enrollment_result {
        Ok(()) => {
            log_device_enrollment_accept_progress(format!("success invitation_id={invitation_id}"));
            owner
                .publish_success_with(issue_device_enrollment_imported_proof(invitation_id))
                .await?;
            Ok(())
        }
        Err(DeviceEnrollmentAcceptConvergenceError::Terminal(detail)) => {
            fail_device_enrollment_accept(app_core, detail).await
        }
        Err(DeviceEnrollmentAcceptConvergenceError::Workflow(error)) => {
            #[cfg(feature = "instrumented")]
            tracing::warn!(
                invitation_id = %invitation.invitation_id,
                expected_min_devices,
                error = %error,
                "device enrollment acceptance failed before local device list convergence"
            );
            fail_device_enrollment_accept(
                app_core,
                format!(
                    "device enrollment acceptance did not converge to {expected_min_devices} local devices: {error}"
                ),
            )
            .await
        }
    }
}

/// Accept an invitation by string ID for callers that only carry string keys.
pub async fn accept_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<InvitationInfo, AuraError> {
    accept_invitation_by_str_with_instance(app_core, invitation_id, None).await
}

/// Accept an invitation by string ID while preserving a caller-owned semantic
/// operation instance identity.
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

/// Accept an existing pending invitation and return the directly-settled
/// terminal status for frontend handoff consumers.
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
        SemanticOperationKind::AcceptPendingChannelInvitation => OperationId::invitation_accept_channel(),
        _ => OperationId::invitation_accept_contact(),
    };
    let owner = SemanticWorkflowOwner::new(
        app_core,
        operation_id,
        instance_id,
        kind,
    );
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
                .publish_failure(command_terminal_error(error.to_string()))
                .await;
        }
    }

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

/// Decline an invitation using typed InvitationId
///
/// **What it does**: Declines a received invitation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
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
    .map_err(|e| AuraError::from(super::error::runtime_call("decline invitation", e)))?
    .map_err(|e| AuraError::from(super::error::runtime_call("decline invitation", e)))?;
    Ok(())
}

/// Decline an invitation by string ID for callers that only carry string keys.
pub async fn decline_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    let invitation = pending_invitation_info_by_id(app_core, invitation_id).await?;
    decline_invitation(app_core, InvitationHandle::new(invitation)).await
}

/// Decline an invitation by string ID with typed terminal status publication.
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
                .publish_failure(command_terminal_error(error.to_string()))
                .await;
        }
    }

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

/// Cancel an invitation using typed InvitationId
///
/// **What it does**: Cancels a sent invitation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
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
    .map_err(|e| AuraError::from(super::error::runtime_call("cancel invitation", e)))?
    .map_err(|e| AuraError::from(super::error::runtime_call("cancel invitation", e)))?;
    Ok(())
}

/// Cancel an invitation by string ID for callers that only carry string keys.
pub async fn cancel_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    let invitation = pending_invitation_info_by_id(app_core, invitation_id).await?;
    cancel_invitation(app_core, InvitationHandle::new(invitation)).await
}

/// Cancel an invitation by string ID with typed terminal status publication.
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
                .publish_failure(command_terminal_error(error.to_string()))
                .await;
        }
    }

    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

/// Import an invitation from a shareable code
///
/// **What it does**: Parses and validates invite code via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
///
/// The code parsing and validation is handled by the RuntimeBridge implementation.
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
    .map_err(|e| AuraError::from(super::error::runtime_call("import invitation", e)))?
    .map_err(|e| AuraError::from(super::error::runtime_call("import invitation", e)))?;

    if let Err(_error) = crate::workflows::system::refresh_account(app_core).await {
        #[cfg(feature = "instrumented")]
        tracing::debug!(error = %_error, "refresh_account after invitation import failed");
    }

    refresh_authoritative_invitation_readiness(app_core).await
}

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

use crate::views::invitations::InvitationType;

/// Portable invitation role value for CLI parsing.
///
/// This enum represents the user-facing role categories for invitation creation.
/// It maps to the underlying `InvitationType` but includes additional context
/// like whether it's a "contact" (default) invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationRoleValue {
    /// Contact invitation.
    Contact,
    /// Guardian invitation
    Guardian,
    /// Channel/Chat invitation
    Channel,
}

impl InvitationRoleValue {
    /// Get the canonical string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Contact => "contact",
            Self::Guardian => "guardian",
            Self::Channel => "channel",
        }
    }

    /// Convert to `InvitationType`.
    #[must_use]
    pub fn to_invitation_type(&self) -> InvitationType {
        match self {
            Self::Contact => InvitationType::Home,
            Self::Guardian => InvitationType::Guardian,
            Self::Channel => InvitationType::Chat,
        }
    }
}

impl std::fmt::Display for InvitationRoleValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contact => write!(f, "contact"),
            Self::Guardian => write!(f, "guardian"),
            Self::Channel => write!(f, "channel"),
        }
    }
}

/// Strict invitation role parse errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationRoleParseError {
    /// Role input was empty.
    Empty,
    /// Role input does not match a supported role.
    InvalidRole(String),
}

impl std::fmt::Display for InvitationRoleParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "role cannot be empty"),
            Self::InvalidRole(role) => write!(
                f,
                "invalid invitation role '{role}' (expected one of: contact, guardian, channel)"
            ),
        }
    }
}

impl std::error::Error for InvitationRoleParseError {}

/// Parse an invitation role string into a portable value.
///
/// Recognizes "contact", "guardian", and "channel" (case-insensitive).
/// Unknown roles are rejected with a parse error.
///
/// # Examples
///
/// ```ignore
/// use aura_app::workflows::invitation::parse_invitation_role;
///
/// // Known roles
/// let guardian = parse_invitation_role("guardian").unwrap();
/// assert!(matches!(guardian, InvitationRoleValue::Guardian));
///
/// let channel = parse_invitation_role("CHANNEL").unwrap();
/// assert!(matches!(channel, InvitationRoleValue::Channel));
///
/// // Invalid roles fail
/// assert!(parse_invitation_role("friend").is_err());
/// ```
pub fn parse_invitation_role(role: &str) -> Result<InvitationRoleValue, InvitationRoleParseError> {
    let normalized = role.trim();
    if normalized.is_empty() {
        return Err(InvitationRoleParseError::Empty);
    }
    if normalized.eq_ignore_ascii_case("contact") {
        return Ok(InvitationRoleValue::Contact);
    }
    if normalized.eq_ignore_ascii_case("guardian") {
        return Ok(InvitationRoleValue::Guardian);
    }
    if normalized.eq_ignore_ascii_case("channel") {
        return Ok(InvitationRoleValue::Channel);
    }
    Err(InvitationRoleParseError::InvalidRole(
        normalized.to_string(),
    ))
}

/// Format an invitation type for human-readable display.
///
/// Provides consistent formatting of invitation types across all frontends.
#[must_use]
pub fn format_invitation_type(inv_type: InvitationType) -> &'static str {
    match inv_type {
        InvitationType::Home => "Home",
        InvitationType::Guardian => "Guardian",
        InvitationType::Chat => "Channel",
    }
}

/// Format an invitation type with additional context.
///
/// For more detailed formatting that includes context like channel IDs or authorities.
#[must_use]
pub fn format_invitation_type_detailed(inv_type: InvitationType, context: Option<&str>) -> String {
    match (inv_type, context) {
        (InvitationType::Home, None) => "Home".to_string(),
        (InvitationType::Home, Some(ctx)) => format!("Home ({ctx})"),
        (InvitationType::Guardian, None) => "Guardian".to_string(),
        (InvitationType::Guardian, Some(ctx)) => format!("Guardian (for: {ctx})"),
        (InvitationType::Chat, None) => "Channel".to_string(),
        (InvitationType::Chat, Some(ctx)) => format!("Channel ({ctx})"),
    }
}

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
    let _ = accept_imported_invitation_inner(app_core, &invitation, owner).await?;
    owner
        .publish_success_with(issue_pending_invitation_consumed_proof(
            invitation_id.clone(),
        ))
        .await?;
    Ok(invitation_id)
}

/// Accept the current pending channel invitation and attribute the semantic operation to a specific UI instance.
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

/// Accept the current pending channel invitation and return the directly-settled
/// terminal status for frontend handoff consumers.
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
async fn run_post_channel_accept_followups(
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

/// Accept the current pending channel invitation and return the settled
/// invitation id plus the authoritative channel selection/binding witness.
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
        accept_imported_invitation_owned(app_core, &pending_invitation, &owner, None).await?;
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

    // === Invitation Role Parsing Tests ===

    #[test]
    fn accept_pending_channel_invitation_owned_boundary_is_declared_and_wrapped() {
        let source = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/workflows/invitation.rs"),
        )
        .expect("invitation workflow source should be readable");

        assert!(source.contains("owner = \"accept_pending_channel_invitation_id_owned\""));
        assert!(source.contains("async fn accept_pending_channel_invitation_id_owned("));
        assert!(source.contains(
            "let _ = accept_imported_invitation_inner(app_core, &invitation, owner).await?;"
        ));
        assert!(source.contains("issue_pending_invitation_consumed_proof("));
    }

    #[test]
    fn accept_imported_invitation_owned_boundary_preserves_wrapper_and_inner_split() {
        let source = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/workflows/invitation.rs"),
        )
        .expect("invitation workflow source should be readable");

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
        let source = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/workflows/invitation.rs"),
        )
        .expect("invitation workflow source should be readable");

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
