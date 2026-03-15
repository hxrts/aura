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
use crate::ui::signals::CONTACTS_SIGNAL;
use crate::ui_contract::AuthoritativeSemanticFact;
use crate::ui_contract::{
    OperationId, OperationInstanceId, SemanticOperationKind, SemanticOperationPhase,
    SemanticOperationStatus,
};
use crate::workflows::runtime::{
    converge_runtime, ensure_runtime_peer_connectivity, require_runtime,
};
use crate::workflows::runtime_error_classification::{
    classify_amp_channel_error, classify_invitation_accept_error, AmpChannelErrorClass,
    InvitationAcceptErrorClass,
};
use crate::workflows::semantic_facts::{
    publish_authoritative_operation_failure, publish_authoritative_operation_phase,
    publish_authoritative_semantic_fact, replace_authoritative_semantic_facts_of_kind,
    update_authoritative_semantic_facts,
};
use crate::workflows::settings;
#[cfg(feature = "signals")]
use crate::workflows::signals::read_signal;
use crate::workflows::signals::read_signal_or_default;
use crate::workflows::time;
use crate::{views::invitations::InvitationsState, AppCore};
use async_lock::RwLock;
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId, InvitationId};
use aura_core::AuraError;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

const CONTACT_LINK_ATTEMPTS: usize = 32;
const CONTACT_LINK_BACKOFF_MS: u64 = 100;
const CHANNEL_BOOTSTRAP_RETRY_ATTEMPTS: usize = 6;
const CHANNEL_BOOTSTRAP_RETRY_BACKOFF_MS: u64 = 75;
const CHANNEL_INVITATION_CREATE_TIMEOUT_MS: u64 = 5_000;

fn update_channel_invitation_stage(
    tracker: &Option<Arc<Mutex<&'static str>>>,
    stage: &'static str,
) {
    if let Some(tracker) = tracker {
        if let Ok(mut guard) = tracker.lock() {
            *guard = stage;
        }
    }
}

async fn timeout_channel_invitation_stage_with_deadline<T>(
    stage: &'static str,
    deadline: Option<Instant>,
    future: impl std::future::Future<Output = Result<T, AuraError>>,
) -> Result<T, AuraError> {
    let timeout = match deadline {
        Some(deadline) => {
            let now = Instant::now();
            if now >= deadline {
                return Err(AuraError::from(crate::workflows::error::WorkflowError::TimedOut {
                    operation: "create_channel_invitation",
                    stage,
                    timeout_ms: 0,
                }));
            }
            std::cmp::min(
                deadline.duration_since(now),
                Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS),
            )
        }
        None => Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS),
    };
    tokio::time::timeout(
        timeout,
        future,
    )
    .await
    .map_err(|_| {
        AuraError::from(crate::workflows::error::WorkflowError::TimedOut {
            operation: "create_channel_invitation",
            stage,
            timeout_ms: timeout.as_millis() as u64,
        })
    })?
}

fn channel_invitation_bootstrap_timeout(
    deadline: Option<Instant>,
    channel_id: ChannelId,
    stage: &'static str,
    context_id: Option<ContextId>,
) -> Result<Duration, ChannelInvitationBootstrapError> {
    match deadline {
        Some(deadline) => {
            let now = Instant::now();
            if now >= deadline {
                let context_detail = context_id
                    .map(|context| format!(" in context {context}"))
                    .unwrap_or_default();
                return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: format!("create_channel_invitation deadline exhausted before {stage}{context_detail}"),
                });
            }
            Ok(std::cmp::min(
                deadline.duration_since(now),
                Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS),
            ))
        }
        None => Ok(Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS)),
    }
}

fn has_pending_home_or_channel_invitation(invitations: &InvitationsState) -> bool {
    invitations
        .all_pending()
        .iter()
        .chain(invitations.all_sent().iter())
        .any(|invitation| {
            matches!(
                invitation.invitation_type,
                crate::views::invitations::InvitationType::Home
                    | crate::views::invitations::InvitationType::Chat
            ) && invitation.status == crate::views::invitations::InvitationStatus::Pending
        })
}

async fn publish_invitation_operation_status(
    app_core: &Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    deadline: Option<Instant>,
    kind: SemanticOperationKind,
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
    if let Some(instance_id) = instance_id {
        timeout_channel_invitation_stage_with_deadline(
            stage,
            deadline,
            publish_authoritative_semantic_fact(
                app_core,
                AuthoritativeSemanticFact::OperationStatus {
                    operation_id,
                    instance_id: Some(instance_id),
                    status: SemanticOperationStatus::new(kind, phase),
                },
            ),
        )
        .await
    } else {
        timeout_channel_invitation_stage_with_deadline(
            stage,
            deadline,
            publish_authoritative_operation_phase(app_core, operation_id, kind, phase),
        )
        .await
    }
}

