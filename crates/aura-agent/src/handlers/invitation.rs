//! Invitation Handlers
//!
//! Handlers for invitation-related operations including creating, accepting,
//! and declining invitations for channels, guardians, and contacts.
//!
//! This module uses `aura_invitation::InvitationService` internally for
//! guard chain integration. Types are re-exported from `aura_invitation`.

use super::shared::{
    load_relational_fact_envelopes_by_type, resolve_charge_peer, HandlerContext,
    HandlerUtilities,
};
use cache::InvitationCacheHandler;
use channel::InvitationChannelHandler;
use contact::InvitationContactHandler;
use crate::core::{default_context_id_for_authority, AgentError, AgentResult, AuthorityContext};
use crate::reactive::app_signal_views;
use crate::runtime::services::InvitationManager;
#[cfg(feature = "choreo-backend-telltale-vm")]
use crate::runtime::{open_owned_manifest_vm_session_admitted, AuraEffectSystem};
#[cfg(feature = "choreo-backend-telltale-vm")]
use crate::runtime::vm_host_bridge::AuraVmHostWaitStatus;
#[cfg(not(feature = "choreo-backend-telltale-vm"))]
use crate::runtime::AuraEffectSystem;
use crate::InvitationServiceApi;
use device_enrollment::InvitationDeviceEnrollmentHandler;
use guardian::InvitationGuardianHandler;
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::effects::amp::{ChannelBootstrapPackage, ChannelCreateParams};
use aura_core::effects::storage::StorageCoreEffects;
use aura_core::effects::RandomExtendedEffects;
use aura_core::effects::{
    AmpChannelEffects, ChannelJoinParams, FlowBudgetEffects, TransportEffects, TransportEnvelope,
    TransportReceipt,
};
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, DeviceId, InvitationId};
use aura_core::time::PhysicalTime;
use aura_core::Hash32;
use aura_core::FlowCost;
use aura_core::Receipt;
use aura_core::CapabilityName;
use aura_core::{
    execute_with_retry_budget, execute_with_timeout_budget, ExponentialBackoffPolicy,
    RetryBudgetPolicy, RetryRunError, TimeoutBudget, TimeoutExecutionProfile, TimeoutRunError,
};
use aura_guards::types::CapabilityId;
use aura_invitation::capabilities::evaluation_candidates_for_invitation_guard;
use aura_invitation::guards::GuardSnapshot;
use aura_invitation::{InvitationConfig, InvitationService as CoreInvitationService};
use aura_invitation::{InvitationFact, INVITATION_FACT_TYPE_ID};
#[cfg(not(feature = "choreo-backend-telltale-vm"))]
use aura_invitation::protocol::exchange_runners::InvitationExchangeRole;
use aura_invitation::protocol::exchange::telltale_session_types_invitation::message_wrappers::{
    InvitationAck as ExchangeInvitationAck,
    InvitationOffer as ExchangeInvitationOffer,
    InvitationResponse as ExchangeInvitationResponse,
};
use aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::message_wrappers::{
    GuardianAccept as GuardianInvitationAccept,
    GuardianConfirm as GuardianInvitationConfirm,
    GuardianRequest as GuardianInvitationRequest,
};
use aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::message_wrappers::{
    DeviceEnrollmentAccept as DeviceEnrollmentAcceptWrapper,
    DeviceEnrollmentConfirm as DeviceEnrollmentConfirmWrapper,
    DeviceEnrollmentRequest as DeviceEnrollmentRequestWrapper,
};
use aura_invitation::{
    DeviceEnrollmentAccept, DeviceEnrollmentConfirm, DeviceEnrollmentRequest,
    GuardianAccept, GuardianConfirm, GuardianRequest, InvitationAck, InvitationOffer,
    InvitationOperation, InvitationResponse,
};
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use aura_journal::DomainFact;
use aura_protocol::amp::AmpJournalEffects;
use aura_protocol::effects::ChoreographyError;
use aura_core::effects::TransportError;
use aura_core::util::serialization::{from_slice, to_vec};
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
use base64::Engine;
use std::collections::{BTreeMap, BTreeSet, HashMap};
#[cfg(test)]
use std::str::FromStr;
use uuid::Uuid;
use validation::InvitationValidationHandler;
#[cfg(target_arch = "wasm32")]
use web_sys::js_sys;
#[cfg(feature = "choreo-backend-telltale-vm")]
use aura_protocol::effects::{ChoreographicRole, RoleIndex};
#[cfg(feature = "choreo-backend-telltale-vm")]
use telltale_vm::vm::StepResult;

mod cache;
mod channel;
mod contact;
mod device_enrollment;
mod guardian;
mod validation;

// Re-export types from aura_invitation for public API
pub use aura_invitation::{Invitation, InvitationStatus, InvitationType};

const CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE: &str =
    "application/aura-contact-invitation-acceptance";
const CHANNEL_INVITATION_ACCEPTANCE_CONTENT_TYPE: &str =
    "application/aura-channel-invitation-acceptance";
const CHAT_FACT_CONTENT_TYPE: &str = "application/aura-chat-fact";
const INVITATION_CONTENT_TYPE: &str = "application/aura-invitation";
const INVITATION_PREPARE_STAGE_TIMEOUT_MS: u64 = 4_000;
const INVITATION_BEST_EFFORT_NETWORK_TIMEOUT_MS: u64 = 2_000;
const INVITATION_BEST_EFFORT_NETWORK_SEND_ATTEMPTS: usize = 8;
const INVITATION_BEST_EFFORT_NETWORK_SEND_BACKOFF_MS: u64 = 200;
const INVITATION_ACCEPT_OPERATION_TIMEOUT_MS: u64 = 60_000;
const INVITATION_ACCEPT_VALIDATE_STAGE_TIMEOUT_MS: u64 = 5_000;
const INVITATION_ACCEPT_PREPARE_STAGE_TIMEOUT_MS: u64 = 5_000;
const INVITATION_ACCEPT_GUARD_STAGE_TIMEOUT_MS: u64 = 5_000;
const INVITATION_ACCEPT_MATERIALIZE_STAGE_TIMEOUT_MS: u64 = 15_000;
const INVITATION_ACCEPT_CHOREOGRAPHY_STAGE_TIMEOUT_MS: u64 = 30_000;
const INVITATION_VM_LOOP_TIMEOUT_MS: u64 = 30_000;
const DESCRIPTOR_VALIDITY_WINDOW_MS: u64 = 86_400_000; // 24h

fn invitation_timeout_profile(effects: &AuraEffectSystem) -> TimeoutExecutionProfile {
    if effects.is_testing() {
        TimeoutExecutionProfile::simulation_test()
    } else if effects.harness_mode_enabled() {
        TimeoutExecutionProfile::harness()
    } else {
        TimeoutExecutionProfile::production()
    }
}

async fn invitation_timeout_budget(
    effects: &AuraEffectSystem,
    stage: &'static str,
    timeout_ms: u64,
) -> AgentResult<TimeoutBudget> {
    let started_at = effects.physical_time().await.map_err(|error| {
        AgentError::runtime(format!(
            "invitation stage `{stage}` could not read physical time: {error}"
        ))
    })?;
    let scaled_timeout = invitation_timeout_profile(effects)
        .scale_duration(Duration::from_millis(timeout_ms))
        .map_err(|error| {
            AgentError::runtime(format!(
                "invitation stage `{stage}` could not scale timeout budget: {error}"
            ))
        })?;
    TimeoutBudget::from_start_and_timeout(&started_at, scaled_timeout)
        .map_err(|error| AgentError::runtime(error.to_string()))
}

async fn timeout_invitation_stage_with_budget<T>(
    effects: &AuraEffectSystem,
    budget: &TimeoutBudget,
    stage: &'static str,
    timeout_ms: u64,
    future: impl Future<Output = AgentResult<T>>,
) -> AgentResult<T> {
    let now = effects.physical_time().await.map_err(|error| {
        AgentError::runtime(format!(
            "invitation stage `{stage}` could not read physical time: {error}"
        ))
    })?;
    let scaled_timeout = invitation_timeout_profile(effects)
        .scale_duration(Duration::from_millis(timeout_ms))
        .map_err(|error| {
            AgentError::runtime(format!(
                "invitation stage `{stage}` could not scale timeout budget: {error}"
            ))
        })?;
    let child_budget = budget.child_budget(&now, scaled_timeout).map_err(|error| {
        AgentError::timeout(format!(
            "invitation stage `{stage}` could not allocate remaining timeout budget: {error}"
        ))
    })?;
    execute_with_timeout_budget(effects, &child_budget, || future)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => AgentError::timeout(format!(
                "invitation stage `{stage}` timed out after {}ms",
                child_budget.timeout_ms()
            )),
            TimeoutRunError::Operation(error) => error,
        })
}

async fn timeout_prepare_invitation_stage<T>(
    effects: &AuraEffectSystem,
    stage: &'static str,
    future: impl Future<Output = AgentResult<T>>,
) -> AgentResult<T> {
    let started_at = effects.physical_time().await.map_err(|error| {
        AgentError::runtime(format!(
            "invitation.prepare stage `{stage}` could not read physical time: {error}"
        ))
    })?;
    let budget = TimeoutBudget::from_start_and_timeout(
        &started_at,
        Duration::from_millis(INVITATION_PREPARE_STAGE_TIMEOUT_MS),
    )
    .map_err(|error| AgentError::runtime(error.to_string()))?;
    execute_with_timeout_budget(effects, &budget, || future)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => AgentError::runtime(format!(
                "invitation.prepare stage `{stage}` timed out after {INVITATION_PREPARE_STAGE_TIMEOUT_MS}ms"
            )),
            TimeoutRunError::Operation(error) => error,
        })
}

async fn timeout_deferred_network_stage<T>(
    effects: &AuraEffectSystem,
    stage: &'static str,
    future: impl Future<Output = AgentResult<T>>,
) -> AgentResult<T> {
    let started_at = effects.physical_time().await.map_err(|error| {
        AgentError::runtime(format!(
            "invitation best-effort network stage `{stage}` could not read physical time: {error}"
        ))
    })?;
    let budget = TimeoutBudget::from_start_and_timeout(
        &started_at,
        Duration::from_millis(INVITATION_BEST_EFFORT_NETWORK_TIMEOUT_MS),
    )
    .map_err(|error| AgentError::runtime(error.to_string()))?;
    execute_with_timeout_budget(effects, &budget, || future)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => AgentError::runtime(format!(
                "invitation best-effort network stage `{stage}` timed out after {INVITATION_BEST_EFFORT_NETWORK_TIMEOUT_MS}ms"
            )),
            TimeoutRunError::Operation(error) => error,
        })
}

async fn attempt_network_send_envelope(
    effects: &AuraEffectSystem,
    stage: &'static str,
    envelope: TransportEnvelope,
) -> AgentResult<()> {
    timeout_deferred_network_stage(effects, stage, async {
        let mut last_error = None;
        for attempt in 0..INVITATION_BEST_EFFORT_NETWORK_SEND_ATTEMPTS {
            match effects.send_envelope(envelope.clone()).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    last_error = Some(error.to_string());
                    if attempt + 1 < INVITATION_BEST_EFFORT_NETWORK_SEND_ATTEMPTS {
                        let _ = effects
                            .sleep_ms(INVITATION_BEST_EFFORT_NETWORK_SEND_BACKOFF_MS)
                            .await;
                    }
                }
            }
        }

        Err(AgentError::effects(format!(
            "{stage}: {}",
            last_error.unwrap_or_else(|| "transport send failed without detail".to_string())
        )))
    })
    .await
}

