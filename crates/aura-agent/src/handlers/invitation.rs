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
use execution::{
    attempt_network_send_envelope, emit_browser_harness_debug_event, invitation_timeout_budget,
    invitation_timeout_profile, timeout_deferred_network_stage, timeout_invitation_stage_with_budget,
    timeout_prepare_invitation_stage,
};
use crate::core::{default_context_id_for_authority, AgentError, AgentResult, AuthorityContext};
use crate::reactive::app_signal_views;
use crate::runtime::services::InvitationManager;
#[cfg(feature = "choreo-backend-telltale-machine")]
use crate::runtime::{open_owned_manifest_vm_session_admitted, AuraEffectSystem};
#[cfg(feature = "choreo-backend-telltale-machine")]
use crate::runtime::vm_host_bridge::AuraVmHostWaitStatus;
#[cfg(not(feature = "choreo-backend-telltale-machine"))]
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
#[cfg(not(feature = "choreo-backend-telltale-machine"))]
use aura_invitation::protocol::exchange_runners::InvitationExchangeRole;
use aura_invitation::protocol::exchange::telltale_session_types_invitation::message_wrappers::{
    InvitationAck as ExchangeInvitationAck,
    InvitationOffer as ExchangeInvitationOffer,
    InvitationResponse as ExchangeInvitationResponse,
};
#[cfg(all(test, feature = "choreo-backend-telltale-machine"))]
use aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::message_wrappers::GuardianAccept as GuardianInvitationAccept;
use aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::message_wrappers::{
    GuardianConfirm as GuardianInvitationConfirm, GuardianRequest as GuardianInvitationRequest,
};
use aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::message_wrappers::{
    DeviceEnrollmentAccept as DeviceEnrollmentAcceptWrapper,
    DeviceEnrollmentConfirm as DeviceEnrollmentConfirmWrapper,
    DeviceEnrollmentRequest as DeviceEnrollmentRequestWrapper,
};
#[cfg(all(test, feature = "choreo-backend-telltale-machine"))]
use aura_invitation::{GuardianAccept, InvitationResponse};
use aura_invitation::{
    DeviceEnrollmentAccept, DeviceEnrollmentConfirm, DeviceEnrollmentRequest, GuardianConfirm,
    GuardianRequest, InvitationAck, InvitationOffer, InvitationOperation,
};
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use aura_journal::DomainFact;
use crate::runtime::transport_boundary::send_guarded_transport_envelope;
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
use zeroize::{Zeroize, ZeroizeOnDrop};
#[cfg(feature = "choreo-backend-telltale-machine")]
use aura_protocol::effects::{ChoreographicRole, RoleIndex};
#[cfg(feature = "choreo-backend-telltale-machine")]
use telltale_machine::StepResult;

mod cache;
mod channel;
mod contact;
mod device_enrollment;
mod exchange;
mod execution;
mod guardian;
mod shareable;
mod validation;
mod vm_loop;

// Re-export types from aura_invitation for public API
pub use aura_invitation::{Invitation, InvitationStatus, InvitationType};
use shareable::StoredImportedInvitation;
pub use shareable::{ShareableInvitation, ShareableInvitationError};

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
    channel_name: Option<String>,
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

#[derive(Debug, Clone, Copy)]
enum CachedInvitationActionValidation {
    Accept { now_ms: u64 },
    Decline,
    Cancel,
}

