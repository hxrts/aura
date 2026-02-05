//! Invitation Handlers
//!
//! Handlers for invitation-related operations including creating, accepting,
//! and declining invitations for channels, guardians, and contacts.
//!
//! This module uses `aura_invitation::InvitationService` internally for
//! guard chain integration. Types are re-exported from `aura_invitation`.

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{default_context_id_for_authority, AgentError, AgentResult, AuthorityContext};
use crate::runtime::services::InvitationManager;
use crate::runtime::AuraEffectSystem;
use crate::InvitationServiceApi;
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::effects::storage::StorageCoreEffects;
use aura_core::effects::RandomExtendedEffects;
use aura_core::effects::{
    FlowBudgetEffects, TransportEffects, TransportEnvelope, TransportReceipt,
};
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId, InvitationId};
use aura_core::time::PhysicalTime;
use aura_core::FlowCost;
use aura_core::Receipt;
use aura_guards::types::CapabilityId;
use aura_invitation::guards::GuardSnapshot;
use aura_invitation::{InvitationConfig, InvitationService as CoreInvitationService};
use aura_invitation::{InvitationFact, INVITATION_FACT_TYPE_ID};
use aura_invitation::protocol::exchange_runners::{
    execute_as as invitation_execute_as, InvitationExchangeRole,
};
use aura_invitation::protocol::exchange::rumpsteak_session_types_invitation::message_wrappers::{
    InvitationAck as ExchangeInvitationAck,
    InvitationOffer as ExchangeInvitationOffer,
    InvitationResponse as ExchangeInvitationResponse,
};
use aura_invitation::protocol::guardian_runners::{
    execute_as as guardian_execute_as, GuardianInvitationRole,
};
use aura_invitation::protocol::guardian::rumpsteak_session_types_invitation_guardian::message_wrappers::{
    GuardianAccept as GuardianInvitationAccept,
    GuardianConfirm as GuardianInvitationConfirm,
    GuardianRequest as GuardianInvitationRequest,
};
use aura_invitation::protocol::device_enrollment_runners::{
    execute_as as device_enrollment_execute_as, DeviceEnrollmentRole,
};
use aura_invitation::protocol::device_enrollment::rumpsteak_session_types_invitation_device_enrollment::message_wrappers::{
    DeviceEnrollmentAccept as DeviceEnrollmentAcceptWrapper,
    DeviceEnrollmentConfirm as DeviceEnrollmentConfirmWrapper,
    DeviceEnrollmentRequest as DeviceEnrollmentRequestWrapper,
};
use aura_invitation::{
    DeviceEnrollmentAccept, DeviceEnrollmentConfirm, DeviceEnrollmentRequest,
    GuardianAccept, GuardianConfirm, GuardianRequest, InvitationAck, InvitationOffer,
    InvitationResponse,
};
use aura_journal::fact::{FactContent, RelationalFact};
use std::sync::Arc;
use aura_journal::DomainFact;
use aura_protocol::effects::EffectApiEffects;
use aura_protocol::effects::ChoreographyError;
use aura_core::effects::TransportError;
use aura_core::util::serialization::from_slice;
use crate::runtime::choreography_adapter::AuraProtocolAdapter;
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

// Re-export types from aura_invitation for public API
pub use aura_invitation::{Invitation, InvitationStatus, InvitationType};

const CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE: &str =
    "application/aura-contact-invitation-acceptance";

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

struct ChannelBootstrapInvite {
    context_id: ContextId,
    channel_id: ChannelId,
    package: ChannelBootstrapPackage,
}

fn channel_id_from_home_id(home_id: &str) -> ChannelId {
    ChannelId::from_str(home_id).unwrap_or_else(|_| ChannelId::from_bytes(hash(home_id.as_bytes())))
}

/// Invitation handler
///
/// Uses `aura_invitation::InvitationService` for guard chain integration.
pub struct InvitationHandler {
    context: HandlerContext,
    /// Core invitation service from aura_invitation
    service: CoreInvitationService,
    /// Cache of pending invitations (for quick lookup)
    invitation_cache: InvitationManager,
}