async fn publish_contact_accept_success(
    app_core: &Arc<RwLock<AppCore>>,
    authority_id: AuthorityId,
    contact_count: u32,
) -> Result<(), AuraError> {
    let contact_link = AuthoritativeSemanticFact::ContactLinkReady {
        authority_id: authority_id.to_string(),
        contact_count,
    };
    let operation_status = AuthoritativeSemanticFact::OperationStatus {
        operation_id: OperationId::invitation_accept(),
        instance_id: None,
        status: crate::ui_contract::SemanticOperationStatus::new(
            SemanticOperationKind::AcceptContactInvitation,
            SemanticOperationPhase::Succeeded,
        ),
    };

    update_authoritative_semantic_facts(app_core, |facts| {
        facts.retain(|existing| {
            existing.key() != contact_link.key() && existing.key() != operation_status.key()
        });
        facts.push(contact_link);
        facts.push(operation_status);
    })
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
            SemanticOperationKind::AcceptPendingChannelInvitation
        }
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
    deadline: Option<Instant>,
    kind: SemanticOperationKind,
    error: crate::ui_contract::SemanticOperationError,
) -> Result<(), AuraError> {
    if let Some(instance_id) = instance_id {
        timeout_channel_invitation_stage_with_deadline(
            "publish_failure",
            deadline,
            publish_authoritative_semantic_fact(
                app_core,
                AuthoritativeSemanticFact::OperationStatus {
                    operation_id,
                    instance_id: Some(instance_id),
                    status: SemanticOperationStatus::failed(kind, error),
                },
            ),
        )
        .await
    } else {
        timeout_channel_invitation_stage_with_deadline(
            "publish_failure",
            deadline,
            publish_authoritative_operation_failure(app_core, operation_id, kind, error),
        )
        .await
    }
}

async fn fail_channel_invitation<T>(
    app_core: &Arc<RwLock<AppCore>>,
    instance_id: Option<OperationInstanceId>,
    _deadline: Option<Instant>,
    error: ChannelInvitationBootstrapError,
) -> Result<T, AuraError> {
    publish_invitation_operation_failure(
        app_core,
        OperationId::invitation_create(),
        instance_id,
        None,
        SemanticOperationKind::InviteActorToChannel,
        error.semantic_error(),
    )
    .await?;
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
    app_core: &Arc<RwLock<AppCore>>,
    kind: SemanticOperationKind,
    error: AcceptInvitationError,
) -> Result<T, AuraError> {
    publish_invitation_operation_failure(
        app_core,
        OperationId::invitation_accept(),
        None,
        None,
        kind,
        error.semantic_error(kind),
    )
    .await?;
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
    publish_authoritative_operation_failure(
        app_core,
        OperationId::device_enrollment(),
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        error.clone(),
    )
    .await?;
    Err(AuraError::agent(error.detail.unwrap_or_else(|| {
        "device enrollment acceptance failed".to_string()
    })))
}