fn is_generic_contact_invitation(
    sender_id: AuthorityId,
    receiver_id: AuthorityId,
    invitation_type: &InvitationType,
) -> bool {
    matches!(invitation_type, InvitationType::Contact { .. }) && sender_id == receiver_id
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
            aura_authorization::VerifiedBiscuitToken,
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
        let biscuit =
            aura_authorization::VerifiedBiscuitToken::from_bytes(&token_bytes, root_public_key)
                .map_err(|error| {
                    AgentError::effects(format!("parse biscuit token cache: {error}"))
                })?;
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
                match bridge.has_verified_capability_with_time(
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

    async fn validate_cached_invitation_for_action(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        action: CachedInvitationActionValidation,
    ) -> AgentResult<()> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;
        match action {
            CachedInvitationActionValidation::Accept { now_ms } => {
                self.validate_cached_invitation_accept(effects, invitation_id, now_ms)
                    .await
            }
            CachedInvitationActionValidation::Decline => {
                self.validate_cached_invitation_decline(effects, invitation_id)
                    .await
            }
            CachedInvitationActionValidation::Cancel => {
                self.validate_cached_invitation_cancel(effects, invitation_id)
                    .await
            }
        }
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
        receiver_nickname: Option<String>,
        context_override: Option<ContextId>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let prepared = self
            .prepare_invitation_with_context(
                effects.clone(),
                receiver_id,
                invitation_type,
                receiver_nickname,
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
        receiver_nickname: Option<String>,
        context_override: Option<ContextId>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<PreparedInvitation> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;
        let sender_id = self.context.authority.authority_id();

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

        let invitation = Invitation {
            invitation_id: invitation_id.clone(),
            context_id: invitation_context,
            sender_id,
            receiver_id,
            invitation_type,
            status: InvitationStatus::Pending,
            created_at: current_time,
            expires_at,
            message,
            receiver_nickname,
        };

        let deferred_network_effects = if is_generic_contact_invitation(
            invitation.sender_id,
            invitation.receiver_id,
            &invitation.invitation_type,
        ) {
            let fact = InvitationFact::Sent {
                context_id: invitation.context_id,
                invitation_id: invitation.invitation_id.clone(),
                sender_id: invitation.sender_id,
                receiver_id: invitation.receiver_id,
                invitation_type: invitation.invitation_type.clone(),
                sent_at: PhysicalTime {
                    ts_ms: current_time,
                    uncertainty: None,
                },
                expires_at: invitation.expires_at.map(|ts_ms| PhysicalTime {
                    ts_ms,
                    uncertainty: None,
                }),
                receiver_nickname: invitation.receiver_nickname.clone(),
                message: invitation.message.clone(),
            };
            timeout_prepare_invitation_stage(
                effects.as_ref(),
                "commit_generic_contact_invitation_fact",
                execute_journal_append(
                    fact,
                    &self.context.authority,
                    invitation.context_id,
                    effects.as_ref(),
                ),
            )
            .await?;
            DeferredInvitationNetworkEffects::new(Vec::new())
        } else {
            // Build snapshot and prepare through service.
            // For channel invitations this must use the channel context so the
            // generated invitation facts and transport metadata are scoped correctly.
            let snapshot = self
                .build_snapshot_for_context(effects.as_ref(), invitation_context)
                .await;

            let outcome = self.service.prepare_send_invitation(
                &snapshot,
                invitation.receiver_id,
                invitation.invitation_type.clone(),
                invitation.message.clone(),
                expires_in_ms,
                invitation.invitation_id.clone(),
            );

            let execution_plan =
                aura_invitation::guards::plan_send_execution(outcome).map_err(|reason| {
                    AgentError::effects(format!("Guard denied operation: {reason}"))
                })?;
            tracing::debug!(
                authority = %self.context.authority.authority_id(),
                local_effect_count = execution_plan.local_effects.len(),
                deferred_network_effect_count = execution_plan.deferred_network_effects.len(),
                "Prepared invitation guard outcome with deferred network side effects"
            );
            timeout_prepare_invitation_stage(
                effects.as_ref(),
                "execute_local_effects",
                execute_invitation_effect_commands(
                    execution_plan.local_effects,
                    &self.context.authority,
                    effects.as_ref(),
                    false,
                ),
            )
            .await?;
            DeferredInvitationNetworkEffects::new(execution_plan.deferred_network_effects)
        };

        if matches!(invitation.invitation_type, InvitationType::Contact { .. })
            && !is_generic_contact_invitation(
                invitation.sender_id,
                invitation.receiver_id,
                &invitation.invitation_type,
            )
        {
            // Reissuance: we always emit a ContactFact::Added here so the
            // recorded invitation_code reflects the latest invitation the
            // sender issued. The signal-view reducer only overwrites the
            // code when the incoming fact carries one, so re-emits with
            // the new code update the contact record; nickname and other
            // fields are preserved by the reducer on idempotent re-adds.
            let sender_contact_exists = self
                .sender_contact_exists(
                    effects.as_ref(),
                    invitation.sender_id,
                    invitation.receiver_id,
                )
                .await;
            let should_emit_contact_fact = !sender_contact_exists;
            let should_update_code = sender_contact_exists;
            if should_emit_contact_fact || should_update_code {
                // Derive the shareable code from the invitation so the
                // contact record persists the code that was (or will be)
                // exported for this invitation. Errors fall through as
                // None — the contact is still valid without the code.
                let invitation_code =
                    crate::handlers::invitation::shareable::ShareableInvitation::from(&invitation)
                        .to_code()
                        .ok();
                let contact_fact = ContactFact::Added {
                    context_id: invitation.context_id,
                    owner_id: invitation.sender_id,
                    contact_id: invitation.receiver_id,
                    nickname: invitation.receiver_id.to_string(),
                    added_at: PhysicalTime {
                        ts_ms: current_time,
                        uncertainty: None,
                    },
                    invitation_code,
                };

                timeout_prepare_invitation_stage(
                    effects.as_ref(),
                    "commit_sender_contact_fact",
                    self.commit_contact_fact_and_sync_view(
                        effects.as_ref(),
                        invitation.context_id,
                        &contact_fact,
                    ),
                )
                .await?;
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
                if let Err(error) = self
                    .execute_guardian_invitation_principal(effects.clone(), &invitation)
                    .await
                {
                    tracing::warn!(
                        invitation_id = %invitation.invitation_id,
                        receiver = %invitation.receiver_id,
                        error = %error,
                        "Guardian principal choreography did not complete during invitation preparation; continuing with deferred delivery"
                    );
                }
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
                self.validate_cached_invitation_for_action(
                    effects.as_ref(),
                    invitation_id,
                    CachedInvitationActionValidation::Accept { now_ms },
                )
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
            if matches!(
                invitation.invitation_type,
                InvitationType::Contact { .. } | InvitationType::Channel { .. }
            ) {
                tracing::debug!(
                    invitation_id = %invitation_id,
                    invitation_type = ?invitation.invitation_type,
                    "Returning immediately after local invitation acceptance; post-accept notification is best effort"
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

    async fn commit_contact_fact_and_sync_view(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        fact: &ContactFact,
    ) -> AgentResult<()> {
        effects
            .commit_generic_fact_bytes(context_id, CONTACT_FACT_TYPE_ID.into(), fact.to_bytes())
            .await
            .map_err(|error| AgentError::effects(format!("commit contact fact: {error}")))?;
        effects.await_next_view_update().await;
        self.invitation_cache.record_contact_fact(fact).await;
        Ok(())
    }

    async fn materialize_contact_acceptance_if_needed(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        accepted_at_ms: u64,
    ) -> AgentResult<()> {
        // Accepting a contact invitation must materialize sender contact state so
        // CONTACTS_SIGNAL converges from facts rather than UI-local mutation.
        if let Some((contact_id, nickname, invitation_code)) = self
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
                invitation_code,
            };

            tracing::debug!(
                invitation_id = %invitation_id,
                contact_id = %contact_id,
                nickname = %nickname,
                context_id = %context_id,
                "Committing ContactFact::Added for accepted invitation"
            );

            self.commit_contact_fact_and_sync_view(effects, context_id, &fact)
                .await?;

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
    ) -> AgentResult<Option<(AuthorityId, String, Option<String>)>> {
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

        for envelope in envelopes.into_iter().rev() {
            match ChatFact::from_envelope(&envelope) {
                Some(ChatFact::ChannelCreated {
                    context_id: seen_context,
                    channel_id: seen_channel,
                    name,
                    ..
                }) if seen_context == context_id && seen_channel == channel_id => {
                    return Some(name);
                }
                Some(ChatFact::ChannelUpdated {
                    context_id: seen_context,
                    channel_id: seen_channel,
                    name: Some(name),
                    ..
                }) if seen_context == context_id && seen_channel == channel_id => {
                    return Some(name);
                }
                _ => {}
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
            receiver_nickname: None,
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
            invitation.receiver_nickname.as_deref(),
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

        fn promote_transport_hint(transport_hints: &mut Vec<TransportHint>, hint: TransportHint) {
            transport_hints.retain(|existing| existing != &hint);
            transport_hints.insert(0, hint);
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
                promote_transport_hint(&mut transport_hints, hint);
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
                promote_transport_hint(&mut local_descriptor.transport_hints, hint);
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
        self.validate_cached_invitation_for_action(
            effects.as_ref(),
            invitation_id,
            CachedInvitationActionValidation::Decline,
        )
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
        let own_id = self.context.authority.authority_id();

        self.validate_cached_invitation_for_action(
            effects.as_ref(),
            invitation_id,
            CachedInvitationActionValidation::Cancel,
        )
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
                    receiver_nickname: None,
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

#[derive(Zeroize, ZeroizeOnDrop)]
struct DeviceEnrollmentInvitation {
    #[zeroize(skip)]
    subject_authority: AuthorityId,
    #[zeroize(skip)]
    device_id: aura_core::DeviceId,
    #[zeroize(skip)]
    pending_epoch: u64,
    /// Security-sensitive serialized key package carried through device
    /// enrollment. Zeroized on drop.
    key_package: Vec<u8>,
    /// Security-sensitive threshold configuration payload. Zeroized on drop.
    threshold_config: Vec<u8>,
    /// Public package bytes are cleared alongside the rest of the invitation
    /// payload to avoid retaining mixed ceremony material.
    public_key_package: Vec<u8>,
    /// Baseline tree ops can embed serialized device-enrollment material and
    /// are treated as sensitive during invitation handling.
    baseline_tree_ops: Vec<Vec<u8>>,
}

impl fmt::Debug for DeviceEnrollmentInvitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceEnrollmentInvitation")
            .field("subject_authority", &self.subject_authority)
            .field("device_id", &self.device_id)
            .field("pending_epoch", &self.pending_epoch)
            .field("key_package_len", &self.key_package.len())
            .field("key_package", &"<redacted>")
            .field("threshold_config_len", &self.threshold_config.len())
            .field("threshold_config", &"<redacted>")
            .field("public_key_package_len", &self.public_key_package.len())
            .field("baseline_tree_ops_count", &self.baseline_tree_ops.len())
            .field("baseline_tree_ops", &"<redacted>")
            .finish()
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
        let reason = aura_invitation::guards::denial_reason(&outcome);
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
    let execution_plan = aura_invitation::guards::plan_accept_execution(outcome)
        .map_err(|reason| AgentError::effects(format!("Guard denied operation: {reason}")))?;
    tracing::debug!(
        authority = %authority.authority_id(),
        local_effect_count = execution_plan.local_effects.len(),
        deferred_network_effect_count = execution_plan.deferred_network_effects.len(),
        "Prepared invitation accept guard outcome with deferred peer notification side effects"
    );
    execute_invitation_effect_commands(execution_plan.local_effects, authority, effects, false)
        .await?;
    if let Err(error) = execute_invitation_effect_commands(
        execution_plan.deferred_network_effects,
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

async fn seed_peer_descriptor_for_authority_context(
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
    peer: AuthorityId,
) {
    let Some(rendezvous_manager) = effects.rendezvous_manager() else {
        return;
    };

    let authority_context = default_context_id_for_authority(peer);
    if rendezvous_manager
        .get_descriptor(authority_context, peer)
        .await
        .is_some()
    {
        return;
    }

    let local_context_id = authority.default_context_id();
    let existing = rendezvous_manager
        .get_descriptor(local_context_id, peer)
        .await;
    let discovered = rendezvous_manager
        .get_lan_discovered_peer(peer)
        .await
        .map(|peer| peer.descriptor);
    let descriptor =
        match (existing, discovered) {
            (Some(existing), Some(discovered))
                if discovered.transport_hints.iter().any(|hint| {
                    matches!(hint, aura_rendezvous::TransportHint::TcpDirect { .. })
                }) && !existing.transport_hints.iter().any(|hint| {
                    matches!(hint, aura_rendezvous::TransportHint::TcpDirect { .. })
                }) =>
            {
                Some(discovered)
            }
            (Some(existing), _) => Some(existing),
            (None, Some(discovered)) => Some(discovered),
            (None, None) => None,
        };

    let Some(mut descriptor) = descriptor else {
        return;
    };

    descriptor.context_id = authority_context;
    let _ = rendezvous_manager.cache_descriptor(descriptor).await;
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

    seed_peer_descriptor_for_authority_context(authority, effects, peer).await;

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
    tracing::info!(
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

    let transport_receipt = receipt.and_then(|receipt| {
        if receipt.ctx == delivery_context {
            Some(transport_receipt_from_flow(receipt))
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                peer = %peer,
                invitation_context = %invitation_context,
                delivery_context = %delivery_context,
                receipt_context = %receipt.ctx,
                "Dropping invitation transport receipt because delivery uses the authority-scoped peer context"
            );
            None
        }
    });

    let envelope = TransportEnvelope {
        destination: peer,
        source: authority.authority_id(),
        context: delivery_context,
        payload: code.into_bytes(),
        metadata,
        receipt: transport_receipt,
    };

    if best_effort_network_failures {
        if let Err(error) =
            attempt_network_send_envelope(effects, "notify peer with invitation failed", envelope)
                .await
        {
            emit_browser_harness_debug_event("invite_notify_error", &error.to_string());
            return Err(error);
        }
    } else if let Err(error) = send_guarded_transport_envelope(effects, envelope).await {
        emit_browser_harness_debug_event("invite_notify_error", &error.to_string());
        return Err(AgentError::effects(format!(
            "Failed to notify peer with invitation: {error}"
        )));
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
    include!("invitation/tests.rs");
}
