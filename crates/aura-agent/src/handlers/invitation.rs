//! Invitation Handlers
//!
//! Handlers for invitation-related operations including creating, accepting,
//! and declining invitations for channels, guardians, and contacts.
//!
//! This module uses `aura_invitation::InvitationService` internally for
//! guard chain integration. Types are re-exported from `aura_invitation`.

use super::shared::{HandlerContext, HandlerUtilities};
use cache::InvitationCacheHandler;
use channel::InvitationChannelHandler;
use contact::InvitationContactHandler;
use crate::core::{default_context_id_for_authority, AgentError, AgentResult, AuthorityContext};
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
use aura_app::signal_defs::HOMES_SIGNAL;
use aura_app::views::home::{HomeMember, HomeRole, HomeState, HomesState};
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
use aura_core::{execute_with_timeout_budget, TimeoutBudget, TimeoutRunError};
use aura_guards::types::CapabilityId;
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
use aura_journal::fact::{FactContent, RelationalFact};
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use aura_journal::DomainFact;
use aura_protocol::amp::AmpJournalEffects;
use aura_protocol::effects::EffectApiEffects;
use aura_protocol::effects::ChoreographyError;
use aura_core::effects::TransportError;
use aura_core::util::serialization::{from_slice, to_vec};
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
#[cfg(test)]
use std::str::FromStr;
use uuid::Uuid;
use validation::InvitationValidationHandler;
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
const CHAT_FACT_CONTENT_TYPE: &str = "application/aura-chat-fact";
const INVITATION_CONTENT_TYPE: &str = "application/aura-invitation";
const INVITATION_PREPARE_STAGE_TIMEOUT_MS: u64 = 4_000;
const INVITATION_BEST_EFFORT_NETWORK_TIMEOUT_MS: u64 = 2_000;

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
        effects
            .send_envelope(envelope)
            .await
            .map_err(|error| AgentError::effects(format!("{stage}: {error}")))
    })
    .await
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ContactInvitationAcceptance {
    invitation_id: InvitationId,
    acceptor_id: AuthorityId,
}