async fn ensure_channel_invitation_context_and_bootstrap(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    receiver: AuthorityId,
    channel_id: ChannelId,
    context_id: Option<ContextId>,
    bootstrap: Option<ChannelBootstrapPackage>,
    stage_tracker: &Option<Arc<Mutex<&'static str>>>,
    deadline: Option<Instant>,
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
        "resolve_runtime_channel_context",
        deadline,
        async {
            runtime
                .resolve_amp_channel_context(channel_id)
                .await
                .map_err(|error| AuraError::internal(error.to_string()))
        },
    )
    .await
    .map_err(|error| ChannelInvitationBootstrapError::BootstrapTransport {
        channel_id,
        detail: format!(
            "{}; requested_context={requested_context:?}; resolved_context_before_runtime={resolved_context}",
            error
        ),
    })? {
        runtime_resolved_context = Some(runtime_context);
        resolved_context = runtime_context;
    }

    let invitees = vec![receiver];
    for attempt in 0..=CHANNEL_BOOTSTRAP_RETRY_ATTEMPTS {
        update_channel_invitation_stage(stage_tracker, "amp_create_channel_bootstrap");
        let bootstrap_timeout = channel_invitation_bootstrap_timeout(
            deadline,
            channel_id,
            "amp_create_channel_bootstrap",
            Some(resolved_context),
        )?;
        let bootstrap_attempt = tokio::time::timeout(
            bootstrap_timeout,
            runtime.amp_create_channel_bootstrap(resolved_context, channel_id, invitees.clone()),
        )
        .await;
        match bootstrap_attempt {
            Err(_) => {
                return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: format!(
                        "amp_create_channel_bootstrap timed out after {}ms in context {resolved_context}",
                        CHANNEL_INVITATION_CREATE_TIMEOUT_MS
                    ),
                });
            }
            Ok(result) => match result {
                Ok(bootstrap) => return Ok((resolved_context, bootstrap)),
                Err(error)
                    if classify_amp_channel_error(&error)
                        == AmpChannelErrorClass::ChannelStateUnavailable =>
                {
                    if attempt == CHANNEL_BOOTSTRAP_RETRY_ATTEMPTS {
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
                        .sleep_ms(CHANNEL_BOOTSTRAP_RETRY_BACKOFF_MS * (attempt as u64 + 1))
                        .await;
                    update_channel_invitation_stage(stage_tracker, "amp_channel_state_exists");
                    let exists_timeout = channel_invitation_bootstrap_timeout(
                        deadline,
                        channel_id,
                        "amp_channel_state_exists",
                        Some(resolved_context),
                    )?;
                    let state_exists = match tokio::time::timeout(
                        exists_timeout,
                        runtime.amp_channel_state_exists(resolved_context, channel_id),
                    )
                    .await
                    {
                        Err(_) => {
                            return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                                channel_id,
                                detail: format!(
                                    "amp_channel_state_exists timed out after {}ms in context {resolved_context}",
                                    CHANNEL_INVITATION_CREATE_TIMEOUT_MS
                                ),
                            });
                        }
                        Ok(Ok(state_exists)) => state_exists,
                        Ok(Err(state_error)) => {
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
                Err(error) => {
                    return Err(ChannelInvitationBootstrapError::BootstrapTransport {
                        channel_id,
                        detail: format!(
                            "{}; requested_context={requested_context:?}; runtime_resolved_context={runtime_resolved_context:?}; bootstrap_context={resolved_context}",
                            error
                        ),
                    });
                }
            },
        }
    }

    Err(ChannelInvitationBootstrapError::BootstrapUnavailable {
        channel_id,
        context_id: resolved_context,
    })
}

/// Refresh authoritative invitation readiness facts from the current invitation state.
pub async fn refresh_authoritative_invitation_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let invitations = read_signal_or_default(app_core, &*INVITATIONS_SIGNAL).await;
    let replacements = if has_pending_home_or_channel_invitation(&invitations) {
        vec![AuthoritativeSemanticFact::PendingHomeInvitationReady]
    } else {
        Vec::new()
    };
    replace_authoritative_semantic_facts_of_kind(
        app_core,
        crate::ui_contract::AuthoritativeSemanticFactKind::PendingHomeInvitationReady,
        replacements,
    )
    .await
}

/// Refresh authoritative contact-link readiness facts from the current contacts state.
pub async fn refresh_authoritative_contact_link_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let contacts = read_signal_or_default(app_core, &*CONTACTS_SIGNAL).await;
    let contact_count = contacts.contact_count() as u32;
    let replacements = contacts
        .all_contacts()
        .map(|contact| AuthoritativeSemanticFact::ContactLinkReady {
            authority_id: contact.id.to_string(),
            contact_count,
        })
        .collect::<Vec<_>>();
    replace_authoritative_semantic_facts_of_kind(
        app_core,
        crate::ui_contract::AuthoritativeSemanticFactKind::ContactLinkReady,
        replacements,
    )
    .await
}

#[cfg(feature = "signals")]
async fn reconcile_accepted_channel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    channel_id: ChannelId,
    sender_id: AuthorityId,
    context_hint: Option<ContextId>,
    channel_name_hint: Option<&str>,
) -> Result<(), AuraError> {
    const CHANNEL_CONTEXT_ATTEMPTS: usize = 60;
    const CHANNEL_CONTEXT_BACKOFF_MS: u64 = 100;

    let mut authoritative_context = context_hint;
    if authoritative_context.is_none() {
        for attempt in 0..CHANNEL_CONTEXT_ATTEMPTS {
            authoritative_context =
                crate::workflows::messaging::authoritative_context_id_for_channel(app_core, channel_id)
                    .await;
            if authoritative_context.is_some() {
                break;
            }
            if attempt + 1 < CHANNEL_CONTEXT_ATTEMPTS {
                converge_runtime(runtime).await;
                runtime.sleep_ms(CHANNEL_CONTEXT_BACKOFF_MS).await;
            }
        }
    }
    if authoritative_context.is_none() {
        let channel_visible = crate::workflows::snapshot_policy::chat_snapshot(app_core)
            .await
            .channel(&channel_id)
            .is_some();
        if !channel_visible {
            let _ = crate::workflows::messaging::join_channel(app_core, channel_id).await;
        }
        authoritative_context =
            crate::workflows::messaging::authoritative_context_id_for_channel(app_core, channel_id)
                .await;
    }
    let authoritative_context = authoritative_context.ok_or_else(|| {
        AuraError::from(super::error::WorkflowError::Precondition(
            "Accepted channel invitation but no authoritative context was materialized",
        ))
    })?;
    crate::workflows::messaging::project_channel_peer_membership_with_context(
        app_core,
        channel_id,
        Some(authoritative_context),
        sender_id,
        channel_name_hint,
    )
    .await?;
    crate::workflows::messaging::refresh_authoritative_channel_membership_readiness(app_core)
        .await?;
    if crate::workflows::snapshot_policy::chat_snapshot(app_core)
        .await
        .channel(&channel_id)
        .is_none()
    {
        let _ = crate::workflows::messaging::join_channel(app_core, channel_id).await;
    }
    converge_runtime(runtime).await;
    Ok(())
}