#[cfg(target_arch = "wasm32")]
fn emit_browser_harness_debug_event(event: &str, detail: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(origin) = window.location().origin() else {
        return;
    };
    let event = js_sys::encode_uri_component(event)
        .as_string()
        .unwrap_or_else(|| event.to_string());
    let detail = js_sys::encode_uri_component(detail)
        .as_string()
        .unwrap_or_else(|| detail.to_string());
    let url = format!("{origin}/__aura_harness_debug__/event?event={event}&detail={detail}");
    let _ = window.fetch_with_str(&url);
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_browser_harness_debug_event(_event: &str, _detail: &str) {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ContactInvitationAcceptance {
    invitation_id: InvitationId,
    acceptor_id: AuthorityId,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ChannelInvitationAcceptance {
    invitation_id: InvitationId,
    acceptor_id: AuthorityId,
    context_id: ContextId,
    channel_id: ChannelId,
}

/// Result of an invitation action
///
/// The outer `AgentResult<_>` owns terminal success or failure; this inner
/// value only carries the authoritative postcondition on success.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvitationResult {
    /// Invitation ID affected
    pub invitation_id: InvitationId,
    /// New status after the action
    pub new_status: InvitationStatus,
}

impl InvitationResult {
    fn new(invitation_id: InvitationId, new_status: InvitationStatus) -> Self {
        Self {
            invitation_id,
            new_status,
        }
    }
}

/// Count of sender-side contact invitation acceptances that were fully
/// materialized into local sender state.
pub type ProcessedContactInvitationAcceptanceCount = usize;

#[derive(Debug)]
pub(crate) struct DeferredInvitationNetworkEffects {
    commands: Vec<aura_invitation::guards::EffectCommand>,
}

impl DeferredInvitationNetworkEffects {
    fn new(commands: Vec<aura_invitation::guards::EffectCommand>) -> Self {
        Self { commands }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub(crate) fn commands(&self) -> &[aura_invitation::guards::EffectCommand] {
        &self.commands
    }

    pub(crate) fn into_commands(self) -> Vec<aura_invitation::guards::EffectCommand> {
        self.commands
    }
}

#[derive(Debug)]
pub(crate) struct PreparedInvitation {
    pub(crate) invitation: Invitation,
    pub(crate) deferred_network_effects: DeferredInvitationNetworkEffects,
}

struct ChannelInviteDetails {
    context_id: ContextId,
    channel_id: ChannelId,
    home_id: String,
    home_name: String,
    sender_id: AuthorityId,
    bootstrap: Option<ChannelBootstrapPackage>,
}

fn require_channel_invitation_name(
    home_id: ChannelId,
    nickname_suggestion: Option<String>,
) -> AgentResult<String> {
    let Some(home_name) = nickname_suggestion
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Err(AgentError::invalid(format!(
            "channel invitation {} missing canonical channel metadata",
            home_id
        )));
    };
    Ok(home_name)
}

fn require_channel_invitation_context(
    invitation_id: &InvitationId,
    sender_id: AuthorityId,
    context_id: Option<ContextId>,
) -> AgentResult<ContextId> {
    context_id.ok_or_else(|| {
        AgentError::invalid(format!(
            "channel invitation {} from {} missing authoritative context",
            invitation_id, sender_id
        ))
    })
}

#[cfg(test)]
fn channel_id_from_home_id(home_id: &str) -> AgentResult<ChannelId> {
    ChannelId::from_str(home_id).map_err(|e| {
        AgentError::invalid(format!(
            "invalid channel/home id `{home_id}`: expected canonical ChannelId format ({e})"
        ))
    })
}

/// Invitation handler
///
/// Uses `aura_invitation::InvitationService` for guard chain integration.
pub struct InvitationHandler {
    context: HandlerContext,
    /// Core invitation service from aura_invitation
    service: CoreInvitationService,
    /// Cache of pending invitations (for quick lookup)
    invitation_cache: Arc<InvitationManager>,
}

impl Clone for InvitationHandler {
    fn clone(&self) -> Self {
        // `CoreInvitationService` is stateless: it only stores the authority id
        // and immutable config used to derive guard outcomes.
        let service =
            CoreInvitationService::new(self.service.authority_id(), self.service.config().clone());
        Self {
            context: self.context.clone(),
            service,
            invitation_cache: Arc::clone(&self.invitation_cache),
        }
    }
}

impl InvitationHandler {
    const IMPORTED_INVITATION_STORAGE_PREFIX: &'static str = "invitation/imported";
    const CREATED_INVITATION_STORAGE_PREFIX: &'static str = "invitation/created";

    /// Create a new invitation handler
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        let service =
            CoreInvitationService::new(authority.authority_id(), InvitationConfig::default());

        Ok(Self {
            context: HandlerContext::new(authority),
            service,
            invitation_cache: Arc::new(InvitationManager::new()),
        })
    }

    async fn persist_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationCacheHandler::persist_created_invitation(effects, authority_id, invitation).await
    }

    async fn best_effort_current_timestamp_ms(effects: &AuraEffectSystem) -> u64 {
        if effects.harness_mode_enabled() {
            let Ok(started_at) = effects.physical_time().await else {
                return 0;
            };
            let Ok(budget) =
                TimeoutBudget::from_start_and_timeout(&started_at, Duration::from_millis(50))
            else {
                return 0;
            };

            return match execute_with_timeout_budget(effects, &budget, || effects.physical_time())
                .await
            {
                Ok(value) => value.ts_ms,
                Err(TimeoutRunError::Operation(_)) | Err(TimeoutRunError::Timeout(_)) => 0,
            };
        }

        effects
            .physical_time()
            .await
            .map(|time| time.ts_ms)
            .unwrap_or(0)
    }

    fn decode_invitation_biscuit_frontier(
        &self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<
        Option<(
            aura_authorization::Biscuit,
            aura_authorization::BiscuitAuthorizationBridge,
        )>,
    > {
        let Some(cache) = effects.biscuit_cache() else {
            return Ok(None);
        };

        let engine = base64::engine::general_purpose::STANDARD;
        let token_bytes = engine
            .decode(cache.token_b64)
            .map_err(|error| AgentError::effects(format!("decode biscuit token cache: {error}")))?;
        let root_pk_bytes = engine.decode(cache.root_pk_b64).map_err(|error| {
            AgentError::effects(format!("decode biscuit root public key cache: {error}"))
        })?;
        let root_public_key =
            aura_authorization::PublicKey::from_bytes(&root_pk_bytes).map_err(|error| {
                AgentError::effects(format!("parse biscuit root public key cache: {error}"))
            })?;
        let biscuit = aura_authorization::Biscuit::from(&token_bytes, root_public_key)
            .map_err(|error| AgentError::effects(format!("parse biscuit token cache: {error}")))?;
        let bridge = aura_authorization::BiscuitAuthorizationBridge::new(
            root_public_key,
            self.context.authority.authority_id(),
        );
        Ok(Some((biscuit, bridge)))
    }

    fn invitation_capability_check_timestamp_seconds(now_ms: u64) -> Option<u64> {
        if now_ms == 0 {
            None
        } else {
            Some(now_ms / 1_000)
        }
    }

    async fn build_invitation_capabilities(
        &self,
        effects: &AuraEffectSystem,
        now_ms: u64,
    ) -> Vec<CapabilityId> {
        let Some((token, bridge)) = (match self.decode_invitation_biscuit_frontier(effects) {
            Ok(frontier) => frontier,
            Err(error) => {
                tracing::warn!(
                    authority = %self.context.authority.authority_id(),
                    error = %error,
                    "failed to decode Biscuit frontier for invitation guard snapshot"
                );
                return Vec::new();
            }
        }) else {
            tracing::debug!(
                authority = %self.context.authority.authority_id(),
                "no Biscuit frontier available for invitation guard snapshot"
            );
            return Vec::new();
        };

        let current_time_seconds = Self::invitation_capability_check_timestamp_seconds(now_ms);
        evaluation_candidates_for_invitation_guard()
            .iter()
            .filter_map(|capability| {
                let capability_name: CapabilityName = capability.as_name();
                match bridge.has_capability_with_time(
                    &token,
                    capability_name.as_str(),
                    current_time_seconds,
                ) {
                    Ok(true) => Some(capability_name),
                    Ok(false) => None,
                    Err(error) => {
                        tracing::warn!(
                            authority = %self.context.authority.authority_id(),
                            capability = capability_name.as_str(),
                            error = %error,
                            "failed to evaluate invitation Biscuit capability"
                        );
                        None
                    }
                }
            })
            .collect()
    }

    pub(crate) async fn load_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        InvitationCacheHandler::load_created_invitation(effects, authority_id, invitation_id).await
    }

    async fn persist_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation: &StoredImportedInvitation,
    ) -> AgentResult<()> {
        InvitationCacheHandler::persist_imported_invitation(effects, authority_id, invitation).await
    }

    async fn load_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
        preserved: Option<&Invitation>,
    ) -> Option<StoredImportedInvitation> {
        InvitationCacheHandler::load_imported_invitation(
            effects,
            authority_id,
            invitation_id,
            preserved,
        )
        .await
    }

    async fn update_imported_invitation_status_if_present(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        status: InvitationStatus,
        created_at: u64,
    ) -> AgentResult<()> {
        let own_id = self.context.authority.authority_id();
        let Some(mut invitation) =
            Self::load_imported_invitation(effects, own_id, invitation_id, None).await
        else {
            return Ok(());
        };
        invitation.status = status;
        if invitation.created_at == 0 {
            invitation.created_at = created_at;
        }
        Self::persist_imported_invitation(effects, own_id, &invitation).await
    }

    fn validate_importable_shareable_invitation(
        &self,
        shareable: &ShareableInvitation,
    ) -> AgentResult<()> {
        // `from_code` already guarantees a well-formed `sender_id` and a
        // recognized `InvitationType`. Validate the remaining authoritative
        // invariants before persisting any imported payload.
        if matches!(shareable.invitation_type, InvitationType::Channel { .. }) {
            let _ = require_channel_invitation_context(
                &shareable.invitation_id,
                shareable.sender_id,
                shareable.context_id,
            )?;
        }

        Ok(())
    }

    async fn refresh_contact_index(
        &self,
        effects: &AuraEffectSystem,
        owner_id: AuthorityId,
    ) -> AgentResult<()> {
        let envelopes =
            load_relational_fact_envelopes_by_type(effects, owner_id, CONTACT_FACT_TYPE_ID).await?;
        let mut index = aura_relational::ContactExistenceIndex::new();
        for envelope in envelopes {
            let Some(contact_fact) = ContactFact::from_envelope(&envelope) else {
                continue;
            };
            index.apply_fact(&contact_fact);
        }
        self.invitation_cache.replace_contact_index(index).await;
        Ok(())
    }

    async fn sender_contact_exists(
        &self,
        effects: &AuraEffectSystem,
        owner_id: AuthorityId,
        contact_id: AuthorityId,
    ) -> bool {
        if !self.invitation_cache.contact_index_seeded().await
            && self.refresh_contact_index(effects, owner_id).await.is_err()
        {
            return false;
        }

        if self
            .invitation_cache
            .contact_exists(owner_id, contact_id)
            .await
        {
            return true;
        }

        self.refresh_contact_index(effects, owner_id).await.is_ok()
            && self
                .invitation_cache
                .contact_exists(owner_id, contact_id)
                .await
    }

    /// Get the authority context
    pub fn authority_context(&self) -> &AuthorityContext {
        &self.context.authority
    }

    /// Build a guard snapshot from the provided context and effects.
    async fn build_snapshot_for_context(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> GuardSnapshot {
        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
        let capabilities = self.build_invitation_capabilities(effects, now_ms).await;
        let budget = aura_core::effects::JournalEffects::get_flow_budget(
            effects,
            &context_id,
            &self.context.authority.authority_id(),
        )
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(
                authority = %self.context.authority.authority_id(),
                context_id = %context_id,
                error = %error,
                "failed to read authoritative invitation flow budget; using bootstrap fallback"
            );
            aura_core::FlowBudget {
                limit: 100,
                spent: 0,
                epoch: aura_core::Epoch::new(1),
            }
        });

        GuardSnapshot::new(
            self.context.authority.authority_id(),
            context_id,
            FlowCost::new(u32::try_from(budget.remaining()).unwrap_or(u32::MAX)),
            capabilities,
            u64::from(budget.epoch),
            now_ms,
        )
    }

    /// Build a guard snapshot from the handler's default context.
    async fn build_snapshot(&self, effects: &AuraEffectSystem) -> GuardSnapshot {
        self.build_snapshot_for_context(effects, self.context.effect_context.context_id())
            .await
    }

    async fn refresh_channel_context_index(
        &self,
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
    ) -> AgentResult<()> {
        let envelopes =
            load_relational_fact_envelopes_by_type(effects, authority_id, CHAT_FACT_TYPE_ID)
                .await?;
        let mut index = aura_chat::ChannelContextIndex::new();
        for envelope in envelopes {
            let Some(chat_fact) = ChatFact::from_envelope(&envelope) else {
                continue;
            };
            index.apply_fact(&chat_fact);
        }
        self.invitation_cache
            .replace_channel_context_index(index)
            .await;
        Ok(())
    }

    /// Resolve the effective invitation context for the outgoing invitation type.
    async fn resolve_invitation_context(
        &self,
        effects: &AuraEffectSystem,
        invitation_type: &InvitationType,
    ) -> AgentResult<ContextId> {
        let InvitationType::Channel { home_id, .. } = invitation_type else {
            return Ok(self.context.effect_context.context_id());
        };

        let own_id = self.context.authority.authority_id();
        if !self.invitation_cache.channel_context_index_seeded().await {
            self.refresh_channel_context_index(effects, own_id).await?;
        }

        if let Some(context_id) = self
            .invitation_cache
            .channel_context(*home_id, own_id)
            .await
        {
            return Ok(context_id);
        }

        self.refresh_channel_context_index(effects, own_id).await?;
        if let Some(context_id) = self
            .invitation_cache
            .channel_context(*home_id, own_id)
            .await
        {
            return Ok(context_id);
        }

        Err(AgentError::context(format!(
            "Failed to resolve authoritative invitation context for channel {home_id}"
        )))
    }

    async fn validate_cached_invitation_accept(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        now_ms: u64,
    ) -> AgentResult<()> {
        InvitationValidationHandler::new(self)
            .validate_cached_invitation_accept(effects, invitation_id, now_ms)
            .await
    }

    async fn validate_cached_invitation_decline(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        InvitationValidationHandler::new(self)
            .validate_cached_invitation_decline(effects, invitation_id)
            .await
    }

    async fn validate_cached_invitation_cancel(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        InvitationValidationHandler::new(self)
            .validate_cached_invitation_cancel(effects, invitation_id)
            .await
    }

    /// Create an invitation
    pub async fn create_invitation(
        &self,
        effects: Arc<AuraEffectSystem>,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        self.create_invitation_with_context(
            effects,
            receiver_id,
            invitation_type,
            None,
            message,
            expires_in_ms,
        )
        .await
    }

    /// Create an invitation with an optional explicit context override.
    pub async fn create_invitation_with_context(
        &self,
        effects: Arc<AuraEffectSystem>,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
        context_override: Option<ContextId>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let prepared = self
            .prepare_invitation_with_context(
                effects.clone(),
                receiver_id,
                invitation_type,
                context_override,
                message,
                expires_in_ms,
            )
            .await?;

        execute_invitation_effect_commands(
            prepared.deferred_network_effects.commands,
            &self.context.authority,
            effects.as_ref(),
            true,
        )
        .await?;

        Ok(prepared.invitation)
    }

    pub(crate) async fn prepare_invitation_with_context(
        &self,
        effects: Arc<AuraEffectSystem>,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
        context_override: Option<ContextId>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<PreparedInvitation> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Generate unique invitation ID
        let invitation_id =
            InvitationId::new(format!("inv-{}", effects.random_uuid().await.simple()));
        let current_time = Self::best_effort_current_timestamp_ms(&effects).await;
        let expires_at = expires_in_ms.map(|ms| current_time + ms);

        let invitation_context = if let Some(context_id) = context_override {
            context_id
        } else {
            timeout_prepare_invitation_stage(
                effects.as_ref(),
                "resolve_invitation_context",
                async {
                    self.resolve_invitation_context(effects.as_ref(), &invitation_type)
                        .await
                },
            )
            .await?
        };
        tracing::debug!(
            receiver_id = %receiver_id,
            invitation_type = ?invitation_type,
            "Preparing invitation with resolved context override={:?} context={}",
            context_override,
            invitation_context
        );
        // Build snapshot and prepare through service.
        // For channel invitations this must use the channel context so the
        // generated invitation facts and transport metadata are scoped correctly.
        let snapshot = self
            .build_snapshot_for_context(effects.as_ref(), invitation_context)
            .await;

        let outcome = self.service.prepare_send_invitation(
            &snapshot,
            receiver_id,
            invitation_type.clone(),
            message.clone(),
            expires_in_ms,
            invitation_id.clone(),
        );

        let (local_effects, deferred_network_effects) =
            split_invitation_send_guard_outcome(outcome, &self.context.authority)?;
        timeout_prepare_invitation_stage(
            effects.as_ref(),
            "execute_local_effects",
            execute_invitation_effect_commands(
                local_effects,
                &self.context.authority,
                effects.as_ref(),
                false,
            ),
        )
        .await?;

        let invitation = Invitation {
            invitation_id: invitation_id.clone(),
            context_id: invitation_context,
            sender_id: self.context.authority.authority_id(),
            receiver_id,
            invitation_type,
            status: InvitationStatus::Pending,
            created_at: current_time,
            expires_at,
            message,
        };

        if let InvitationType::Contact { .. } = invitation.invitation_type {
            let sender_contact_exists = self
                .sender_contact_exists(
                    effects.as_ref(),
                    invitation.sender_id,
                    invitation.receiver_id,
                )
                .await;
            if !sender_contact_exists {
                let contact_fact = ContactFact::Added {
                    context_id: invitation.context_id,
                    owner_id: invitation.sender_id,
                    contact_id: invitation.receiver_id,
                    nickname: invitation.receiver_id.to_string(),
                    added_at: PhysicalTime {
                        ts_ms: current_time,
                        uncertainty: None,
                    },
                };

                timeout_prepare_invitation_stage(
                    effects.as_ref(),
                    "commit_sender_contact_fact",
                    async {
                        effects
                            .commit_generic_fact_bytes(
                                invitation.context_id,
                                CONTACT_FACT_TYPE_ID.into(),
                                contact_fact.to_bytes(),
                            )
                            .await
                            .map_err(|e| AgentError::effects(format!("commit contact fact: {e}")))
                    },
                )
                .await?;
                self.invitation_cache
                    .record_contact_fact(&contact_fact)
                    .await;
            }
        }

        // Persist the invitation to storage (so it survives service recreation)
        timeout_prepare_invitation_stage(
            effects.as_ref(),
            "persist_created_invitation",
            Self::persist_created_invitation(
                effects.as_ref(),
                self.context.authority.authority_id(),
                &invitation,
            ),
        )
        .await?;

        // Cache the pending invitation (for fast lookup within same service instance)
        self.invitation_cache
            .cache_invitation(invitation.clone())
            .await;

        match invitation.invitation_type {
            InvitationType::Contact { .. } => {
                tracing::debug!(
                    invitation_id = %invitation.invitation_id,
                    "Skipping synchronous invitation exchange sender for contact invitation"
                );
            }
            InvitationType::Guardian { .. } => {
                self.execute_guardian_invitation_principal(effects.clone(), &invitation)
                    .await?;
            }
            InvitationType::DeviceEnrollment { .. } => {}
            InvitationType::Channel { .. } => {}
        }

        Ok(PreparedInvitation {
            invitation,
            deferred_network_effects,
        })
    }

    /// Accept an invitation
    pub async fn accept_invitation(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        tracing::debug!(
            invitation_id = %invitation_id,
            authority = %self.context.authority.authority_id(),
            "Accepting invitation"
        );

        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        self.accept_invitation_owned(effects, invitation_id).await
    }

    async fn accept_invitation_owned(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        let operation_budget = invitation_timeout_budget(
            effects.as_ref(),
            "accept_invitation",
            INVITATION_ACCEPT_OPERATION_TIMEOUT_MS,
        )
        .await?;
        let now_ms = timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_validate",
            INVITATION_ACCEPT_VALIDATE_STAGE_TIMEOUT_MS,
            async {
                let now_ms = Self::best_effort_current_timestamp_ms(&effects).await;
                self.validate_cached_invitation_accept(effects.as_ref(), invitation_id, now_ms)
                    .await?;
                Ok(now_ms)
            },
        )
        .await?;

        // Build snapshot and prepare through service
        let outcome = timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_prepare",
            INVITATION_ACCEPT_PREPARE_STAGE_TIMEOUT_MS,
            async {
                let snapshot = self.build_snapshot(effects.as_ref()).await;
                Ok(self
                    .service
                    .prepare_accept_invitation(&snapshot, invitation_id))
            },
        )
        .await?;

        tracing::debug!(
            invitation_id = %invitation_id,
            allowed = %outcome.is_allowed(),
            denied = %outcome.is_denied(),
            "Guard outcome for invitation accept"
        );

        // Accept should not be blocked by best-effort budget/notify side effects.
        timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_guard_outcome",
            INVITATION_ACCEPT_GUARD_STAGE_TIMEOUT_MS,
            execute_guard_outcome_for_accept(outcome, &self.context.authority, effects.as_ref()),
        )
        .await?;

        timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_materialize",
            INVITATION_ACCEPT_MATERIALIZE_STAGE_TIMEOUT_MS,
            self.materialize_accept_invitation_state(effects.clone(), invitation_id, now_ms),
        )
        .await?;

        self.update_imported_invitation_status_if_present(
            effects.as_ref(),
            invitation_id,
            InvitationStatus::Accepted,
            now_ms,
        )
        .await?;

        // Update cache if we have this invitation
        let _ = self
            .invitation_cache
            .update_invitation(invitation_id, |inv| {
                inv.status = InvitationStatus::Accepted;
            })
            .await;

        let choreography_invitation = self
            .load_invitation_for_choreography(effects.as_ref(), invitation_id)
            .await;

        if let Some(invitation) = choreography_invitation.as_ref() {
            if matches!(invitation.invitation_type, InvitationType::Contact { .. }) {
                tracing::debug!(
                    invitation_id = %invitation_id,
                    "Returning immediately after local contact invitation acceptance"
                );
                return Ok(InvitationResult::new(
                    invitation_id.clone(),
                    InvitationStatus::Accepted,
                ));
            }
        }

        timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_choreography",
            INVITATION_ACCEPT_CHOREOGRAPHY_STAGE_TIMEOUT_MS,
            self.execute_accept_invitation_follow_up(
                effects.clone(),
                invitation_id,
                choreography_invitation.as_ref(),
            ),
        )
        .await?;

        Ok(InvitationResult::new(
            invitation_id.clone(),
            InvitationStatus::Accepted,
        ))
    }

    async fn materialize_accept_invitation_state(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
        accepted_at_ms: u64,
    ) -> AgentResult<()> {
        self.materialize_contact_acceptance_if_needed(
            effects.as_ref(),
            invitation_id,
            accepted_at_ms,
        )
        .await?;
        self.materialize_channel_acceptance_if_needed(effects.as_ref(), invitation_id)
            .await?;
        self.materialize_device_enrollment_acceptance_if_needed(effects.as_ref(), invitation_id)
            .await
    }

    async fn materialize_contact_acceptance_if_needed(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        accepted_at_ms: u64,
    ) -> AgentResult<()> {
        // Accepting a contact invitation must materialize sender contact state so
        // CONTACTS_SIGNAL converges from facts rather than UI-local mutation.
        if let Some((contact_id, nickname)) = self
            .resolve_contact_invitation(effects, invitation_id)
            .await?
        {
            let context_id = self.context.effect_context.context_id();
            let fact = ContactFact::Added {
                context_id,
                owner_id: self.context.authority.authority_id(),
                contact_id,
                nickname: nickname.clone(),
                added_at: PhysicalTime {
                    ts_ms: accepted_at_ms,
                    uncertainty: None,
                },
            };

            tracing::debug!(
                invitation_id = %invitation_id,
                contact_id = %contact_id,
                nickname = %nickname,
                context_id = %context_id,
                "Committing ContactFact::Added for accepted invitation"
            );

            effects
                .commit_generic_fact_bytes(context_id, CONTACT_FACT_TYPE_ID.into(), fact.to_bytes())
                .await
                .map_err(|e| {
                    crate::core::AgentError::effects(format!("commit contact fact: {e}"))
                })?;
            self.invitation_cache.record_contact_fact(&fact).await;

            // Promote LAN-discovered descriptor into the local context so that
            // is_peer_online() / resolve_peer_addr() can find it immediately.
            if let Some(rendezvous) = effects.rendezvous_manager() {
                if let Some(lan_peer) = rendezvous.get_lan_discovered_peer(contact_id).await {
                    let mut desc = lan_peer.descriptor.clone();
                    desc.context_id = context_id;
                    let _ = rendezvous.cache_descriptor(desc).await;
                    tracing::debug!(
                        contact_id = %contact_id,
                        "Promoted LAN descriptor to local context after contact acceptance"
                    );
                }
            }

            tracing::debug!(
                contact_id = %contact_id,
                "ContactFact committed successfully"
            );
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "No contact resolution for invitation (not a contact invitation or already resolved)"
            );
        }

        Ok(())
    }

    async fn materialize_channel_acceptance_if_needed(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        if let Some(mut channel_invite) = self
            .resolve_channel_invitation(effects, invitation_id)
            .await?
        {
            channel_invite.context_id = self
                .resolve_channel_context_from_chat_facts(effects, &channel_invite)
                .await;

            if let Some(package) = channel_invite.bootstrap.clone() {
                let ChannelBootstrapPackage { bootstrap_id, key } = package;

                if key.len() != 32 {
                    return Err(crate::core::AgentError::invalid(format!(
                        "AMP bootstrap key has invalid length: {}",
                        key.len()
                    )));
                }

                let location = SecureStorageLocation::amp_bootstrap_key(
                    &channel_invite.context_id,
                    &channel_invite.channel_id,
                    &bootstrap_id,
                );

                effects
                    .secure_store(
                        &location,
                        &key,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await
                    .map_err(|e| {
                        crate::core::AgentError::effects(format!("store AMP bootstrap key: {e}"))
                    })?;

                self.materialize_channel_bootstrap_acceptance(
                    effects,
                    &channel_invite,
                    bootstrap_id,
                )
                .await?;
            }

            self.materialize_channel_invitation_acceptance(effects, &channel_invite)
                .await?;
        }

        Ok(())
    }

    async fn materialize_device_enrollment_acceptance_if_needed(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        // Device enrollment acceptance installs the issued share before the
        // invitee notifies the initiator runtime.
        if let Some(enrollment) = self
            .resolve_device_enrollment_invitation(effects, invitation_id)
            .await?
        {
            if !enrollment.baseline_tree_ops.is_empty() {
                let baseline_ops = enrollment
                    .baseline_tree_ops
                    .iter()
                    .map(|bytes| {
                        aura_core::util::serialization::from_slice(bytes).map_err(|e| {
                            crate::core::AgentError::internal(format!(
                                "decode device enrollment baseline tree op: {e}"
                            ))
                        })
                    })
                    .collect::<Result<Vec<aura_core::AttestedOp>, _>>()?;
                effects.import_tree_ops(&baseline_ops).await?;
            }

            let participant =
                aura_core::threshold::ParticipantIdentity::device(enrollment.device_id);
            let location = SecureStorageLocation::with_sub_key(
                "participant_shares",
                format!(
                    "{}/{}",
                    enrollment.subject_authority, enrollment.pending_epoch
                ),
                participant.storage_key(),
            );

            effects
                .secure_store(
                    &location,
                    &enrollment.key_package,
                    &[
                        SecureStorageCapability::Read,
                        SecureStorageCapability::Write,
                    ],
                )
                .await
                .map_err(|e| {
                    crate::core::AgentError::effects(format!(
                        "store device enrollment key package: {e}"
                    ))
                })?;

            let config_location = SecureStorageLocation::with_sub_key(
                "threshold_config",
                format!("{}", enrollment.subject_authority),
                format!("{}", enrollment.pending_epoch),
            );
            let pubkey_location = SecureStorageLocation::with_sub_key(
                "threshold_pubkey",
                format!("{}", enrollment.subject_authority),
                format!("{}", enrollment.pending_epoch),
            );

            if !enrollment.threshold_config.is_empty() {
                effects
                    .secure_store(
                        &config_location,
                        &enrollment.threshold_config,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await
                    .map_err(|e| {
                        crate::core::AgentError::effects(format!(
                            "store device enrollment threshold config: {e}"
                        ))
                    })?;
            }

            if !enrollment.public_key_package.is_empty() {
                effects
                    .secure_store(
                        &pubkey_location,
                        &enrollment.public_key_package,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await
                    .map_err(|e| {
                        crate::core::AgentError::effects(format!(
                            "store device enrollment public key package: {e}"
                        ))
                    })?;
            }
        }

        Ok(())
    }

    async fn execute_accept_invitation_follow_up(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
        invitation: Option<&Invitation>,
    ) -> AgentResult<()> {
        let Some(invitation) = invitation else {
            return Ok(());
        };

        match invitation.invitation_type {
            InvitationType::Contact { .. } => {
                tracing::debug!(
                    invitation_id = %invitation_id,
                    "Skipping synchronous invitation exchange receiver for accepted contact invitation"
                );
            }
            InvitationType::Guardian { .. } => {
                self
                    .execute_guardian_invitation_guardian(effects.clone(), invitation)
                    .await
                    .map_err(|error| {
                        AgentError::choreography(format!(
                            "guardian invitation accept follow-up failed for {invitation_id}: {error}"
                        ))
                    })?;
            }
            InvitationType::DeviceEnrollment { .. } => {
                let _ = effects;
                tracing::debug!(
                    invitation_id = %invitation_id,
                    "Skipping synchronous device enrollment invitee follow-up; invitation service owns the bounded post-accept task"
                );
            }
            InvitationType::Channel { .. } => {
                self.notify_channel_invitation_acceptance(effects.as_ref(), invitation_id)
                    .await?;
                tracing::debug!(
                    invitation_id = %invitation_id,
                    "Skipping synchronous invitation exchange receiver for accepted channel invitation"
                );
            }
        }

        Ok(())
    }

    pub(crate) async fn notify_contact_invitation_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        InvitationContactHandler::new(self)
            .notify_contact_invitation_acceptance(effects, invitation_id)
            .await
    }

    pub(crate) async fn notify_channel_invitation_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        InvitationChannelHandler::new(self)
            .notify_channel_invitation_acceptance(effects, invitation_id)
            .await
    }

    /// Process sender-side contact invitation acceptances.
    ///
    /// "Processed" means the acceptance envelope was decoded, validated, and
    /// materialized into the sender's authoritative contact/invitation state.
    pub async fn process_contact_invitation_acceptances(
        &self,
        effects: Arc<AuraEffectSystem>,
    ) -> AgentResult<ProcessedContactInvitationAcceptanceCount> {
        InvitationContactHandler::new(self)
            .process_contact_invitation_acceptances(effects)
            .await
    }

    async fn resolve_contact_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<(AuthorityId, String)>> {
        InvitationContactHandler::new(self)
            .resolve_contact_invitation(effects, invitation_id)
            .await
    }

    async fn resolve_device_enrollment_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<DeviceEnrollmentInvitation>> {
        InvitationDeviceEnrollmentHandler::new(self)
            .resolve_device_enrollment_invitation(effects, invitation_id)
            .await
    }

    async fn resolve_channel_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<ChannelInviteDetails>> {
        InvitationChannelHandler::new(self)
            .resolve_channel_invitation(effects, invitation_id)
            .await
    }

    async fn channel_created_fact_name(
        &self,
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        context_id: ContextId,
        channel_id: ChannelId,
    ) -> Option<String> {
        let Ok(envelopes) =
            load_relational_fact_envelopes_by_type(effects, authority_id, CHAT_FACT_TYPE_ID).await
        else {
            return None;
        };

        for envelope in envelopes {
            let Some(ChatFact::ChannelCreated {
                context_id: seen_context,
                channel_id: seen_channel,
                name,
                ..
            }) = ChatFact::from_envelope(&envelope)
            else {
                continue;
            };

            if seen_context == context_id && seen_channel == channel_id {
                return Some(name);
            }
        }

        None
    }

    async fn resolve_channel_context_from_chat_facts(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
    ) -> ContextId {
        InvitationChannelHandler::new(self)
            .resolve_channel_context_from_chat_facts(effects, invite)
            .await
    }

    async fn materialize_channel_invitation_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
    ) -> AgentResult<()> {
        InvitationChannelHandler::new(self)
            .materialize_channel_invitation_acceptance(effects, invite)
            .await
    }

    async fn materialize_channel_bootstrap_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
        bootstrap_id: Hash32,
    ) -> AgentResult<()> {
        InvitationChannelHandler::new(self)
            .materialize_channel_bootstrap_acceptance(effects, invite, bootstrap_id)
            .await
    }

    /// Import an invitation from a shareable code into the local cache.
    ///
    /// This is a best-effort, local-only operation used for out-of-band invite
    /// transfer (copy/paste). It does not commit any facts by itself; callers
    /// should accept/decline via the normal guard-chain paths.
    pub async fn import_invitation_code(
        &self,
        effects: &AuraEffectSystem,
        code: &str,
    ) -> AgentResult<Invitation> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        let shareable = ShareableInvitation::from_code(code)
            .map_err(|e| crate::core::AgentError::invalid(format!("{e}")))?;
        let sender_hint_addr = ShareableInvitation::sender_addr_from_code(code);
        let sender_device_id = ShareableInvitation::sender_device_id_from_code(code);
        tracing::info!(
            invitation_id = %shareable.invitation_id,
            sender = %shareable.sender_id,
            sender_hint_addr = ?sender_hint_addr,
            sender_device_id = ?sender_device_id,
            "import_invitation_code parsed sender hint"
        );

        tracing::debug!(
            invitation_id = %shareable.invitation_id,
            sender = %shareable.sender_id,
            invitation_type = ?shareable.invitation_type,
            "Importing invite code with context={:?}",
            shareable.context_id
        );

        let invitation_id = shareable.invitation_id.clone();

        // Fast path: already cached.
        if let Some(existing) = self.invitation_cache.get_invitation(&invitation_id).await {
            tracing::debug!(
                invitation_id = %invitation_id,
                status = ?existing.status,
                "Returning existing cached invitation"
            );
            return Ok(existing);
        }

        self.validate_importable_shareable_invitation(&shareable)?;

        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
        // Persist the imported invitation with local status so later
        // storage-backed reads do not downgrade accepted/declined state.
        Self::persist_imported_invitation(
            effects,
            self.context.authority.authority_id(),
            &StoredImportedInvitation::pending(shareable.clone(), now_ms),
        )
        .await?;
        if let Some(addr) = sender_hint_addr.as_deref() {
            self.cache_peer_descriptor_for_peer(
                effects,
                shareable.sender_id,
                sender_device_id,
                Some(addr),
                now_ms,
            )
            .await;
            let cached_descriptor = if let Some(manager) = effects.rendezvous_manager() {
                manager
                    .get_descriptor(
                        default_context_id_for_authority(shareable.sender_id),
                        shareable.sender_id,
                    )
                    .await
            } else {
                None
            };
            let websocket_hint_count = cached_descriptor
                .as_ref()
                .map(|descriptor| {
                    descriptor
                        .transport_hints
                        .iter()
                        .filter(|hint| matches!(hint, TransportHint::WebSocketDirect { .. }))
                        .count()
                })
                .unwrap_or(0);
            tracing::info!(
                invitation_id = %shareable.invitation_id,
                sender = %shareable.sender_id,
                websocket_hint_count,
                "import_invitation_code cached direct descriptor"
            );
        } else if sender_device_id.is_some() {
            self.cache_peer_descriptor_for_peer(
                effects,
                shareable.sender_id,
                sender_device_id,
                None,
                now_ms,
            )
            .await;
        } else if let Some(manager) = effects.rendezvous_manager() {
            if let Some(peer) = manager.get_lan_discovered_peer(shareable.sender_id).await {
                let _ = manager.cache_descriptor(peer.descriptor.clone()).await;
                let websocket_hint_count = peer
                    .descriptor
                    .transport_hints
                    .iter()
                    .filter(|hint| matches!(hint, TransportHint::WebSocketDirect { .. }))
                    .count();
                tracing::info!(
                    invitation_id = %shareable.invitation_id,
                    sender = %shareable.sender_id,
                    websocket_hint_count,
                    "import_invitation_code cached discovered peer descriptor"
                );
            }
        }
        let context_id = match &shareable.invitation_type {
            InvitationType::Channel { .. } => require_channel_invitation_context(
                &shareable.invitation_id,
                shareable.sender_id,
                shareable.context_id,
            )?,
            _ => self.context.effect_context.context_id(),
        };

        // Imported invitations are "received" by the current authority.
        let invitation = Invitation {
            invitation_id: invitation_id.clone(),
            context_id,
            sender_id: shareable.sender_id,
            receiver_id: self.context.authority.authority_id(),
            invitation_type: shareable.invitation_type,
            status: InvitationStatus::Pending,
            created_at: now_ms,
            expires_at: shareable.expires_at,
            message: shareable.message,
        };

        // Known limitation: imported invitations are cached eagerly and the
        // cache is currently unbounded until a proper TTL/LRU policy lands.
        self.invitation_cache
            .cache_invitation(invitation.clone())
            .await;
        crate::reactive::app_signal_views::materialize_pending_invitation_signal(
            &effects.reactive_handler(),
            self.context.authority.authority_id(),
            invitation.invitation_id.as_str(),
            invitation.sender_id,
            invitation.receiver_id,
            &invitation.invitation_type,
            invitation.created_at,
            invitation.expires_at,
            invitation.message.clone(),
        )
        .await
        .map_err(AgentError::runtime)?;

        Ok(invitation)
    }

    pub(crate) async fn cache_peer_descriptor_for_peer(
        &self,
        effects: &AuraEffectSystem,
        peer: AuthorityId,
        device_id: Option<DeviceId>,
        addr: Option<&str>,
        now_ms: u64,
    ) {
        fn descriptor_hint_from_addr(addr: Option<&str>) -> Option<TransportHint> {
            let addr = addr?;
            if addr.starts_with("ws://") || addr.starts_with("wss://") {
                let normalized = addr
                    .trim_start_matches("ws://")
                    .trim_start_matches("wss://");
                TransportHint::websocket_direct(normalized).ok()
            } else if addr.starts_with("tcp://") {
                let normalized = addr.trim_start_matches("tcp://");
                TransportHint::tcp_direct(normalized).ok()
            } else {
                // Treat bare host:port hints as TCP. WebSocket hints must carry an
                // explicit scheme so browser runtimes never misclassify raw TCP
                // listener addresses as websocket endpoints.
                TransportHint::tcp_direct(addr).ok()
            }
        }

        let Some(manager) = effects.rendezvous_manager() else {
            return;
        };
        let peer_context_id = default_context_id_for_authority(peer);
        // Placeholder descriptors are transport-hint carriers only. Their
        // cryptographic fields remain zero-filled until a real rendezvous
        // descriptor arrives from the peer or is materialized from invite data.
        let descriptor = if let Some(existing) = manager.get_descriptor(peer_context_id, peer).await
        {
            let mut transport_hints = existing.transport_hints.clone();
            if let Some(hint) = descriptor_hint_from_addr(addr) {
                if !transport_hints.contains(&hint) {
                    transport_hints.push(hint);
                }
            }
            RendezvousDescriptor {
                authority_id: existing.authority_id,
                device_id: device_id.or(existing.device_id),
                context_id: existing.context_id,
                transport_hints,
                handshake_psk_commitment: existing.handshake_psk_commitment,
                public_key: existing.public_key,
                valid_from: existing.valid_from.min(now_ms.saturating_sub(1)),
                valid_until: existing
                    .valid_until
                    .max(now_ms.saturating_add(DESCRIPTOR_VALIDITY_WINDOW_MS)),
                nonce: existing.nonce,
                nickname_suggestion: existing.nickname_suggestion.clone(),
            }
        } else {
            RendezvousDescriptor {
                authority_id: peer,
                device_id,
                context_id: peer_context_id,
                transport_hints: descriptor_hint_from_addr(addr).into_iter().collect(),
                handshake_psk_commitment: [0u8; 32],
                public_key: [0u8; 32],
                valid_from: now_ms.saturating_sub(1),
                valid_until: now_ms.saturating_add(DESCRIPTOR_VALIDITY_WINDOW_MS),
                nonce: [0u8; 32],
                nickname_suggestion: None,
            }
        };
        let _ = manager.cache_descriptor(descriptor).await;

        let local_context_id = self.context.authority.default_context_id();
        if local_context_id != peer_context_id {
            let mut local_descriptor = manager
                .get_descriptor(local_context_id, peer)
                .await
                .unwrap_or_else(|| RendezvousDescriptor {
                    authority_id: peer,
                    device_id,
                    context_id: local_context_id,
                    transport_hints: Vec::new(),
                    handshake_psk_commitment: [0u8; 32],
                    public_key: [0u8; 32],
                    valid_from: now_ms.saturating_sub(1),
                    valid_until: now_ms.saturating_add(DESCRIPTOR_VALIDITY_WINDOW_MS),
                    nonce: [0u8; 32],
                    nickname_suggestion: None,
                });
            if local_descriptor.device_id.is_none() {
                local_descriptor.device_id = device_id;
            }
            if let Some(hint) = descriptor_hint_from_addr(addr) {
                if !local_descriptor.transport_hints.contains(&hint) {
                    local_descriptor.transport_hints.push(hint);
                }
            }
            local_descriptor.valid_from = local_descriptor.valid_from.min(now_ms.saturating_sub(1));
            local_descriptor.valid_until = local_descriptor
                .valid_until
                .max(now_ms.saturating_add(DESCRIPTOR_VALIDITY_WINDOW_MS));
            let _ = manager.cache_descriptor(local_descriptor).await;
        }
    }

    /// Decline an invitation
    pub async fn decline_invitation(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        self.validate_cached_invitation_decline(effects.as_ref(), invitation_id)
            .await?;

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects.as_ref()).await;
        let outcome = self
            .service
            .prepare_decline_invitation(&snapshot, invitation_id);

        // Execute the outcome
        execute_guard_outcome(outcome, &self.context.authority, effects.as_ref()).await?;

        let now_ms = Self::best_effort_current_timestamp_ms(effects.as_ref()).await;
        self.update_imported_invitation_status_if_present(
            effects.as_ref(),
            invitation_id,
            InvitationStatus::Declined,
            now_ms,
        )
        .await?;

        // Update cache if we have this invitation
        let _ = self
            .invitation_cache
            .update_invitation(invitation_id, |inv| {
                inv.status = InvitationStatus::Declined;
            })
            .await;

        if let Some(invitation) = self
            .load_invitation_for_choreography(effects.as_ref(), invitation_id)
            .await
        {
            if matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                tracing::debug!(
                    invitation_id = %invitation_id,
                    "Skipping synchronous invitation exchange receiver for declined channel invitation"
                );
            } else if !matches!(invitation.invitation_type, InvitationType::Guardian { .. }) {
                if let Err(error) = self
                    .execute_invitation_exchange_receiver(effects.clone(), &invitation, false)
                    .await
                {
                    tracing::warn!(
                        invitation_id = %invitation_id,
                        error = %error,
                        "decline invitation follow-up exchange failed after local decline"
                    );
                }
            }
        }

        Ok(InvitationResult::new(
            invitation_id.clone(),
            InvitationStatus::Declined,
        ))
    }

    /// Cancel an invitation (sender only)
    pub async fn cancel_invitation(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;
        let own_id = self.context.authority.authority_id();

        self.validate_cached_invitation_cancel(effects.as_ref(), invitation_id)
            .await?;

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects.as_ref()).await;
        let outcome = self
            .service
            .prepare_cancel_invitation(&snapshot, invitation_id);

        // Execute the outcome
        execute_guard_outcome(outcome, &self.context.authority, effects.as_ref()).await?;

        if let Some(mut invitation) =
            InvitationCacheHandler::load_created_invitation(effects.as_ref(), own_id, invitation_id)
                .await
        {
            invitation.status = InvitationStatus::Cancelled;
            InvitationCacheHandler::persist_created_invitation(
                effects.as_ref(),
                own_id,
                &invitation,
            )
            .await?;
            self.invitation_cache.cache_invitation(invitation).await;
        } else {
            let _ = self.invitation_cache.remove_invitation(invitation_id).await;
        }

        Ok(InvitationResult::new(
            invitation_id.clone(),
            InvitationStatus::Cancelled,
        ))
    }

    /// List pending invitations (from cache)
    pub async fn list_pending(&self) -> Vec<Invitation> {
        self.invitation_cache
            .list_matching(|inv| inv.status == InvitationStatus::Pending)
            .await
    }

    /// List cached invitations matching a predicate.
    pub async fn list_cached_matching(
        &self,
        predicate: impl Fn(&Invitation) -> bool,
    ) -> Vec<Invitation> {
        self.invitation_cache.list_matching(predicate).await
    }

    /// List invitations from cache plus persisted stores.
    pub async fn list_with_storage(&self, effects: &AuraEffectSystem) -> Vec<Invitation> {
        let mut invitations: HashMap<InvitationId, Invitation> = HashMap::new();
        for invitation in self.list_cached_matching(|_| true).await {
            InvitationCacheHandler::merge_invitation(&mut invitations, invitation);
        }
        let own_id = self.context.authority.authority_id();
        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;

        let created_prefix = InvitationCacheHandler::created_invitation_prefix(own_id);
        if let Ok(keys) = effects.list_keys(Some(&created_prefix)).await {
            for key in keys {
                let Ok(Some(bytes)) = effects.retrieve(&key).await else {
                    continue;
                };
                let Ok(invitation) = serde_json::from_slice::<Invitation>(&bytes) else {
                    continue;
                };
                self.invitation_cache
                    .cache_invitation(invitation.clone())
                    .await;
                InvitationCacheHandler::merge_invitation(&mut invitations, invitation);
            }
        }

        let imported_prefix = InvitationCacheHandler::imported_invitation_prefix(own_id);
        if let Ok(keys) = effects.list_keys(Some(&imported_prefix)).await {
            for key in keys {
                let Ok(Some(bytes)) = effects.retrieve(&key).await else {
                    continue;
                };
                let preserved = serde_json::from_slice::<ShareableInvitation>(&bytes)
                    .ok()
                    .and_then(|shareable| invitations.get(&shareable.invitation_id));
                let Some(stored) =
                    InvitationCacheHandler::parse_imported_invitation_bytes(&bytes, preserved)
                else {
                    continue;
                };
                let status = stored.status.clone();
                let created_at = stored.created_at;
                let shareable = stored.shareable;

                let context_id = match &shareable.invitation_type {
                    InvitationType::Channel { .. } => match require_channel_invitation_context(
                        &shareable.invitation_id,
                        shareable.sender_id,
                        shareable.context_id,
                    ) {
                        Ok(context_id) => context_id,
                        Err(error) => {
                            tracing::warn!(
                                invitation_id = %shareable.invitation_id,
                                sender = %shareable.sender_id,
                                error = %error,
                                "Skipping imported channel invitation without authoritative context"
                            );
                            continue;
                        }
                    },
                    _ => self.context.effect_context.context_id(),
                };

                let invitation = Invitation {
                    invitation_id: shareable.invitation_id,
                    context_id,
                    sender_id: shareable.sender_id,
                    receiver_id: own_id,
                    invitation_type: shareable.invitation_type,
                    status,
                    created_at: if created_at == 0 { now_ms } else { created_at },
                    expires_at: shareable.expires_at,
                    message: shareable.message,
                };

                let should_cache =
                    invitations
                        .get(&invitation.invitation_id)
                        .map_or(true, |existing| {
                            InvitationCacheHandler::should_replace_invitation(existing, &invitation)
                        });
                if should_cache {
                    self.invitation_cache
                        .cache_invitation(invitation.clone())
                        .await;
                }
                InvitationCacheHandler::merge_invitation(&mut invitations, invitation);
            }
        }

        invitations.into_values().collect()
    }

    /// List pending invitations from cache plus persisted stores.
    ///
    /// This allows runtime components using separate handler instances to
    /// converge on a shared pending invitation view.
    pub async fn list_pending_with_storage(&self, effects: &AuraEffectSystem) -> Vec<Invitation> {
        self.list_with_storage(effects)
            .await
            .into_iter()
            .filter(|inv| inv.status == InvitationStatus::Pending)
            .collect()
    }

    async fn load_invitation_for_choreography(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        if let Some(inv) = self.invitation_cache.get_invitation(invitation_id).await {
            return Some(inv);
        }

        let own_id = self.context.authority.authority_id();
        if let Some(inv) = Self::load_created_invitation(effects, own_id, invitation_id).await {
            return Some(inv);
        }

        if let Some(stored) =
            Self::load_imported_invitation(effects, own_id, invitation_id, None).await
        {
            let status = stored.status.clone();
            let created_at = stored.created_at;
            let shareable = stored.shareable;
            let context_id = match &shareable.invitation_type {
                InvitationType::Channel { .. } => {
                    match require_channel_invitation_context(
                        &shareable.invitation_id,
                        shareable.sender_id,
                        shareable.context_id,
                    ) {
                        Ok(context_id) => context_id,
                        Err(error) => {
                            tracing::warn!(
                                invitation_id = %shareable.invitation_id,
                                sender = %shareable.sender_id,
                                error = %error,
                                "Skipping imported channel invitation choreography without authoritative context"
                            );
                            return None;
                        }
                    }
                }
                _ => self.context.effect_context.context_id(),
            };
            let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
            return Some(Invitation {
                invitation_id: shareable.invitation_id,
                context_id,
                sender_id: shareable.sender_id,
                receiver_id: own_id,
                invitation_type: shareable.invitation_type,
                status,
                created_at: if created_at == 0 { now_ms } else { created_at },
                expires_at: shareable.expires_at,
                message: shareable.message,
            });
        }

        None
    }

    fn invitation_session_id(invitation_id: &InvitationId) -> Uuid {
        let digest = hash(invitation_id.as_str().as_bytes());
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        Uuid::from_bytes(bytes)
    }

    fn build_invitation_offer(invitation: &Invitation) -> InvitationOffer {
        let mut material = Vec::new();
        material.extend_from_slice(invitation.invitation_id.as_str().as_bytes());
        material.extend_from_slice(&invitation.sender_id.to_bytes());
        material.extend_from_slice(&invitation.receiver_id.to_bytes());
        if let Some(expires_at) = invitation.expires_at {
            material.extend_from_slice(&expires_at.to_le_bytes());
        }
        let commitment_hash = hash(&material);

        InvitationOffer {
            invitation_id: invitation.invitation_id.clone(),
            invitation_type: invitation.invitation_type.clone(),
            sender: invitation.sender_id,
            message: invitation.message.clone(),
            expires_at_ms: invitation.expires_at,
            commitment: commitment_hash,
        }
    }

    #[cfg(feature = "choreo-backend-telltale-vm")]
    fn invitation_exchange_peer_roles(
        authority_id: AuthorityId,
        peer_id: AuthorityId,
    ) -> (ChoreographicRole, ChoreographicRole, Vec<ChoreographicRole>) {
        // Invitation exchanges use one protocol role per authority. The VM
        // session resolves concrete participants by authority id and protocol
        // role name ("Sender"/"Receiver"), so both authorities legitimately use
        // their local role slot 0 here.
        let sender_index = RoleIndex::new(0).expect("sender role index");
        let receiver_index = RoleIndex::new(0).expect("receiver role index");
        let local_role = ChoreographicRole::for_authority(authority_id, sender_index);
        let peer_role = ChoreographicRole::for_authority(peer_id, receiver_index);
        (local_role, peer_role, vec![local_role, peer_role])
    }

    #[cfg(feature = "choreo-backend-telltale-vm")]
    async fn execute_invitation_exchange_sender_vm(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let authority_id = self.context.authority.authority_id();
        let (_local_role, peer_role, roles) =
            Self::invitation_exchange_peer_roles(authority_id, invitation.receiver_id);
        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        let offer = ExchangeInvitationOffer(Self::build_invitation_offer(invitation));
        let peer_roles = BTreeMap::from([("Receiver".to_string(), peer_role)]);
        let budget = invitation_timeout_budget(
            effects.as_ref(),
            "invitation_exchange_sender_vm",
            INVITATION_VM_LOOP_TIMEOUT_MS,
        )
        .await?;

        let manifest =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::composition_manifest();
        let global_type =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::global_type();
        let local_types =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::local_types();
        let mut session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id,
            roles,
            &manifest,
            "Sender",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(|error| AgentError::internal(error.to_string()))?;
        session.queue_send_bytes(
            to_vec(&offer)
                .map_err(|error| AgentError::internal(format!("offer encode failed: {error}")))?,
        );

        let loop_result = execute_with_timeout_budget(effects.as_ref(), &budget, || async {
            loop {
                let round = session
                    .advance_round_until_receive(
                        "Sender",
                        &peer_roles,
                        Self::is_transport_no_message,
                    )
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    let response: ExchangeInvitationResponse = from_slice(&blocked.payload)
                        .map_err(|error| {
                            AgentError::internal(format!(
                                "invitation response decode failed: {error}"
                            ))
                        })?;
                    if response.0.accepted {
                        if let InvitationType::Channel {
                            home_id,
                            nickname_suggestion,
                            ..
                        } = &invitation.invitation_type
                        {
                            let reactive = effects.reactive_handler();
                            let now_ms =
                                Self::best_effort_current_timestamp_ms(effects.as_ref()).await;
                            let home_name = require_channel_invitation_name(
                                *home_id,
                                nickname_suggestion.clone(),
                            )?;
                            app_signal_views::materialize_home_signal_for_channel_acceptance(
                                &reactive,
                                *home_id,
                                &home_name,
                                invitation.sender_id,
                                invitation.receiver_id,
                                invitation.context_id,
                                now_ms,
                            )
                            .await
                            .map_err(AgentError::runtime)?;
                        }
                    }
                    let status = if response.0.accepted {
                        aura_invitation::InvitationAckStatus::Accepted
                    } else {
                        aura_invitation::InvitationAckStatus::Declined
                    };
                    let ack = ExchangeInvitationAck(InvitationAck {
                        invitation_id: invitation.invitation_id.clone(),
                        success: true,
                        status,
                    });
                    session.queue_send_bytes(to_vec(&ack).map_err(|error| {
                        AgentError::internal(format!("invitation ack encode failed: {error}"))
                    })?);
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Deferred => break Ok(()),
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "invitation sender VM timed out while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "invitation sender VM cancelled while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "invitation sender VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            }
        })
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => AgentError::timeout(format!(
                "invitation sender VM exceeded {}ms overall timeout",
                budget.timeout_ms()
            )),
            TimeoutRunError::Operation(error) => error,
        });

        let _ = session.close().await;
        loop_result
    }

    #[cfg(feature = "choreo-backend-telltale-vm")]
    async fn execute_invitation_exchange_receiver_vm(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        accepted: bool,
    ) -> AgentResult<()> {
        let authority_id = self.context.authority.authority_id();
        let (_local_role, peer_role, roles) =
            Self::invitation_exchange_peer_roles(authority_id, invitation.sender_id);
        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        let response = ExchangeInvitationResponse(InvitationResponse {
            invitation_id: invitation.invitation_id.clone(),
            accepted,
            message: None,
            signature: Vec::new(),
        });
        let mut response_queued = false;
        let peer_roles = BTreeMap::from([("Sender".to_string(), peer_role)]);
        let budget = invitation_timeout_budget(
            effects.as_ref(),
            "invitation_exchange_receiver_vm",
            INVITATION_VM_LOOP_TIMEOUT_MS,
        )
        .await?;

        let manifest =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::composition_manifest();
        let global_type =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::global_type();
        let local_types =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::local_types();
        let mut session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id,
            roles,
            &manifest,
            "Receiver",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(|error| AgentError::internal(error.to_string()))?;

        let loop_result = execute_with_timeout_budget(effects.as_ref(), &budget, || async {
            loop {
                let round = session
                    .advance_round("Receiver", &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    if !response_queued {
                        session.queue_send_bytes(to_vec(&response).map_err(|error| {
                            AgentError::internal(format!(
                                "invitation response encode failed: {error}"
                            ))
                        })?);
                        response_queued = true;
                    }
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "invitation receiver VM timed out while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "invitation receiver VM cancelled while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "invitation receiver VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            }
        })
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => AgentError::timeout(format!(
                "invitation receiver VM exceeded {}ms overall timeout",
                budget.timeout_ms()
            )),
            TimeoutRunError::Operation(error) => error,
        });

        let _ = session.close().await;
        loop_result
    }

    async fn execute_invitation_exchange_sender(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        self.execute_invitation_exchange_sender_vm(effects, invitation)
            .await
    }

    pub(crate) async fn execute_channel_invitation_exchange_sender(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        self.execute_invitation_exchange_sender(effects, invitation)
            .await
    }

    async fn execute_invitation_exchange_receiver(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        accepted: bool,
    ) -> AgentResult<()> {
        self.execute_invitation_exchange_receiver_vm(effects, invitation, accepted)
            .await
    }

    async fn execute_guardian_invitation_principal(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationGuardianHandler::new(self)
            .execute_guardian_invitation_principal(effects, invitation)
            .await
    }

    /// Check if a choreography error is a recoverable transport condition.
    ///
    /// `NoMessage` (no pending inbound envelope) and `DestinationUnreachable`
    /// (peer not routable) are both expected when the remote party hasn't
    /// joined the choreography yet or when transport is unavailable.
    fn is_transport_no_message(err: &ChoreographyError) -> bool {
        match err {
            ChoreographyError::Transport { source } => source
                .downcast_ref::<TransportError>()
                .is_some_and(|inner| {
                    matches!(
                        inner,
                        TransportError::NoMessage | TransportError::DestinationUnreachable { .. }
                    )
                }),
            _ => false,
        }
    }

    async fn execute_guardian_invitation_guardian(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationGuardianHandler::new(self)
            .execute_guardian_invitation_guardian(effects, invitation)
            .await
    }

    /// Execute the DeviceEnrollment choreography as initiator (existing device).
    ///
    /// This method runs the 3-message choreography:
    /// 1. Initiator sends DeviceEnrollmentRequest to Invitee
    /// 2. Invitee responds with DeviceEnrollmentAccept
    /// 3. Initiator sends DeviceEnrollmentConfirm
    pub(crate) async fn execute_device_enrollment_initiator(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationDeviceEnrollmentHandler::new(self)
            .execute_device_enrollment_initiator(effects, invitation)
            .await
    }

    /// Execute the DeviceEnrollment choreography as invitee (new device).
    ///
    /// This method runs the invitee side of the 3-message choreography:
    /// 1. Receive DeviceEnrollmentRequest from Initiator
    /// 2. Send DeviceEnrollmentAccept to Initiator
    /// 3. Receive DeviceEnrollmentConfirm from Initiator
    pub(crate) async fn execute_device_enrollment_invitee(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationDeviceEnrollmentHandler::new(self)
            .execute_device_enrollment_invitee(effects, invitation)
            .await
    }

    /// Get an invitation by ID (from in-memory cache only)
    pub async fn get_invitation(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        InvitationCacheHandler::new(self)
            .get_invitation(invitation_id)
            .await
    }

    /// Get an invitation by ID, checking both cache and persistent storage
    pub async fn get_invitation_with_storage(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        InvitationCacheHandler::new(self)
            .get_invitation_with_storage(effects, invitation_id)
            .await
    }
}