impl Clone for InvitationHandler {
    fn clone(&self) -> Self {
        let service =
            CoreInvitationService::new(self.service.authority_id(), self.service.config().clone());
        Self {
            context: self.context.clone(),
            service,
            invitation_cache: self.invitation_cache.clone(),
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
            invitation_cache: InvitationManager::new(),
        })
    }

    fn imported_invitation_key(authority_id: AuthorityId, invitation_id: &InvitationId) -> String {
        format!(
            "{}/{}/{}",
            Self::IMPORTED_INVITATION_STORAGE_PREFIX,
            authority_id.uuid(),
            invitation_id.as_str()
        )
    }

    fn created_invitation_key(authority_id: AuthorityId, invitation_id: &InvitationId) -> String {
        format!(
            "{}/{}/{}",
            Self::CREATED_INVITATION_STORAGE_PREFIX,
            authority_id.uuid(),
            invitation_id.as_str()
        )
    }

    async fn persist_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let key = Self::created_invitation_key(authority_id, &invitation.invitation_id);
        let bytes = serde_json::to_vec(invitation).map_err(|e| {
            crate::core::AgentError::internal(format!("serialize created invitation: {e}"))
        })?;
        effects
            .store(&key, bytes)
            .await
            .map_err(|e| crate::core::AgentError::effects(format!("store invitation: {e}")))?;
        Ok(())
    }

    pub(crate) async fn load_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        let key = Self::created_invitation_key(authority_id, invitation_id);
        let Ok(Some(bytes)) = effects.retrieve(&key).await else {
            return None;
        };
        serde_json::from_slice::<Invitation>(&bytes).ok()
    }

    async fn persist_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        shareable: &ShareableInvitation,
    ) -> AgentResult<()> {
        let key = Self::imported_invitation_key(authority_id, &shareable.invitation_id);
        let bytes = serde_json::to_vec(shareable).map_err(|e| {
            crate::core::AgentError::internal(format!("serialize shareable invitation: {e}"))
        })?;
        effects
            .store(&key, bytes)
            .await
            .map_err(|e| crate::core::AgentError::effects(format!("store invitation: {e}")))?;
        Ok(())
    }

    async fn load_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
    ) -> Option<ShareableInvitation> {
        let key = Self::imported_invitation_key(authority_id, invitation_id);
        let Ok(Some(bytes)) = effects.retrieve(&key).await else {
            return None;
        };
        serde_json::from_slice::<ShareableInvitation>(&bytes).ok()
    }

    /// Get the authority context
    pub fn authority_context(&self) -> &AuthorityContext {
        &self.context.authority
    }

    /// Build a guard snapshot from the current context and effects
    async fn build_snapshot(&self, effects: &AuraEffectSystem) -> GuardSnapshot {
        let now_ms = effects.current_timestamp().await.unwrap_or(0);

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
                CapabilityId::from("invitation:device"),
            ]
        };

        GuardSnapshot::new(
            self.context.authority.authority_id(),
            self.context.effect_context.context_id(),
            FlowCost::new(100), // Default flow budget
            capabilities,
            1, // Default epoch
            now_ms,
        )
    }

    async fn validate_cached_invitation_accept(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        now_ms: u64,
    ) -> AgentResult<()> {
        if let Some(invitation) = self
            .get_invitation_with_storage(effects, invitation_id)
            .await
        {
            tracing::debug!(
                invitation_id = %invitation_id,
                status = ?invitation.status,
                sender = %invitation.sender_id,
                "Validating invitation for accept"
            );

            if !invitation.is_pending() {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    status = ?invitation.status,
                    sender = %invitation.sender_id,
                    "Invitation is not pending"
                );
                return Err(AgentError::invalid(format!(
                    "Invitation {} is not pending (status: {:?}, sender: {})",
                    invitation_id, invitation.status, invitation.sender_id
                )));
            }

            if invitation.is_expired(now_ms) {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    expires_at = ?invitation.expires_at,
                    now_ms = now_ms,
                    "Invitation has expired"
                );
                return Err(AgentError::invalid(format!(
                    "Invitation {} has expired (expires_at: {:?}, now: {})",
                    invitation_id, invitation.expires_at, now_ms
                )));
            }
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "Invitation not found in cache or storage, proceeding anyway"
            );
        }

        Ok(())
    }

    async fn validate_cached_invitation_decline(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        if let Some(invitation) = self
            .get_invitation_with_storage(effects, invitation_id)
            .await
        {
            if !invitation.is_pending() {
                return Err(AgentError::invalid(format!(
                    "Invitation {} is not pending (status: {:?})",
                    invitation_id, invitation.status
                )));
            }
        }

        Ok(())
    }

    async fn validate_cached_invitation_cancel(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        if let Some(invitation) = self
            .get_invitation_with_storage(effects, invitation_id)
            .await
        {
            if !invitation.is_pending() {
                return Err(AgentError::invalid(format!(
                    "Invitation {} is not pending (status: {:?})",
                    invitation_id, invitation.status
                )));
            }

            if invitation.sender_id != self.context.authority.authority_id() {
                return Err(AgentError::invalid(format!(
                    "Only sender can cancel invitation {}",
                    invitation_id
                )));
            }
        }

        Ok(())
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
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Generate unique invitation ID
        let invitation_id =
            InvitationId::new(format!("inv-{}", effects.random_uuid().await.simple()));
        let current_time = effects.current_timestamp().await.unwrap_or(0);
        let expires_at = expires_in_ms.map(|ms| current_time + ms);

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects.as_ref()).await;

        let outcome = self.service.prepare_send_invitation(
            &snapshot,
            receiver_id,
            invitation_type.clone(),
            message.clone(),
            expires_in_ms,
            invitation_id.clone(),
        );

        // Execute the outcome (handles denial and effects)
        execute_guard_outcome(outcome, &self.context.authority, effects.as_ref()).await?;

        let invitation = Invitation {
            invitation_id: invitation_id.clone(),
            context_id: self.context.effect_context.context_id(),
            sender_id: self.context.authority.authority_id(),
            receiver_id,
            invitation_type,
            status: InvitationStatus::Pending,
            created_at: current_time,
            expires_at,
            message,
        };

        // Persist the invitation to storage (so it survives service recreation)
        Self::persist_created_invitation(
            effects.as_ref(),
            self.context.authority.authority_id(),
            &invitation,
        )
        .await?;

        // Cache the pending invitation (for fast lookup within same service instance)
        self.invitation_cache
            .cache_invitation(invitation.clone())
            .await;

        match invitation.invitation_type {
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
            _ => {
                if invitation.receiver_id != invitation.sender_id {
                    self.execute_invitation_exchange_sender(effects.clone(), &invitation)
                        .await?;
                }
            }
        }

        Ok(invitation)
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

        let now_ms = effects.current_timestamp().await.unwrap_or(0);
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

        // Execute the outcome
        execute_guard_outcome(outcome, &self.context.authority, effects.as_ref()).await?;

        // Best-effort: accepting a contact invitation should add the sender as a contact.
        //
        // This needs to be fact-backed so the Contacts reactive view (CONTACTS_SIGNAL)
        // can converge from journal state rather than UI-local mutations.
        if let Some((contact_id, nickname)) = self
            .resolve_contact_invitation(effects.as_ref(), invitation_id)
            .await?
        {
            let now_ms = effects.current_timestamp().await.unwrap_or(0);
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
                .commit_generic_fact_bytes(context_id, CONTACT_FACT_TYPE_ID, fact.to_bytes())
                .await
                .map_err(|e| {
                    crate::core::AgentError::effects(format!("commit contact fact: {e}"))
                })?;

            // Wait for the reactive scheduler to process the committed fact.
            // This ensures the contact appears in the UI before we return "success".
            // Without this, there's a race condition where the TUI refreshes before
            // the scheduler has processed the new ContactFact.
            effects.await_next_view_update().await;

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

        if let Err(e) = self
            .notify_contact_invitation_acceptance(effects.as_ref(), invitation_id)
            .await
        {
            tracing::debug!(
                invitation_id = %invitation_id,
                error = %e,
                "Failed to notify contact invitation acceptance"
            );
        }

        if let Some(channel_invite) = self
            .resolve_channel_invitation(effects.as_ref(), invitation_id)
            .await?
        {
            let ChannelBootstrapPackage { bootstrap_id, key } = channel_invite.package;

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
        }

        // Device enrollment: install share + notify initiator device runtime.
        if let Some(enrollment) = self
            .resolve_device_enrollment_invitation(effects.as_ref(), invitation_id)
            .await?
        {
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

            // Send an acceptance envelope to the initiator device.
            let context_entropy = {
                let mut h = aura_core::hash::hasher();
                h.update(b"DEVICE_ENROLLMENT_CONTEXT");
                h.update(&enrollment.subject_authority.to_bytes());
                h.update(enrollment.ceremony_id.as_str().as_bytes());
                h.finalize()
            };
            let ceremony_context =
                aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

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

            effects.send_envelope(envelope).await.map_err(|e| {
                crate::core::AgentError::effects(format!("send device enrollment acceptance: {e}"))
            })?;
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
            match invitation.invitation_type {
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
                _ => {
                    let _ = self
                        .execute_invitation_exchange_receiver(effects.clone(), &invitation, true)
                        .await;
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

    async fn notify_contact_invitation_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        if effects.is_test_mode() {
            return Ok(());
        }

        let Some(invitation) = self
            .load_invitation_for_choreography(effects, invitation_id)
            .await
        else {
            return Ok(());
        };

        if !matches!(invitation.invitation_type, InvitationType::Contact { .. }) {
            return Ok(());
        }

        let acceptor_id = self.context.authority.authority_id();
        if invitation.sender_id == acceptor_id {
            return Ok(());
        }

        let acceptance = ContactInvitationAcceptance {
            invitation_id: invitation.invitation_id.clone(),
            acceptor_id,
        };
        let payload = serde_json::to_vec(&acceptance).map_err(|e| {
            AgentError::internal(format!("serialize contact invitation acceptance: {e}"))
        })?;

        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE.to_string(),
        );
        metadata.insert("invitation-id".to_string(), invitation.invitation_id.to_string());
        metadata.insert("acceptor-id".to_string(), acceptor_id.to_string());

        let envelope = TransportEnvelope {
            destination: invitation.sender_id,
            source: acceptor_id,
            context: default_context_id_for_authority(invitation.sender_id),
            payload,
            metadata,
            receipt: None,
        };

        effects.send_envelope(envelope).await.map_err(|e| {
            AgentError::effects(format!(
                "send contact invitation acceptance to {}: {e}",
                invitation.sender_id
            ))
        })?;

        Ok(())
    }

    /// Process incoming contact invitation acceptance envelopes.
    pub async fn process_contact_invitation_acceptances(
        &self,
        effects: Arc<AuraEffectSystem>,
    ) -> AgentResult<usize> {
        let mut processed = 0usize;

        loop {
            let envelope = match effects.receive_envelope().await {
                Ok(env) => env,
                Err(TransportError::NoMessage) => break,
                Err(e) => {
                    tracing::warn!("Error receiving contact invitation acceptance: {}", e);
                    break;
                }
            };

            let Some(content_type) = envelope.metadata.get("content-type") else {
                effects.requeue_envelope(envelope);
                break;
            };

            if content_type != CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE {
                effects.requeue_envelope(envelope);
                break;
            }

            let acceptance: ContactInvitationAcceptance =
                match serde_json::from_slice(&envelope.payload) {
                    Ok(data) => data,
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Invalid contact invitation acceptance payload"
                        );
                        continue;
                    }
                };

            if acceptance.acceptor_id == self.context.authority.authority_id() {
                continue;
            }

            let Some(invitation) = Self::load_created_invitation(
                effects.as_ref(),
                self.context.authority.authority_id(),
                &acceptance.invitation_id,
            )
            .await
            else {
                tracing::debug!(
                    invitation_id = %acceptance.invitation_id,
                    "Ignoring acceptance for unknown invitation"
                );
                continue;
            };

            if !matches!(invitation.invitation_type, InvitationType::Contact { .. }) {
                continue;
            }

            if invitation.status == InvitationStatus::Accepted {
                continue;
            }

            let now_ms = effects.current_timestamp().await.unwrap_or(0);
            let context_id = self.context.authority.default_context_id();

            let fact = InvitationFact::accepted_ms(
                acceptance.invitation_id.clone(),
                acceptance.acceptor_id,
                now_ms,
            );
            execute_journal_append(fact, &self.context.authority, context_id, effects.as_ref())
                .await?;

            let contact_fact = ContactFact::Added {
                context_id,
                owner_id: self.context.authority.authority_id(),
                contact_id: acceptance.acceptor_id,
                nickname: acceptance.acceptor_id.to_string(),
                added_at: PhysicalTime {
                    ts_ms: now_ms,
                    uncertainty: None,
                },
            };

            effects
                .commit_generic_fact_bytes(context_id, CONTACT_FACT_TYPE_ID, contact_fact.to_bytes())
                .await
                .map_err(|e| AgentError::effects(format!("commit contact fact: {e}")))?;

            effects.await_next_view_update().await;

            let mut updated = invitation.clone();
            updated.status = InvitationStatus::Accepted;
            Self::persist_created_invitation(
                effects.as_ref(),
                self.context.authority.authority_id(),
                &updated,
            )
            .await?;
            self.invitation_cache.cache_invitation(updated).await;

            processed = processed.saturating_add(1);
        }

        Ok(processed)
    }

    async fn resolve_contact_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<(AuthorityId, String)>> {
        let own_id = self.context.authority.authority_id();

        tracing::debug!(
            invitation_id = %invitation_id,
            own_authority = %own_id,
            "resolve_contact_invitation: starting lookup"
        );

        // First try the local cache (fast path when the same handler instance is reused).
        if let Some(inv) = self.invitation_cache.get_invitation(invitation_id).await {
            tracing::debug!(
                invitation_id = %invitation_id,
                invitation_type = ?inv.invitation_type,
                sender_id = %inv.sender_id,
                "resolve_contact_invitation: found in cache"
            );
            if let InvitationType::Contact { nickname } = &inv.invitation_type {
                let other = if inv.sender_id == own_id {
                    inv.receiver_id
                } else {
                    inv.sender_id
                };
                let nickname = nickname.clone().unwrap_or_else(|| other.to_string());
                tracing::debug!(
                    contact_id = %other,
                    nickname = %nickname,
                    "resolve_contact_invitation: resolved from cache"
                );
                return Ok(Some((other, nickname)));
            }
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "resolve_contact_invitation: not found in cache"
            );
        }

        // Next try the persisted imported invitation store (covers out-of-band imports across
        // handler instances, since AuraAgent constructs services on demand).
        if let Some(shareable) =
            Self::load_imported_invitation(effects, own_id, invitation_id).await
        {
            tracing::debug!(
                invitation_id = %invitation_id,
                invitation_type = ?shareable.invitation_type,
                sender_id = %shareable.sender_id,
                "resolve_contact_invitation: found in persisted store"
            );
            if let InvitationType::Contact { nickname } = shareable.invitation_type {
                if shareable.sender_id != own_id {
                    let other = shareable.sender_id;
                    let nickname = nickname.unwrap_or_else(|| other.to_string());
                    tracing::debug!(
                        contact_id = %other,
                        nickname = %nickname,
                        "resolve_contact_invitation: resolved from persisted store"
                    );
                    return Ok(Some((other, nickname)));
                }
            }
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "resolve_contact_invitation: not found in persisted store"
            );
        }

        // Fallback: attempt to resolve from committed InvitationFact::Sent.
        //
        // This supports in-band invites that arrived via sync and are visible in the journal.
        let Ok(facts) = effects.load_committed_facts(own_id).await else {
            return Ok(None);
        };

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
                receiver_id,
                invitation_type,
                message,
                ..
            } = inv_fact
            else {
                continue;
            };

            if seen_id != *invitation_id {
                continue;
            }

            // Only treat it as a "contact invitation" if the type is Contact.
            if !matches!(
                invitation_type,
                aura_invitation::InvitationType::Contact { .. }
            ) {
                return Ok(None);
            }

            if receiver_id != own_id {
                // Not a received invite; don't derive contact relationship.
                return Ok(None);
            }

            let nickname = message
                .as_deref()
                .and_then(|m| m.split("from ").nth(1))
                .and_then(|s| s.split_whitespace().next())
                .map(|s| s.to_string())
                .unwrap_or_else(|| sender_id.to_string());

            return Ok(Some((sender_id, nickname)));
        }

        Ok(None)
    }

    async fn resolve_device_enrollment_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<DeviceEnrollmentInvitation>> {
        let own_id = self.context.authority.authority_id();

        // First try the local cache (fast path when the same handler instance is reused).
        if let Some(inv) = self.invitation_cache.get_invitation(invitation_id).await {
            if let InvitationType::DeviceEnrollment {
                subject_authority,
                initiator_device_id,
                device_id,
                nickname_suggestion: _,
                ceremony_id,
                pending_epoch,
                key_package,
                threshold_config,
                public_key_package,
            } = &inv.invitation_type
            {
                return Ok(Some(DeviceEnrollmentInvitation {
                    subject_authority: *subject_authority,
                    initiator_device_id: *initiator_device_id,
                    device_id: *device_id,
                    ceremony_id: ceremony_id.clone(),
                    pending_epoch: *pending_epoch,
                    key_package: key_package.clone(),
                    threshold_config: threshold_config.clone(),
                    public_key_package: public_key_package.clone(),
                }));
            }
        }

        // Next try the persisted imported invitation store (covers out-of-band imports across
        // handler instances, since AuraAgent constructs services on demand).
        if let Some(shareable) =
            Self::load_imported_invitation(effects, own_id, invitation_id).await
        {
            if let InvitationType::DeviceEnrollment {
                subject_authority,
                initiator_device_id,
                device_id,
                nickname_suggestion: _,
                ceremony_id,
                pending_epoch,
                key_package,
                threshold_config,
                public_key_package,
            } = shareable.invitation_type
            {
                return Ok(Some(DeviceEnrollmentInvitation {
                    subject_authority,
                    initiator_device_id,
                    device_id,
                    ceremony_id,
                    pending_epoch,
                    key_package,
                    threshold_config,
                    public_key_package,
                }));
            }
        }

        Ok(None)
    }

    async fn resolve_channel_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<ChannelBootstrapInvite>> {
        let own_id = self.context.authority.authority_id();

        if let Some(inv) = self.invitation_cache.get_invitation(invitation_id).await {
            if let InvitationType::Channel {
                home_id,
                nickname_suggestion: _,
                bootstrap: Some(package),
            } = &inv.invitation_type
            {
                return Ok(Some(ChannelBootstrapInvite {
                    context_id: inv.context_id,
                    channel_id: channel_id_from_home_id(home_id),
                    package: package.clone(),
                }));
            }
        }

        if let Some(shareable) =
            Self::load_imported_invitation(effects, own_id, invitation_id).await
        {
            if let InvitationType::Channel {
                home_id,
                nickname_suggestion: _,
                bootstrap: Some(package),
            } = shareable.invitation_type
            {
                return Ok(Some(ChannelBootstrapInvite {
                    context_id: default_context_id_for_authority(shareable.sender_id),
                    channel_id: channel_id_from_home_id(&home_id),
                    package,
                }));
            }
        }

        let Ok(facts) = effects.load_committed_facts(own_id).await else {
            return Ok(None);
        };

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
                sender_id: _,
                receiver_id,
                invitation_type,
                context_id,
                ..
            } = inv_fact
            else {
                continue;
            };

            if seen_id != *invitation_id {
                continue;
            }

            if receiver_id != own_id {
                return Ok(None);
            }

            if let InvitationType::Channel {
                home_id,
                nickname_suggestion: _,
                bootstrap: Some(package),
            } = invitation_type
            {
                return Ok(Some(ChannelBootstrapInvite {
                    context_id,
                    channel_id: channel_id_from_home_id(&home_id),
                    package,
                }));
            }

            return Ok(None);
        }

        Ok(None)
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

        tracing::debug!(
            invitation_id = %shareable.invitation_id,
            sender = %shareable.sender_id,
            invitation_type = ?shareable.invitation_type,
            "Importing invitation code"
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

        let now_ms = effects.current_timestamp().await.unwrap_or(0);
        let context_id = match &shareable.invitation_type {
            InvitationType::Channel { .. } => default_context_id_for_authority(shareable.sender_id),
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
            if !matches!(invitation.invitation_type, InvitationType::Guardian { .. }) {
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
            let now_ms = effects.current_timestamp().await.unwrap_or(0);
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

    fn type_matches(type_name: &str, expected_suffix: &str) -> bool {
        type_name.ends_with(expected_suffix)
    }

    async fn execute_invitation_exchange_sender(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(InvitationExchangeRole::Sender, authority_id);
        role_map.insert(InvitationExchangeRole::Receiver, invitation.receiver_id);

        let offer = ExchangeInvitationOffer(Self::build_invitation_offer(invitation));
        let invitation_id = invitation.invitation_id.clone();

        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            InvitationExchangeRole::Sender,
            role_map,
        )
        .with_message_provider(move |request, received| {
            if Self::type_matches(request.type_name, "InvitationOffer") {
                return Some(Box::new(offer.clone()));
            }

            if Self::type_matches(request.type_name, "InvitationAck") {
                let mut accepted = false;
                for msg in received {
                    if Self::type_matches(msg.type_name, "InvitationResponse") {
                        if let Ok(response) = from_slice::<InvitationResponse>(&msg.bytes) {
                            accepted = response.accepted;
                            break;
                        }
                    }
                }
                let status = if accepted { "accepted" } else { "declined" };
                let ack = ExchangeInvitationAck(InvitationAck {
                    invitation_id: invitation_id.clone(),
                    success: true,
                    status: status.to_string(),
                });
                return Some(Box::new(ack));
            }

            None
        });

        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("invitation start failed: {e}")))?;

        let result = invitation_execute_as(InvitationExchangeRole::Sender, &mut adapter).await;

        let _ = adapter.end_session().await;
        match result {
            Ok(()) => Ok(()),
            Err(err) if Self::is_transport_no_message(&err) => Ok(()),
            Err(err) => Err(AgentError::internal(format!(
                "invitation exchange failed: {err}"
            ))),
        }
    }

    async fn execute_invitation_exchange_receiver(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        accepted: bool,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(InvitationExchangeRole::Sender, invitation.sender_id);
        role_map.insert(InvitationExchangeRole::Receiver, authority_id);

        let response = ExchangeInvitationResponse(InvitationResponse {
            invitation_id: invitation.invitation_id.clone(),
            accepted,
            message: None,
            signature: Vec::new(),
        });
        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            InvitationExchangeRole::Receiver,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if Self::type_matches(request.type_name, "InvitationResponse") {
                return Some(Box::new(response.clone()));
            }
            None
        });

        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("invitation start failed: {e}")))?;

        let result = invitation_execute_as(InvitationExchangeRole::Receiver, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("invitation exchange failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    async fn execute_guardian_invitation_principal(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(GuardianInvitationRole::Principal, authority_id);
        role_map.insert(GuardianInvitationRole::Guardian, invitation.receiver_id);

        let role_description = invitation
            .message
            .clone()
            .unwrap_or_else(|| "guardian invitation".to_string());
        let request = GuardianInvitationRequest(GuardianRequest {
            invitation_id: invitation.invitation_id.clone(),
            principal: authority_id,
            role_description,
            recovery_capabilities: Vec::new(),
            expires_at_ms: invitation.expires_at,
        });
        let invitation_id = invitation.invitation_id.clone();

        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            GuardianInvitationRole::Principal,
            role_map,
        )
        .with_message_provider(move |request_ctx, _received| {
            if Self::type_matches(request_ctx.type_name, "GuardianRequest") {
                return Some(Box::new(request.clone()));
            }

            if Self::type_matches(request_ctx.type_name, "GuardianConfirm") {
                let confirm = GuardianInvitationConfirm(GuardianConfirm {
                    invitation_id: invitation_id.clone(),
                    established: true,
                    relationship_id: None,
                });
                return Some(Box::new(confirm));
            }

            None
        });

        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("guardian invite start failed: {e}")))?;

        let result = guardian_execute_as(GuardianInvitationRole::Principal, &mut adapter).await;

        let _ = adapter.end_session().await;
        match result {
            Ok(()) => Ok(()),
            Err(err) if Self::is_transport_no_message(&err) => Ok(()),
            Err(err) => Err(AgentError::internal(format!(
                "guardian invite failed: {err}"
            ))),
        }
    }

    fn is_transport_no_message(err: &ChoreographyError) -> bool {
        match err {
            ChoreographyError::Transport { source } => source
                .downcast_ref::<TransportError>()
                .is_some_and(|inner| matches!(inner, TransportError::NoMessage)),
            _ => false,
        }
    }

    async fn execute_guardian_invitation_guardian(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(GuardianInvitationRole::Principal, invitation.sender_id);
        role_map.insert(GuardianInvitationRole::Guardian, authority_id);

        let accept = GuardianInvitationAccept(GuardianAccept {
            invitation_id: invitation.invitation_id.clone(),
            signature: Vec::new(),
            recovery_public_key: Vec::new(),
        });
        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            GuardianInvitationRole::Guardian,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if Self::type_matches(request.type_name, "GuardianAccept") {
                return Some(Box::new(accept.clone()));
            }
            None
        });

        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("guardian invite start failed: {e}")))?;

        let result = guardian_execute_as(GuardianInvitationRole::Guardian, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("guardian invite failed: {e}")));

        let _ = adapter.end_session().await;
        result
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
        use crate::core::AgentError;

        let authority_id = self.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(DeviceEnrollmentRole::Initiator, authority_id);
        role_map.insert(DeviceEnrollmentRole::Invitee, invitation.receiver_id);

        // Extract enrollment details from the invitation type
        let (subject_authority, ceremony_id, pending_epoch, device_id) =
            match &invitation.invitation_type {
                InvitationType::DeviceEnrollment {
                    subject_authority,
                    ceremony_id,
                    pending_epoch,
                    device_id,
                    ..
                } => (
                    *subject_authority,
                    ceremony_id.clone(),
                    *pending_epoch,
                    *device_id,
                ),
                _ => {
                    return Err(AgentError::internal(
                        "Expected DeviceEnrollment invitation type".to_string(),
                    ))
                }
            };

        let request = DeviceEnrollmentRequestWrapper(DeviceEnrollmentRequest {
            invitation_id: invitation.invitation_id.clone(),
            subject_authority,
            ceremony_id: ceremony_id.clone(),
            pending_epoch,
            device_id,
        });
        let invitation_id = invitation.invitation_id.clone();
        let ceremony_id_for_confirm = ceremony_id.clone();

        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            DeviceEnrollmentRole::Initiator,
            role_map,
        )
        .with_message_provider(move |request_ctx, _received| {
            if Self::type_matches(request_ctx.type_name, "DeviceEnrollmentRequest") {
                return Some(Box::new(request.clone()));
            }

            if Self::type_matches(request_ctx.type_name, "DeviceEnrollmentConfirm") {
                let confirm = DeviceEnrollmentConfirmWrapper(DeviceEnrollmentConfirm {
                    invitation_id: invitation_id.clone(),
                    ceremony_id: ceremony_id_for_confirm.clone(),
                    established: true,
                    new_epoch: Some(pending_epoch),
                });
                return Some(Box::new(confirm));
            }

            None
        });

        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("device enrollment start failed: {e}")))?;

        let result =
            device_enrollment_execute_as(DeviceEnrollmentRole::Initiator, &mut adapter).await;

        let _ = adapter.end_session().await;
        match result {
            Ok(()) => Ok(()),
            Err(err) if Self::is_transport_no_message(&err) => Ok(()),
            Err(err) => Err(AgentError::internal(format!(
                "device enrollment choreography failed: {err}"
            ))),
        }
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
        use crate::core::AgentError;

        let authority_id = self.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(DeviceEnrollmentRole::Initiator, invitation.sender_id);
        role_map.insert(DeviceEnrollmentRole::Invitee, authority_id);

        // Extract enrollment details from the invitation type
        let (ceremony_id, device_id) = match &invitation.invitation_type {
            InvitationType::DeviceEnrollment {
                ceremony_id,
                device_id,
                ..
            } => (ceremony_id.clone(), *device_id),
            _ => {
                return Err(AgentError::internal(
                    "Expected DeviceEnrollment invitation type".to_string(),
                ))
            }
        };

        let accept = DeviceEnrollmentAcceptWrapper(DeviceEnrollmentAccept {
            invitation_id: invitation.invitation_id.clone(),
            ceremony_id,
            device_id,
        });

        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            DeviceEnrollmentRole::Invitee,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if Self::type_matches(request.type_name, "DeviceEnrollmentAccept") {
                return Some(Box::new(accept.clone()));
            }
            None
        });

        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("device enrollment start failed: {e}")))?;

        let result = device_enrollment_execute_as(DeviceEnrollmentRole::Invitee, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("device enrollment failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    /// Get an invitation by ID (from in-memory cache only)
    pub async fn get_invitation(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        self.invitation_cache.get_invitation(invitation_id).await
    }

    /// Get an invitation by ID, checking both cache and persistent storage
    pub async fn get_invitation_with_storage(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        // First check in-memory cache
        if let Some(inv) = self.invitation_cache.get_invitation(invitation_id).await {
            return Some(inv);
        }

        // Fall back to persistent storage for created invitations
        if let Some(inv) = Self::load_created_invitation(
            effects,
            self.context.authority.authority_id(),
            invitation_id,
        )
        .await
        {
            return Some(inv);
        }

        // Check imported invitations and reconstruct if found
        if let Some(shareable) = Self::load_imported_invitation(
            effects,
            self.context.authority.authority_id(),
            invitation_id,
        )
        .await
        {
            // Reconstruct Invitation from ShareableInvitation
            return Some(Invitation {
                invitation_id: shareable.invitation_id,
                context_id: self.context.effect_context.context_id(),
                sender_id: shareable.sender_id,
                receiver_id: self.context.authority.authority_id(),
                invitation_type: shareable.invitation_type,
                status: InvitationStatus::Pending,
                created_at: 0, // Unknown from shareable
                expires_at: shareable.expires_at,
                message: shareable.message,
            });
        }

        None
    }
}

#[derive(Debug, Clone)]
struct DeviceEnrollmentInvitation {
    subject_authority: AuthorityId,
    initiator_device_id: aura_core::DeviceId,
    device_id: aura_core::DeviceId,
    ceremony_id: aura_core::identifiers::CeremonyId,
    pending_epoch: u64,
    key_package: Vec<u8>,
    threshold_config: Vec<u8>,
    public_key_package: Vec<u8>,
}

// =============================================================================
// Shareable Invitation (Out-of-Band Sharing)
// =============================================================================

/// Error type for shareable invitation operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareableInvitationError {
    /// Invalid invitation code format
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
            Self::InvalidFormat => write!(f, "invalid invitation code format"),
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

    /// Protocol prefix for invitation codes
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
        if parts.len() != 3 {
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
}

impl From<&Invitation> for ShareableInvitation {
    fn from(inv: &Invitation) -> Self {
        Self {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: inv.invitation_id.clone(),
            sender_id: inv.sender_id,
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
        )
        .await?;
    }

    Ok(())
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

async fn execute_effect_command(
    command: aura_invitation::guards::EffectCommand,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
    charge_peer: AuthorityId,
    pending_receipt: &mut Option<Receipt>,
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
        "invitation",
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

    let envelope = TransportEnvelope {
        destination: peer,
        source: authority.authority_id(),
        context: invitation_context,
        payload: code.into_bytes(),
        metadata,
        receipt: receipt.map(transport_receipt_from_flow),
    };

    effects
        .send_envelope(envelope)
        .await
        .map_err(|e| AgentError::effects(format!("Failed to notify peer with invitation: {e}")))?;

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
    operation: String,
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
    let operation_key = operation.replace(' ', "_");
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
    use aura_core::identifiers::{AuthorityId, ContextId, InvitationId};
    use aura_invitation::guards::{EffectCommand, GuardOutcome};
    use aura_journal::fact::{FactContent, RelationalFact};
    use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
    use std::sync::Arc;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
    }

    fn effects_for(authority: &AuthorityContext) -> Arc<AuraEffectSystem> {
        let config = AgentConfig {
            device_id: authority.device_id(),
            ..Default::default()
        };
        Arc::new(AuraEffectSystem::testing(&config).unwrap())
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
        let effects = effects_for(&authority);

        let peer = AuthorityId::new_from_entropy([135u8; 32]);
        let outcome = GuardOutcome::allowed(vec![EffectCommand::NotifyPeer {
            peer,
            invitation_id: InvitationId::new("inv-notify"),
        }]);

        let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_record_receipt() {
        let authority = create_test_authority(136);
        let effects = effects_for(&authority);

        let outcome = GuardOutcome::allowed(vec![EffectCommand::RecordReceipt {
            operation: "send_invitation".to_string(),
            peer: Some(AuthorityId::new_from_entropy([137u8; 32])),
        }]);

        let result = execute_guard_outcome(outcome, &authority, effects.as_ref()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_multiple_commands() {
        let authority = create_test_authority(138);
        let effects = effects_for(&authority);

        let peer = AuthorityId::new_from_entropy([139u8; 32]);
        let outcome = GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: FlowCost::new(1),
            },
            EffectCommand::NotifyPeer {
                peer,
                invitation_id: InvitationId::new("inv-multi"),
            },
            EffectCommand::RecordReceipt {
                operation: "send_invitation".to_string(),
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
        let authority_context = create_test_authority(93);
        let effects = effects_for(&authority_context);
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([94u8; 32]);

        let invitation = handler
            .create_invitation(
                effects.clone(),
                receiver_id,
                InvitationType::Guardian {
                    subject_authority: AuthorityId::new_from_entropy([95u8; 32]),
                },
                None,
                None,
            )
            .await
            .unwrap();

        let result = handler
            .accept_invitation(effects.clone(), &invitation.invitation_id)
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

        let invitation = handler
            .create_invitation(
                effects.clone(),
                receiver_id,
                InvitationType::Channel {
                    home_id: "home-123".to_string(),
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

    #[tokio::test]
    async fn importing_and_accepting_contact_invitation_commits_contact_fact() {
        let own_authority = AuthorityId::new_from_entropy([120u8; 32]);
        let config = AgentConfig::default();
        // Use unique deterministic seed to avoid master key caching issues
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_authority(&config, 10006, own_authority).unwrap(),
        );

        let authority_context = AuthorityContext::new(own_authority);

        let handler = InvitationHandler::new(authority_context).unwrap();

        let sender_id = AuthorityId::new_from_entropy([121u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-demo-contact-1"),
            sender_id,
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
            AuraEffectSystem::simulation_with_shared_transport_for_authority(
                &config,
                20011,
                sender_id,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let receiver_effects = Arc::new(
            AuraEffectSystem::simulation_with_shared_transport_for_authority(
                &config,
                20012,
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
                sender_id,
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

        let processed = sender_handler
            .process_contact_invitation_acceptances(sender_effects.clone())
            .await
            .unwrap();
        assert_eq!(processed, 1);

        let committed = sender_effects.load_committed_facts(sender_id).await.unwrap();

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

        let fact = found.expect("Expected ContactFact from acceptance processing");
        match fact {
            ContactFact::Added {
                owner_id,
                contact_id,
                nickname,
                ..
            } => {
                assert_eq!(owner_id, sender_id);
                assert_eq!(contact_id, receiver_id);
                assert_eq!(nickname, receiver_id.to_string());
            }
            other => panic!("Expected ContactFact::Added, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn imported_invitation_is_resolvable_across_handler_instances() {
        let own_authority = AuthorityId::new_from_entropy([122u8; 32]);
        let config = AgentConfig::default();
        let effects =
            Arc::new(AuraEffectSystem::testing_for_authority(&config, own_authority).unwrap());

        let authority_context = AuthorityContext::new(own_authority);

        let handler_import = InvitationHandler::new(authority_context.clone()).unwrap();
        let handler_accept = InvitationHandler::new(authority_context).unwrap();

        let sender_id = AuthorityId::new_from_entropy([123u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-demo-contact-2"),
            sender_id,
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
        let effects =
            Arc::new(AuraEffectSystem::testing_for_authority(&config, own_authority).unwrap());

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
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-channel-789"),
            sender_id,
            invitation_type: InvitationType::Channel {
                home_id: "home-xyz".to_string(),
                nickname_suggestion: None,
                bootstrap: None,
            },
            expires_at: Some(1800000000000),
            message: Some("Join my channel!".to_string()),
        };

        let code = shareable.to_code();
        let decoded = ShareableInvitation::from_code(&code).unwrap();

        match decoded.invitation_type {
            InvitationType::Channel {
                home_id,
                nickname_suggestion: _,
                bootstrap: _,
            } => {
                assert_eq!(home_id, "home-xyz");
            }
            _ => panic!("wrong invitation type"),
        }
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
        let effects =
            Arc::new(AuraEffectSystem::testing_for_authority(&config, own_authority).unwrap());

        let authority_context = AuthorityContext::new(own_authority);
        let handler = InvitationHandler::new(authority_context).unwrap();

        // Create Alice's invitation (matching DemoHints pattern)
        let alice_sender_id = AuthorityId::new_from_entropy([151u8; 32]);
        let alice_shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("inv-demo-alice-sequential"),
            sender_id: alice_sender_id,
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