#[cfg(not(feature = "signals"))]
async fn reconcile_accepted_channel_invitation(
    _app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    _channel_id: ChannelId,
    _sender_id: AuthorityId,
    _channel_name_hint: Option<&str>,
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
) -> Result<InvitationInfo, AuraError> {
    publish_invitation_operation_status(
        app_core,
        OperationId::invitation_create(),
        None,
        None,
        SemanticOperationKind::CreateContactInvitation,
        SemanticOperationPhase::WorkflowDispatched,
    )
    .await?;
    let runtime = require_runtime(app_core).await?;

    let invitation = runtime
        .create_contact_invitation(receiver, nickname, message, ttl_ms)
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("create contact invitation", e)))?;
    Ok(invitation)
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
) -> Result<InvitationInfo, AuraError> {
    publish_invitation_operation_status(
        app_core,
        OperationId::invitation_create(),
        None,
        None,
        SemanticOperationKind::CreateContactInvitation,
        SemanticOperationPhase::WorkflowDispatched,
    )
    .await?;
    let runtime = require_runtime(app_core).await?;

    let invitation = runtime
        .create_guardian_invitation(receiver, subject, message, ttl_ms)
        .await
        .map_err(|e| {
            AuraError::from(super::error::runtime_call("create guardian invitation", e))
        })?;
    Ok(invitation)
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
    deadline: Option<Instant>,
    external_stage_tracker: Option<Arc<Mutex<&'static str>>>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationInfo, AuraError> {
    let stage_tracker = external_stage_tracker.or_else(|| Some(Arc::new(Mutex::new("require_runtime"))));
    let fallback_channel_id = home_id.parse::<ChannelId>().ok();
    let invitation_result = tokio::time::timeout(
        Duration::from_millis(CHANNEL_INVITATION_CREATE_TIMEOUT_MS),
        async {
            let runtime = timeout_channel_invitation_stage_with_deadline(
                "require_runtime",
                deadline,
                require_runtime(app_core),
            )
            .await
            .map_err(|error| ChannelInvitationBootstrapError::BootstrapTransport {
                channel_id: fallback_channel_id.unwrap_or_else(|| ChannelId::new(aura_core::Hash32([0; 32]))),
                detail: error.to_string(),
            })?;
            update_channel_invitation_stage(&stage_tracker, "publish_workflow_dispatched");
            publish_invitation_operation_status(
                app_core,
                OperationId::invitation_create(),
                operation_instance_id.clone(),
                deadline,
                SemanticOperationKind::InviteActorToChannel,
                SemanticOperationPhase::WorkflowDispatched,
            )
            .await
            .map_err(|error| ChannelInvitationBootstrapError::BootstrapTransport {
                channel_id: fallback_channel_id.unwrap_or_else(|| ChannelId::new(aura_core::Hash32([0; 32]))),
                detail: error.to_string(),
            })?;
            update_channel_invitation_stage(&stage_tracker, "parse_channel_id");
            let channel_id = match home_id.parse::<ChannelId>() {
                Ok(channel_id) => channel_id,
                Err(_) => {
                    return Err(ChannelInvitationBootstrapError::InvalidCanonicalChannelId {
                        raw: home_id.clone(),
                    });
                }
            };
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
            publish_invitation_operation_status(
                app_core,
                OperationId::invitation_create(),
                operation_instance_id.clone(),
                deadline,
                SemanticOperationKind::InviteActorToChannel,
                SemanticOperationPhase::AuthoritativeContextReady,
            )
            .await
            .map_err(|error| ChannelInvitationBootstrapError::BootstrapTransport {
                channel_id,
                detail: error.to_string(),
            })?;

            update_channel_invitation_stage(&stage_tracker, "runtime.create_channel_invitation");
            let invitation = match tokio::time::timeout(
                channel_invitation_bootstrap_timeout(
                    deadline,
                    channel_id,
                    "runtime.create_channel_invitation",
                    Some(context_id),
                )
                ?,
                runtime.create_channel_invitation(
                    receiver,
                    home_id,
                    Some(context_id),
                    channel_name_hint.clone(),
                    Some(bootstrap),
                    message,
                    ttl_ms,
                ),
            )
            .await
            {
                Ok(Ok(invitation)) => invitation,
                Ok(Err(error)) => {
                    return Err(ChannelInvitationBootstrapError::CreateFailed {
                        channel_id,
                        receiver_id: receiver,
                        detail: error.to_string(),
                    });
                }
                Err(_) => {
                    return Err(ChannelInvitationBootstrapError::CreateTimedOut {
                        channel_id,
                        receiver_id: receiver,
                        timeout_ms: CHANNEL_INVITATION_CREATE_TIMEOUT_MS,
                    });
                }
            };

            Ok((runtime, channel_id, context_id, invitation))
        },
    )
    .await;

    let (_runtime, _channel_id, _context_id, invitation) = match invitation_result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            return fail_channel_invitation(
                app_core,
                operation_instance_id.clone(),
                deadline,
                error,
            )
            .await;
        }
        Err(_) => {
            let detail = stage_tracker
                .as_ref()
                .and_then(|tracker| tracker.lock().ok().map(|guard| *guard))
                .unwrap_or("operation");
            let channel_id = fallback_channel_id
                .unwrap_or_else(|| ChannelId::new(aura_core::Hash32([0; 32])));
            return fail_channel_invitation(
                app_core,
                operation_instance_id.clone(),
                deadline,
                ChannelInvitationBootstrapError::BootstrapTransport {
                    channel_id,
                    detail: format!("create_channel_invitation timed out in stage {detail}"),
                },
            )
            .await;
        }
    };
    publish_invitation_operation_status(
        app_core,
        OperationId::invitation_create(),
        operation_instance_id,
        deadline,
        SemanticOperationKind::InviteActorToChannel,
        SemanticOperationPhase::Succeeded,
    )
    .await?;
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
pub async fn list_pending_invitations(app_core: &Arc<RwLock<AppCore>>) -> Vec<InvitationInfo> {
    let runtime = {
        let core = app_core.read().await;
        match core.runtime() {
            Some(r) => r.clone(),
            None => return Vec::new(),
        }
    };

    runtime.list_pending_invitations().await
}