#[derive(Debug, Clone)]
struct DeviceEnrollmentInvitation {
    subject_authority: AuthorityId,
    device_id: aura_core::DeviceId,
    pending_epoch: u64,
    key_package: Vec<u8>,
    threshold_config: Vec<u8>,
    public_key_package: Vec<u8>,
    baseline_tree_ops: Vec<Vec<u8>>,
}

// =============================================================================
// Shareable Invitation (Out-of-Band Sharing)
// =============================================================================

/// Error type for shareable invitation operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareableInvitationError {
    /// Invalid invite code format
    InvalidFormat,
    /// Unsupported version
    UnsupportedVersion(u8),
    /// Base64 decoding failed
    DecodingFailed,
    /// JSON parsing failed
    ParsingFailed,
    /// JSON serialization failed
    SerializationFailed,
}

impl std::fmt::Display for ShareableInvitationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "invalid invite code format"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version: {}", v),
            Self::DecodingFailed => write!(f, "base64 decoding failed"),
            Self::ParsingFailed => write!(f, "JSON parsing failed"),
            Self::SerializationFailed => write!(f, "JSON serialization failed"),
        }
    }
}

impl std::error::Error for ShareableInvitationError {}

/// Shareable invitation for out-of-band transfer
///
/// This struct contains the minimal information needed to redeem an invitation.
/// It can be encoded as a string (format: `aura:v1:<base64>`) for sharing via
/// copy/paste, QR codes, etc.
///
/// # Example
///
/// ```ignore
/// // Export an invitation
/// let shareable = ShareableInvitation::from(&invitation);
/// let code = shareable.to_code()?;
/// println!("Share this code: {}", code);
///
/// // Import an invitation
/// let decoded = ShareableInvitation::from_code(&code)?;
/// println!("Invitation from: {}", decoded.sender_id);
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShareableInvitation {
    /// Version number for forward compatibility
    pub version: u8,
    /// Unique invitation identifier
    pub invitation_id: InvitationId,
    /// Sender authority
    pub sender_id: AuthorityId,
    /// Context for invitation-scoped facts, when known.
    ///
    /// Older invite codes may omit this and rely on channel defaults.
    #[serde(default)]
    pub context_id: Option<ContextId>,
    /// Type of invitation
    pub invitation_type: InvitationType,
    /// Expiration timestamp (ms), if any
    pub expires_at: Option<u64>,
    /// Optional message from sender
    pub message: Option<String>,
}

