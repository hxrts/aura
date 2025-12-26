//! Invitation Handlers
//!
//! Handlers for invitation-related operations including creating, accepting,
//! and declining invitations for channels, guardians, and contacts.
//!
//! This module uses `aura_invitation::InvitationService` internally for
//! guard chain integration. Types are re-exported from `aura_invitation`.

use super::invitation_bridge::execute_guard_outcome;
use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::storage::StorageEffects;
use aura_core::effects::RandomEffects;
use aura_core::effects::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation, TransportEffects,
};
use aura_core::identifiers::AuthorityId;
use aura_core::time::PhysicalTime;
use aura_invitation::guards::GuardSnapshot;
use aura_invitation::{InvitationConfig, InvitationService as CoreInvitationService};
use aura_invitation::{InvitationFact, INVITATION_FACT_TYPE_ID};
use aura_journal::fact::{FactContent, RelationalFact};
use aura_journal::DomainFact;
use aura_protocol::effects::EffectApiEffects;
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// Re-export types from aura_invitation for public API
pub use aura_invitation::{Invitation, InvitationStatus, InvitationType};

/// Result of an invitation action
///
/// This type is specific to the agent handler layer, providing a simplified
/// result type for handler operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvitationResult {
    /// Whether the action succeeded
    pub success: bool,
    /// Invitation ID affected
    pub invitation_id: String,
    /// New status after the action
    pub new_status: Option<InvitationStatus>,
    /// Error message if action failed
    pub error: Option<String>,
}

/// Invitation handler
///
/// Uses `aura_invitation::InvitationService` for guard chain integration.
pub struct InvitationHandler {
    context: HandlerContext,
    /// Core invitation service from aura_invitation
    service: CoreInvitationService,
    /// Cache of pending invitations (for quick lookup)
    pending_invitations: Arc<RwLock<HashMap<String, Invitation>>>,
}

impl InvitationHandler {
    const IMPORTED_INVITATION_STORAGE_PREFIX: &'static str = "invitation/imported";
    const CREATED_INVITATION_STORAGE_PREFIX: &'static str = "invitation/created";

    /// Create a new invitation handler
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        let service =
            CoreInvitationService::new(authority.authority_id, InvitationConfig::default());

        Ok(Self {
            context: HandlerContext::new(authority),
            service,
            pending_invitations: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    fn imported_invitation_key(authority_id: AuthorityId, invitation_id: &str) -> String {
        format!(
            "{}/{}/{}",
            Self::IMPORTED_INVITATION_STORAGE_PREFIX,
            authority_id.uuid(),
            invitation_id
        )
    }

    fn created_invitation_key(authority_id: AuthorityId, invitation_id: &str) -> String {
        format!(
            "{}/{}/{}",
            Self::CREATED_INVITATION_STORAGE_PREFIX,
            authority_id.uuid(),
            invitation_id
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

    async fn load_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &str,
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
        invitation_id: &str,
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
                "invitation:send".to_string(),
                "invitation:accept".to_string(),
                "invitation:decline".to_string(),
                "invitation:cancel".to_string(),
                "invitation:guardian".to_string(),
                "invitation:channel".to_string(),
                "invitation:device".to_string(),
            ]
        } else {
            // Capabilities will be derived from Biscuit token when integrated.
            // Currently uses default set for non-testing mode.
            vec![
                "invitation:send".to_string(),
                "invitation:accept".to_string(),
                "invitation:decline".to_string(),
                "invitation:cancel".to_string(),
                "invitation:device".to_string(),
            ]
        };

        GuardSnapshot::new(
            self.context.authority.authority_id,
            self.context.effect_context.context_id(),
            100, // Default flow budget
            capabilities,
            1, // Default epoch
            now_ms,
        )
    }

    async fn validate_cached_invitation_accept(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &str,
        now_ms: u64,
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

            if invitation.is_expired(now_ms) {
                return Err(AgentError::invalid(format!(
                    "Invitation {} has expired",
                    invitation_id
                )));
            }
        }

        Ok(())
    }