/// Import and get invitation details from a shareable code
///
/// **What it does**: Parses invite code and returns the details
/// **Returns**: InvitationInfo with parsed details
/// **Signal pattern**: Read-only until acceptance
pub async fn import_invitation_details(
    app_core: &Arc<RwLock<AppCore>>,
    code: &str,
) -> Result<InvitationInfo, AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .import_invitation(code)
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("import invitation", e)))
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
    let runtime = require_runtime(app_core).await?;

    let code = runtime
        .export_invitation(invitation_id.as_str())
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("export invitation", e)))?;
    publish_invitation_operation_status(
        app_core,
        OperationId::invitation_create(),
        None,
        None,
        SemanticOperationKind::CreateContactInvitation,
        SemanticOperationPhase::Succeeded,
    )
    .await?;
    Ok(code)
}

/// Export an invitation by string ID (legacy/convenience API).
pub async fn export_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<String, AuraError> {
    export_invitation(app_core, &InvitationId::new(invitation_id)).await
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
    invitation_id: &InvitationId,
) -> Result<(), AuraError> {
    let accepted_invitation = list_invitations(app_core)
        .await
        .invitation(invitation_id.as_str())
        .cloned();
    let operation_kind = if accepted_invitation.as_ref().is_some_and(|invitation| {
        invitation.invitation_type == crate::views::invitations::InvitationType::Home
    }) {
        SemanticOperationKind::AcceptContactInvitation
    } else {
        SemanticOperationKind::AcceptPendingChannelInvitation
    };
    publish_invitation_operation_status(
        app_core,
        OperationId::invitation_accept(),
        None,
        None,
        operation_kind,
        SemanticOperationPhase::WorkflowDispatched,
    )
    .await?;
    let runtime = require_runtime(app_core).await?;

    if let Err(error) = runtime.accept_invitation(invitation_id.as_str()).await {
        if classify_invitation_accept_error(&error) != InvitationAcceptErrorClass::AlreadyHandled {
            return fail_invitation_accept(
                app_core,
                operation_kind,
                AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                },
            )
            .await;
        }
    }

    for _ in 0..4 {
        let _ = runtime.trigger_discovery().await;
        let _ = runtime.process_ceremony_messages().await;
        let _ = runtime.trigger_sync().await;
        converge_runtime(&runtime).await;
        let _ = crate::workflows::system::refresh_account(app_core).await;
        if ensure_runtime_peer_connectivity(&runtime, "accept_invitation")
            .await
            .is_ok()
        {
            break;
        }
    }

    if accepted_invitation.as_ref().is_some_and(|invitation| {
        invitation.invitation_type == crate::views::invitations::InvitationType::Home
    }) {
        if let Some(invitation) = accepted_invitation.as_ref() {
            if let Err(error) = wait_for_contact_link(app_core, &runtime, invitation.from_id).await
            {
                return fail_invitation_accept(app_core, operation_kind, error).await;
            }
            let contact_count = read_signal_or_default(app_core, &*CONTACTS_SIGNAL)
                .await
                .contact_count() as u32;
            publish_contact_accept_success(app_core, invitation.from_id, contact_count).await?;
            return Ok(());
        }
    } else if let Some(invitation) = accepted_invitation.as_ref().filter(|invitation| {
        invitation.invitation_type == crate::views::invitations::InvitationType::Chat
    }) {
        if let Some(channel_id) = invitation.home_id {
            if let Err(error) = reconcile_accepted_channel_invitation(
                app_core,
                &runtime,
                channel_id,
                invitation.from_id,
                None,
                invitation.home_name.as_deref(),
            )
            .await
            {
                return fail_invitation_accept(
                    app_core,
                    operation_kind,
                    AcceptInvitationError::AcceptFailed {
                        detail: error.to_string(),
                    },
                )
                .await;
            }
        }
    }

    publish_invitation_operation_status(
        app_core,
        OperationId::invitation_accept(),
        None,
        None,
        operation_kind,
        SemanticOperationPhase::Succeeded,
    )
    .await?;

    Ok(())
}