fn default_imported_invitation_status() -> InvitationStatus {
    InvitationStatus::Pending
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredImportedInvitation {
    #[serde(flatten)]
    shareable: ShareableInvitation,
    #[serde(default = "default_imported_invitation_status")]
    status: InvitationStatus,
    #[serde(default)]
    created_at: u64,
}

impl StoredImportedInvitation {
    fn pending(shareable: ShareableInvitation, created_at: u64) -> Self {
        Self {
            shareable,
            status: InvitationStatus::Pending,
            created_at,
        }
    }
}

impl std::ops::Deref for StoredImportedInvitation {
    type Target = ShareableInvitation;

    fn deref(&self) -> &Self::Target {
        &self.shareable
    }
}

impl ShareableInvitation {
    /// Current version of the shareable invitation format
    pub const CURRENT_VERSION: u8 = 1;

    /// Protocol prefix for invite codes
    pub const PREFIX: &'static str = "aura";

    /// Encode the invitation as a shareable code string
    ///
    /// Format: `aura:v1:<base64-encoded-json>`
    pub fn to_code(&self) -> Result<String, ShareableInvitationError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let json =
            serde_json::to_vec(self).map_err(|_| ShareableInvitationError::SerializationFailed)?;
        let b64 = URL_SAFE_NO_PAD.encode(&json);
        Ok(format!("{}:v{}:{}", Self::PREFIX, self.version, b64))
    }

    /// Decode an invitation from a code string
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The format is invalid (wrong prefix, missing parts)
    /// - The version is unsupported
    /// - Base64 decoding fails
    /// - JSON parsing fails
    pub fn from_code(code: &str) -> Result<Self, ShareableInvitationError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let parts: Vec<&str> = code.split(':').collect();
        if !(3..=5).contains(&parts.len()) {
            return Err(ShareableInvitationError::InvalidFormat);
        }

        if parts[0] != Self::PREFIX {
            return Err(ShareableInvitationError::InvalidFormat);
        }

        // Parse version (format: "v1", "v2", etc.)
        let version_str = parts[1];
        if !version_str.starts_with('v') {
            return Err(ShareableInvitationError::InvalidFormat);
        }
        let version: u8 = version_str[1..]
            .parse()
            .map_err(|_| ShareableInvitationError::InvalidFormat)?;

        if version != Self::CURRENT_VERSION {
            return Err(ShareableInvitationError::UnsupportedVersion(version));
        }

        let json = URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|_| ShareableInvitationError::DecodingFailed)?;

        serde_json::from_slice(&json).map_err(|_| ShareableInvitationError::ParsingFailed)
    }

    /// Extract optional sender transport address from a code.
    ///
    /// Codes may include an optional 4th segment:
    /// `aura:v1:<payload-b64>:<sender-addr-b64>`.
    pub fn sender_addr_from_code(code: &str) -> Option<String> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != 4 && parts.len() != 5 {
            return None;
        }
        if parts[0] != Self::PREFIX {
            return None;
        }

        let decoded = URL_SAFE_NO_PAD.decode(parts[3]).ok()?;
        let addr = String::from_utf8(decoded).ok()?;
        let trimmed = addr.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.to_string())
    }

    /// Extract optional sender device identity from a code.
    ///
    /// Codes may include an optional 5th segment:
    /// `aura:v1:<payload-b64>:<sender-addr-b64>:<sender-device-id-b64>`.
    pub fn sender_device_id_from_code(code: &str) -> Option<DeviceId> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != 5 {
            return None;
        }
        if parts[0] != Self::PREFIX {
            return None;
        }

        let decoded = URL_SAFE_NO_PAD.decode(parts[4]).ok()?;
        let device_id = String::from_utf8(decoded).ok()?;
        device_id.trim().parse().ok()
    }
}

impl From<&Invitation> for ShareableInvitation {
    fn from(inv: &Invitation) -> Self {
        Self {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: inv.invitation_id.clone(),
            sender_id: inv.sender_id,
            context_id: Some(inv.context_id),
            invitation_type: inv.invitation_type.clone(),
            expires_at: inv.expires_at,
            message: inv.message.clone(),
        }
    }
}

// =============================================================================
// Guard Outcome Execution (effect commands)
// =============================================================================

/// Execute a guard outcome's effect commands.
///
/// Takes a `GuardOutcome` from `aura_invitation::InvitationService` and
/// executes each `EffectCommand` using the agent's effect system.
pub async fn execute_guard_outcome(
    outcome: aura_invitation::guards::GuardOutcome,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    if outcome.is_denied() {
        let reason = outcome
            .decision
            .denial_reason()
            .map(ToString::to_string)
            .unwrap_or_else(|| "Operation denied".to_string());
        return Err(AgentError::effects(format!(
            "Guard denied operation: {}",
            reason
        )));
    }

    let context_id = authority.default_context_id();
    let charge_peer =
        resolve_charge_peer(
            &outcome.effects,
            authority.authority_id(),
            |command| match command {
                aura_invitation::guards::EffectCommand::NotifyPeer { peer, .. } => Some(*peer),
                aura_invitation::guards::EffectCommand::RecordReceipt { peer, .. } => *peer,
                _ => None,
            },
        );
    let mut pending_receipt: Option<Receipt> = None;

    for command in outcome.effects {
        execute_effect_command(
            command,
            authority,
            context_id,
            effects,
            charge_peer,
            &mut pending_receipt,
            false,
        )
        .await?;
    }

    Ok(())
}

pub async fn execute_guard_outcome_for_accept(
    outcome: aura_invitation::guards::GuardOutcome,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    let (local_effects, deferred_network_effects) =
        split_invitation_accept_guard_outcome(outcome, authority)?;
    execute_invitation_effect_commands(local_effects, authority, effects, false).await?;
    if let Err(error) = execute_invitation_effect_commands(
        deferred_network_effects.commands,
        authority,
        effects,
        true,
    )
    .await
    {
        tracing::warn!(
            authority = %authority.authority_id(),
            error = %error,
            "accept invitation continuing after deferred network side-effect failure"
        );
    }
    Ok(())
}

/// Accept keeps flow-budget charging and receipt recording local so the
/// authoritative accept settlement stays atomic even if the peer notification is
/// deferred or fails later.
fn split_invitation_accept_guard_outcome(
    outcome: aura_invitation::guards::GuardOutcome,
    authority: &AuthorityContext,
) -> AgentResult<(
    Vec<aura_invitation::guards::EffectCommand>,
    DeferredInvitationNetworkEffects,
)> {
    if outcome.is_denied() {
        let reason = outcome
            .decision
            .denial_reason()
            .map(ToString::to_string)
            .unwrap_or_else(|| "Operation denied".to_string());
        return Err(AgentError::effects(format!(
            "Guard denied operation: {}",
            reason
        )));
    }

    let mut local_effects = Vec::new();
    let mut deferred_network_effects = Vec::new();
    for command in outcome.effects {
        match command {
            aura_invitation::guards::EffectCommand::NotifyPeer { .. } => {
                deferred_network_effects.push(command);
            }
            aura_invitation::guards::EffectCommand::ChargeFlowBudget { .. }
            | aura_invitation::guards::EffectCommand::JournalAppend { .. }
            | aura_invitation::guards::EffectCommand::RecordReceipt { .. } => {
                local_effects.push(command);
            }
        }
    }

    tracing::debug!(
        authority = %authority.authority_id(),
        local_effect_count = local_effects.len(),
        deferred_network_effect_count = deferred_network_effects.len(),
        "Prepared invitation accept guard outcome with deferred peer notification side effects"
    );

    Ok((
        local_effects,
        DeferredInvitationNetworkEffects::new(deferred_network_effects),
    ))
}

/// Send defers every outwardly visible side effect except the journal append so
/// invitation creation can publish the authoritative pending fact before budget,
/// receipt, and network effects run on their own timeout policy.
fn split_invitation_send_guard_outcome(
    outcome: aura_invitation::guards::GuardOutcome,
    authority: &AuthorityContext,
) -> AgentResult<(
    Vec<aura_invitation::guards::EffectCommand>,
    DeferredInvitationNetworkEffects,
)> {
    if outcome.is_denied() {
        let reason = outcome
            .decision
            .denial_reason()
            .map(ToString::to_string)
            .unwrap_or_else(|| "Operation denied".to_string());
        return Err(AgentError::effects(format!(
            "Guard denied operation: {}",
            reason
        )));
    }

    let mut local_effects = Vec::new();
    let mut deferred_network_effects = Vec::new();
    for command in outcome.effects {
        match command {
            aura_invitation::guards::EffectCommand::ChargeFlowBudget { .. }
            | aura_invitation::guards::EffectCommand::NotifyPeer { .. }
            | aura_invitation::guards::EffectCommand::RecordReceipt { .. } => {
                deferred_network_effects.push(command);
            }
            aura_invitation::guards::EffectCommand::JournalAppend { .. } => {
                local_effects.push(command);
            }
        }
    }

    tracing::debug!(
        authority = %authority.authority_id(),
        local_effect_count = local_effects.len(),
        deferred_network_effect_count = deferred_network_effects.len(),
        "Prepared invitation guard outcome with deferred network side effects"
    );

    Ok((
        local_effects,
        DeferredInvitationNetworkEffects::new(deferred_network_effects),
    ))
}