/// Result of an invitation action
///
/// This type is specific to the agent handler layer, providing a simplified
/// result type for handler operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvitationResult {
    /// Whether the action succeeded
    pub success: bool,
    /// Invitation ID affected
    pub invitation_id: InvitationId,
    /// New status after the action
    pub new_status: Option<InvitationStatus>,
    /// Error message if action failed
    pub error: Option<String>,
}

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

    fn imported_invitation_prefix(authority_id: AuthorityId) -> String {
        InvitationCacheHandler::imported_invitation_prefix(authority_id)
    }

    async fn persist_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationCacheHandler::persist_created_invitation(effects, authority_id, invitation).await
    }

    async fn best_effort_current_timestamp_ms(effects: &AuraEffectSystem) -> u64 {
        if std::env::var_os("AURA_HARNESS_MODE").is_some() {
            let Ok(started_at) = effects.physical_time().await else {
                return 0;
            };
            let Ok(budget) =
                TimeoutBudget::from_start_and_timeout(&started_at, Duration::from_millis(50))
            else {
                return 0;
            };

            return match execute_with_timeout_budget(effects, &budget, || {
                effects.current_timestamp()
            })
            .await
            {
                Ok(value) => value,
                Err(TimeoutRunError::Operation(_)) | Err(TimeoutRunError::Timeout(_)) => 0,
            };
        }

        effects.current_timestamp().await.unwrap_or(0)
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
        shareable: &ShareableInvitation,
    ) -> AgentResult<()> {
        InvitationCacheHandler::persist_imported_invitation(effects, authority_id, shareable).await
    }

    async fn load_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
    ) -> Option<ShareableInvitation> {
        InvitationCacheHandler::load_imported_invitation(effects, authority_id, invitation_id).await
    }

    async fn sender_contact_exists(
        effects: &AuraEffectSystem,
        owner_id: AuthorityId,
        contact_id: AuthorityId,
    ) -> bool {
        let Ok(facts) = effects.load_committed_facts(owner_id).await else {
            return false;
        };

        for fact in facts.iter().rev() {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != CONTACT_FACT_TYPE_ID {
                continue;
            }

            let Some(contact_fact) = ContactFact::from_envelope(envelope) else {
                continue;
            };

            match contact_fact {
                ContactFact::Added {
                    owner_id: seen_owner,
                    contact_id: seen_contact,
                    ..
                } if seen_owner == owner_id && seen_contact == contact_id => {
                    return true;
                }
                ContactFact::Removed {
                    owner_id: seen_owner,
                    contact_id: seen_contact,
                    ..
                } if seen_owner == owner_id && seen_contact == contact_id => {
                    return false;
                }
                _ => {}
            }
        }

        false
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

        // Build capabilities list - in testing mode, grant all capabilities
        let capabilities = if effects.is_testing() {
            vec![
                CapabilityId::from("invitation:send"),
                CapabilityId::from("invitation:accept"),
                CapabilityId::from("invitation:decline"),
                CapabilityId::from("invitation:cancel"),
                CapabilityId::from("invitation:guardian"),
                CapabilityId::from("invitation:channel"),
                CapabilityId::from("invitation:device"),
            ]
        } else {
            // Capabilities will be derived from Biscuit token when integrated.
            // Currently uses default set for non-testing mode.
            vec![
                CapabilityId::from("invitation:send"),
                CapabilityId::from("invitation:accept"),
                CapabilityId::from("invitation:decline"),
                CapabilityId::from("invitation:cancel"),
                CapabilityId::from("invitation:guardian"),
                CapabilityId::from("invitation:channel"),
                CapabilityId::from("invitation:device"),
            ]
        };

        GuardSnapshot::new(
            self.context.authority.authority_id(),
            context_id,
            FlowCost::new(100), // Default flow budget
            capabilities,
            1, // Default epoch
            now_ms,
        )
    }

    /// Build a guard snapshot from the handler's default context.
    async fn build_snapshot(&self, effects: &AuraEffectSystem) -> GuardSnapshot {
        self.build_snapshot_for_context(effects, self.context.effect_context.context_id())
            .await
    }

    /// Resolve the effective invitation context for the outgoing invitation type.
    async fn resolve_invitation_context(
        &self,
        effects: &AuraEffectSystem,
        invitation_type: &InvitationType,
    ) -> ContextId {
        let fallback_context = self.context.effect_context.context_id();
        let InvitationType::Channel { home_id, .. } = invitation_type else {
            return fallback_context;
        };

        let own_id = self.context.authority.authority_id();
        let Ok(facts) = effects.load_committed_facts(own_id).await else {
            return fallback_context;
        };

        for fact in facts.into_iter().rev() {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                continue;
            }

            let Some(ChatFact::ChannelCreated {
                context_id,
                channel_id,
                creator_id,
                ..
            }) = ChatFact::from_envelope(&envelope)
            else {
                continue;
            };

            if channel_id == *home_id && creator_id == own_id {
                return context_id;
            }
        }

        fallback_context
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
                    Ok(self
                        .resolve_invitation_context(effects.as_ref(), &invitation_type)
                        .await)
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
            let sender_contact_exists = Self::sender_contact_exists(
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

                effects
                    .commit_generic_fact_bytes(
                        invitation.context_id,
                        CONTACT_FACT_TYPE_ID.into(),
                        contact_fact.to_bytes(),
                    )
                    .await
                    .map_err(|e| AgentError::effects(format!("commit contact fact: {e}")))?;
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
            InvitationType::DeviceEnrollment { .. } => {
                // For the two-step exchange flow (when invitee has their own authority),
                // run the device enrollment choreography. For legacy self-addressed
                // invitations, skip (invitee will accept via import).
                if invitation.receiver_id != invitation.sender_id {
                    self.execute_device_enrollment_initiator(effects.clone(), &invitation)
                        .await?;
                }
            }
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

        let now_ms = Self::best_effort_current_timestamp_ms(&effects).await;
        self.validate_cached_invitation_accept(effects.as_ref(), invitation_id, now_ms)
            .await?;

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects.as_ref()).await;
        let outcome = self
            .service
            .prepare_accept_invitation(&snapshot, invitation_id);

        tracing::debug!(
            invitation_id = %invitation_id,
            allowed = %outcome.is_allowed(),
            denied = %outcome.is_denied(),
            "Guard outcome for invitation accept"
        );

        // Accept should not be blocked by best-effort budget/notify side effects.
        execute_guard_outcome_for_accept(outcome, &self.context.authority, effects.as_ref())
            .await?;

        // Best-effort: accepting a contact invitation should add the sender as a contact.
        //
        // This needs to be fact-backed so the Contacts reactive view (CONTACTS_SIGNAL)
        // can converge from journal state rather than UI-local mutations.
        if let Some((contact_id, nickname)) = self
            .resolve_contact_invitation(effects.as_ref(), invitation_id)
            .await?
        {
            let now_ms = Self::best_effort_current_timestamp_ms(&effects).await;
            let context_id = self.context.effect_context.context_id();
            let fact = ContactFact::Added {
                context_id,
                owner_id: self.context.authority.authority_id(),
                contact_id,
                nickname: nickname.clone(),
                added_at: PhysicalTime {
                    ts_ms: now_ms,
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

        if let Some(mut channel_invite) = self
            .resolve_channel_invitation(effects.as_ref(), invitation_id)
            .await?
        {
            channel_invite.context_id = self
                .resolve_channel_context_from_chat_facts(effects.as_ref(), &channel_invite)
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
                    effects.as_ref(),
                    &channel_invite,
                    bootstrap_id,
                )
                .await?;
            }

            self.materialize_channel_invitation_acceptance(effects.as_ref(), &channel_invite)
                .await?;
        }

        // Device enrollment: install share + notify initiator device runtime.
        if let Some(enrollment) = self
            .resolve_device_enrollment_invitation(effects.as_ref(), invitation_id)
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

            if let Some(invitation) = self
                .load_invitation_for_choreography(effects.as_ref(), invitation_id)
                .await
            {
                if invitation.receiver_id == invitation.sender_id {
                    // Legacy self-addressed device enrollment still relies on a direct
                    // acceptance envelope because there is no cross-authority invitee
                    // choreography to drive the response.
                    let context_entropy = {
                        let mut h = aura_core::hash::hasher();
                        h.update(b"DEVICE_ENROLLMENT_CONTEXT");
                        h.update(&enrollment.subject_authority.to_bytes());
                        h.update(enrollment.ceremony_id.as_str().as_bytes());
                        h.finalize()
                    };
                    let ceremony_context =
                        aura_core::types::identifiers::ContextId::new_from_entropy(context_entropy);

                    let mut metadata = std::collections::HashMap::new();
                    metadata.insert(
                        "content-type".to_string(),
                        "application/aura-device-enrollment-acceptance".to_string(),
                    );
                    metadata.insert(
                        "ceremony-id".to_string(),
                        enrollment.ceremony_id.to_string(),
                    );
                    metadata.insert(
                        "acceptor-device-id".to_string(),
                        enrollment.device_id.to_string(),
                    );
                    metadata.insert(
                        "aura-destination-device-id".to_string(),
                        enrollment.initiator_device_id.to_string(),
                    );

                    let envelope = aura_core::effects::TransportEnvelope {
                        destination: enrollment.subject_authority,
                        source: self.context.authority.authority_id(),
                        context: ceremony_context,
                        payload: Vec::new(),
                        metadata,
                        receipt: None,
                    };

                    if let Err(error) = attempt_network_send_envelope(
                        &effects,
                        "device enrollment acceptance envelope send failed",
                        envelope,
                    )
                    .await
                    {
                        tracing::warn!(
                            invitation_id = %invitation_id,
                            error = %error,
                            "Device enrollment acceptance envelope send failed; continuing with convergence path"
                        );
                    }
                } else {
                    tracing::debug!(
                        invitation_id = %invitation_id,
                        receiver_id = %invitation.receiver_id,
                        sender_id = %invitation.sender_id,
                        "Skipping legacy direct device enrollment acceptance envelope for addressed invitation"
                    );
                }
            }
        }

        // Update cache if we have this invitation
        let _ = self
            .invitation_cache
            .update_invitation(invitation_id, |inv| {
                inv.status = InvitationStatus::Accepted;
            })
            .await;

        if let Some(invitation) = self
            .load_invitation_for_choreography(effects.as_ref(), invitation_id)
            .await
        {
            if matches!(invitation.invitation_type, InvitationType::Contact { .. }) {
                tracing::debug!(
                    invitation_id = %invitation_id,
                    "Returning immediately after local contact invitation acceptance"
                );
                return Ok(InvitationResult {
                    success: true,
                    invitation_id: invitation_id.clone(),
                    new_status: Some(InvitationStatus::Accepted),
                    error: None,
                });
            }
        }

        if let Some(invitation) = self
            .load_invitation_for_choreography(effects.as_ref(), invitation_id)
            .await
        {
            match invitation.invitation_type {
                InvitationType::Contact { .. } => {
                    tracing::debug!(
                        invitation_id = %invitation_id,
                        "Skipping synchronous invitation exchange receiver for accepted contact invitation"
                    );
                }
                InvitationType::Guardian { .. } => {
                    let _ = self
                        .execute_guardian_invitation_guardian(effects.clone(), &invitation)
                        .await;
                }
                InvitationType::DeviceEnrollment { .. } => {
                    // For the two-step exchange flow (when invitee has their own authority),
                    // run the device enrollment choreography as invitee. For legacy self-addressed
                    // invitations, acceptance was already sent via direct envelope.
                    if invitation.receiver_id != invitation.sender_id {
                        let _ = self
                            .execute_device_enrollment_invitee(effects.clone(), &invitation)
                            .await;
                    }
                }
                InvitationType::Channel { .. } => {
                    if let Err(error) = self
                        .notify_channel_invitation_acceptance(effects.as_ref(), invitation_id)
                        .await
                    {
                        tracing::warn!(
                            invitation_id = %invitation_id,
                            error = %error,
                            "Channel invitation acceptance envelope send failed; continuing with local convergence path"
                        );
                    }
                    tracing::debug!(
                        invitation_id = %invitation_id,
                        "Skipping synchronous invitation exchange receiver for accepted channel invitation"
                    );
                }
            }
        }

        Ok(InvitationResult {
            success: true,
            invitation_id: invitation_id.clone(),
            new_status: Some(InvitationStatus::Accepted),
            error: None,
        })
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

    /// Process incoming invitation-related envelopes.
    pub async fn process_contact_invitation_acceptances(
        &self,
        effects: Arc<AuraEffectSystem>,
    ) -> AgentResult<usize> {
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

    async fn channel_created_fact_exists(
        &self,
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        context_id: ContextId,
        channel_id: ChannelId,
    ) -> bool {
        let Ok(facts) = effects.load_committed_facts(authority_id).await else {
            return false;
        };

        for fact in facts.into_iter().rev() {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                continue;
            }

            let Some(ChatFact::ChannelCreated {
                context_id: seen_context,
                channel_id: seen_channel,
                ..
            }) = ChatFact::from_envelope(&envelope)
            else {
                continue;
            };

            if seen_context == context_id && seen_channel == channel_id {
                return true;
            }
        }

        false
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

    async fn materialize_home_signal_for_channel_invitation(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
    ) -> AgentResult<()> {
        use aura_effects::ReactiveEffects;

        let reactive = effects.reactive_handler();
        let mut homes: HomesState = match reactive.read(&*HOMES_SIGNAL).await {
            Ok(state) => state,
            Err(_) => {
                let _ = reactive
                    .register(&*HOMES_SIGNAL, HomesState::default())
                    .await;
                reactive.read(&*HOMES_SIGNAL).await.unwrap_or_default()
            }
        };

        let own_id = self.context.authority.authority_id();
        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
        let mut changed = false;

        if !homes.has_home(&invite.channel_id) {
            let mut home = HomeState::new(
                invite.channel_id,
                Some(invite.home_name.clone()),
                invite.sender_id,
                now_ms,
                invite.context_id,
            );

            if invite.sender_id != own_id {
                if let Some(owner) = home.member_mut(&invite.sender_id) {
                    owner.name = invite.sender_id.to_string();
                    owner.is_online = false;
                    owner.last_seen = Some(now_ms);
                }
                home.my_role = HomeRole::Participant;
            }

            if home.member(&own_id).is_none() {
                home.add_member(HomeMember {
                    id: own_id,
                    name: "You".to_string(),
                    role: HomeRole::Participant,
                    is_online: true,
                    joined_at: now_ms,
                    last_seen: Some(now_ms),
                    storage_allocated: HomeState::MEMBER_ALLOCATION,
                });
            }

            homes.add_home(home);
            if homes.current_home_id().is_none() {
                homes.select_home(Some(invite.channel_id));
            }
            changed = true;
        } else if let Some(home) = homes.home_mut(&invite.channel_id) {
            if home.context_id != Some(invite.context_id) {
                home.context_id = Some(invite.context_id);
                changed = true;
            }

            if invite.sender_id != own_id && home.member(&own_id).is_none() {
                home.add_member(HomeMember {
                    id: own_id,
                    name: "You".to_string(),
                    role: HomeRole::Participant,
                    is_online: true,
                    joined_at: now_ms,
                    last_seen: Some(now_ms),
                    storage_allocated: HomeState::MEMBER_ALLOCATION,
                });
                changed = true;
            }

            if invite.sender_id != own_id && matches!(home.my_role, HomeRole::Member) {
                home.my_role = HomeRole::Participant;
                changed = true;
            }
        }

        if !changed {
            return Ok(());
        }

        if homes.current_home_id().is_none() && homes.has_home(&invite.channel_id) {
            homes.select_home(Some(invite.channel_id));
        }

        reactive
            .emit(&*HOMES_SIGNAL, homes)
            .await
            .map_err(|e| AgentError::effects(format!("emit homes signal: {e}")))?;

        Ok(())
    }

    async fn materialize_home_signal_for_channel_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        use aura_effects::ReactiveEffects;

        let InvitationType::Channel {
            home_id,
            nickname_suggestion,
            ..
        } = &invitation.invitation_type
        else {
            return Ok(());
        };

        let reactive = effects.reactive_handler();
        let mut homes: HomesState = match reactive.read(&*HOMES_SIGNAL).await {
            Ok(state) => state,
            Err(_) => {
                let _ = reactive
                    .register(&*HOMES_SIGNAL, HomesState::default())
                    .await;
                reactive.read(&*HOMES_SIGNAL).await.unwrap_or_default()
            }
        };

        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
        let mut changed = false;
        let home_name = nickname_suggestion
            .clone()
            .unwrap_or_else(|| home_id.to_string());

        if !homes.has_home(home_id) {
            let mut home = HomeState::new(
                *home_id,
                Some(home_name),
                invitation.sender_id,
                now_ms,
                invitation.context_id,
            );
            if home.member(&invitation.receiver_id).is_none() {
                home.add_member(HomeMember {
                    id: invitation.receiver_id,
                    name: invitation.receiver_id.to_string(),
                    role: HomeRole::Participant,
                    is_online: false,
                    joined_at: now_ms,
                    last_seen: Some(now_ms),
                    storage_allocated: HomeState::MEMBER_ALLOCATION,
                });
            }
            homes.add_home(home);
            changed = true;
        } else if let Some(home) = homes.home_mut(home_id) {
            if home.context_id != Some(invitation.context_id) {
                home.context_id = Some(invitation.context_id);
                changed = true;
            }
            if home.member(&invitation.receiver_id).is_none() {
                home.add_member(HomeMember {
                    id: invitation.receiver_id,
                    name: invitation.receiver_id.to_string(),
                    role: HomeRole::Participant,
                    is_online: false,
                    joined_at: now_ms,
                    last_seen: Some(now_ms),
                    storage_allocated: HomeState::MEMBER_ALLOCATION,
                });
                changed = true;
            }
        }

        if changed {
            reactive
                .emit(&*HOMES_SIGNAL, homes)
                .await
                .map_err(|e| AgentError::effects(format!("emit homes signal: {e}")))?;
        }

        Ok(())
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

        // Persist the shareable invitation so later operations (accept/decline) can resolve it
        // even if AuraAgent constructs a fresh InvitationService/InvitationHandler.
        Self::persist_imported_invitation(
            effects,
            self.context.authority.authority_id(),
            &shareable,
        )
        .await?;

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

        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
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
            InvitationType::Channel { .. } => shareable
                .context_id
                .unwrap_or_else(|| default_context_id_for_authority(shareable.sender_id)),
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

        self.invitation_cache
            .cache_invitation(invitation.clone())
            .await;

        Ok(invitation)
    }

    pub(super) async fn cache_peer_descriptor_for_peer(
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
                valid_until: existing.valid_until.max(now_ms.saturating_add(86_400_000)),
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
                valid_until: now_ms.saturating_add(86_400_000),
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
                    valid_until: now_ms.saturating_add(86_400_000),
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
                .max(now_ms.saturating_add(86_400_000));
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
                let _ = self
                    .execute_invitation_exchange_receiver(effects.clone(), &invitation, false)
                    .await;
            }
        }

        Ok(InvitationResult {
            success: true,
            invitation_id: invitation_id.clone(),
            new_status: Some(InvitationStatus::Declined),
            error: None,
        })
    }

    /// Cancel an invitation (sender only)
    pub async fn cancel_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        self.validate_cached_invitation_cancel(effects, invitation_id)
            .await?;

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects).await;
        let outcome = self
            .service
            .prepare_cancel_invitation(&snapshot, invitation_id);

        // Execute the outcome
        execute_guard_outcome(outcome, &self.context.authority, effects).await?;

        // Remove from cache
        let _ = self.invitation_cache.remove_invitation(invitation_id).await;

        Ok(InvitationResult {
            success: true,
            invitation_id: invitation_id.clone(),
            new_status: Some(InvitationStatus::Cancelled),
            error: None,
        })
    }

    /// List pending invitations (from cache)
    pub async fn list_pending(&self) -> Vec<Invitation> {
        self.invitation_cache
            .list_pending(|inv| inv.status == InvitationStatus::Pending)
            .await
    }

    /// List pending invitations from cache plus persisted stores.
    ///
    /// This allows runtime components using separate handler instances to
    /// converge on a shared pending invitation view.
    pub async fn list_pending_with_storage(&self, effects: &AuraEffectSystem) -> Vec<Invitation> {
        let mut pending = self.list_pending().await;
        let mut seen: HashSet<InvitationId> = pending
            .iter()
            .map(|inv| inv.invitation_id.clone())
            .collect();
        let own_id = self.context.authority.authority_id();
        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;

        let imported_prefix = Self::imported_invitation_prefix(own_id);
        if let Ok(keys) = effects.list_keys(Some(&imported_prefix)).await {
            for key in keys {
                let Ok(Some(bytes)) = effects.retrieve(&key).await else {
                    continue;
                };
                let Ok(shareable) = serde_json::from_slice::<ShareableInvitation>(&bytes) else {
                    continue;
                };
                if !seen.insert(shareable.invitation_id.clone()) {
                    continue;
                }

                let context_id = match &shareable.invitation_type {
                    InvitationType::Channel { .. } => shareable
                        .context_id
                        .unwrap_or_else(|| default_context_id_for_authority(shareable.sender_id)),
                    _ => self.context.effect_context.context_id(),
                };

                let invitation = Invitation {
                    invitation_id: shareable.invitation_id,
                    context_id,
                    sender_id: shareable.sender_id,
                    receiver_id: own_id,
                    invitation_type: shareable.invitation_type,
                    status: InvitationStatus::Pending,
                    created_at: now_ms,
                    expires_at: shareable.expires_at,
                    message: shareable.message,
                };

                self.invitation_cache
                    .cache_invitation(invitation.clone())
                    .await;
                pending.push(invitation);
            }
        }

        pending
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

        if let Some(shareable) =
            Self::load_imported_invitation(effects, own_id, invitation_id).await
        {
            let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
            return Some(Invitation {
                invitation_id: shareable.invitation_id,
                context_id: self.context.effect_context.context_id(),
                sender_id: shareable.sender_id,
                receiver_id: own_id,
                invitation_type: shareable.invitation_type,
                status: InvitationStatus::Pending,
                created_at: now_ms,
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

        let result = async {
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
                to_vec(&offer).map_err(|error| {
                    AgentError::internal(format!("offer encode failed: {error}"))
                })?,
            );

            let loop_result = loop {
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
                        self.materialize_home_signal_for_channel_acceptance(
                            effects.as_ref(),
                            invitation,
                        )
                        .await?;
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
            };

            let _ = session.close().await;
            loop_result
        }
        .await;
        result
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

        let result = async {
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

            let loop_result = loop {
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
            };

            let _ = session.close().await;
            loop_result
        }
        .await;
        result
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
    async fn execute_device_enrollment_initiator(
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
    async fn execute_device_enrollment_invitee(
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
    initiator_device_id: aura_core::DeviceId,
    device_id: aura_core::DeviceId,
    ceremony_id: aura_core::types::identifiers::CeremonyId,
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
}

impl std::fmt::Display for ShareableInvitationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "invalid invite code format"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version: {}", v),
            Self::DecodingFailed => write!(f, "base64 decoding failed"),
            Self::ParsingFailed => write!(f, "JSON parsing failed"),
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
/// let code = shareable.to_code();
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

impl ShareableInvitation {
    /// Current version of the shareable invitation format
    pub const CURRENT_VERSION: u8 = 1;

    /// Protocol prefix for invite codes
    pub const PREFIX: &'static str = "aura";

    /// Encode the invitation as a shareable code string
    ///
    /// Format: `aura:v1:<base64-encoded-json>`
    pub fn to_code(&self) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let json = serde_json::to_vec(self).expect("serialization should not fail");
        let b64 = URL_SAFE_NO_PAD.encode(&json);
        format!("{}:v{}:{}", Self::PREFIX, self.version, b64)
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
    let charge_peer = resolve_charge_peer(&outcome.effects, authority.authority_id());
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
        split_invitation_send_guard_outcome(outcome, authority)?;
    execute_invitation_effect_commands(local_effects, authority, effects, false).await?;
    execute_invitation_effect_commands(deferred_network_effects.commands, authority, effects, true)
        .await
}

fn resolve_charge_peer(
    commands: &[aura_invitation::guards::EffectCommand],
    fallback: AuthorityId,
) -> AuthorityId {
    commands
        .iter()
        .find_map(|command| match command {
            aura_invitation::guards::EffectCommand::NotifyPeer { peer, .. } => Some(*peer),
            aura_invitation::guards::EffectCommand::RecordReceipt { peer, .. } => *peer,
            _ => None,
        })
        .unwrap_or(fallback)
}

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
    let charge_peer = resolve_charge_peer(&commands, authority.authority_id());
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
    if effects.is_testing() {
        return Ok(None);
    }

    let receipt = effects
        .charge_flow(&context_id, &peer, cost)
        .await
        .map_err(|e| AgentError::effects(format!("Failed to charge invitation flow: {e}")))?;
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
    if effects.is_test_mode() {
        return Ok(());
    }

    if peer == authority.authority_id() {
        // Self-addressed invitations are intended for out-of-band sharing.
        // Skip network notify when inviting ourselves.
        return Ok(());
    }

    let authority_id = authority.authority_id();
    let (code, invitation_context) = if let Some(invitation) =
        InvitationHandler::load_created_invitation(effects, authority_id, &invitation_id).await
    {
        (
            InvitationServiceApi::export_invitation(&invitation),
            invitation.context_id,
        )
    } else {
        let facts = effects
            .load_committed_facts(authority_id)
            .await
            .map_err(|_| {
                AgentError::context(format!("Invitation not found for notify: {invitation_id}"))
            })?;

        let mut shareable: Option<(ShareableInvitation, ContextId)> = None;
        for fact in facts.iter().rev() {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != INVITATION_FACT_TYPE_ID {
                continue;
            }

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

        (shareable.to_code(), context_id)
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

    let envelope = TransportEnvelope {
        destination: peer,
        source: authority.authority_id(),
        context: invitation_context,
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
    use crate::runtime::effects::AuraEffectSystem;
    use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
    use aura_core::types::identifiers::{
        AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId,
    };
    use aura_core::DeviceId;
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

    #[track_caller]
    fn effects_for(authority: &AuthorityContext) -> Arc<AuraEffectSystem> {
        let config = AgentConfig {
            device_id: authority.device_id(),
            ..Default::default()
        };
        Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, authority.authority_id())
                .unwrap(),
        )
    }

    fn canonical_home_id(seed: u8) -> ChannelId {
        ChannelId::from_bytes([seed; 32])
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
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                authority.authority_id(),
                shared_transport.clone(),
            )
            .unwrap(),
        );
        // Materialize a destination participant on the shared transport.
        let _peer_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                peer,
                shared_transport,
            )
            .unwrap(),
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
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                authority.authority_id(),
                shared_transport.clone(),
            )
            .unwrap(),
        );
        // Materialize a destination participant on the shared transport.
        let _peer_effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                peer,
                shared_transport,
            )
            .unwrap(),
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
        let code = InvitationServiceApi::export_invitation(&invitation);
        let imported = receiver_handler
            .import_invitation_code(&receiver_effects, &code)
            .await
            .unwrap();

        let result = receiver_handler
            .accept_invitation(receiver_effects, &imported.invitation_id)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, Some(InvitationStatus::Accepted));
    }

    #[tokio::test]
    async fn invitation_can_be_declined() {
        let authority_context = create_test_authority(96);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([97u8; 32]);
        let home_id = canonical_home_id(11);

        let invitation = handler
            .create_invitation(
                effects.clone(),
                receiver_id,
                InvitationType::Channel {
                    home_id,
                    nickname_suggestion: None,
                    bootstrap: None,
                },
                None,
                None,
            )
            .await
            .unwrap();

        let result = handler
            .decline_invitation(effects.clone(), &invitation.invitation_id)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, Some(InvitationStatus::Declined));
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
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, own_authority).unwrap(),
        );

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
        let code = shareable.to_code();

        let imported = handler
            .import_invitation_code(&effects, &code)
            .await
            .unwrap();
        assert_eq!(imported.sender_id, sender_id);
        assert_eq!(imported.receiver_id, own_authority);

        handler
            .accept_invitation(effects.clone(), &imported.invitation_id)
            .await
            .unwrap();

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
                Some("Contact invitation from sender".to_string()),
                None,
            )
            .await
            .unwrap();

        let code = InvitationServiceApi::export_invitation(&invitation);
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
        assert_eq!(processed, 1);

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

        let code = InvitationServiceApi::export_invitation(&invitation);
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
        assert_eq!(processed, 1);
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

        let receiver_handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

        let invitation_id = InvitationId::new("inv-envelope-home-1");
        let home_id = canonical_home_id(12);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation_id.clone(),
            sender_id,
            context_id: None,
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
                payload: shareable.to_code().into_bytes(),
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
    }

    #[tokio::test]
    async fn accepting_channel_invitation_materializes_home_and_channel_state() {
        let sender_id = AuthorityId::new_from_entropy([213u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([214u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, receiver_id).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

        let invitation_id = InvitationId::new("inv-materialize-home-1");
        let home_id = canonical_home_id(13);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation_id.clone(),
            sender_id,
            context_id: None,
            invitation_type: InvitationType::Channel {
                home_id,
                nickname_suggestion: Some("Oak House".to_string()),
                bootstrap: None,
            },
            expires_at: None,
            message: Some("Join Oak House".to_string()),
        };

        let imported = handler
            .import_invitation_code(effects.as_ref(), &shareable.to_code())
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
            .unwrap_or_default();
        let home = homes
            .home_state(&expected_channel)
            .expect("accepted invitation should materialize home state");
        assert_eq!(home.context_id, Some(expected_context));
        assert!(home.member(&receiver_id).is_some());
        assert_eq!(home.my_role, HomeRole::Participant);
    }

    #[tokio::test]
    async fn accepting_channel_invitation_materializes_amp_bootstrap_state() {
        let sender_id = AuthorityId::new_from_entropy([217u8; 32]);
        let receiver_id = AuthorityId::new_from_entropy([218u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, receiver_id).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

        let invitation_id = InvitationId::new("inv-materialize-bootstrap-1");
        let home_id = canonical_home_id(14);
        let bootstrap_key = [7u8; 32];
        let bootstrap_id = Hash32::from_bytes(&bootstrap_key);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: invitation_id.clone(),
            sender_id,
            context_id: None,
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
            .import_invitation_code(effects.as_ref(), &shareable.to_code())
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
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, receiver_id).unwrap(),
        );
        let handler = InvitationHandler::new(AuthorityContext::new(receiver_id)).unwrap();

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
            .import_invitation_code(effects.as_ref(), &shareable.to_code())
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
            .unwrap_or_default();
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
        let code = shareable.to_code();

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
            .cancel_invitation(&effects, &invitation.invitation_id)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, Some(InvitationStatus::Cancelled));

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

        // Accept one, decline another
        handler
            .accept_invitation(effects.clone(), &inv1.invitation_id)
            .await
            .unwrap();
        handler
            .decline_invitation(effects.clone(), &inv2.invitation_id)
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

        let code = shareable.to_code();
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

        let code = shareable.to_code();
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

        let code = shareable.to_code();
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

        let code = shareable.to_code();
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
        let base = shareable.to_code();
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
        let code = shareable.to_code();
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
        let alice_code = alice_shareable.to_code();

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
        let carol_code = carol_shareable.to_code();

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