/// Accept an imported invitation using the invitation metadata returned by the runtime bridge.
pub async fn accept_imported_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &crate::runtime_bridge::InvitationInfo,
) -> Result<(), AuraError> {
    let operation_kind = semantic_kind_for_bridge_invitation(invitation);
    publish_invitation_operation_status(
        app_core,
        OperationId::invitation_accept(),
        None,
        None,
        operation_kind,
        SemanticOperationPhase::WorkflowDispatched,
    )
    .await?;
    if matches!(
        invitation.invitation_type,
        crate::runtime_bridge::InvitationBridgeType::DeviceEnrollment { .. }
    ) {
        return fail_invitation_accept(
            app_core,
            operation_kind,
            AcceptInvitationError::AcceptFailed {
                detail:
                    "device enrollment invitations must use accept_device_enrollment_invitation"
                        .to_string(),
            },
        )
        .await;
    }

    let runtime = require_runtime(app_core).await?;

    if let Err(error) = runtime
        .accept_invitation(invitation.invitation_id.as_str())
        .await
    {
        if classify_invitation_accept_error(&error) != InvitationAcceptErrorClass::AlreadyHandled {
            return fail_invitation_accept(
                app_core,
                operation_kind,
                AcceptInvitationError::AcceptFailed {
                    detail: error.to_string(),
                },
            )
            .await;
        }
    }

    for _ in 0..4 {
        let _ = runtime.trigger_discovery().await;
        let _ = runtime.process_ceremony_messages().await;
        let _ = runtime.trigger_sync().await;
        converge_runtime(&runtime).await;
        let _ = crate::workflows::system::refresh_account(app_core).await;
        if ensure_runtime_peer_connectivity(&runtime, "accept_invitation")
            .await
            .is_ok()
        {
            break;
        }
    }

    match &invitation.invitation_type {
        crate::runtime_bridge::InvitationBridgeType::Contact { .. } => {
            if let Err(error) =
                wait_for_contact_link(app_core, &runtime, invitation.sender_id).await
            {
                return fail_invitation_accept(app_core, operation_kind, error).await;
            }
            let contact_count = read_signal_or_default(app_core, &*CONTACTS_SIGNAL)
                .await
                .contact_count() as u32;
            publish_contact_accept_success(app_core, invitation.sender_id, contact_count).await?;
            return Ok(());
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
                        app_core,
                        operation_kind,
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
            if let Err(error) = reconcile_accepted_channel_invitation(
                app_core,
                &runtime,
                channel_id,
                invitation.sender_id,
                *context_id,
                nickname_suggestion.as_deref(),
            )
            .await
            {
                return fail_invitation_accept(
                    app_core,
                    operation_kind,
                    AcceptInvitationError::AcceptFailed {
                        detail: error.to_string(),
                    },
                )
                .await;
            }
        }
        crate::runtime_bridge::InvitationBridgeType::Guardian { .. } => {}
        crate::runtime_bridge::InvitationBridgeType::DeviceEnrollment { .. } => unreachable!(),
    }

    publish_invitation_operation_status(
        app_core,
        OperationId::invitation_accept(),
        None,
        None,
        operation_kind,
        SemanticOperationPhase::Succeeded,
    )
    .await?;

    Ok(())
}