pub(crate) async fn execute_invitation_effect_commands(
    commands: Vec<aura_invitation::guards::EffectCommand>,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
    best_effort_network_failures: bool,
) -> AgentResult<()> {
    let context_id = authority.default_context_id();
    let charge_peer =
        resolve_charge_peer(
            &commands,
            authority.authority_id(),
            |command| match command {
                aura_invitation::guards::EffectCommand::NotifyPeer { peer, .. } => Some(*peer),
                aura_invitation::guards::EffectCommand::RecordReceipt { peer, .. } => *peer,
                _ => None,
            },
        );
    let mut pending_receipt: Option<Receipt> = None;

    for command in commands {
        let is_network_side_effect = matches!(
            &command,
            aura_invitation::guards::EffectCommand::ChargeFlowBudget { .. }
                | aura_invitation::guards::EffectCommand::NotifyPeer { .. }
                | aura_invitation::guards::EffectCommand::RecordReceipt { .. }
        );

        let result = if best_effort_network_failures && is_network_side_effect {
            timeout_deferred_network_stage(
                effects,
                "accept_network_side_effect",
                execute_effect_command(
                    command,
                    authority,
                    context_id,
                    effects,
                    charge_peer,
                    &mut pending_receipt,
                    best_effort_network_failures,
                ),
            )
            .await
        } else {
            execute_effect_command(
                command,
                authority,
                context_id,
                effects,
                charge_peer,
                &mut pending_receipt,
                best_effort_network_failures,
            )
            .await
        };

        match result {
            Ok(()) => {}
            Err(error) if best_effort_network_failures && is_network_side_effect => {
                tracing::warn!(
                    authority = %authority.authority_id(),
                    context = %context_id,
                    "Invitation side effect continuing after best-effort network failure: {}",
                    error
                );
            }
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

async fn execute_effect_command(
    command: aura_invitation::guards::EffectCommand,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
    charge_peer: AuthorityId,
    pending_receipt: &mut Option<Receipt>,
    best_effort_network_failures: bool,
) -> AgentResult<()> {
    match command {
        aura_invitation::guards::EffectCommand::JournalAppend { fact } => {
            execute_journal_append(fact, authority, context_id, effects).await
        }
        aura_invitation::guards::EffectCommand::ChargeFlowBudget { cost } => {
            *pending_receipt =
                execute_charge_flow_budget(cost, context_id, charge_peer, effects).await?;
            Ok(())
        }
        aura_invitation::guards::EffectCommand::NotifyPeer {
            peer,
            invitation_id,
        } => {
            execute_notify_peer(
                peer,
                invitation_id,
                authority,
                pending_receipt.clone(),
                effects,
                best_effort_network_failures,
            )
            .await
        }
        aura_invitation::guards::EffectCommand::RecordReceipt { operation, peer } => {
            execute_record_receipt(operation, peer, context_id, pending_receipt.take(), effects)
                .await
        }
    }
}

async fn execute_journal_append(
    fact: InvitationFact,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    HandlerUtilities::append_generic_fact(
        authority,
        effects,
        context_id,
        INVITATION_FACT_TYPE_ID.into(),
        &fact.to_bytes(),
    )
    .await
}

async fn execute_charge_flow_budget(
    cost: aura_core::FlowCost,
    context_id: ContextId,
    peer: AuthorityId,
    effects: &AuraEffectSystem,
) -> AgentResult<Option<Receipt>> {
    emit_browser_harness_debug_event("invite_charge_begin", &format!("{context_id}:{peer}"));
    // Deterministic testing/simulation modes do not model flow charging.
    if effects.is_testing() {
        emit_browser_harness_debug_event("invite_charge_testing", "");
        return Ok(None);
    }

    let receipt = effects
        .charge_flow(&context_id, &peer, cost)
        .await
        .map_err(|e| {
            emit_browser_harness_debug_event("invite_charge_err", &e.to_string());
            AgentError::effects(format!("Failed to charge invitation flow: {e}"))
        })?;
    emit_browser_harness_debug_event("invite_charge_ok", "");
    Ok(Some(receipt))
}

async fn execute_notify_peer(
    peer: AuthorityId,
    invitation_id: InvitationId,
    authority: &AuthorityContext,
    receipt: Option<Receipt>,
    effects: &AuraEffectSystem,
    best_effort_network_failures: bool,
) -> AgentResult<()> {
    emit_browser_harness_debug_event("invite_notify_begin", &peer.to_string());
    // Use explicit test mode, not `is_testing()`: simulation runs should still
    // exercise transport delivery on the shared deterministic network.
    if effects.is_test_mode() {
        emit_browser_harness_debug_event("invite_notify_test_mode", "");
        return Ok(());
    }

    if peer == authority.authority_id() {
        // Self-addressed invitations are intended for out-of-band sharing.
        // Skip network notify when inviting ourselves.
        emit_browser_harness_debug_event("invite_notify_self", "");
        return Ok(());
    }

    let authority_id = authority.authority_id();
    let (code, invitation_context) = if let Some(invitation) =
        InvitationHandler::load_created_invitation(effects, authority_id, &invitation_id).await
    {
        (
            InvitationServiceApi::export_invitation(&invitation)
                .map_err(|error| AgentError::invalid(error.to_string()))?,
            invitation.context_id,
        )
    } else {
        let envelopes =
            load_relational_fact_envelopes_by_type(effects, authority_id, INVITATION_FACT_TYPE_ID)
                .await
                .map_err(|_| {
                    AgentError::context(format!("Invitation not found for notify: {invitation_id}"))
                })?;

        let mut shareable: Option<(ShareableInvitation, ContextId)> = None;
        for envelope in &envelopes {
            let Some(inv_fact) = InvitationFact::from_envelope(envelope) else {
                continue;
            };

            let InvitationFact::Sent {
                invitation_id: seen_id,
                sender_id,
                context_id,
                invitation_type,
                expires_at,
                message,
                ..
            } = inv_fact
            else {
                continue;
            };

            if seen_id != invitation_id {
                continue;
            }

            shareable = Some((
                ShareableInvitation {
                    version: ShareableInvitation::CURRENT_VERSION,
                    invitation_id: invitation_id.clone(),
                    sender_id,
                    context_id: Some(context_id),
                    invitation_type,
                    expires_at: expires_at.map(|time| time.ts_ms),
                    message,
                },
                context_id,
            ));
            break;
        }

        let (shareable, context_id) = shareable.ok_or_else(|| {
            AgentError::context(format!("Invitation not found for notify: {invitation_id}"))
        })?;

        (
            shareable
                .to_code()
                .map_err(|error| AgentError::invalid(error.to_string()))?,
            context_id,
        )
    };
    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        "application/aura-invitation".to_string(),
    );
    metadata.insert("invitation-id".to_string(), invitation_id.to_string());
    metadata.insert(
        "invitation-context".to_string(),
        invitation_context.to_string(),
    );
    tracing::debug!(
        destination = %peer,
        invitation_context = %invitation_context,
        code_has_context_field = code.contains("\"context_id\""),
        "Sending invitation envelope"
    );
    emit_browser_harness_debug_event("invite_notify_send", &peer.to_string());

    // The invitation establishes or extends semantic access to `invitation_context`,
    // so the transport envelope itself must ride over the existing authority-scoped
    // peer path instead of assuming the invitee is already routable on that context.
    let delivery_context = default_context_id_for_authority(peer);

    let envelope = TransportEnvelope {
        destination: peer,
        source: authority.authority_id(),
        context: delivery_context,
        payload: code.into_bytes(),
        metadata,
        receipt: receipt.map(transport_receipt_from_flow),
    };

    if best_effort_network_failures {
        attempt_network_send_envelope(effects, "notify peer with invitation failed", envelope)
            .await?;
    } else {
        effects.send_envelope(envelope).await.map_err(|e| {
            AgentError::effects(format!("Failed to notify peer with invitation: {e}"))
        })?;
    }
    emit_browser_harness_debug_event("invite_notify_ok", &peer.to_string());

    Ok(())
}

fn transport_receipt_from_flow(receipt: Receipt) -> TransportReceipt {
    TransportReceipt {
        context: receipt.ctx,
        src: receipt.src,
        dst: receipt.dst,
        epoch: receipt.epoch.value(),
        cost: receipt.cost.value(),
        nonce: receipt.nonce.value(),
        prev: receipt.prev.0,
        sig: receipt.sig.into_bytes(),
    }
}

async fn execute_record_receipt(
    operation: InvitationOperation,
    peer: Option<AuthorityId>,
    context_id: ContextId,
    receipt: Option<Receipt>,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // Deterministic testing/simulation modes do not persist transport receipts.
    if effects.is_testing() {
        return Ok(());
    }

    let Some(receipt) = receipt else {
        tracing::debug!(
            operation = %operation,
            peer = ?peer,
            context = %context_id,
            "Invitation receipt recording skipped (no receipt available)"
        );
        return Ok(());
    };

    let peer_id = peer.unwrap_or(receipt.dst);
    let operation_key = match operation {
        InvitationOperation::SendInvitation => "send_invitation",
        InvitationOperation::AcceptInvitation => "accept_invitation",
        InvitationOperation::DeclineInvitation => "decline_invitation",
        InvitationOperation::CancelInvitation => "cancel_invitation",
        InvitationOperation::Ceremony => "invitation_ceremony",
    };
    let key = format!(
        "invitation/receipts/{}/{}/{}/{}",
        receipt.ctx, peer_id, operation_key, receipt.nonce
    );
    let bytes = serde_json::to_vec(&receipt)
        .map_err(|e| AgentError::effects(format!("Failed to serialize invitation receipt: {e}")))?;
    effects
        .store(&key, bytes)
        .await
        .map_err(|e| AgentError::effects(format!("Failed to store invitation receipt: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::reactive::app_signal_views;
    use crate::runtime::effects::AuraEffectSystem;
    use crate::runtime::services::ceremony_runner::CeremonyRunner;
    use crate::runtime::services::CeremonyTracker;
    use crate::runtime::TaskSupervisor;
    use aura_app::signal_defs::{register_app_signals, HOMES_SIGNAL, INVITATIONS_SIGNAL};
    use aura_app::views::home::{HomeRole, HomesState};
    use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
    use aura_core::effects::reactive::ReactiveEffects;
    use aura_core::types::identifiers::{
        AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId,
    };
    use aura_core::DeviceId;
    use aura_effects::reactive::ReactiveHandler;
    use aura_invitation::guards::{EffectCommand, GuardOutcome};
    use aura_journal::fact::{FactContent, RelationalFact};
    use aura_journal::DomainFact;
    use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
    use aura_social::moderation::facts::HomeGrantModeratorFact;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
    }

    fn install_full_invitation_biscuit_cache(
        effects: &Arc<AuraEffectSystem>,
        authority: AuthorityId,
    ) {
        let issuer = aura_authorization::TokenAuthority::new(authority);
        let token = issuer
            .create_token(
                authority,
                crate::token_profiles::TokenCapabilityProfile::StandardDevice,
            )
            .expect("full invitation biscuit should build");
        let engine = base64::engine::general_purpose::STANDARD;
        effects.set_biscuit_cache(crate::runtime::effects::BiscuitCache {
            token_b64: engine.encode(token.to_vec().expect("token should serialize")),
            root_pk_b64: engine.encode(issuer.root_public_key().to_bytes()),
        });
    }

    #[track_caller]
    fn effects_for(authority: &AuthorityContext) -> Arc<AuraEffectSystem> {
        let config = AgentConfig {
            device_id: authority.device_id(),
            ..Default::default()
        };
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, authority.authority_id())
                .unwrap(),
        );
        install_full_invitation_biscuit_cache(&effects, authority.authority_id());
        effects
    }

    #[track_caller]
    fn production_effects_for(authority: &AuthorityContext) -> Arc<AuraEffectSystem> {
        let config = AgentConfig {
            device_id: authority.device_id(),
            ..Default::default()
        };
        let effects = Arc::new(
            AuraEffectSystem::production_for_authority(config, authority.authority_id()).unwrap(),
        );
        install_full_invitation_biscuit_cache(&effects, authority.authority_id());
        effects
    }

    fn canonical_home_id(seed: u8) -> ChannelId {
        ChannelId::from_bytes([seed; 32])
    }

    async fn register_test_app_signals(effects: &AuraEffectSystem) {
        register_app_signals(&effects.reactive_handler())
            .await
            .unwrap();
    }

    async fn attach_test_rendezvous_manager(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
    ) -> Arc<crate::runtime::TaskSupervisor> {
        let manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
            authority_id,
            crate::runtime::services::RendezvousManagerConfig::default(),
            Arc::new(effects.time_effects().clone()),
        );
        effects.attach_rendezvous_manager(manager.clone());
        let tasks = Arc::new(crate::runtime::TaskSupervisor::new());
        let service_context = crate::runtime::services::RuntimeServiceContext::new(
            tasks.clone(),
            Arc::new(effects.time_effects().clone()),
        );
        crate::runtime::services::RuntimeService::start(&manager, &service_context)
            .await
            .unwrap();
        tasks
    }

    async fn cache_test_peer_descriptor(
        effects: &AuraEffectSystem,
        local_authority: AuthorityId,
        peer: AuthorityId,
        addr: &str,
        now_ms: u64,
    ) {
        let manager = effects
            .rendezvous_manager()
            .expect("test rendezvous manager should be attached");
        let hint = TransportHint::tcp_direct(addr.trim_start_matches("tcp://")).unwrap();
        let peer_context_id = default_context_id_for_authority(peer);
        manager
            .cache_descriptor(RendezvousDescriptor {
                authority_id: peer,
                device_id: None,
                context_id: peer_context_id,
                transport_hints: vec![hint.clone()],
                handshake_psk_commitment: [0u8; 32],
                public_key: [0u8; 32],
                valid_from: now_ms.saturating_sub(1),
                valid_until: now_ms.saturating_add(86_400_000),
                nonce: [0u8; 32],
                nickname_suggestion: None,
            })
            .await
            .unwrap();

        let local_context_id = default_context_id_for_authority(local_authority);
        if local_context_id != peer_context_id {
            manager
                .cache_descriptor(RendezvousDescriptor {
                    authority_id: peer,
                    device_id: None,
                    context_id: local_context_id,
                    transport_hints: vec![hint],
                    handshake_psk_commitment: [0u8; 32],
                    public_key: [0u8; 32],
                    valid_from: now_ms.saturating_sub(1),
                    valid_until: now_ms.saturating_add(86_400_000),
                    nonce: [0u8; 32],
                    nickname_suggestion: None,
                })
                .await
                .unwrap();
        }
    }

    async fn accept_invitation_without_notification(
        handler: &InvitationHandler,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) {
        handler
            .accept_invitation(effects, invitation_id)
            .await
            .unwrap();
    }

    fn invitation_service_for(
        authority_context: AuthorityContext,
        effects: Arc<AuraEffectSystem>,
    ) -> InvitationServiceApi {
        let time_effects: Arc<dyn aura_core::effects::time::PhysicalTimeEffects> =
            Arc::new(effects.time_effects().clone());
        let ceremony_runner = CeremonyRunner::new(CeremonyTracker::new(time_effects));
        InvitationServiceApi::new_with_runner(
            effects,
            authority_context,
            ceremony_runner,
            Arc::new(TaskSupervisor::new()),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn channel_home_materialization_requires_registered_homes_signal() {
        let reactive = ReactiveHandler::new();

        let error = app_signal_views::materialize_home_signal_for_channel_invitation(
            &reactive,
            AuthorityId::new_from_entropy([1u8; 32]),
            canonical_home_id(1),
            "shared-parity-lab",
            AuthorityId::new_from_entropy([2u8; 32]),
            ContextId::new_from_entropy([3u8; 32]),
            0,
        )
        .await
        .unwrap_err();
        let message = error.clone();
        assert!(
            message.contains("requires registered homes signal"),
            "unexpected error: {message}"
        );
    }

    #[tokio::test]
    async fn test_execute_allowed_outcome() {
        let authority = create_test_authority(130);
        let effects = effects_for(&authority);

        let outcome = GuardOutcome::allowed(vec![EffectCommand::ChargeFlowBudget {
            cost: FlowCost::new(1),
        }]);

        let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_denied_outcome() {
        let authority = create_test_authority(131);
        let effects = effects_for(&authority);

        let outcome = GuardOutcome::denied(aura_guards::types::GuardViolation::other(
            "Test denial reason",
        ));

        let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Test denial reason"));
    }

    #[tokio::test]
    async fn test_execute_journal_append() {
        let authority = create_test_authority(132);
        let effects = effects_for(&authority);

        let fact = InvitationFact::sent_ms(
            ContextId::new_from_entropy([232u8; 32]),
            InvitationId::new("inv-test"),
            authority.authority_id(),
            AuthorityId::new_from_entropy([133u8; 32]),
            InvitationType::Contact { nickname: None },
            1000,
            Some(2000),
            None,
        );

        let outcome = GuardOutcome::allowed(vec![EffectCommand::JournalAppend { fact }]);

        let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_notify_peer() {
        let authority = create_test_authority(134);
        let shared_transport = crate::runtime::SharedTransport::new();
        let config = AgentConfig::default();
        let peer = AuthorityId::new_from_entropy([135u8; 32]);
        let effects =
            crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
                &config,
                authority.authority_id(),
                shared_transport.clone(),
            );
        // Materialize a destination participant on the shared transport.
        let _peer_effects =
            crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
                &config,
                peer,
                shared_transport,
            );
        let handler = InvitationHandler::new(authority.clone()).unwrap();

        let invitation = handler
            .create_invitation(
                effects.clone(),
                peer,
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let outcome = GuardOutcome::allowed(vec![EffectCommand::NotifyPeer {
            peer,
            invitation_id: invitation.invitation_id,
        }]);

        let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_record_receipt() {
        let authority = create_test_authority(136);
        let effects = effects_for(&authority);

        let outcome = GuardOutcome::allowed(vec![EffectCommand::RecordReceipt {
            operation: InvitationOperation::SendInvitation,
            peer: Some(AuthorityId::new_from_entropy([137u8; 32])),
        }]);

        let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_multiple_commands() {
        let authority = create_test_authority(138);
        let shared_transport = crate::runtime::SharedTransport::new();
        let config = AgentConfig::default();
        let peer = AuthorityId::new_from_entropy([139u8; 32]);
        let effects =
            crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
                &config,
                authority.authority_id(),
                shared_transport.clone(),
            );
        // Materialize a destination participant on the shared transport.
        let _peer_effects =
            crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
                &config,
                peer,
                shared_transport,
            );
        let handler = InvitationHandler::new(authority.clone()).unwrap();

        let invitation = handler
            .create_invitation(
                effects.clone(),
                peer,
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();
        let outcome = GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: FlowCost::new(1),
            },
            EffectCommand::NotifyPeer {
                peer,
                invitation_id: invitation.invitation_id,
            },
            EffectCommand::RecordReceipt {
                operation: InvitationOperation::SendInvitation,
                peer: Some(peer),
            },
        ]);

        let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn invitation_can_be_created() {
        let authority_context = create_test_authority(91);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([92u8; 32]);

        let invitation = handler
            .create_invitation(
                effects.clone(),
                receiver_id,
                InvitationType::Contact {
                    nickname: Some("alice".to_string()),
                },
                Some("Let's connect!".to_string()),
                Some(86400000), // 1 day
            )
            .await
            .unwrap();

        assert!(invitation.invitation_id.as_str().starts_with("inv-"));
        assert_eq!(invitation.sender_id, authority_context.authority_id());
        assert_eq!(invitation.receiver_id, receiver_id);
        assert_eq!(invitation.status, InvitationStatus::Pending);
        assert!(invitation.expires_at.is_some());
    }

    #[tokio::test]
    async fn invitation_can_be_accepted() {
        let sender_context = create_test_authority(93);
        let receiver_id = AuthorityId::new_from_entropy([94u8; 32]);
        let receiver_context = AuthorityContext::new(receiver_id);

        let sender_effects = effects_for(&sender_context);
        let receiver_effects = effects_for(&receiver_context);
        let sender_handler = InvitationHandler::new(sender_context).unwrap();
        let receiver_handler = InvitationHandler::new(receiver_context).unwrap();

        let invitation = sender_handler
            .create_invitation(
                sender_effects,
                receiver_id,
                InvitationType::Contact {
                    nickname: Some("receiver".to_string()),
                },
                None,
                None,
            )
            .await
            .unwrap();
        let code = InvitationServiceApi::export_invitation(&invitation)
            .expect("shareable invitation should serialize");
        let imported = receiver_handler
            .import_invitation_code(&receiver_effects, &code)
            .await
            .unwrap();

        let result = receiver_handler
            .accept_invitation(receiver_effects, &imported.invitation_id)
            .await
            .unwrap();

        assert_eq!(result.new_status, InvitationStatus::Accepted);
    }

    #[tokio::test]
    async fn invitation_can_be_declined() {
        let authority_context = create_test_authority(96);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([97u8; 32]);
        let context_id = ContextId::new_from_entropy([98u8; 32]);
        let home_id = canonical_home_id(11);

        effects
            .create_channel(ChannelCreateParams {
                context: context_id,
                channel: Some(home_id),
                skip_window: None,
                topic: None,
            })
            .await
            .unwrap();

        let invitation = handler
            .create_invitation_with_context(
                effects.clone(),
                receiver_id,
                InvitationType::Channel {
                    home_id,
                    nickname_suggestion: None,
                    bootstrap: None,
                },
                Some(context_id),
                None,
                None,
            )
            .await
            .unwrap();

        let result = handler
            .decline_invitation(effects.clone(), &invitation.invitation_id)
            .await
            .unwrap();

        assert_eq!(result.new_status, InvitationStatus::Declined);
    }

    #[tokio::test]
    async fn importing_channel_invitation_without_context_rejects_before_persist() {
        let authority_context = create_test_authority(101);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();

        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-channel-missing-context"),
            sender_id: AuthorityId::new_from_entropy([102u8; 32]),
            context_id: None,
            invitation_type: InvitationType::Channel {
                home_id: canonical_home_id(17),
                nickname_suggestion: Some("shared-parity-lab".to_string()),
                bootstrap: None,
            },
            expires_at: None,
            message: None,
        };
        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");

        let error = handler
            .import_invitation_code(effects.as_ref(), &code)
            .await
            .expect_err("channel invitation without authoritative context should fail");
        assert!(error.to_string().contains("missing authoritative context"));

        let persisted = InvitationHandler::load_imported_invitation(
            effects.as_ref(),
            authority_context.authority_id(),
            &shareable.invitation_id,
            None,
        )
        .await;
        assert!(persisted.is_none());
    }

    #[tokio::test]
    async fn accepting_guardian_invitation_surfaces_choreography_failure() {
        let authority_context = create_test_authority(103);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context).unwrap();
        let sender_id = AuthorityId::new_from_entropy([104u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-guardian-missing-ceremony"),
            sender_id,
            context_id: None,
            invitation_type: InvitationType::Guardian {
                subject_authority: sender_id,
            },
            expires_at: None,
            message: None,
        };
        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");
        let imported = handler
            .import_invitation_code(effects.as_ref(), &code)
            .await
            .expect("guardian invitation should import");

        let error = timeout(
            Duration::from_secs(5),
            handler.accept_invitation(effects.clone(), &imported.invitation_id),
        )
        .await
        .expect("guardian accept should terminate")
        .expect_err("guardian choreography failure should surface");
        assert!(error
            .to_string()
            .contains("guardian invitation accept follow-up failed"));
    }

    #[tokio::test]
    async fn declining_contact_invitation_succeeds_locally_when_exchange_failure_occurs() {
        let authority_context = create_test_authority(105);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context).unwrap();
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-contact-missing-decline-exchange"),
            sender_id: AuthorityId::new_from_entropy([106u8; 32]),
            context_id: None,
            invitation_type: InvitationType::Contact {
                nickname: Some("Alice".to_string()),
            },
            expires_at: None,
            message: None,
        };
        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");
        let imported = handler
            .import_invitation_code(effects.as_ref(), &code)
            .await
            .expect("contact invitation should import");

        let result = timeout(
            Duration::from_secs(5),
            handler.decline_invitation(effects.clone(), &imported.invitation_id),
        )
        .await
        .expect("decline should terminate")
        .expect("decline should settle locally even if follow-up exchange fails");
        assert_eq!(result.new_status, InvitationStatus::Declined);

        let stored = handler
            .get_invitation_with_storage(effects.as_ref(), &imported.invitation_id)
            .await
            .expect("declined invitation should remain queryable");
        assert_eq!(stored.status, InvitationStatus::Declined);
    }

    #[tokio::test]
    async fn old_format_imported_invitation_preserves_cached_terminal_status() {
        let authority_context = create_test_authority(111);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("legacy-imported-status-preserved"),
            sender_id: AuthorityId::new_from_entropy([112u8; 32]),
            context_id: None,
            invitation_type: InvitationType::Contact {
                nickname: Some("Alice".to_string()),
            },
            expires_at: None,
            message: Some("legacy invite".to_string()),
        };
        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");
        let imported = handler
            .import_invitation_code(effects.as_ref(), &code)
            .await
            .expect("legacy invite import should succeed");

        handler
            .invitation_cache
            .update_invitation(&imported.invitation_id, |invitation| {
                invitation.status = InvitationStatus::Accepted;
                invitation.created_at = 123;
            })
            .await;

        let legacy_key = InvitationCacheHandler::imported_invitation_key(
            authority_context.authority_id(),
            &imported.invitation_id,
        );
        effects
            .store(&legacy_key, serde_json::to_vec(&shareable).unwrap())
            .await
            .unwrap();

        let listed = handler.list_with_storage(effects.as_ref()).await;
        let invitation = listed
            .into_iter()
            .find(|invitation| invitation.invitation_id == imported.invitation_id)
            .expect("legacy imported invitation should remain listable");
        assert_eq!(invitation.status, InvitationStatus::Accepted);
        assert_eq!(invitation.created_at, 123);
    }

    #[tokio::test]
    async fn choreography_load_preserves_cached_terminal_status_for_legacy_imports() {
        let authority_context = create_test_authority(113);
        let effects = effects_for(&authority_context);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("legacy-imported-choreo-status"),
            sender_id: AuthorityId::new_from_entropy([114u8; 32]),
            context_id: None,
            invitation_type: InvitationType::Contact {
                nickname: Some("Alice".to_string()),
            },
            expires_at: None,
            message: Some("legacy invite".to_string()),
        };
        let legacy_key = InvitationCacheHandler::imported_invitation_key(
            authority_context.authority_id(),
            &shareable.invitation_id,
        );
        effects
            .store(&legacy_key, serde_json::to_vec(&shareable).unwrap())
            .await
            .unwrap();

        let preserved = Invitation {
            invitation_id: shareable.invitation_id.clone(),
            context_id: authority_context.default_context_id(),
            sender_id: shareable.sender_id,
            receiver_id: authority_context.authority_id(),
            invitation_type: shareable.invitation_type.clone(),
            status: InvitationStatus::Declined,
            created_at: 456,
            expires_at: shareable.expires_at,
            message: shareable.message.clone(),
        };
        let stored = InvitationHandler::load_imported_invitation(
            effects.as_ref(),
            authority_context.authority_id(),
            &shareable.invitation_id,
            Some(&preserved),
        )
        .await
        .expect("legacy imported invitation should remain loadable");
        assert_eq!(stored.status, InvitationStatus::Declined);
        assert_eq!(stored.created_at, 456);
    }

    #[tokio::test]
    async fn build_snapshot_uses_authoritative_flow_budget_state() {
        let authority_context = create_test_authority(115);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();
        let context_id = authority_context.default_context_id();

        aura_core::effects::JournalEffects::update_flow_budget(
            effects.as_ref(),
            &context_id,
            &authority_context.authority_id(),
            &aura_core::FlowBudget {
                limit: 50,
                spent: 27,
                epoch: aura_core::Epoch::new(7),
            },
        )
        .await
        .unwrap();

        let snapshot = handler
            .build_snapshot_for_context(effects.as_ref(), context_id)
            .await;
        assert_eq!(snapshot.flow_budget_remaining, FlowCost::new(23));
        assert_eq!(snapshot.epoch, 7);
    }

    #[tokio::test]
    async fn build_snapshot_without_biscuit_frontier_has_empty_capability_frontier() {
        let authority_context = create_test_authority(140);
        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();
        effects.clear_biscuit_cache();

        let snapshot = handler
            .build_snapshot_for_context(effects.as_ref(), authority_context.default_context_id())
            .await;

        assert!(snapshot.capabilities.is_empty());
    }

    #[tokio::test]
    async fn creating_invitation_is_denied_when_biscuit_lacks_invitation_send_capability() {
        let authority_context = create_test_authority(116);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();
        let keypair = aura_authorization::KeyPair::new();
        let authority = authority_context.authority_id().to_string();
        let token = biscuit_auth::macros::biscuit!(
            r#"
            authority({authority});
            role("member");
            capability("read");
            capability("write");
        "#
        )
        .build(&keypair)
        .expect("capability-limited biscuit should build");
        let token_bytes = token.to_vec().expect("token should serialize");
        let engine = base64::engine::general_purpose::STANDARD;
        effects.set_biscuit_cache(crate::runtime::effects::BiscuitCache {
            token_b64: engine.encode(&token_bytes),
            root_pk_b64: engine.encode(keypair.public().to_bytes()),
        });

        let error = handler
            .create_invitation(
                effects.clone(),
                AuthorityId::new_from_entropy([117u8; 32]),
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .expect_err("missing invitation:send capability should deny invitation creation");
        assert!(error.to_string().contains("Guard denied operation"));
    }

    #[tokio::test]
    async fn accepting_unknown_invitation_is_rejected() {
        let authority_context = create_test_authority(118);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context).unwrap();

        let error = handler
            .accept_invitation(effects, &InvitationId::new("invitation-does-not-exist"))
            .await
            .expect_err("unknown invitation should be rejected");
        assert!(error.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn accept_guard_outcome_continues_after_deferred_network_failures() {
        let authority = create_test_authority(107);
        let effects = production_effects_for(&authority);
        let peer = AuthorityId::new_from_entropy([108u8; 32]);
        let outcome = aura_invitation::guards::GuardOutcome::allowed(vec![
            aura_invitation::guards::EffectCommand::ChargeFlowBudget {
                cost: FlowCost::new(1),
            },
            aura_invitation::guards::EffectCommand::NotifyPeer {
                peer,
                invitation_id: InvitationId::new("inv-missing-notify"),
            },
            aura_invitation::guards::EffectCommand::RecordReceipt {
                operation: InvitationOperation::AcceptInvitation,
                peer: Some(peer),
            },
        ]);

        execute_guard_outcome_for_accept(outcome, &authority, effects.as_ref())
            .await
            .expect("deferred network failures should not block accept settlement");
    }

    #[test]
    fn accept_guard_outcome_only_defers_peer_notification() {
        let authority = create_test_authority(109);
        let peer = AuthorityId::new_from_entropy([110u8; 32]);
        let invitation_id = InvitationId::new("inv-accept-split");
        let outcome = aura_invitation::guards::GuardOutcome::allowed(vec![
            aura_invitation::guards::EffectCommand::ChargeFlowBudget {
                cost: FlowCost::new(1),
            },
            aura_invitation::guards::EffectCommand::JournalAppend {
                fact: InvitationFact::Accepted {
                    context_id: Some(authority.default_context_id()),
                    invitation_id: invitation_id.clone(),
                    acceptor_id: authority.authority_id(),
                    accepted_at: PhysicalTime {
                        ts_ms: 1,
                        uncertainty: None,
                    },
                },
            },
            aura_invitation::guards::EffectCommand::NotifyPeer {
                peer,
                invitation_id: invitation_id.clone(),
            },
            aura_invitation::guards::EffectCommand::RecordReceipt {
                operation: InvitationOperation::AcceptInvitation,
                peer: Some(peer),
            },
        ]);

        let (local_effects, deferred_network_effects) =
            split_invitation_accept_guard_outcome(outcome, &authority)
                .expect("accept split should succeed");

        assert_eq!(local_effects.len(), 3);
        assert_eq!(deferred_network_effects.commands().len(), 1);
        assert!(matches!(
            deferred_network_effects.commands().first(),
            Some(aura_invitation::guards::EffectCommand::NotifyPeer { .. })
        ));
    }

    #[test]
    fn malformed_home_id_rejected_at_string_boundary() {
        let err =
            channel_id_from_home_id("oak-house").expect_err("malformed home id should be rejected");
        assert!(matches!(err, AgentError::Config(_)));
    }

    #[tokio::test]
    async fn importing_and_accepting_contact_invitation_commits_contact_fact() {
        let own_authority = AuthorityId::new_from_entropy([120u8; 32]);
        let config = AgentConfig::default();
        let effects =
            crate::testing::simulation_effect_system_for_authority_arc(&config, own_authority);

        let authority_context = AuthorityContext::new(own_authority);

        let handler = InvitationHandler::new(authority_context).unwrap();

        let sender_id = AuthorityId::new_from_entropy([121u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-demo-contact-1"),
            sender_id,
            context_id: None,
            invitation_type: InvitationType::Contact {
                nickname: Some("Alice".to_string()),
            },
            expires_at: None,
            message: Some("Contact invitation from Alice (demo)".to_string()),
        };
        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");

        let imported = handler
            .import_invitation_code(&effects, &code)
            .await
            .unwrap();
        assert_eq!(imported.sender_id, sender_id);
        assert_eq!(imported.receiver_id, own_authority);

        accept_invitation_without_notification(&handler, effects.clone(), &imported.invitation_id)
            .await;

        let committed = effects.load_committed_facts(own_authority).await.unwrap();

        let mut found = None::<ContactFact>;
        let mut seen_binding_types: Vec<String> = Vec::new();
        for fact in committed {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };

            seen_binding_types.push(envelope.type_id.as_str().to_string());
            if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
                continue;
            }

            found = ContactFact::from_envelope(&envelope);
        }

        if found.is_none() {
            panic!(
                "Expected a committed ContactFact, saw bindings: {:?}",
                seen_binding_types
            );
        }
        let fact = found.unwrap();
        match fact {
            ContactFact::Added {
                owner_id,
                contact_id,
                nickname,
                ..
            } => {
                assert_eq!(owner_id, own_authority);
                assert_eq!(contact_id, sender_id);
                assert_eq!(nickname, "Alice");
            }
            other => panic!("Expected ContactFact::Added, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn accepting_contact_invitation_notifies_sender_and_adds_contact() {
        let shared_transport = crate::runtime::SharedTransport::new();
        let config = AgentConfig::default();

        let sender_id = AuthorityId::new_from_entropy([124u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([125u8; 32]);

        let sender_effects =
            crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
                &config,
                sender_id,
                shared_transport.clone(),
            );
        let receiver_effects =
            crate::testing::simulation_effect_system_with_shared_transport_for_authority_arc(
                &config,
                receiver_id,
                shared_transport.clone(),
            );

        let sender_handler = InvitationHandler::new(AuthorityContext::new(sender_id)).unwrap();
        let receiver_handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

        let invitation = sender_handler
            .create_invitation(
                sender_effects.clone(),
                receiver_id,
                InvitationType::Contact { nickname: None },
                Some("Contact invitation from sender".to_string()),
                None,
            )
            .await
            .unwrap();

        let code = InvitationServiceApi::export_invitation(&invitation)
            .expect("shareable invitation should serialize");
        let imported = receiver_handler
            .import_invitation_code(&receiver_effects, &code)
            .await
            .unwrap();

        receiver_handler
            .accept_invitation(receiver_effects.clone(), &imported.invitation_id)
            .await
            .unwrap();
        receiver_handler
            .notify_contact_invitation_acceptance(
                receiver_effects.as_ref(),
                &imported.invitation_id,
            )
            .await
            .unwrap();
        let processed = sender_handler
            .process_contact_invitation_acceptances(sender_effects.clone())
            .await
            .unwrap();
        assert!(
            processed >= 1,
            "expected at least one transported acceptance envelope to be processed"
        );

        let committed = sender_effects
            .load_committed_facts(sender_id)
            .await
            .unwrap();

        let mut found = false;
        for fact in committed {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
                continue;
            }

            let Some(ContactFact::Added {
                owner_id,
                contact_id,
                nickname,
                ..
            }) = ContactFact::from_envelope(&envelope)
            else {
                continue;
            };
            if owner_id == sender_id
                && contact_id == receiver_id
                && nickname == receiver_id.to_string()
            {
                found = true;
                break;
            }
        }
        assert!(
            found,
            "expected sender-side ContactFact::Added for receiver"
        );
    }

    #[tokio::test]
    async fn creating_contact_invitation_materializes_sender_contact() {
        let sender_id = AuthorityId::new_from_entropy([128u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([129u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, sender_id).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(sender_id)).unwrap();

        handler
            .create_invitation(
                effects.clone(),
                receiver_id,
                InvitationType::Contact { nickname: None },
                Some("Contact invitation".to_string()),
                None,
            )
            .await
            .unwrap();

        let committed = effects.load_committed_facts(sender_id).await.unwrap();
        let mut found = false;
        for fact in committed {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };
            if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
                continue;
            }
            let Some(ContactFact::Added {
                owner_id,
                contact_id,
                ..
            }) = ContactFact::from_envelope(&envelope)
            else {
                continue;
            };
            if owner_id == sender_id && contact_id == receiver_id {
                found = true;
                break;
            }
        }

        assert!(
            found,
            "expected ContactFact::Added for sender invitation recipient"
        );
    }

    #[tokio::test]
    async fn creating_contact_invitation_does_not_overwrite_existing_sender_contact() {
        let sender_id = AuthorityId::new_from_entropy([130u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([131u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, sender_id).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(sender_id)).unwrap();

        let context_id = default_context_id_for_authority(sender_id);
        let existing_contact = ContactFact::Added {
            context_id,
            owner_id: sender_id,
            contact_id: receiver_id,
            nickname: "Alice-Maple".to_string(),
            added_at: PhysicalTime {
                ts_ms: 1,
                uncertainty: None,
            },
        };
        effects
            .commit_generic_fact_bytes(
                context_id,
                CONTACT_FACT_TYPE_ID.into(),
                existing_contact.to_bytes(),
            )
            .await
            .unwrap();
        effects.await_next_view_update().await;

        let before_count = effects
            .load_committed_facts(sender_id)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|fact| match fact.content {
                FactContent::Relational(RelationalFact::Generic { envelope, .. })
                    if envelope.type_id.as_str() == CONTACT_FACT_TYPE_ID =>
                {
                    ContactFact::from_envelope(&envelope)
                }
                _ => None,
            })
            .filter(|fact| {
                matches!(
                    fact,
                    ContactFact::Added {
                        owner_id,
                        contact_id,
                        ..
                    } if *owner_id == sender_id && *contact_id == receiver_id
                )
            })
            .count();

        handler
            .create_invitation(
                effects.clone(),
                receiver_id,
                InvitationType::Contact { nickname: None },
                Some("Contact invitation".to_string()),
                None,
            )
            .await
            .unwrap();

        let after_count = effects
            .load_committed_facts(sender_id)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|fact| match fact.content {
                FactContent::Relational(RelationalFact::Generic { envelope, .. })
                    if envelope.type_id.as_str() == CONTACT_FACT_TYPE_ID =>
                {
                    ContactFact::from_envelope(&envelope)
                }
                _ => None,
            })
            .filter(|fact| {
                matches!(
                    fact,
                    ContactFact::Added {
                        owner_id,
                        contact_id,
                        ..
                    } if *owner_id == sender_id && *contact_id == receiver_id
                )
            })
            .count();

        assert_eq!(
            after_count, before_count,
            "sender-side contact materialization should not overwrite an existing contact"
        );
    }

    #[tokio::test]
    async fn contact_acceptance_processing_skips_unrelated_envelopes() {
        let shared_transport = crate::runtime::SharedTransport::new();
        let config = AgentConfig::default();

        let sender_id = AuthorityId::new_from_entropy([126u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([127u8; 32]);

        let sender_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let receiver_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );

        let sender_handler = InvitationHandler::new(AuthorityContext::new(sender_id)).unwrap();
        let receiver_handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

        let invitation = sender_handler
            .create_invitation(
                sender_effects.clone(),
                receiver_id,
                InvitationType::Contact { nickname: None },
                Some("Contact invitation".to_string()),
                None,
            )
            .await
            .unwrap();

        // Queue a large unrelated backlog ahead of the acceptance notification.
        // This guards against starvation when inbox scanning encounters many
        // unknown content-types before actionable invitation/chat envelopes.
        for _ in 0..300 {
            let mut metadata = HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                "application/aura-unrelated".to_string(),
            );
            receiver_effects
                .send_envelope(TransportEnvelope {
                    destination: sender_id,
                    source: receiver_id,
                    context: default_context_id_for_authority(sender_id),
                    payload: b"noop".to_vec(),
                    metadata,
                    receipt: None,
                })
                .await
                .unwrap();
        }

        let code = InvitationServiceApi::export_invitation(&invitation)
            .expect("shareable invitation should serialize");
        let imported = receiver_handler
            .import_invitation_code(&receiver_effects, &code)
            .await
            .unwrap();
        receiver_handler
            .accept_invitation(receiver_effects.clone(), &imported.invitation_id)
            .await
            .unwrap();
        receiver_handler
            .notify_contact_invitation_acceptance(
                receiver_effects.as_ref(),
                &imported.invitation_id,
            )
            .await
            .unwrap();

        let processed = sender_handler
            .process_contact_invitation_acceptances(sender_effects.clone())
            .await
            .unwrap();
        assert!(processed >= 1);
    }

    #[tokio::test]
    async fn contact_acceptance_processing_commits_chat_fact_envelopes() {
        let authority = AuthorityId::new_from_entropy([201u8; 32]);
        let peer = AuthorityId::new_from_entropy([202u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, authority).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(authority)).unwrap();

        let context_id = ContextId::new_from_entropy([203u8; 32]);
        let channel_id = ChannelId::from_bytes([204u8; 32]);
        let chat_fact = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            "dm".to_string(),
            Some("Direct messages".to_string()),
            true,
            1_700_000_000_000,
            peer,
        )
        .to_generic();

        let payload = aura_core::util::serialization::to_vec(&chat_fact).unwrap();
        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            CHAT_FACT_CONTENT_TYPE.to_string(),
        );

        effects
            .send_envelope(TransportEnvelope {
                destination: authority,
                source: peer,
                context: context_id,
                payload,
                metadata,
                receipt: None,
            })
            .await
            .unwrap();

        let processed = handler
            .process_contact_invitation_acceptances(effects.clone())
            .await
            .unwrap();
        assert_eq!(processed, 1);

        let committed = effects.load_committed_facts(authority).await.unwrap();
        let mut found = false;
        for fact in committed {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };
            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                continue;
            }
            let Some(ChatFact::ChannelCreated {
                channel_id: seen, ..
            }) = ChatFact::from_envelope(&envelope)
            else {
                continue;
            };
            if seen == channel_id {
                found = true;
                break;
            }
        }

        assert!(found, "expected committed chat fact from inbound envelope");
    }

    #[tokio::test]
    async fn contact_acceptance_processing_commits_non_chat_relational_fact_envelopes() {
        let authority = AuthorityId::new_from_entropy([205u8; 32]);
        let peer = AuthorityId::new_from_entropy([206u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, authority).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(authority)).unwrap();

        let context_id = ContextId::new_from_entropy([207u8; 32]);
        let grant = HomeGrantModeratorFact::new_ms(context_id, authority, peer, 1_700_000_000_001)
            .to_generic();

        let payload = aura_core::util::serialization::to_vec(&grant).unwrap();
        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            CHAT_FACT_CONTENT_TYPE.to_string(),
        );

        effects
            .send_envelope(TransportEnvelope {
                destination: authority,
                source: peer,
                context: context_id,
                payload,
                metadata,
                receipt: None,
            })
            .await
            .unwrap();

        let processed = handler
            .process_contact_invitation_acceptances(effects.clone())
            .await
            .unwrap();
        assert_eq!(processed, 1);

        let committed = effects.load_committed_facts(authority).await.unwrap();
        let mut found = false;
        for fact in committed {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };
            let Some(grant_fact) = HomeGrantModeratorFact::from_envelope(&envelope) else {
                continue;
            };
            if grant_fact.target_authority == authority && grant_fact.actor_authority == peer {
                found = true;
                break;
            }
        }

        assert!(
            found,
            "expected committed non-chat relational fact from inbound envelope"
        );
    }

    #[tokio::test]
    async fn channel_acceptance_processing_marks_created_invitation_accepted_for_sender() {
        let sender_id = AuthorityId::new_from_entropy([207u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([208u8; 32]);
        let config = AgentConfig::default();
        let shared_transport = crate::runtime::SharedTransport::new();
        let sender_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let receiver_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver_id,
                shared_transport,
            )
            .unwrap(),
        );
        let sender_context = AuthorityContext::new(sender_id);
        let sender_handler = InvitationHandler::new(sender_context.clone()).unwrap();
        let receiver_handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
        let sender_service = invitation_service_for(sender_context, sender_effects.clone());

        let sender_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
            sender_id,
            crate::runtime::services::RendezvousManagerConfig::default(),
            Arc::new(sender_effects.time_effects().clone()),
        );
        sender_effects.attach_rendezvous_manager(sender_manager.clone());
        let sender_service_context = crate::runtime::services::RuntimeServiceContext::new(
            Arc::new(crate::runtime::TaskSupervisor::new()),
            Arc::new(sender_effects.time_effects().clone()),
        );
        crate::runtime::services::RuntimeService::start(&sender_manager, &sender_service_context)
            .await
            .unwrap();

        let receiver_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
            receiver_id,
            crate::runtime::services::RendezvousManagerConfig::default(),
            Arc::new(receiver_effects.time_effects().clone()),
        );
        receiver_effects.attach_rendezvous_manager(receiver_manager.clone());
        let receiver_service_context = crate::runtime::services::RuntimeServiceContext::new(
            Arc::new(crate::runtime::TaskSupervisor::new()),
            Arc::new(receiver_effects.time_effects().clone()),
        );
        crate::runtime::services::RuntimeService::start(
            &receiver_manager,
            &receiver_service_context,
        )
        .await
        .unwrap();

        register_test_app_signals(sender_effects.as_ref()).await;
        register_test_app_signals(receiver_effects.as_ref()).await;

        let now_ms = 1_700_000_000_000;
        sender_handler
            .cache_peer_descriptor_for_peer(
                sender_effects.as_ref(),
                receiver_id,
                None,
                Some("tcp://127.0.0.1:55021"),
                now_ms,
            )
            .await;
        receiver_handler
            .cache_peer_descriptor_for_peer(
                receiver_effects.as_ref(),
                sender_id,
                None,
                Some("tcp://127.0.0.1:55022"),
                now_ms,
            )
            .await;

        let context_id = ContextId::new_from_entropy([209u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"channel-acceptance-sender-propagation"));
        sender_effects
            .create_channel(ChannelCreateParams {
                context: context_id,
                channel: Some(channel_id),
                skip_window: None,
                topic: None,
            })
            .await
            .unwrap();
        sender_effects
            .join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: sender_id,
            })
            .await
            .unwrap();

        let invitation = sender_service
            .invite_to_channel(
                receiver_id,
                channel_id.to_string(),
                Some(context_id),
                Some("shared-parity-lab".to_string()),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let code = InvitationServiceApi::export_invitation(&invitation)
            .expect("shareable invitation should serialize");
        let imported = receiver_handler
            .import_invitation_code(&receiver_effects, &code)
            .await
            .unwrap();

        receiver_handler
            .accept_invitation(receiver_effects.clone(), &imported.invitation_id)
            .await
            .unwrap();
        let acceptance = ChannelInvitationAcceptance {
            invitation_id: imported.invitation_id.clone(),
            acceptor_id: receiver_id,
            context_id,
            channel_id,
        };
        let payload = serde_json::to_vec(&acceptance).unwrap();
        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            CHANNEL_INVITATION_ACCEPTANCE_CONTENT_TYPE.to_string(),
        );
        metadata.insert(
            "invitation-id".to_string(),
            imported.invitation_id.to_string(),
        );
        metadata.insert("acceptor-id".to_string(), receiver_id.to_string());
        metadata.insert("channel-id".to_string(), channel_id.to_string());
        sender_effects
            .send_envelope(TransportEnvelope {
                destination: sender_id,
                source: receiver_id,
                context: default_context_id_for_authority(sender_id),
                payload,
                metadata,
                receipt: None,
            })
            .await
            .unwrap();

        let processed = sender_handler
            .process_contact_invitation_acceptances(sender_effects.clone())
            .await
            .unwrap();
        assert!(processed >= 1);

        let stored = InvitationHandler::load_created_invitation(
            sender_effects.as_ref(),
            sender_id,
            &invitation.invitation_id,
        )
        .await
        .expect("created invitation should remain accessible");
        assert_eq!(stored.status, InvitationStatus::Accepted);
    }

    #[tokio::test]
    async fn channel_acceptance_notification_transports_and_updates_sender_state() {
        let sender_id = AuthorityId::new_from_entropy([221u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([222u8; 32]);
        let config = AgentConfig::default();
        let shared_transport = crate::runtime::SharedTransport::new();
        let sender_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let receiver_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver_id,
                shared_transport,
            )
            .unwrap(),
        );
        let sender_context = AuthorityContext::new(sender_id);
        let sender_handler = InvitationHandler::new(sender_context.clone()).unwrap();
        let receiver_handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
        let sender_service = invitation_service_for(sender_context, sender_effects.clone());

        let sender_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
            sender_id,
            crate::runtime::services::RendezvousManagerConfig::default(),
            Arc::new(sender_effects.time_effects().clone()),
        );
        sender_effects.attach_rendezvous_manager(sender_manager.clone());
        let sender_service_context = crate::runtime::services::RuntimeServiceContext::new(
            Arc::new(crate::runtime::TaskSupervisor::new()),
            Arc::new(sender_effects.time_effects().clone()),
        );
        crate::runtime::services::RuntimeService::start(&sender_manager, &sender_service_context)
            .await
            .unwrap();

        let receiver_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
            receiver_id,
            crate::runtime::services::RendezvousManagerConfig::default(),
            Arc::new(receiver_effects.time_effects().clone()),
        );
        receiver_effects.attach_rendezvous_manager(receiver_manager.clone());
        let receiver_service_context = crate::runtime::services::RuntimeServiceContext::new(
            Arc::new(crate::runtime::TaskSupervisor::new()),
            Arc::new(receiver_effects.time_effects().clone()),
        );
        crate::runtime::services::RuntimeService::start(
            &receiver_manager,
            &receiver_service_context,
        )
        .await
        .unwrap();

        register_test_app_signals(sender_effects.as_ref()).await;
        register_test_app_signals(receiver_effects.as_ref()).await;

        let now_ms = 1_700_000_000_000;
        sender_handler
            .cache_peer_descriptor_for_peer(
                sender_effects.as_ref(),
                receiver_id,
                None,
                Some("tcp://127.0.0.1:55002"),
                now_ms,
            )
            .await;
        receiver_handler
            .cache_peer_descriptor_for_peer(
                receiver_effects.as_ref(),
                sender_id,
                None,
                Some("tcp://127.0.0.1:55001"),
                now_ms,
            )
            .await;

        let context_id = ContextId::new_from_entropy([223u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"channel-acceptance-real-transport"));
        sender_effects
            .create_channel(ChannelCreateParams {
                context: context_id,
                channel: Some(channel_id),
                skip_window: None,
                topic: None,
            })
            .await
            .unwrap();
        sender_effects
            .join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: sender_id,
            })
            .await
            .unwrap();

        let invitation = sender_service
            .invite_to_channel(
                receiver_id,
                channel_id.to_string(),
                Some(context_id),
                Some("shared-parity-lab".to_string()),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let code = InvitationServiceApi::export_invitation(&invitation)
            .expect("shareable invitation should serialize");
        let imported = receiver_handler
            .import_invitation_code(&receiver_effects, &code)
            .await
            .unwrap();
        receiver_handler
            .accept_invitation(receiver_effects.clone(), &imported.invitation_id)
            .await
            .unwrap();
        receiver_handler
            .notify_channel_invitation_acceptance(
                receiver_effects.as_ref(),
                &imported.invitation_id,
            )
            .await
            .unwrap();

        let processed = sender_handler
            .process_contact_invitation_acceptances(sender_effects.clone())
            .await
            .unwrap();
        assert!(processed >= 1);

        let stored = InvitationHandler::load_created_invitation(
            sender_effects.as_ref(),
            sender_id,
            &invitation.invitation_id,
        )
        .await
        .expect("created invitation should remain accessible");
        assert_eq!(stored.status, InvitationStatus::Accepted);

        use aura_effects::ReactiveEffects;
        let homes: HomesState = sender_effects
            .reactive_handler()
            .read(&*HOMES_SIGNAL)
            .await
            .unwrap();
        let home = homes
            .home_state(&channel_id)
            .expect("sender should materialize channel acceptance home state");
        assert_eq!(home.context_id, Some(context_id));
        assert!(
            home.member(&receiver_id).is_some(),
            "sender home state should include receiver after transported acceptance"
        );
    }

    #[tokio::test]
    async fn import_channel_invitation_requires_authoritative_context() {
        let receiver_id = AuthorityId::new_from_entropy([217u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, receiver_id).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-missing-channel-context"),
            sender_id: AuthorityId::new_from_entropy([218u8; 32]),
            context_id: None,
            invitation_type: InvitationType::Channel {
                home_id: canonical_home_id(18),
                nickname_suggestion: Some("No Context House".to_string()),
                bootstrap: None,
            },
            expires_at: None,
            message: Some("Join No Context House".to_string()),
        };

        let error = handler
            .import_invitation_code(
                effects.as_ref(),
                &shareable
                    .to_code()
                    .expect("shareable invitation should serialize"),
            )
            .await
            .expect_err("channel invitation import must require authoritative context");

        assert!(error.to_string().contains("missing authoritative context"));
    }

    #[tokio::test]
    async fn channel_acceptance_notification_surfaces_peer_channel_establishment_failure() {
        let sender_id = AuthorityId::new_from_entropy([219u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([220u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, receiver_id).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
        register_test_app_signals(effects.as_ref()).await;
        let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
        cache_test_peer_descriptor(
            effects.as_ref(),
            receiver_id,
            sender_id,
            "tcp://127.0.0.1:55118",
            1_700_000_000_000,
        )
        .await;

        let invitation_context = ContextId::new_from_entropy([56u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-channel-context-strict"),
            sender_id,
            context_id: Some(invitation_context),
            invitation_type: InvitationType::Channel {
                home_id: canonical_home_id(19),
                nickname_suggestion: Some("Context Strict House".to_string()),
                bootstrap: None,
            },
            expires_at: None,
            message: Some("Join Context Strict House".to_string()),
        };

        let imported = handler
            .import_invitation_code(
                effects.as_ref(),
                &shareable
                    .to_code()
                    .expect("shareable invitation should serialize"),
            )
            .await
            .expect("channel invitation import should succeed");
        let channel_invite = handler
            .resolve_channel_invitation(effects.as_ref(), &imported.invitation_id)
            .await
            .expect("channel invitation resolution should succeed")
            .expect("channel invitation should remain available");
        handler
            .materialize_channel_invitation_acceptance(effects.as_ref(), &channel_invite)
            .await
            .expect("channel invitation accept should succeed locally");

        let error = handler
            .notify_channel_invitation_acceptance(effects.as_ref(), &imported.invitation_id)
            .await
            .expect_err("notification must not fall back to sender default context");

        assert!(matches!(
            error,
            AgentError::Runtime(_) | AgentError::Effects(_)
        ));
    }

    #[tokio::test]
    async fn contact_acceptance_processing_provisions_amp_state_for_channel_created_facts() {
        let authority = AuthorityId::new_from_entropy([208u8; 32]);
        let peer = AuthorityId::new_from_entropy([209u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, authority).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(authority)).unwrap();

        let context_id = ContextId::new_from_entropy([210u8; 32]);
        let channel_id = ChannelId::from_bytes([211u8; 32]);
        let chat_fact = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            "provisioned".to_string(),
            Some("Provisioned channel".to_string()),
            false,
            1_700_000_000_100,
            peer,
        )
        .to_generic();

        let payload = aura_core::util::serialization::to_vec(&chat_fact).unwrap();
        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            CHAT_FACT_CONTENT_TYPE.to_string(),
        );

        effects
            .send_envelope(TransportEnvelope {
                destination: authority,
                source: peer,
                context: context_id,
                payload,
                metadata,
                receipt: None,
            })
            .await
            .unwrap();

        let processed = handler
            .process_contact_invitation_acceptances(effects.clone())
            .await
            .unwrap();
        assert_eq!(processed, 1);

        timeout(Duration::from_secs(5), async {
            loop {
                if aura_protocol::amp::get_channel_state(effects.as_ref(), context_id, channel_id)
                    .await
                    .is_ok()
                {
                    break;
                }
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("timed out waiting for provisioned AMP channel state");
    }

    #[tokio::test]
    async fn invitation_envelope_processing_imports_pending_channel_invites() {
        let sender_id = AuthorityId::new_from_entropy([211u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([212u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, receiver_id).unwrap(),
        );
        register_app_signals(&effects.reactive_handler())
            .await
            .expect("app signals should register");

        let receiver_handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

        let invitation_id = InvitationId::new("inv-envelope-home-1");
        let home_id = canonical_home_id(12);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation_id.clone(),
            sender_id,
            context_id: Some(default_context_id_for_authority(sender_id)),
            invitation_type: InvitationType::Channel {
                home_id,
                nickname_suggestion: Some("Maple House".to_string()),
                bootstrap: None,
            },
            expires_at: None,
            message: Some("Join Maple House".to_string()),
        };

        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            INVITATION_CONTENT_TYPE.to_string(),
        );
        metadata.insert("invitation-id".to_string(), invitation_id.to_string());
        metadata.insert(
            "invitation-context".to_string(),
            default_context_id_for_authority(sender_id).to_string(),
        );

        effects
            .send_envelope(TransportEnvelope {
                destination: receiver_id,
                source: sender_id,
                context: default_context_id_for_authority(sender_id),
                payload: shareable
                    .to_code()
                    .expect("shareable invitation should serialize")
                    .into_bytes(),
                metadata,
                receipt: None,
            })
            .await
            .unwrap();

        let processed = receiver_handler
            .process_contact_invitation_acceptances(effects.clone())
            .await
            .unwrap();
        assert_eq!(processed, 1);

        let fresh_handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
        let pending = fresh_handler
            .list_pending_with_storage(effects.as_ref())
            .await;
        let found = pending.iter().any(|inv| {
            inv.invitation_id == invitation_id
                && matches!(inv.invitation_type, InvitationType::Channel { .. })
                && inv.status == InvitationStatus::Pending
                && inv.sender_id == sender_id
                && inv.receiver_id == receiver_id
        });
        assert!(
            found,
            "expected imported channel invitation to appear in pending list"
        );

        let invitations = effects
            .reactive_handler()
            .read(&*INVITATIONS_SIGNAL)
            .await
            .expect("invitation signal should be registered");
        assert!(invitations.all_pending().iter().any(|inv| {
            inv.id == invitation_id.to_string()
                && inv.direction == aura_app::views::invitations::InvitationDirection::Received
                && inv.status == aura_app::views::invitations::InvitationStatus::Pending
        }));
    }

    #[tokio::test]
    async fn accepting_channel_invitation_materializes_home_and_channel_state() {
        let sender_id = AuthorityId::new_from_entropy([213u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([214u8; 32]);
        let config = AgentConfig::default();
        let shared_transport = crate::runtime::SharedTransport::new();
        let _sender_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver_id,
                shared_transport,
            )
            .unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
        register_test_app_signals(effects.as_ref()).await;
        let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
        cache_test_peer_descriptor(
            effects.as_ref(),
            receiver_id,
            sender_id,
            "tcp://127.0.0.1:55113",
            1_700_000_000_000,
        )
        .await;

        let invitation_id = InvitationId::new("inv-materialize-home-1");
        let home_id = canonical_home_id(13);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation_id.clone(),
            sender_id,
            context_id: Some(default_context_id_for_authority(sender_id)),
            invitation_type: InvitationType::Channel {
                home_id,
                nickname_suggestion: Some("Oak House".to_string()),
                bootstrap: None,
            },
            expires_at: None,
            message: Some("Join Oak House".to_string()),
        };

        let imported = handler
            .import_invitation_code(
                effects.as_ref(),
                &shareable
                    .to_code()
                    .expect("shareable invitation should serialize"),
            )
            .await
            .unwrap();

        handler
            .accept_invitation(effects.clone(), &imported.invitation_id)
            .await
            .unwrap();

        let expected_context = default_context_id_for_authority(sender_id);
        let expected_channel = home_id;

        let committed = effects.load_committed_facts(receiver_id).await.unwrap();
        let found_channel_fact = committed.iter().any(|fact| {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
            else {
                return false;
            };
            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                return false;
            }
            matches!(
                ChatFact::from_envelope(envelope),
                Some(ChatFact::ChannelCreated {
                    context_id,
                    channel_id,
                    ..
                }) if context_id == expected_context && channel_id == expected_channel
            )
        });
        assert!(
            found_channel_fact,
            "expected ChannelCreated fact for accepted channel invitation"
        );

        use aura_effects::ReactiveEffects;
        let homes: HomesState = effects
            .reactive_handler()
            .read(&*HOMES_SIGNAL)
            .await
            .unwrap();
        let home = homes
            .home_state(&expected_channel)
            .expect("accepted invitation should materialize home state");
        assert_eq!(home.context_id, Some(expected_context));
        assert!(home.member(&receiver_id).is_some());
        assert_eq!(home.my_role, HomeRole::Participant);
    }

    #[tokio::test]
    async fn accepting_channel_invitation_corrects_preexisting_raw_channel_name() {
        let sender_id = AuthorityId::new_from_entropy([219u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([220u8; 32]);
        let config = AgentConfig::default();
        let shared_transport = crate::runtime::SharedTransport::new();
        let _sender_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver_id,
                shared_transport,
            )
            .unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
        register_test_app_signals(effects.as_ref()).await;
        let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
        cache_test_peer_descriptor(
            effects.as_ref(),
            receiver_id,
            sender_id,
            "tcp://127.0.0.1:55116",
            1_700_000_000_000,
        )
        .await;

        let invitation_id = InvitationId::new("inv-materialize-home-raw-name");
        let home_id = canonical_home_id(16);
        let expected_context = default_context_id_for_authority(sender_id);

        effects
            .commit_relational_facts(vec![ChatFact::channel_created_ms(
                expected_context,
                home_id,
                home_id.to_string(),
                Some(format!("Home channel {}", home_id)),
                false,
                1_700_000_000_000,
                sender_id,
            )
            .to_generic()])
            .await
            .unwrap();

        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation_id.clone(),
            sender_id,
            context_id: Some(expected_context),
            invitation_type: InvitationType::Channel {
                home_id,
                nickname_suggestion: Some("Maple House".to_string()),
                bootstrap: None,
            },
            expires_at: None,
            message: Some("Join Maple House".to_string()),
        };

        let imported = handler
            .import_invitation_code(
                effects.as_ref(),
                &shareable
                    .to_code()
                    .expect("shareable invitation should serialize"),
            )
            .await
            .unwrap();

        accept_invitation_without_notification(&handler, effects.clone(), &imported.invitation_id)
            .await;

        let committed = effects.load_committed_facts(receiver_id).await.unwrap();
        let found_named_update = committed.iter().any(|fact| {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
            else {
                return false;
            };
            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                return false;
            }
            matches!(
                ChatFact::from_envelope(envelope),
                Some(ChatFact::ChannelUpdated {
                    context_id,
                    channel_id,
                    name: Some(name),
                    ..
                }) if context_id == expected_context
                    && channel_id == home_id
                    && name == "Maple House"
            )
        });
        assert!(
            found_named_update,
            "accepted invitation should correct preexisting raw-id channel metadata"
        );
    }

    #[tokio::test]
    async fn accepting_channel_invitation_materializes_amp_bootstrap_state() {
        let sender_id = AuthorityId::new_from_entropy([217u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([218u8; 32]);
        let config = AgentConfig::default();
        let shared_transport = crate::runtime::SharedTransport::new();
        let _sender_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver_id,
                shared_transport,
            )
            .unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
        register_test_app_signals(effects.as_ref()).await;
        let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
        cache_test_peer_descriptor(
            effects.as_ref(),
            receiver_id,
            sender_id,
            "tcp://127.0.0.1:55114",
            1_700_000_000_000,
        )
        .await;

        let invitation_id = InvitationId::new("inv-materialize-bootstrap-1");
        let home_id = canonical_home_id(14);
        let bootstrap_key = [7u8; 32];
        let bootstrap_id = Hash32::from_bytes(&bootstrap_key);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation_id.clone(),
            sender_id,
            context_id: Some(default_context_id_for_authority(sender_id)),
            invitation_type: InvitationType::Channel {
                home_id,
                nickname_suggestion: Some("Elm House".to_string()),
                bootstrap: Some(ChannelBootstrapPackage {
                    bootstrap_id,
                    key: bootstrap_key.to_vec(),
                }),
            },
            expires_at: None,
            message: Some("Join Elm House".to_string()),
        };

        let imported = handler
            .import_invitation_code(
                effects.as_ref(),
                &shareable
                    .to_code()
                    .expect("shareable invitation should serialize"),
            )
            .await
            .unwrap();

        handler
            .accept_invitation(effects.clone(), &imported.invitation_id)
            .await
            .unwrap();

        let expected_context = default_context_id_for_authority(sender_id);
        let expected_channel = home_id;

        let state = aura_protocol::amp::get_channel_state(
            effects.as_ref(),
            expected_context,
            expected_channel,
        )
        .await
        .expect("accepted invitation should materialize AMP channel state");
        let bootstrap = state
            .bootstrap
            .expect("accepted invitation should materialize bootstrap metadata");
        assert_eq!(bootstrap.bootstrap_id, bootstrap_id);
        assert_eq!(bootstrap.dealer, sender_id);
        assert!(bootstrap.recipients.contains(&sender_id));
        assert!(bootstrap.recipients.contains(&receiver_id));

        let location = SecureStorageLocation::amp_bootstrap_key(
            &expected_context,
            &expected_channel,
            &bootstrap_id,
        );
        let stored_key = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .expect("bootstrap key should be persisted");
        assert_eq!(stored_key, bootstrap_key.to_vec());
    }

    #[tokio::test]
    async fn accepting_channel_invitation_uses_shareable_context_when_present() {
        let sender_id = AuthorityId::new_from_entropy([215u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([216u8; 32]);
        let config = AgentConfig::default();
        let shared_transport = crate::runtime::SharedTransport::new();
        let _sender_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver_id,
                shared_transport,
            )
            .unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();
        register_test_app_signals(effects.as_ref()).await;
        let _rendezvous_tasks = attach_test_rendezvous_manager(effects.as_ref(), receiver_id).await;
        cache_test_peer_descriptor(
            effects.as_ref(),
            receiver_id,
            sender_id,
            "tcp://127.0.0.1:55115",
            1_700_000_000_000,
        )
        .await;

        let invitation_id = InvitationId::new("inv-materialize-home-context");
        let custom_context = ContextId::new_from_entropy([55u8; 32]);
        let home_id = canonical_home_id(15);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation_id.clone(),
            sender_id,
            context_id: Some(custom_context),
            invitation_type: InvitationType::Channel {
                home_id,
                nickname_suggestion: Some("Birch House".to_string()),
                bootstrap: None,
            },
            expires_at: None,
            message: Some("Join Birch House".to_string()),
        };

        let imported = handler
            .import_invitation_code(
                effects.as_ref(),
                &shareable
                    .to_code()
                    .expect("shareable invitation should serialize"),
            )
            .await
            .unwrap();
        assert_eq!(imported.context_id, custom_context);
        assert_ne!(
            imported.context_id,
            default_context_id_for_authority(sender_id),
            "custom context must override sender default context"
        );

        handler
            .accept_invitation(effects.clone(), &imported.invitation_id)
            .await
            .unwrap();

        let expected_channel = home_id;
        let committed = effects.load_committed_facts(receiver_id).await.unwrap();
        let found_channel_fact = committed.iter().any(|fact| {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
            else {
                return false;
            };
            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                return false;
            }
            matches!(
                ChatFact::from_envelope(envelope),
                Some(ChatFact::ChannelCreated {
                    context_id,
                    channel_id,
                    ..
                }) if context_id == custom_context && channel_id == expected_channel
            )
        });
        assert!(
            found_channel_fact,
            "expected ChannelCreated fact to use shareable context id"
        );

        use aura_effects::ReactiveEffects;
        let homes: HomesState = effects
            .reactive_handler()
            .read(&*HOMES_SIGNAL)
            .await
            .unwrap();
        let home = homes
            .home_state(&expected_channel)
            .expect("accepted invitation should materialize home state");
        assert_eq!(home.context_id, Some(custom_context));
    }

    #[tokio::test]
    async fn imported_invitation_is_resolvable_across_handler_instances() {
        let own_authority = AuthorityId::new_from_entropy([122u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
        );

        let authority_context = AuthorityContext::new(own_authority);

        let handler_import = InvitationHandler::new(authority_context.clone()).unwrap();
        let handler_accept = InvitationHandler::new(authority_context).unwrap();

        let sender_id = AuthorityId::new_from_entropy([123u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-demo-contact-2"),
            sender_id,
            context_id: None,
            invitation_type: InvitationType::Contact {
                nickname: Some("Alice".to_string()),
            },
            expires_at: None,
            message: Some("Contact invitation from Alice (demo)".to_string()),
        };
        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");

        let imported = handler_import
            .import_invitation_code(&effects, &code)
            .await
            .unwrap();

        // Accept using a separate handler instance to ensure we don't rely on in-memory caches.
        handler_accept
            .accept_invitation(effects.clone(), &imported.invitation_id)
            .await
            .unwrap();

        let committed = effects.load_committed_facts(own_authority).await.unwrap();

        let mut found = None::<ContactFact>;
        for fact in committed {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
                continue;
            }

            found = ContactFact::from_envelope(&envelope);
        }

        let fact = found.expect("expected a committed ContactFact");
        match fact {
            ContactFact::Added { contact_id, .. } => {
                assert_eq!(contact_id, sender_id);
            }
            other => panic!("Expected ContactFact::Added, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn imported_channel_invitation_preserves_authoritative_context_for_choreography() {
        let own_authority = AuthorityId::new_from_entropy([211u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([212u8; 32]);
        let invitation_context = ContextId::new_from_entropy([213u8; 32]);
        let channel_id = canonical_home_id(214);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
        );

        let authority_context = AuthorityContext::new(own_authority);
        let handler = InvitationHandler::new(authority_context).unwrap();

        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-demo-channel-context"),
            sender_id,
            context_id: Some(invitation_context),
            invitation_type: InvitationType::Channel {
                home_id: channel_id,
                nickname_suggestion: Some("shared-parity-lab".to_string()),
                bootstrap: None,
            },
            expires_at: None,
            message: Some("Channel invitation".to_string()),
        };
        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");

        let imported = handler
            .import_invitation_code(&effects, &code)
            .await
            .expect("channel import should succeed");

        let choreography_invitation = handler
            .load_invitation_for_choreography(effects.as_ref(), &imported.invitation_id)
            .await
            .expect("imported invitation should be resolvable for choreography");

        assert_eq!(
            choreography_invitation.context_id, invitation_context,
            "channel invitation choreography must preserve the authoritative invitation context"
        );
    }

    #[tokio::test]
    async fn created_invitation_is_retrievable_across_handler_instances() {
        // This test verifies that created invitations are persisted to storage
        // and can be retrieved by a different handler instance (fixing the
        // "failed to export" bug where each agent.invitations() call creates
        // a new handler with an empty in-memory cache).
        let own_authority = AuthorityId::new_from_entropy([124u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
        );

        let authority_context = AuthorityContext::new(own_authority);

        // Handler 1: Create an invitation
        let handler_create = InvitationHandler::new(authority_context.clone()).unwrap();
        let receiver_id = AuthorityId::new_from_entropy([125u8; 32]);
        let invitation = handler_create
            .create_invitation(
                effects.clone(),
                receiver_id,
                InvitationType::Contact {
                    nickname: Some("Bob".to_string()),
                },
                Some("Hello Bob!".to_string()),
                None,
            )
            .await
            .unwrap();

        // Handler 2: Retrieve the invitation (simulates new service instance)
        let handler_retrieve = InvitationHandler::new(authority_context).unwrap();
        let retrieved = handler_retrieve
            .get_invitation_with_storage(&effects, &invitation.invitation_id)
            .await;

        assert!(
            retrieved.is_some(),
            "Created invitation should be retrievable across handler instances"
        );
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.invitation_id, invitation.invitation_id);
        assert_eq!(retrieved.receiver_id, receiver_id);
        assert_eq!(retrieved.sender_id, own_authority);
    }

    #[tokio::test]
    async fn accepted_imported_invitation_persists_status_across_handler_instances() {
        let own_authority = AuthorityId::new_from_entropy([126u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([127u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
        );
        let authority_context = AuthorityContext::new(own_authority);
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("imported-contact-persists-accepted"),
            sender_id,
            context_id: None,
            invitation_type: InvitationType::Contact {
                nickname: Some("Alice".to_string()),
            },
            expires_at: None,
            message: Some("hello".to_string()),
        };

        let imported = handler
            .import_invitation_code(
                effects.as_ref(),
                &shareable
                    .to_code()
                    .expect("shareable invitation should serialize"),
            )
            .await
            .expect("contact invitation import should succeed");
        handler
            .accept_invitation(effects.clone(), &imported.invitation_id)
            .await
            .expect("contact invitation accept should persist imported status");

        let retrieved = InvitationHandler::new(authority_context)
            .unwrap()
            .get_invitation_with_storage(effects.as_ref(), &imported.invitation_id)
            .await
            .expect("accepted imported invitation should remain available");
        assert_eq!(retrieved.status, InvitationStatus::Accepted);
        assert_eq!(retrieved.sender_id, sender_id);
        assert_eq!(retrieved.receiver_id, own_authority);
    }

    #[tokio::test]
    async fn invitation_can_be_cancelled() {
        let authority_context = create_test_authority(98);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([99u8; 32]);

        let invitation = handler
            .create_invitation(
                effects.clone(),
                receiver_id,
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let result = handler
            .cancel_invitation(effects.clone(), &invitation.invitation_id)
            .await
            .unwrap();

        assert_eq!(result.new_status, InvitationStatus::Cancelled);

        // Verify it was removed from pending
        let pending = handler.list_pending().await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn list_pending_shows_only_pending() {
        let authority_context = create_test_authority(100);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context).unwrap();

        // Create 3 invitations
        let inv1 = handler
            .create_invitation(
                effects.clone(),
                AuthorityId::new_from_entropy([101u8; 32]),
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let inv2 = handler
            .create_invitation(
                effects.clone(),
                AuthorityId::new_from_entropy([102u8; 32]),
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let _inv3 = handler
            .create_invitation(
                effects.clone(),
                AuthorityId::new_from_entropy([103u8; 32]),
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        // Accept one, cancel another
        handler
            .accept_invitation(effects.clone(), &inv1.invitation_id)
            .await
            .unwrap();
        handler
            .cancel_invitation(effects.clone(), &inv2.invitation_id)
            .await
            .unwrap();

        // Only inv3 should be pending
        let pending = handler.list_pending().await;
        assert_eq!(pending.len(), 1);
    }

    // =========================================================================
    // ShareableInvitation Tests
    // =========================================================================

    #[test]
    fn shareable_invitation_roundtrip_contact() {
        let sender_id = AuthorityId::new_from_entropy([42u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-test-123"),
            sender_id,
            context_id: None,
            invitation_type: InvitationType::Contact {
                nickname: Some("alice".to_string()),
            },
            expires_at: Some(1700000000000),
            message: Some("Hello!".to_string()),
        };

        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");
        assert!(code.starts_with("aura:v1:"));

        let decoded = ShareableInvitation::from_code(&code).unwrap();
        assert_eq!(decoded.version, shareable.version);
        assert_eq!(decoded.invitation_id, shareable.invitation_id);
        assert_eq!(decoded.sender_id, shareable.sender_id);
        assert_eq!(decoded.expires_at, shareable.expires_at);
        assert_eq!(decoded.message, shareable.message);
    }

    #[test]
    fn shareable_invitation_roundtrip_guardian() {
        let sender_id = AuthorityId::new_from_entropy([43u8; 32]);
        let subject_authority = AuthorityId::new_from_entropy([44u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-guardian-456"),
            sender_id,
            context_id: None,
            invitation_type: InvitationType::Guardian { subject_authority },
            expires_at: None,
            message: None,
        };

        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");
        let decoded = ShareableInvitation::from_code(&code).unwrap();

        match decoded.invitation_type {
            InvitationType::Guardian {
                subject_authority: sa,
            } => {
                assert_eq!(sa, subject_authority);
            }
            _ => panic!("wrong invitation type"),
        }
    }

    #[test]
    fn shareable_invitation_roundtrip_channel() {
        let sender_id = AuthorityId::new_from_entropy([45u8; 32]);
        let context_id = ContextId::new_from_entropy([56u8; 32]);
        let home_id = ChannelId::from_bytes([21u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-channel-789"),
            sender_id,
            context_id: Some(context_id),
            invitation_type: InvitationType::Channel {
                home_id,
                nickname_suggestion: None,
                bootstrap: None,
            },
            expires_at: Some(1800000000000),
            message: Some("Join my channel!".to_string()),
        };

        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");
        let decoded = ShareableInvitation::from_code(&code).unwrap();
        assert_eq!(decoded.context_id, Some(context_id));

        match decoded.invitation_type {
            InvitationType::Channel {
                home_id,
                nickname_suggestion: _,
                bootstrap: _,
            } => {
                assert_eq!(home_id, ChannelId::from_bytes([21u8; 32]));
            }
            _ => panic!("wrong invitation type"),
        }
    }

    #[test]
    fn shareable_invitation_roundtrip_device_enrollment_preserves_baseline_tree_ops() {
        let sender_id = AuthorityId::new_from_entropy([145u8; 32]);
        let subject_authority = AuthorityId::new_from_entropy([146u8; 32]);
        let context_id = ContextId::new_from_entropy([147u8; 32]);
        let initiator_device_id = DeviceId::new_from_entropy([148u8; 32]);
        let device_id = DeviceId::new_from_entropy([149u8; 32]);
        let ceremony_id = CeremonyId::new("ceremony:test-device-enrollment");
        let baseline_tree_ops = vec![vec![1, 2, 3], vec![4, 5, 6, 7]];
        let threshold_config = vec![9, 8, 7];
        let public_key_package = vec![6, 5, 4, 3];
        let key_package = vec![3, 4, 5];

        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-device-enrollment"),
            sender_id,
            context_id: Some(context_id),
            invitation_type: InvitationType::DeviceEnrollment {
                subject_authority,
                initiator_device_id,
                device_id,
                nickname_suggestion: Some("WebApp".to_string()),
                ceremony_id: ceremony_id.clone(),
                pending_epoch: 1,
                key_package: key_package.clone(),
                threshold_config: threshold_config.clone(),
                public_key_package: public_key_package.clone(),
                baseline_tree_ops: baseline_tree_ops.clone(),
            },
            expires_at: None,
            message: None,
        };

        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");
        let decoded = ShareableInvitation::from_code(&code).unwrap();

        match decoded.invitation_type {
            InvitationType::DeviceEnrollment {
                subject_authority: decoded_subject_authority,
                initiator_device_id: decoded_initiator_device_id,
                device_id: decoded_device_id,
                nickname_suggestion,
                ceremony_id: decoded_ceremony_id,
                pending_epoch,
                key_package: decoded_key_package,
                threshold_config: decoded_threshold_config,
                public_key_package: decoded_public_key_package,
                baseline_tree_ops: decoded_baseline_tree_ops,
            } => {
                assert_eq!(decoded_subject_authority, subject_authority);
                assert_eq!(decoded_initiator_device_id, initiator_device_id);
                assert_eq!(decoded_device_id, device_id);
                assert_eq!(nickname_suggestion.as_deref(), Some("WebApp"));
                assert_eq!(decoded_ceremony_id, ceremony_id);
                assert_eq!(pending_epoch, 1);
                assert_eq!(decoded_key_package, key_package);
                assert_eq!(decoded_threshold_config, threshold_config);
                assert_eq!(decoded_public_key_package, public_key_package);
                assert_eq!(decoded_baseline_tree_ops, baseline_tree_ops);
            }
            _ => panic!("wrong invitation type"),
        }
    }

    #[test]
    fn shareable_invitation_parses_optional_sender_addr_and_device_segments() {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let sender_id = AuthorityId::new_from_entropy([46u8; 32]);
        let sender_device_id = DeviceId::new_from_entropy([47u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-addr-001"),
            sender_id,
            context_id: None,
            invitation_type: InvitationType::Contact { nickname: None },
            expires_at: None,
            message: None,
        };
        let base = shareable
            .to_code()
            .expect("shareable invitation should serialize");
        let code = format!(
            "{base}:{}:{}",
            URL_SAFE_NO_PAD.encode("127.0.0.1:43501".as_bytes()),
            URL_SAFE_NO_PAD.encode(sender_device_id.to_string().as_bytes())
        );

        let decoded = ShareableInvitation::from_code(&code).unwrap();
        assert_eq!(decoded.invitation_id, shareable.invitation_id);
        assert_eq!(decoded.sender_id, shareable.sender_id);
        assert_eq!(
            ShareableInvitation::sender_addr_from_code(&code),
            Some("127.0.0.1:43501".to_string())
        );
        assert_eq!(
            ShareableInvitation::sender_device_id_from_code(&code),
            Some(sender_device_id)
        );
    }

    #[test]
    fn shareable_invitation_invalid_format() {
        // Missing parts
        assert_eq!(
            ShareableInvitation::from_code("aura:v1").unwrap_err(),
            ShareableInvitationError::InvalidFormat
        );

        // Wrong prefix
        assert_eq!(
            ShareableInvitation::from_code("badprefix:v1:abc").unwrap_err(),
            ShareableInvitationError::InvalidFormat
        );

        // Invalid version format
        assert_eq!(
            ShareableInvitation::from_code("aura:1:abc").unwrap_err(),
            ShareableInvitationError::InvalidFormat
        );
    }

    #[test]
    fn shareable_invitation_unsupported_version() {
        // Version 99 doesn't exist
        assert_eq!(
            ShareableInvitation::from_code("aura:v99:abc").unwrap_err(),
            ShareableInvitationError::UnsupportedVersion(99)
        );
    }

    #[test]
    fn shareable_invitation_decoding_failed() {
        // Not valid base64
        assert_eq!(
            ShareableInvitation::from_code("aura:v1:!!!invalid!!!").unwrap_err(),
            ShareableInvitationError::DecodingFailed
        );
    }

    #[test]
    fn shareable_invitation_parsing_failed() {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        // Valid base64 but not valid JSON
        let bad_json = URL_SAFE_NO_PAD.encode("not json");
        let code = format!("aura:v1:{}", bad_json);
        assert_eq!(
            ShareableInvitation::from_code(&code).unwrap_err(),
            ShareableInvitationError::ParsingFailed
        );
    }

    #[test]
    fn shareable_invitation_from_invitation() {
        let invitation = Invitation {
            invitation_id: InvitationId::new("inv-from-full"),
            context_id: ContextId::new_from_entropy([50u8; 32]),
            sender_id: AuthorityId::new_from_entropy([51u8; 32]),
            receiver_id: AuthorityId::new_from_entropy([52u8; 32]),
            invitation_type: InvitationType::Contact {
                nickname: Some("bob".to_string()),
            },
            status: InvitationStatus::Pending,
            created_at: 1600000000000,
            expires_at: Some(1700000000000),
            message: Some("Hi Bob!".to_string()),
        };

        let shareable = ShareableInvitation::from(&invitation);
        assert_eq!(shareable.invitation_id, invitation.invitation_id);
        assert_eq!(shareable.sender_id, invitation.sender_id);
        assert_eq!(shareable.expires_at, invitation.expires_at);
        assert_eq!(shareable.message, invitation.message);

        // Round-trip via code
        let code = shareable
            .to_code()
            .expect("shareable invitation should serialize");
        let decoded = ShareableInvitation::from_code(&code).unwrap();
        assert_eq!(decoded.invitation_id, invitation.invitation_id);
    }

    /// Test that importing and accepting multiple contact invitations works sequentially.
    ///
    /// This test mimics the TUI demo mode flow where:
    /// 1. Alice's invitation is imported and accepted
    /// 2. Carol's invitation is imported and accepted
    ///
    /// Both should succeed without interfering with each other.
    #[tokio::test]
    async fn importing_multiple_contact_invitations_sequentially() {
        let own_authority = AuthorityId::new_from_entropy([150u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
        );

        let authority_context = AuthorityContext::new(own_authority);
        let handler = InvitationHandler::new(authority_context).unwrap();

        // Create Alice's invitation (matching DemoHints pattern)
        let alice_sender_id = AuthorityId::new_from_entropy([151u8; 32]);
        let alice_shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-demo-alice-sequential"),
            sender_id: alice_sender_id,
            context_id: None,
            invitation_type: InvitationType::Contact {
                nickname: Some("Alice".to_string()),
            },
            expires_at: None,
            message: Some("Contact invitation from Alice (demo)".to_string()),
        };
        let alice_code = alice_shareable
            .to_code()
            .expect("shareable invitation should serialize");

        // Create Carol's invitation (matching DemoHints pattern - different seed)
        let carol_sender_id = AuthorityId::new_from_entropy([152u8; 32]);
        let carol_shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-demo-carol-sequential"),
            sender_id: carol_sender_id,
            context_id: None,
            invitation_type: InvitationType::Contact {
                nickname: Some("Carol".to_string()),
            },
            expires_at: None,
            message: Some("Contact invitation from Carol (demo)".to_string()),
        };
        let carol_code = carol_shareable
            .to_code()
            .expect("shareable invitation should serialize");

        // Import and accept Alice's invitation
        let alice_imported = handler
            .import_invitation_code(&effects, &alice_code)
            .await
            .expect("Alice import should succeed");
        assert_eq!(alice_imported.sender_id, alice_sender_id);
        assert_eq!(
            alice_imported.invitation_id.as_str(),
            "inv-demo-alice-sequential"
        );

        handler
            .accept_invitation(effects.clone(), &alice_imported.invitation_id)
            .await
            .expect("Alice accept should succeed");

        // Import and accept Carol's invitation (this is the step that was failing in TUI)
        let carol_imported = handler
            .import_invitation_code(&effects, &carol_code)
            .await
            .expect("Carol import should succeed");
        assert_eq!(carol_imported.sender_id, carol_sender_id);
        assert_eq!(
            carol_imported.invitation_id.as_str(),
            "inv-demo-carol-sequential"
        );

        // This is the critical assertion - Carol's accept should work after Alice's
        handler
            .accept_invitation(effects.clone(), &carol_imported.invitation_id)
            .await
            .expect("Carol accept should succeed after Alice");

        // Verify both contacts were added
        let committed = effects.load_committed_facts(own_authority).await.unwrap();

        let mut contact_facts: Vec<ContactFact> = Vec::new();
        for fact in committed {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
                continue;
            }

            if let Some(contact_fact) = ContactFact::from_envelope(&envelope) {
                contact_facts.push(contact_fact);
            }
        }

        // Verify we have both Alice and Carol as contacts
        // (other tests may add additional contact facts, so we just verify these two are present)
        let contact_ids: Vec<AuthorityId> = contact_facts
            .iter()
            .filter_map(|f| match f {
                ContactFact::Added { contact_id, .. } => Some(*contact_id),
                _ => None,
            })
            .collect();

        assert!(
            contact_ids.contains(&alice_sender_id),
            "Alice should be in contacts, found: {:?}",
            contact_ids
        );
        assert!(
            contact_ids.contains(&carol_sender_id),
            "Carol should be in contacts, found: {:?}",
            contact_ids
        );
    }
}
