//! Invitation Service - Public API for Invitation Operations
//!
//! Provides a clean public interface for invitation operations.
//! Wraps `InvitationHandler` with ergonomic methods and proper error handling.

use super::invitation::{
    execute_invitation_effect_commands, DeferredInvitationNetworkEffects, Invitation,
    InvitationHandler, InvitationResult, InvitationStatus, InvitationType, ShareableInvitation,
    ShareableInvitationError, ShareableInvitationSenderProof, ShareableInvitationTransportMetadata,
};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::services::ceremony_runner::{
    CeremonyCommitMetadata, CeremonyInitRequest, CeremonyRunner,
};
use crate::runtime::{AuraEffectSystem, TaskSupervisor};
use aura_core::crypto::single_signer::SingleSignerKeyPackage;
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::effects::secure::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::CryptoCoreEffects;
use aura_core::hash::hash;
use aura_core::secrets::SecretExportContext;
use aura_core::types::identifiers::{AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId};
use aura_core::DeviceId;
use aura_core::Hash32;
use aura_signature::sign_ed25519_transcript;
use std::str::FromStr;
use std::sync::Arc;

const DEFERRED_INVITATION_DELIVERY_ATTEMPTS: usize = 12;
const DEFERRED_INVITATION_DELIVERY_BACKOFF_MS: u64 = 500;

/// Invitation service API
///
/// Provides invitation operations through a clean public API.
#[derive(Clone)]
pub struct InvitationServiceApi {
    handler: InvitationHandler,
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: CeremonyRunner,
    tasks: Arc<TaskSupervisor>,
}

impl std::fmt::Debug for InvitationServiceApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvitationServiceApi")
            .finish_non_exhaustive()
    }
}