/// Accept a device-enrollment invitation and wait for the local device view to converge.
pub async fn accept_device_enrollment_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &InvitationInfo,
) -> Result<(), AuraError> {
    publish_invitation_operation_status(
        app_core,
        OperationId::device_enrollment(),
        None,
        None,
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        SemanticOperationPhase::WorkflowDispatched,
    )
    .await?;
    let InvitationBridgeType::DeviceEnrollment { .. } = &invitation.invitation_type else {
        return fail_device_enrollment_accept(
            app_core,
            "accept_device_enrollment_invitation requires a device enrollment invitation",
        )
        .await;
    };

    let runtime = require_runtime(app_core).await?;
    if let Err(error) = runtime
        .accept_invitation(invitation.invitation_id.as_str())
        .await
    {
        return fail_device_enrollment_accept(
            app_core,
            format!("accept invitation failed: {error}"),
        )
        .await;
    }
    converge_runtime(&runtime).await;

    let expected_min_devices = 2_usize;
    for attempt in 0..16 {
        #[cfg(not(feature = "instrumented"))]
        let _ = attempt;
        if let Err(_error) = runtime.process_ceremony_messages().await {
            #[cfg(feature = "instrumented")]
            tracing::info!(
                invitation_id = %invitation.invitation_id,
                attempt,
                error = %_error,
                "device enrollment process_ceremony_messages failed during convergence"
            );
        }
        converge_runtime(&runtime).await;
        settings::refresh_settings_from_runtime(app_core).await?;

        let runtime_device_count = runtime.list_devices().await.len();
        let settings_device_count = settings::get_settings(app_core).await?.devices.len();
        #[cfg(feature = "instrumented")]
        tracing::info!(
            invitation_id = %invitation.invitation_id,
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
            if let Err(_error) =
                ensure_runtime_peer_connectivity(&runtime, "device_enrollment_accept").await
            {
                #[cfg(feature = "instrumented")]
                tracing::warn!(
                    error = %_error,
                    invitation_id = %invitation.invitation_id,
                    "device enrollment acceptance completed without reachable peers"
                );
            }

            publish_invitation_operation_status(
                app_core,
                OperationId::device_enrollment(),
                None,
                None,
                SemanticOperationKind::ImportDeviceEnrollmentCode,
                SemanticOperationPhase::Succeeded,
            )
            .await?;
            return Ok(());
        }

        let _ = time::sleep_ms(app_core, 250).await;
    }

    #[cfg(feature = "instrumented")]
    tracing::warn!(
        invitation_id = %invitation.invitation_id,
        expected_min_devices,
        "device enrollment acceptance completed before local device list convergence"
    );
    publish_invitation_operation_status(
        app_core,
        OperationId::device_enrollment(),
        None,
        None,
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        SemanticOperationPhase::Succeeded,
    )
    .await?;
    Ok(())
}

/// Accept an invitation by string ID (legacy/convenience API).
pub async fn accept_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    accept_invitation(app_core, &InvitationId::new(invitation_id)).await
}

/// Decline an invitation using typed InvitationId
///
/// **What it does**: Declines a received invitation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn decline_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &InvitationId,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .decline_invitation(invitation_id.as_str())
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("decline invitation", e)))
}

/// Decline an invitation by string ID (legacy/convenience API).
pub async fn decline_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    decline_invitation(app_core, &InvitationId::new(invitation_id)).await
}

/// Cancel an invitation using typed InvitationId
///
/// **What it does**: Cancels a sent invitation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn cancel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &InvitationId,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .cancel_invitation(invitation_id.as_str())
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("cancel invitation", e)))
}

/// Cancel an invitation by string ID (legacy/convenience API).
pub async fn cancel_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    cancel_invitation(app_core, &InvitationId::new(invitation_id)).await
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

    runtime
        .import_invitation(code)
        .await
        .map(|_| ()) // Discard InvitationInfo, just return success
        .map_err(|e| AuraError::from(super::error::runtime_call("import invitation", e)))
}