    async fn validate_cached_invitation_decline(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &str,
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
        invitation_id: &str,
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

            if invitation.sender_id != self.context.authority.authority_id {
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
        effects: &AuraEffectSystem,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Generate unique invitation ID
        let invitation_id = format!("inv-{}", effects.random_uuid().await.simple());
        let current_time = effects.current_timestamp().await.unwrap_or(0);
        let expires_at = expires_in_ms.map(|ms| current_time + ms);

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects).await;

        let outcome = self.service.prepare_send_invitation(
            &snapshot,
            receiver_id,
            invitation_type.clone(),
            message.clone(),
            expires_in_ms,
            invitation_id.clone(),
        );

        // Execute the outcome (handles denial and effects)
        execute_guard_outcome(outcome, &self.context.authority, effects).await?;

        let invitation = Invitation {
            invitation_id: invitation_id.clone(),
            context_id: self.context.effect_context.context_id(),
            sender_id: self.context.authority.authority_id,
            receiver_id,
            invitation_type,
            status: InvitationStatus::Pending,
            created_at: current_time,
            expires_at,
            message,
        };

        // Persist the invitation to storage (so it survives service recreation)
        Self::persist_created_invitation(effects, self.context.authority.authority_id, &invitation)
            .await?;

        // Cache the pending invitation (for fast lookup within same service instance)
        {
            let mut cache = self.pending_invitations.write().await;
            cache.insert(invitation_id, invitation.clone());
        }

        Ok(invitation)
    }