impl InvitationServiceApi {
    /// Create a new invitation service with shared runtime-owned supervisors.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        tasks: Arc<TaskSupervisor>,
    ) -> AgentResult<Self> {
        let handler = InvitationHandler::new(authority_context)?;
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
            tasks,
        })
    }

    fn spawn_channel_invitation_exchange(&self, invitation: &Invitation) {
        if invitation.receiver_id == invitation.sender_id {
            return;
        }

        let invitation = invitation.clone();
        let handler = self.handler.clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.channel_exchange.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let sender_id = invitation.sender_id;
        let receiver_id = invitation.receiver_id;
        let fut = async move {
            if let Err(error) = handler
                .execute_channel_invitation_exchange_sender(effects, &invitation)
                .await
            {
                tracing::error!(
                    invitation_id = %invitation_id,
                    sender_id = %sender_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "channel invitation exchange sender failed"
                );
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("sender_exchange", fut);
            } else {
                let _task_handle = tasks.spawn_named("sender_exchange", fut);
            }
        }
    }

    fn spawn_device_enrollment_initiator(&self, invitation: &Invitation) {
        if invitation.receiver_id == invitation.sender_id {
            return;
        }

        let invitation = invitation.clone();
        let handler = self.handler.clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.device_enrollment.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let sender_id = invitation.sender_id;
        let receiver_id = invitation.receiver_id;
        let fut = async move {
            if let Err(error) = handler
                .execute_device_enrollment_initiator(effects, &invitation)
                .await
            {
                tracing::error!(
                    invitation_id = %invitation_id,
                    sender_id = %sender_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "device enrollment initiator choreography failed"
                );
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("device_enrollment_initiator", fut);
            } else {
                let _task_handle = tasks.spawn_named("device_enrollment_initiator", fut);
            }
        }
    }

    fn spawn_device_enrollment_accept_follow_up(&self, invitation: &Invitation) {
        let invitation = invitation.clone();
        let handler = self.handler.clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.device_enrollment_accept.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let sender_id = invitation.sender_id;
        let receiver_id = invitation.receiver_id;
        let fut = async move {
            if let Err(error) = handler
                .execute_device_enrollment_invitee(effects, &invitation)
                .await
            {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    sender_id = %sender_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "device enrollment accept follow-up failed after local acceptance settlement"
                );
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("device_enrollment_accept", fut);
            } else {
                let _task_handle = tasks.spawn_named("device_enrollment_accept", fut);
            }
        }
    }

    fn spawn_deferred_invitation_delivery(
        &self,
        invitation: &Invitation,
        deferred_network_effects: DeferredInvitationNetworkEffects,
    ) {
        if deferred_network_effects.is_empty() {
            return;
        }

        let authority = self.handler.authority_context().clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.delivery.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let sender_id = invitation.sender_id;
        let receiver_id = invitation.receiver_id;
        let command_count = deferred_network_effects.commands().len();
        let commands = deferred_network_effects.into_commands();
        tracing::info!(
            invitation_id = %invitation_id,
            sender_id = %sender_id,
            receiver_id = %receiver_id,
            command_count,
            "Scheduling deferred invitation delivery side effects"
        );
        let fut = async move {
            tracing::debug!(
                invitation_id = %invitation_id,
                sender_id = %sender_id,
                receiver_id = %receiver_id,
                command_count,
                "Executing deferred invitation delivery side effects"
            );
            for attempt in 0..DEFERRED_INVITATION_DELIVERY_ATTEMPTS {
                match execute_invitation_effect_commands(
                    commands.clone(),
                    &authority,
                    effects.as_ref(),
                    true,
                )
                .await
                {
                    Ok(()) => {
                        if attempt > 0 {
                            tracing::info!(
                                invitation_id = %invitation_id,
                                sender_id = %sender_id,
                                receiver_id = %receiver_id,
                                attempts = attempt + 1,
                                "Deferred invitation delivery succeeded after retry"
                            );
                        }
                        return;
                    }
                    Err(error) => {
                        let final_attempt = attempt + 1 == DEFERRED_INVITATION_DELIVERY_ATTEMPTS;
                        if final_attempt {
                            tracing::warn!(
                                invitation_id = %invitation_id,
                                sender_id = %sender_id,
                                receiver_id = %receiver_id,
                                attempts = attempt + 1,
                                error = %error,
                                "Deferred invitation delivery side effects failed after retries"
                            );
                            return;
                        }

                        tracing::warn!(
                            invitation_id = %invitation_id,
                            sender_id = %sender_id,
                            receiver_id = %receiver_id,
                            attempt = attempt + 1,
                            retry_in_ms = DEFERRED_INVITATION_DELIVERY_BACKOFF_MS,
                            error = %error,
                            "Deferred invitation delivery failed; retrying"
                        );
                        let _ = effects
                            .sleep_ms(DEFERRED_INVITATION_DELIVERY_BACKOFF_MS)
                            .await;
                    }
                }
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("delivery", fut);
            } else {
                let _task_handle = tasks.spawn_named("delivery", fut);
            }
        }
    }

    fn spawn_contact_acceptance_notification(&self, invitation_id: InvitationId) {
        let handler = self.handler.clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.contact_acceptance.{}",
            invitation_id
        ));
        let task_name = format!("notify.{}", invitation_id);
        let invitation_id_for_log = invitation_id.clone();
        let fut = async move {
            if let Err(error) = handler
                .notify_contact_invitation_acceptance(effects.as_ref(), &invitation_id)
                .await
            {
                tracing::warn!(
                    invitation_id = %invitation_id_for_log,
                    error = %error,
                    "Contact acceptance notification failed; continuing"
                );
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            let _task_handle = tasks.spawn_local_named(task_name, fut);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _task_handle = tasks.spawn_named(task_name, fut);
        }
    }

    fn spawn_channel_acceptance_notification(&self, invitation_id: InvitationId) {
        let handler = self.handler.clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.channel_acceptance.{}",
            invitation_id
        ));
        let task_name = format!("notify.{}", invitation_id);
        let invitation_id_for_log = invitation_id.clone();
        let fut = async move {
            if let Err(error) = handler
                .notify_channel_invitation_acceptance(effects.as_ref(), &invitation_id)
                .await
            {
                tracing::warn!(
                    invitation_id = %invitation_id_for_log,
                    error = %error,
                    "Channel acceptance notification failed; continuing"
                );
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            let _task_handle = tasks.spawn_local_named(task_name, fut);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _task_handle = tasks.spawn_named(task_name, fut);
        }
    }

    fn spawn_invitation_ceremony_registration(&self, invitation: &Invitation) {
        if !Self::should_track_ceremony(&invitation.invitation_type) {
            return;
        }

        let invitation = invitation.clone();
        let service = self.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.ceremony_registration.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let sender_id = invitation.sender_id;
        let receiver_id = invitation.receiver_id;
        let fut = async move {
            if let Err(error) = service.ensure_invitation_ceremony(&invitation).await {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    sender_id = %sender_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "Invitation ceremony registration failed"
                );
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("register", fut);
            } else {
                let _task_handle = tasks.spawn_named("register", fut);
            }
        }
    }

    fn spawn_invitation_acceptance_ceremony_progress(
        &self,
        ceremony_id: CeremonyId,
        invitation: &Invitation,
    ) {
        let ceremony_runner = self.ceremony_runner.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.accept_ceremony.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let receiver_id = invitation.receiver_id;
        let fut = async move {
            let participant = aura_core::threshold::ParticipantIdentity::guardian(receiver_id);
            if let Err(error) = ceremony_runner
                .record_local_response(&ceremony_id, participant)
                .await
            {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    ceremony_id = %ceremony_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "Invitation acceptance ceremony response registration failed"
                );
                return;
            }
            if let Err(error) = ceremony_runner
                .commit(&ceremony_id, CeremonyCommitMetadata::default())
                .await
            {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    ceremony_id = %ceremony_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "Invitation acceptance ceremony commit failed"
                );
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("accept_commit", fut);
            } else {
                let _task_handle = tasks.spawn_named("accept_commit", fut);
            }
        }
    }

    fn should_track_ceremony(invitation_type: &InvitationType) -> bool {
        matches!(
            invitation_type,
            InvitationType::Guardian { .. } | InvitationType::Channel { .. }
        )
    }

    async fn ensure_invitation_ceremony(
        &self,
        invitation: &Invitation,
    ) -> AgentResult<Option<CeremonyId>> {
        if !Self::should_track_ceremony(&invitation.invitation_type) {
            return Ok(None);
        }

        let ceremony_id = CeremonyId::new(invitation.invitation_id.to_string());
        if self.ceremony_runner.status(&ceremony_id).await.is_ok() {
            return Ok(Some(ceremony_id));
        }

        let prestate_hash = Hash32(hash(invitation.invitation_id.as_str().as_bytes()));
        let participants = vec![aura_core::threshold::ParticipantIdentity::guardian(
            invitation.receiver_id,
        )];

        self.ceremony_runner
            .start(CeremonyInitRequest {
                ceremony_id: ceremony_id.clone(),
                kind: aura_app::runtime_bridge::CeremonyKind::Invitation,
                initiator_id: invitation.sender_id,
                threshold_k: 1,
                total_n: 1,
                participants,
                new_epoch: 0,
                enrollment_device_id: None,
                enrollment_nickname_suggestion: None,
                prestate_hash,
            })
            .await
            .map_err(|e| AgentError::internal(format!("Failed to register ceremony: {e}")))?;

        Ok(Some(ceremony_id))
    }

    /// Create an invitation to a channel/home
    ///
    /// # Arguments
    /// * `receiver_id` - Authority to invite
    /// * `home_id` - Home/channel ID to invite to
    /// * `message` - Optional message
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The created invitation
    pub async fn invite_to_channel(
        &self,
        receiver_id: AuthorityId,
        home_id: String,
        context_id: Option<ContextId>,
        nickname_suggestion: Option<String>,
        bootstrap: Option<ChannelBootstrapPackage>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let home_id = ChannelId::from_str(&home_id).map_err(|e| {
            AgentError::invalid(format!(
                "invalid channel/home id `{home_id}`: expected canonical ChannelId format ({e})"
            ))
        })?;

        let prepared = self
            .handler
            .prepare_invitation_with_context(
                self.effects.clone(),
                receiver_id,
                InvitationType::Channel {
                    home_id,
                    nickname_suggestion,
                    bootstrap,
                },
                None,
                context_id,
                message,
                expires_in_ms,
            )
            .await?;
        let invitation = prepared.invitation;
        let deferred_network_effects = prepared.deferred_network_effects;
        #[cfg(target_arch = "wasm32")]
        if self.effects.harness_mode_enabled() {
            if let Err(error) = execute_invitation_effect_commands(
                deferred_network_effects.into_commands(),
                self.handler.authority_context(),
                self.effects.as_ref(),
                true,
            )
            .await
            {
                tracing::warn!(
                    invitation_id = %invitation.invitation_id,
                    sender_id = %invitation.sender_id,
                    receiver_id = %invitation.receiver_id,
                    error = %error,
                    "Inline harness channel invitation delivery failed"
                );
            }
        } else {
            self.spawn_deferred_invitation_delivery(&invitation, deferred_network_effects);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.spawn_deferred_invitation_delivery(&invitation, deferred_network_effects);
        self.spawn_invitation_ceremony_registration(&invitation);
        self.spawn_channel_invitation_exchange(&invitation);
        Ok(invitation)
    }

    /// Create an invitation to become a guardian
    ///
    /// # Arguments
    /// * `receiver_id` - Authority to invite as guardian
    /// * `subject_authority` - Authority to guard
    /// * `message` - Optional message
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The created invitation
    pub async fn invite_as_guardian(
        &self,
        receiver_id: AuthorityId,
        subject_authority: AuthorityId,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let prepared = self
            .handler
            .prepare_invitation_with_context(
                self.effects.clone(),
                receiver_id,
                InvitationType::Guardian { subject_authority },
                None,
                None,
                message,
                expires_in_ms,
            )
            .await?;
        let invitation = prepared.invitation;
        self.spawn_invitation_ceremony_registration(&invitation);
        self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);
        Ok(invitation)
    }

    /// Create an invitation to become a contact
    ///
    /// # Arguments
    /// * `receiver_id` - Authority to invite as contact
    /// * `nickname` - Optional nickname for the contact
    /// * `message` - Optional message
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The created invitation
    pub async fn invite_as_contact(
        &self,
        receiver_id: AuthorityId,
        nickname: Option<String>,
        receiver_nickname: Option<String>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let prepared = self
            .handler
            .prepare_invitation_with_context(
                self.effects.clone(),
                receiver_id,
                InvitationType::Contact { nickname },
                receiver_nickname,
                None,
                message,
                expires_in_ms,
            )
            .await?;
        let invitation = prepared.invitation;
        self.spawn_invitation_ceremony_registration(&invitation);
        #[cfg(target_arch = "wasm32")]
        if self.effects.harness_mode_enabled() {
            if let Err(error) = execute_invitation_effect_commands(
                prepared.deferred_network_effects.into_commands(),
                self.handler.authority_context(),
                self.effects.as_ref(),
                true,
            )
            .await
            {
                tracing::warn!(
                    invitation_id = %invitation.invitation_id,
                    sender_id = %invitation.sender_id,
                    receiver_id = %invitation.receiver_id,
                    error = %error,
                    "Inline harness contact invitation delivery failed"
                );
            }
        } else {
            self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);
        Ok(invitation)
    }

    /// Create an invitation to enroll a new device for the current authority.
    ///
    /// This is intended for out-of-band transfer (copy/paste, QR).
    #[allow(clippy::too_many_arguments)]
    pub async fn invite_device_enrollment(
        &self,
        receiver_id: AuthorityId,
        subject_authority: AuthorityId,
        initiator_device_id: DeviceId,
        device_id: DeviceId,
        nickname_suggestion: Option<String>,
        ceremony_id: CeremonyId,
        pending_epoch: u64,
        key_package: Vec<u8>,
        threshold_config: Vec<u8>,
        public_key_package: Vec<u8>,
        baseline_tree_ops: Vec<Vec<u8>>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let prepared = self
            .handler
            .prepare_invitation_with_context(
                self.effects.clone(),
                receiver_id,
                InvitationType::DeviceEnrollment {
                    subject_authority,
                    initiator_device_id,
                    device_id,
                    nickname_suggestion,
                    ceremony_id,
                    pending_epoch,
                    key_package,
                    threshold_config,
                    public_key_package,
                    baseline_tree_ops,
                },
                None,
                None,
                None,
                expires_in_ms,
            )
            .await?;
        let invitation = prepared.invitation;
        self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);
        self.spawn_device_enrollment_initiator(&invitation);
        Ok(invitation)
    }

    /// Accept an invitation
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to accept
    ///
    /// # Returns
    /// Result of the acceptance
    pub async fn accept(&self, invitation_id: &InvitationId) -> AgentResult<InvitationResult> {
        let result = self
            .handler
            .accept_invitation(self.effects.clone(), invitation_id)
            .await?;

        if let Some(invitation) = self
            .handler
            .get_invitation_with_storage(self.effects.as_ref(), invitation_id)
            .await
        {
            if matches!(invitation.invitation_type, InvitationType::Contact { .. }) {
                self.spawn_contact_acceptance_notification(invitation.invitation_id.clone());
            }
            if matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                self.spawn_channel_acceptance_notification(invitation.invitation_id.clone());
            }
            if matches!(
                invitation.invitation_type,
                InvitationType::DeviceEnrollment { .. }
            ) {
                self.spawn_device_enrollment_accept_follow_up(&invitation);
            }
            if let Some(ceremony_id) = self.ensure_invitation_ceremony(&invitation).await? {
                self.spawn_invitation_acceptance_ceremony_progress(ceremony_id, &invitation);
            }
        }

        Ok(result)
    }

    /// Decline an invitation
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to decline
    ///
    /// # Returns
    /// Result of the decline
    pub async fn decline(&self, invitation_id: &InvitationId) -> AgentResult<InvitationResult> {
        let result = self
            .handler
            .decline_invitation(self.effects.clone(), invitation_id)
            .await?;

        if let Some(invitation) = self
            .handler
            .get_invitation_with_storage(self.effects.as_ref(), invitation_id)
            .await
        {
            if let Some(ceremony_id) = self.ensure_invitation_ceremony(&invitation).await? {
                let _ = self
                    .ceremony_runner
                    .abort(&ceremony_id, Some("Invitation declined".to_string()))
                    .await;
            }
        }

        Ok(result)
    }

    /// Cancel an invitation (sender only)
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to cancel
    ///
    /// # Returns
    /// Result of the cancellation
    pub async fn cancel(&self, invitation_id: &InvitationId) -> AgentResult<InvitationResult> {
        let result = self
            .handler
            .cancel_invitation(self.effects.clone(), invitation_id)
            .await?;

        if let Some(invitation) = self
            .handler
            .get_invitation_with_storage(self.effects.as_ref(), invitation_id)
            .await
        {
            if let Some(ceremony_id) = self.ensure_invitation_ceremony(&invitation).await? {
                let _ = self
                    .ceremony_runner
                    .abort(&ceremony_id, Some("Invitation canceled".to_string()))
                    .await;
            }
        }

        Ok(result)
    }

    /// List pending invitations
    ///
    /// # Returns
    /// List of pending invitations
    pub async fn list_pending(&self) -> Vec<Invitation> {
        self.handler.list_pending_with_storage(&self.effects).await
    }

    /// List cached invitations matching a predicate.
    pub async fn list_cached_matching(
        &self,
        predicate: impl Fn(&Invitation) -> bool,
    ) -> Vec<Invitation> {
        self.handler.list_cached_matching(predicate).await
    }

    /// List invitations from cache plus persisted stores.
    pub async fn list_with_storage(&self) -> Vec<Invitation> {
        self.handler.list_with_storage(&self.effects).await
    }

    /// Get an invitation by ID
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation
    ///
    /// # Returns
    /// The invitation if found
    pub async fn get(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        self.handler.get_invitation(invitation_id).await
    }

    /// Check if an invitation is pending
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation
    ///
    /// # Returns
    /// True if the invitation exists and is pending
    pub async fn is_pending(&self, invitation_id: &InvitationId) -> bool {
        self.handler
            .get_invitation(invitation_id)
            .await
            .map(|inv| inv.status == InvitationStatus::Pending)
            .unwrap_or(false)
    }

    // =========================================================================
    // Sharing Methods (Out-of-Band Transfer)
    // =========================================================================

    fn sender_transport_metadata(&self) -> ShareableInvitationTransportMetadata {
        #[cfg(target_arch = "wasm32")]
        let sender_addr = self
            .effects
            .lan_transport()
            .and_then(|transport| transport.websocket_addrs().first().cloned());
        #[cfg(not(target_arch = "wasm32"))]
        let sender_addr = self
            .effects
            .lan_transport()
            .and_then(|transport| transport.advertised_addrs().first().cloned());

        #[cfg(target_arch = "wasm32")]
        let sender_hint = sender_addr.as_deref().map(|addr| {
            if addr.starts_with("ws://") || addr.starts_with("wss://") {
                addr.to_string()
            } else {
                format!("ws://{addr}")
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        let sender_hint = sender_addr.as_deref().map(|addr| format!("tcp://{addr}"));
        tracing::info!(
            sender_addr = ?sender_addr,
            sender_hint = ?sender_hint,
            "export invitation sender transport hint"
        );
        ShareableInvitationTransportMetadata {
            sender_hint,
            sender_device_id: Some(self.effects.device_id()),
        }
    }

    fn append_sender_hint(
        &self,
        mut code: String,
        transport: &ShareableInvitationTransportMetadata,
    ) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let sender_hint_segment = transport
            .sender_hint
            .as_deref()
            .map(|hint| URL_SAFE_NO_PAD.encode(hint.as_bytes()))
            .unwrap_or_else(|| URL_SAFE_NO_PAD.encode("".as_bytes()));
        let encoded_device_id = transport
            .sender_device_id
            .map(|device_id| URL_SAFE_NO_PAD.encode(device_id.to_string().as_bytes()))
            .unwrap_or_else(|| URL_SAFE_NO_PAD.encode("".as_bytes()));
        if transport.sender_hint.is_some() || self.effects.harness_mode_enabled() {
            code = format!("{code}:{sender_hint_segment}:{encoded_device_id}");
        }

        code
    }

    async fn retrieve_identity_keys(&self, authority: &AuthorityId) -> Option<(Vec<u8>, Vec<u8>)> {
        let caps = vec![SecureStorageCapability::Read];
        for epoch in [1_u64, 0_u64] {
            let location = SecureStorageLocation::with_sub_key(
                "signing_keys",
                format!("{authority}:{epoch}"),
                "1",
            );
            let Ok(bytes) = self.effects.secure_retrieve(&location, &caps).await else {
                continue;
            };
            let Ok(pkg) = SingleSignerKeyPackage::import_from_secure_storage(
                &bytes,
                SecretExportContext::secure_storage(
                    "aura-agent::handlers::invitation_service::retrieve_identity_keys",
                ),
            ) else {
                continue;
            };
            return Some((pkg.signing_key().to_vec(), pkg.verifying_key().to_vec()));
        }
        None
    }

    async fn export_testing_invitation(
        &self,
        invitation: &Invitation,
        transport: &ShareableInvitationTransportMetadata,
    ) -> Result<String, ShareableInvitationError> {
        let shareable = ShareableInvitation::from(invitation);
        let (private_key, public_key) =
            match self.retrieve_identity_keys(&shareable.sender_id).await {
                Some(identity_keys) => identity_keys,
                None => self
                    .effects
                    .ed25519_generate_keypair()
                    .await
                    .map_err(|_| ShareableInvitationError::SerializationFailed)?,
            };
        let signature = sign_ed25519_transcript(
            self.effects.as_ref(),
            &shareable.signing_transcript_with_transport(transport),
            &private_key,
        )
        .await
        .map_err(|_| ShareableInvitationError::SerializationFailed)?;
        shareable.to_signed_code_with_transport(
            ShareableInvitationSenderProof {
                scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
                public_key,
                signature,
                sender_device_id: transport.sender_device_id,
            },
            transport.clone(),
        )
    }

    /// Export an invitation as a shareable code string (compile-time safe)
    ///
    /// This is the preferred method when you already have the `Invitation` object.
    ///
    /// # Arguments
    /// * `invitation` - The invitation to export
    ///
    /// # Returns
    /// A shareable code string (format: `aura:v1:<base64>`)
    pub fn export_invitation(invitation: &Invitation) -> Result<String, ShareableInvitationError> {
        #[cfg(test)]
        {
            ShareableInvitation::from(invitation).to_code()
        }
        #[cfg(not(test))]
        {
            let _ = invitation;
            Err(ShareableInvitationError::MissingSenderProof)
        }
    }

    /// Export an invitation as a shareable code string with transport metadata.
    ///
    /// This should be used for codes that will be imported by another runtime
    /// and may need a direct sender websocket hint for the first return path.
    pub async fn export_invitation_with_sender_hint(
        &self,
        invitation: &Invitation,
    ) -> Result<String, ShareableInvitationError> {
        let transport = self.sender_transport_metadata();
        if self.effects.is_testing() {
            let code = self
                .export_testing_invitation(invitation, &transport)
                .await?;
            return Ok(self.append_sender_hint(code, &transport));
        }

        Err(ShareableInvitationError::MissingSenderProof)
    }

    /// Export an invitation by ID as a shareable code string
    ///
    /// The code can be shared out-of-band (copy/paste, QR code, etc.)
    /// and imported by the receiver using `import_code`.
    ///
    /// **Note**: Prefer `export_invitation(&Invitation)` when you have the
    /// invitation object, as it provides compile-time safety.
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to export
    ///
    /// # Returns
    /// A shareable code string (format: `aura:v1:<base64>`)
    ///
    /// # Errors
    /// Returns an error if the invitation is not found
    pub async fn export_code(&self, invitation_id: &InvitationId) -> AgentResult<String> {
        let invitation = self
            .handler
            .get_invitation_with_storage(&self.effects, invitation_id)
            .await
            .ok_or_else(|| {
                aura_core::AuraError::not_found(format!("Invitation not found: {}", invitation_id))
            })?;
        tracing::info!(
            invitation_id = %invitation_id,
            "export invitation sender websocket hint"
        );
        self.export_invitation_with_sender_hint(&invitation)
            .await
            .map_err(|error| AgentError::invalid(error.to_string()))
    }

    /// Import an invitation from a shareable code string
    ///
    /// Decodes the code and returns the shareable invitation details.
    /// The receiver can then decide whether to accept.
    ///
    /// # Arguments
    /// * `code` - The shareable code string (format: `aura:v1:<base64>`)
    ///
    /// # Returns
    /// The decoded `ShareableInvitation`
    ///
    /// # Errors
    /// Returns an error if the code is invalid
    pub fn import_code(code: &str) -> Result<ShareableInvitation, ShareableInvitationError> {
        ShareableInvitation::from_code(code)
    }

    /// Import an out-of-band invite code into the local invitation cache.
    ///
    /// This enables follow-up operations (e.g., accept) to look up the invitation
    /// details by `invitation_id` without requiring the original `Sent` fact to
    /// be present in the local journal.
    pub async fn import_and_cache(&self, code: &str) -> AgentResult<Invitation> {
        self.handler
            .import_invitation_code(&self.effects, code)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::runtime::services::ceremony_runner::CeremonyRunner;
    use crate::runtime::services::CeremonyTracker;
    use crate::runtime::TaskSupervisor;
    use aura_core::effects::amp::ChannelCreateParams;
    use aura_core::effects::time::PhysicalTimeEffects;
    use aura_effects::AmpChannelEffects;
    use std::future::Future;

    #[track_caller]
    fn run_async_test_on_large_stack<F>(future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        std::thread::Builder::new()
            .name("invitation-service-large-stack".to_string())
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap_or_else(|error| panic!("test runtime should build: {error}"));
                runtime.block_on(future);
            })
            .unwrap_or_else(|error| panic!("large-stack test thread should spawn: {error}"))
            .join()
            .unwrap_or_else(|error| panic!("large-stack test thread should complete: {error:?}"));
    }

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
        crate::testing::simulation_effect_system_for_authority_arc(
            &config,
            authority.authority_id(),
        )
    }

    #[track_caller]
    fn effects_for_simulation(authority: &AuthorityContext, seed: u64) -> Arc<AuraEffectSystem> {
        let config = AgentConfig {
            device_id: authority.device_id(),
            ..Default::default()
        };
        Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority_with_salt(
                &config,
                authority.authority_id(),
                seed,
            )
            .unwrap(),
        )
    }

    fn service_for(
        authority_context: AuthorityContext,
        effects: Arc<AuraEffectSystem>,
    ) -> InvitationServiceApi {
        let time_effects: Arc<dyn PhysicalTimeEffects> = Arc::new(effects.time_effects().clone());
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
    async fn test_invitation_service_creation() {
        let authority_context = create_test_authority(110);
        let effects = effects_for(&authority_context);
        let expected_authority = authority_context.authority_id();

        let service = service_for(authority_context, effects);
        assert_eq!(
            service.handler.authority_context().authority_id(),
            expected_authority
        );
    }

    #[tokio::test]
    async fn test_invite_as_contact() {
        let authority_context = create_test_authority(111);
        let effects = effects_for(&authority_context);
        let service = service_for(authority_context, effects);

        let receiver_id = AuthorityId::new_from_entropy([112u8; 32]);
        let invitation = service
            .invite_as_contact(
                receiver_id,
                Some("bob".to_string()),
                None,
                Some("Hey Bob!".to_string()),
                None,
            )
            .await
            .unwrap();

        assert!(invitation.invitation_id.as_str().starts_with("inv-"));
        assert_eq!(invitation.receiver_id, receiver_id);
        assert_eq!(invitation.status, InvitationStatus::Pending);
    }

    #[tokio::test]
    async fn test_invite_as_contact_self_out_of_band_does_not_require_peer() {
        let authority_context = create_test_authority(141);
        let effects = effects_for_simulation(&authority_context, 141);
        let service = service_for(authority_context.clone(), effects);

        let receiver_id = authority_context.authority_id();
        let result = service
            .invite_as_contact(
                receiver_id,
                None,
                None,
                Some("Out-of-band invite".to_string()),
                None,
            )
            .await;

        assert!(
            result.is_ok(),
            "contact invite to self should succeed for out-of-band sharing, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_invite_as_guardian() {
        let authority_context = create_test_authority(113);
        let effects = effects_for(&authority_context);
        let service = service_for(authority_context.clone(), effects);

        let receiver_id = AuthorityId::new_from_entropy([114u8; 32]);
        let invitation = service
            .invite_as_guardian(
                receiver_id,
                authority_context.authority_id(),
                Some("Please guard my identity".to_string()),
                Some(604800000), // 1 week
            )
            .await
            .unwrap();

        assert!(invitation.invitation_id.as_str().starts_with("inv-"));
        assert!(invitation.expires_at.is_some());
    }

    #[tokio::test]
    async fn test_invite_to_channel() {
        let authority_context = create_test_authority(115);
        let effects = effects_for(&authority_context);
        let service = service_for(authority_context, effects.clone());

        let receiver_id = AuthorityId::new_from_entropy([116u8; 32]);
        let context_id = ContextId::new_from_entropy([117u8; 32]);
        let home_id = ChannelId::from_bytes([116u8; 32]);
        effects
            .create_channel(ChannelCreateParams {
                context: context_id,
                channel: Some(home_id),
                skip_window: None,
                topic: None,
            })
            .await
            .unwrap();
        let invitation = service
            .invite_to_channel(
                receiver_id,
                home_id.to_string(),
                Some(context_id),
                Some("shared-parity-lab".to_string()),
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(invitation.invitation_id.as_str().starts_with("inv-"));
        match &invitation.invitation_type {
            InvitationType::Channel {
                nickname_suggestion,
                ..
            } => assert_eq!(nickname_suggestion.as_deref(), Some("shared-parity-lab")),
            _ => panic!("expected channel invitation"),
        }
    }

    #[test]
    fn invite_to_channel_defers_best_effort_delivery() {
        let source = include_str!("invitation_service.rs");
        let start = source
            .find("pub async fn invite_to_channel(")
            .expect("invite_to_channel definition");
        let body = &source[start..];
        assert!(
            body.contains(
                "self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);"
            ),
            "channel invites must defer best-effort delivery instead of blocking terminal settlement"
        );
        assert!(
            !body.contains("execute_invitation_effect_commands(\n            prepared.deferred_network_effects.into_commands(),"),
            "channel invites must not execute deferred network effects inline"
        );
    }

    #[test]
    fn deferred_invitation_delivery_retries_after_failure() {
        let source = include_str!("invitation_service.rs");
        let start = source
            .find("fn spawn_deferred_invitation_delivery(")
            .expect("deferred delivery definition");
        let body = &source[start..];
        assert!(
            body.contains("for attempt in 0..DEFERRED_INVITATION_DELIVERY_ATTEMPTS"),
            "deferred invitation delivery must retry bounded background delivery attempts"
        );
        assert!(
            body.contains("Deferred invitation delivery failed; retrying"),
            "deferred invitation delivery should log retryable failures explicitly"
        );
    }

    #[test]
    fn test_accept_decline_flow() {
        run_async_test_on_large_stack(async move {
            let sender_context = create_test_authority(117);
            let sender_effects = effects_for(&sender_context);
            let sender_service = service_for(sender_context, sender_effects);
            let receiver_context = create_test_authority(118);
            let receiver_effects = effects_for(&receiver_context);
            let receiver_service = service_for(receiver_context.clone(), receiver_effects);
            let receiver_id = receiver_context.authority_id();

            // Create two invitations
            let inv1 = sender_service
                .invite_as_contact(receiver_id, None, None, None, None)
                .await
                .unwrap();
            let inv2 = sender_service
                .invite_as_contact(receiver_id, None, None, None, None)
                .await
                .unwrap();
            let imported1 = receiver_service
                .import_and_cache(
                    &sender_service
                        .export_invitation_with_sender_hint(&inv1)
                        .await
                        .unwrap(),
                )
                .await
                .unwrap();
            let imported2 = receiver_service
                .import_and_cache(
                    &sender_service
                        .export_invitation_with_sender_hint(&inv2)
                        .await
                        .unwrap(),
                )
                .await
                .unwrap();

            // Accept one
            let accept_result = receiver_service
                .accept(&imported1.invitation_id)
                .await
                .unwrap();
            assert_eq!(accept_result.new_status, InvitationStatus::Accepted);

            // Decline the other
            let decline_result = receiver_service
                .decline(&imported2.invitation_id)
                .await
                .unwrap();
            assert_eq!(decline_result.new_status, InvitationStatus::Declined);

            // Check pending is empty
            let pending = receiver_service.list_pending().await;
            assert!(pending.is_empty());
        });
    }

    #[test]
    fn test_is_pending() {
        run_async_test_on_large_stack(async move {
            let sender_context = create_test_authority(120);
            let sender_effects = effects_for(&sender_context);
            let sender_service = service_for(sender_context, sender_effects);
            let receiver_context = create_test_authority(121);
            let receiver_effects = effects_for(&receiver_context);
            let receiver_id = receiver_context.authority_id();
            let receiver_service = service_for(receiver_context, receiver_effects);

            let invitation = sender_service
                .invite_as_contact(receiver_id, None, None, None, None)
                .await
                .unwrap();
            let imported = receiver_service
                .import_and_cache(
                    &sender_service
                        .export_invitation_with_sender_hint(&invitation)
                        .await
                        .unwrap(),
                )
                .await
                .unwrap();

            assert!(receiver_service.is_pending(&imported.invitation_id).await);

            receiver_service
                .accept(&imported.invitation_id)
                .await
                .unwrap();

            assert!(!receiver_service.is_pending(&imported.invitation_id).await);
        });
    }
}