async fn wait_for_contact_link(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    contact_id: AuthorityId,
) -> Result<(), AcceptInvitationError> {
    for attempt in 0..CONTACT_LINK_ATTEMPTS {
        let linked = read_signal_or_default(app_core, &*CONTACTS_SIGNAL)
            .await
            .all_contacts()
            .any(|contact| contact.id == contact_id);
        if linked {
            return Ok(());
        }

        converge_runtime(runtime).await;
        if attempt + 1 < CONTACT_LINK_ATTEMPTS {
            runtime.sleep_ms(CONTACT_LINK_BACKOFF_MS).await;
        }
    }

    Err(AcceptInvitationError::ContactLinkDidNotConverge { contact_id })
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
/// This is used by UI to quickly accept a pending home invitation without
/// requiring the user to select a specific invitation ID.
/// Returns the typed InvitationId of the accepted invitation.
pub async fn accept_pending_home_invitation(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<InvitationId, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let our_authority = runtime.authority_id();
    const HOME_ACCEPT_ATTEMPTS: usize = 200;
    const HOME_ACCEPT_BACKOFF_MS: u64 = 150;

    for attempt in 0..HOME_ACCEPT_ATTEMPTS {
        let pending = runtime.list_pending_invitations().await;
        let home_invitation = pending.iter().find(|inv| {
            matches!(inv.invitation_type, InvitationBridgeType::Channel { .. })
                && inv.sender_id != our_authority
        });

        if let Some(inv) = home_invitation {
            let invitation_id = inv.invitation_id.clone();
            accept_imported_invitation(app_core, inv).await?;
            return Ok(invitation_id);
        }

        if attempt + 1 < HOME_ACCEPT_ATTEMPTS {
            converge_runtime(&runtime).await;
            runtime.sleep_ms(HOME_ACCEPT_BACKOFF_MS).await;
        }
    }

    #[cfg(feature = "signals")]
    {
        let invitations = read_signal(
            app_core,
            &*crate::signal_defs::INVITATIONS_SIGNAL,
            crate::signal_defs::INVITATIONS_SIGNAL_NAME,
        )
        .await
        .unwrap_or_default();

        if let Some(accepted) = invitations.all_history().iter().rev().find(|inv| {
            inv.direction == crate::views::invitations::InvitationDirection::Received
                && inv.from_id != our_authority
                && inv.status == crate::views::invitations::InvitationStatus::Accepted
                && matches!(
                    inv.invitation_type,
                    crate::views::invitations::InvitationType::Home
                        | crate::views::invitations::InvitationType::Chat
                )
        }) {
            return Ok(InvitationId::new(accepted.id.clone()));
        }
    }

    Err(super::error::WorkflowError::Precondition("No pending home invitation found").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_defs::{
        AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, INVITATIONS_SIGNAL, INVITATIONS_SIGNAL_NAME,
    };
    use crate::ui_contract::{SemanticFailureCode, SemanticFailureDomain};
    use crate::views::invitations::{
        Invitation, InvitationDirection, InvitationStatus, InvitationType,
    };
    use crate::workflows::signals::emit_signal;
    use crate::AppConfig;

    // === Invitation Role Parsing Tests ===

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
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let invitations = list_invitations(&app_core).await;
        assert_eq!(invitations.sent_count(), 0);
        assert_eq!(invitations.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_refresh_authoritative_invitation_readiness_tracks_pending_home_invitations() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }
        let sender = AuthorityId::new_from_entropy([41u8; 32]);

        let pending_home = Invitation {
            id: "pending-home".to_string(),
            invitation_type: InvitationType::Home,
            status: InvitationStatus::Pending,
            direction: InvitationDirection::Received,
            from_id: sender,
            from_name: "Alice".to_string(),
            to_id: None,
            to_name: None,
            created_at: 1,
            expires_at: None,
            message: None,
            home_id: None,
            home_name: Some("shared".to_string()),
        };

        emit_signal(
            &app_core,
            &*INVITATIONS_SIGNAL,
            InvitationsState::from_parts(vec![pending_home], Vec::new(), Vec::new()),
            INVITATIONS_SIGNAL_NAME,
        )
        .await
        .unwrap();

        refresh_authoritative_invitation_readiness(&app_core)
            .await
            .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts
            .iter()
            .any(|fact| matches!(fact, AuthoritativeSemanticFact::PendingHomeInvitationReady)));

        emit_signal(
            &app_core,
            &*INVITATIONS_SIGNAL,
            InvitationsState::default(),
            INVITATIONS_SIGNAL_NAME,
        )
        .await
        .unwrap();

        refresh_authoritative_invitation_readiness(&app_core)
            .await
            .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(!facts
            .iter()
            .any(|fact| matches!(fact, AuthoritativeSemanticFact::PendingHomeInvitationReady)));
    }

    #[tokio::test]
    async fn test_refresh_authoritative_contact_link_readiness_tracks_contacts_signal() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
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
            } if authority_id == &contact_id.to_string() && *contact_count == 1
        )));
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
        assert!(semantic.detail.as_deref().is_some_and(
            |detail| detail.contains(&CHANNEL_INVITATION_CREATE_TIMEOUT_MS.to_string())
        ));
    }

    #[tokio::test]
    async fn test_fail_channel_invitation_publishes_terminal_failure_fact() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let channel_id = ChannelId::from_bytes([51u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([52u8; 32]);
        let result = fail_channel_invitation::<()>(
            &app_core,
            None,
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