    /// Accept an invitation
    pub async fn accept_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &str,
    ) -> AgentResult<InvitationResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        let now_ms = effects.current_timestamp().await.unwrap_or(0);
        self.validate_cached_invitation_accept(effects, invitation_id, now_ms)
            .await?;

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects).await;
        let outcome = self
            .service
            .prepare_accept_invitation(&snapshot, invitation_id);

        // Execute the outcome
        execute_guard_outcome(outcome, &self.context.authority, effects).await?;

        // Best-effort: accepting a contact invitation should add the sender as a contact.
        //
        // This needs to be fact-backed so the Contacts reactive view (CONTACTS_SIGNAL)
        // can converge from journal state rather than UI-local mutations.
        if let Some((contact_id, nickname)) = self
            .resolve_contact_invitation(effects, invitation_id)
            .await?
        {
            let now_ms = effects.current_timestamp().await.unwrap_or(0);
            let context_id = self.context.effect_context.context_id();
            let fact = ContactFact::Added {
                context_id,
                owner_id: self.context.authority.authority_id,
                contact_id,
                nickname,
                added_at: PhysicalTime {
                    ts_ms: now_ms,
                    uncertainty: None,
                },
            };

            effects
                .commit_generic_fact_bytes(context_id, CONTACT_FACT_TYPE_ID, fact.to_bytes())
                .await
                .map_err(|e| {
                    crate::core::AgentError::effects(format!("commit contact fact: {e}"))
                })?;
        }

        // Device enrollment: install share + notify initiator device runtime.
        if let Some(enrollment) = self
            .resolve_device_enrollment_invitation(effects, invitation_id)
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
                h.update(enrollment.ceremony_id.as_bytes());
                h.finalize()
            };
            let ceremony_context =
                aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                "application/aura-device-enrollment-acceptance".to_string(),
            );
            metadata.insert("ceremony-id".to_string(), enrollment.ceremony_id.clone());
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
                source: self.context.authority.authority_id,
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
        {
            let mut cache = self.pending_invitations.write().await;
            if let Some(inv) = cache.get_mut(invitation_id) {
                inv.status = InvitationStatus::Accepted;
            }
        }

        Ok(InvitationResult {
            success: true,
            invitation_id: invitation_id.to_string(),
            new_status: Some(InvitationStatus::Accepted),
            error: None,
        })
    }

    async fn resolve_contact_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &str,
    ) -> AgentResult<Option<(AuthorityId, String)>> {
        let own_id = self.context.authority.authority_id;

        // First try the local cache (fast path when the same handler instance is reused).
        {
            let cache = self.pending_invitations.read().await;
            if let Some(inv) = cache.get(invitation_id) {
                if let InvitationType::Contact { nickname } = &inv.invitation_type {
                    let other = if inv.sender_id == own_id {
                        inv.receiver_id
                    } else {
                        inv.sender_id
                    };
                    let nickname = nickname.clone().unwrap_or_else(|| other.to_string());
                    return Ok(Some((other, nickname)));
                }
            }
        }

        // Next try the persisted imported invitation store (covers out-of-band imports across
        // handler instances, since AuraAgent constructs services on demand).
        if let Some(shareable) =
            Self::load_imported_invitation(effects, own_id, invitation_id).await
        {
            if let InvitationType::Contact { nickname } = shareable.invitation_type {
                if shareable.sender_id != own_id {
                    let other = shareable.sender_id;
                    let nickname = nickname.unwrap_or_else(|| other.to_string());
                    return Ok(Some((other, nickname)));
                }
            }
        }

        // Fallback: attempt to resolve from committed InvitationFact::Sent.
        //
        // This supports in-band invites that arrived via sync and are visible in the journal.
        let Ok(facts) = effects.load_committed_facts(own_id).await else {
            return Ok(None);
        };

        for fact in facts.iter().rev() {
            let FactContent::Relational(RelationalFact::Generic {
                binding_type,
                binding_data,
                ..
            }) = &fact.content
            else {
                continue;
            };

            if binding_type != INVITATION_FACT_TYPE_ID {
                continue;
            }

            let Some(inv_fact) = InvitationFact::from_bytes(binding_data) else {
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

            if seen_id != invitation_id {
                continue;
            }

            // Only treat it as a "contact invitation" if the type string is contact-like.
            if invitation_type.to_lowercase() != "contact" {
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
        invitation_id: &str,
    ) -> AgentResult<Option<DeviceEnrollmentInvitation>> {
        let own_id = self.context.authority.authority_id;

        // First try the local cache (fast path when the same handler instance is reused).
        {
            let cache = self.pending_invitations.read().await;
            if let Some(inv) = cache.get(invitation_id) {
                if let InvitationType::DeviceEnrollment {
                    subject_authority,
                    initiator_device_id,
                    device_id,
                    device_name: _,
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
                device_name: _,
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

        // Persist the shareable invitation so later operations (accept/decline) can resolve it
        // even if AuraAgent constructs a fresh InvitationService/InvitationHandler.
        Self::persist_imported_invitation(effects, self.context.authority.authority_id, &shareable)
            .await?;

        let invitation_id = shareable.invitation_id.clone();

        // Fast path: already cached.
        {
            let cache = self.pending_invitations.read().await;
            if let Some(existing) = cache.get(&invitation_id) {
                return Ok(existing.clone());
            }
        }

        let now_ms = effects.current_timestamp().await.unwrap_or(0);

        // Imported invitations are "received" by the current authority.
        let invitation = Invitation {
            invitation_id: invitation_id.clone(),
            context_id: self.context.effect_context.context_id(),
            sender_id: shareable.sender_id,
            receiver_id: self.context.authority.authority_id,
            invitation_type: shareable.invitation_type,
            status: InvitationStatus::Pending,
            created_at: now_ms,
            expires_at: shareable.expires_at,
            message: shareable.message,
        };

        let mut cache = self.pending_invitations.write().await;
        cache.insert(invitation_id, invitation.clone());

        Ok(invitation)
    }

    /// Decline an invitation
    pub async fn decline_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &str,
    ) -> AgentResult<InvitationResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        self.validate_cached_invitation_decline(effects, invitation_id)
            .await?;

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects).await;
        let outcome = self
            .service
            .prepare_decline_invitation(&snapshot, invitation_id);

        // Execute the outcome
        execute_guard_outcome(outcome, &self.context.authority, effects).await?;

        // Update cache if we have this invitation
        {
            let mut cache = self.pending_invitations.write().await;
            if let Some(inv) = cache.get_mut(invitation_id) {
                inv.status = InvitationStatus::Declined;
            }
        }

        Ok(InvitationResult {
            success: true,
            invitation_id: invitation_id.to_string(),
            new_status: Some(InvitationStatus::Declined),
            error: None,
        })
    }

    /// Cancel an invitation (sender only)
    pub async fn cancel_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &str,
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
        {
            let mut cache = self.pending_invitations.write().await;
            cache.remove(invitation_id);
        }

        Ok(InvitationResult {
            success: true,
            invitation_id: invitation_id.to_string(),
            new_status: Some(InvitationStatus::Cancelled),
            error: None,
        })
    }

    /// List pending invitations (from cache)
    pub async fn list_pending(&self) -> Vec<Invitation> {
        let cache = self.pending_invitations.read().await;
        cache
            .values()
            .filter(|inv| inv.status == InvitationStatus::Pending)
            .cloned()
            .collect()
    }

    /// Get an invitation by ID (from in-memory cache only)
    pub async fn get_invitation(&self, invitation_id: &str) -> Option<Invitation> {
        let cache = self.pending_invitations.read().await;
        cache.get(invitation_id).cloned()
    }

    /// Get an invitation by ID, checking both cache and persistent storage
    pub async fn get_invitation_with_storage(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &str,
    ) -> Option<Invitation> {
        // First check in-memory cache
        {
            let cache = self.pending_invitations.read().await;
            if let Some(inv) = cache.get(invitation_id) {
                return Some(inv.clone());
            }
        }

        // Fall back to persistent storage for created invitations
        if let Some(inv) =
            Self::load_created_invitation(effects, self.context.authority.authority_id, invitation_id)
                .await
        {
            return Some(inv);
        }

        // Check imported invitations and reconstruct if found
        if let Some(shareable) =
            Self::load_imported_invitation(effects, self.context.authority.authority_id, invitation_id)
                .await
        {
            // Reconstruct Invitation from ShareableInvitation
            return Some(Invitation {
                invitation_id: shareable.invitation_id,
                context_id: self.context.effect_context.context_id(),
                sender_id: shareable.sender_id,
                receiver_id: self.context.authority.authority_id,
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
    ceremony_id: String,
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
    pub invitation_id: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::RelationalContext;
    use crate::core::AgentConfig;
    use crate::runtime::effects::AuraEffectSystem;
    use aura_core::identifiers::{AuthorityId, ContextId};
    use aura_journal::fact::{FactContent, RelationalFact};
    use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
    use std::sync::Arc;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(RelationalContext {
            context_id: ContextId::new_from_entropy([seed + 100; 32]),
            participants: vec![],
            metadata: Default::default(),
        });
        authority_context
    }

    #[tokio::test]
    async fn invitation_can_be_created() {
        let authority_context = create_test_authority(91);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([92u8; 32]);

        let invitation = handler
            .create_invitation(
                &effects,
                receiver_id,
                InvitationType::Contact {
                    nickname: Some("alice".to_string()),
                },
                Some("Let's connect!".to_string()),
                Some(86400000), // 1 day
            )
            .await
            .unwrap();

        assert!(invitation.invitation_id.starts_with("inv-"));
        assert_eq!(invitation.sender_id, authority_context.authority_id);
        assert_eq!(invitation.receiver_id, receiver_id);
        assert_eq!(invitation.status, InvitationStatus::Pending);
        assert!(invitation.expires_at.is_some());
    }

    #[tokio::test]
    async fn invitation_can_be_accepted() {
        let authority_context = create_test_authority(93);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([94u8; 32]);

        let invitation = handler
            .create_invitation(
                &effects,
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
            .accept_invitation(&effects, &invitation.invitation_id)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, Some(InvitationStatus::Accepted));
    }

    #[tokio::test]
    async fn invitation_can_be_declined() {
        let authority_context = create_test_authority(96);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([97u8; 32]);

        let invitation = handler
            .create_invitation(
                &effects,
                receiver_id,
                InvitationType::Channel {
                    block_id: "block-123".to_string(),
                },
                None,
                None,
            )
            .await
            .unwrap();

        let result = handler
            .decline_invitation(&effects, &invitation.invitation_id)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, Some(InvitationStatus::Declined));
    }

    #[tokio::test]
    async fn importing_and_accepting_contact_invitation_commits_contact_fact() {
        let own_authority = AuthorityId::new_from_entropy([120u8; 32]);
        let config = AgentConfig::default();
        let effects =
            Arc::new(AuraEffectSystem::testing_for_authority(&config, own_authority).unwrap());

        let mut authority_context = AuthorityContext::new(own_authority);
        authority_context.add_context(RelationalContext {
            context_id: ContextId::new_from_entropy([120u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        let handler = InvitationHandler::new(authority_context).unwrap();

        let sender_id = AuthorityId::new_from_entropy([121u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: "inv-demo-contact-1".to_string(),
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
            .accept_invitation(&effects, &imported.invitation_id)
            .await
            .unwrap();

        let committed = effects.load_committed_facts(own_authority).await.unwrap();

        let mut found = None::<ContactFact>;
        let mut seen_binding_types: Vec<String> = Vec::new();
        for fact in committed {
            let FactContent::Relational(RelationalFact::Generic {
                binding_type,
                binding_data,
                ..
            }) = fact.content
            else {
                continue;
            };

            seen_binding_types.push(binding_type.clone());
            if binding_type != CONTACT_FACT_TYPE_ID {
                continue;
            }

            found = ContactFact::from_bytes(&binding_data);
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
    async fn imported_invitation_is_resolvable_across_handler_instances() {
        let own_authority = AuthorityId::new_from_entropy([122u8; 32]);
        let config = AgentConfig::default();
        let effects =
            Arc::new(AuraEffectSystem::testing_for_authority(&config, own_authority).unwrap());

        let mut authority_context = AuthorityContext::new(own_authority);
        authority_context.add_context(RelationalContext {
            context_id: ContextId::new_from_entropy([122u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        let handler_import = InvitationHandler::new(authority_context.clone()).unwrap();
        let handler_accept = InvitationHandler::new(authority_context).unwrap();

        let sender_id = AuthorityId::new_from_entropy([123u8; 32]);
        let shareable = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: "inv-demo-contact-2".to_string(),
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
            .accept_invitation(&effects, &imported.invitation_id)
            .await
            .unwrap();

        let committed = effects.load_committed_facts(own_authority).await.unwrap();

        let mut found = None::<ContactFact>;
        for fact in committed {
            let FactContent::Relational(RelationalFact::Generic {
                binding_type,
                binding_data,
                ..
            }) = fact.content
            else {
                continue;
            };

            if binding_type != CONTACT_FACT_TYPE_ID {
                continue;
            }

            found = ContactFact::from_bytes(&binding_data);
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

        let mut authority_context = AuthorityContext::new(own_authority);
        authority_context.add_context(RelationalContext {
            context_id: ContextId::new_from_entropy([124u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        // Handler 1: Create an invitation
        let handler_create = InvitationHandler::new(authority_context.clone()).unwrap();
        let receiver_id = AuthorityId::new_from_entropy([125u8; 32]);
        let invitation = handler_create
            .create_invitation(
                &effects,
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
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([99u8; 32]);

        let invitation = handler
            .create_invitation(
                &effects,
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
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let handler = InvitationHandler::new(authority_context).unwrap();

        // Create 3 invitations
        let inv1 = handler
            .create_invitation(
                &effects,
                AuthorityId::new_from_entropy([101u8; 32]),
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let inv2 = handler
            .create_invitation(
                &effects,
                AuthorityId::new_from_entropy([102u8; 32]),
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let _inv3 = handler
            .create_invitation(
                &effects,
                AuthorityId::new_from_entropy([103u8; 32]),
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        // Accept one, decline another
        handler
            .accept_invitation(&effects, &inv1.invitation_id)
            .await
            .unwrap();
        handler
            .decline_invitation(&effects, &inv2.invitation_id)
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
            invitation_id: "inv-test-123".to_string(),
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
            invitation_id: "inv-guardian-456".to_string(),
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
            invitation_id: "inv-channel-789".to_string(),
            sender_id,
            invitation_type: InvitationType::Channel {
                block_id: "block-xyz".to_string(),
            },
            expires_at: Some(1800000000000),
            message: Some("Join my channel!".to_string()),
        };

        let code = shareable.to_code();
        let decoded = ShareableInvitation::from_code(&code).unwrap();

        match decoded.invitation_type {
            InvitationType::Channel { block_id } => {
                assert_eq!(block_id, "block-xyz");
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
            invitation_id: "inv-from-full".to_string(),
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
}
