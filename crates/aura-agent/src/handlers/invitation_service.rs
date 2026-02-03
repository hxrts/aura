//! Invitation Service - Public API for Invitation Operations
//!
//! Provides a clean public interface for invitation operations.
//! Wraps `InvitationHandler` with ergonomic methods and proper error handling.

use super::invitation::{
    Invitation, InvitationHandler, InvitationResult, InvitationStatus, InvitationType,
    ShareableInvitation, ShareableInvitationError,
};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::services::ceremony_runner::{
    CeremonyCommitMetadata, CeremonyInitRequest, CeremonyRunner,
};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, CeremonyId, InvitationId};
use aura_core::DeviceId;
use aura_core::Hash32;
use std::sync::Arc;

/// Invitation service API
///
/// Provides invitation operations through a clean public API.
#[derive(Clone)]
pub struct InvitationServiceApi {
    handler: InvitationHandler,
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: CeremonyRunner,
}

impl std::fmt::Debug for InvitationServiceApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvitationServiceApi")
            .finish_non_exhaustive()
    }
}

impl InvitationServiceApi {
    /// Create a new invitation service
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
    ) -> AgentResult<Self> {
        let handler = InvitationHandler::new(authority_context)?;
        let ceremony_runner = CeremonyRunner::new(crate::runtime::services::CeremonyTracker::new());
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
        })
    }

    /// Create a new invitation service with a shared ceremony runner.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
    ) -> AgentResult<Self> {
        let handler = InvitationHandler::new(authority_context)?;
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
        })
    }

    fn should_track_ceremony(invitation_type: &InvitationType) -> bool {
        matches!(
            invitation_type,
            InvitationType::Contact { .. }
                | InvitationType::Guardian { .. }
                | InvitationType::Channel { .. }
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
                prestate_hash: Some(prestate_hash),
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
        bootstrap: Option<ChannelBootstrapPackage>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let invitation = self
            .handler
            .create_invitation(
                self.effects.clone(),
                receiver_id,
                InvitationType::Channel {
                    home_id,
                    nickname_suggestion: None,
                    bootstrap,
                },
                message,
                expires_in_ms,
            )
            .await?;
        let _ = self.ensure_invitation_ceremony(&invitation).await?;
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
        let invitation = self
            .handler
            .create_invitation(
                self.effects.clone(),
                receiver_id,
                InvitationType::Guardian { subject_authority },
                message,
                expires_in_ms,
            )
            .await?;
        let _ = self.ensure_invitation_ceremony(&invitation).await?;
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
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let invitation = self
            .handler
            .create_invitation(
                self.effects.clone(),
                receiver_id,
                InvitationType::Contact { nickname },
                message,
                expires_in_ms,
            )
            .await?;
        let _ = self.ensure_invitation_ceremony(&invitation).await?;
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
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let invitation = self
            .handler
            .create_invitation(
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
                },
                None,
                expires_in_ms,
            )
            .await?;
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
            if let Some(ceremony_id) = self.ensure_invitation_ceremony(&invitation).await? {
                let _ = self
                    .ceremony_runner
                    .record_response(
                        &ceremony_id,
                        aura_core::threshold::ParticipantIdentity::guardian(invitation.receiver_id),
                    )
                    .await
                    .map_err(|e| {
                        AgentError::internal(format!("Failed to record invitation acceptance: {e}"))
                    })?;
                let _ = self
                    .ceremony_runner
                    .commit(&ceremony_id, CeremonyCommitMetadata::default())
                    .await;
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
            .cancel_invitation(&self.effects, invitation_id)
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
        self.handler.list_pending().await
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

    /// Export an invitation as a shareable code string (compile-time safe)
    ///
    /// This is the preferred method when you already have the `Invitation` object.
    /// It cannot fail since no lookup is required.
    ///
    /// # Arguments
    /// * `invitation` - The invitation to export
    ///
    /// # Returns
    /// A shareable code string (format: `aura:v1:<base64>`)
    pub fn export_invitation(invitation: &Invitation) -> String {
        let shareable = ShareableInvitation::from(invitation);
        shareable.to_code()
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

        Ok(Self::export_invitation(&invitation))
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

    /// Import an out-of-band invitation code into the local invitation cache.
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

    fn effects_for_simulation(authority: &AuthorityContext, seed: u64) -> Arc<AuraEffectSystem> {
        let config = AgentConfig {
            device_id: authority.device_id(),
            ..Default::default()
        };
        Arc::new(
            AuraEffectSystem::simulation_for_authority(&config, seed, authority.authority_id())
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn test_invitation_service_creation() {
        let authority_context = create_test_authority(110);
        let effects = effects_for(&authority_context);

        let service = InvitationServiceApi::new(effects, authority_context);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_invite_as_contact() {
        let authority_context = create_test_authority(111);
        let effects = effects_for(&authority_context);
        let service = InvitationServiceApi::new(effects, authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([112u8; 32]);
        let invitation = service
            .invite_as_contact(
                receiver_id,
                Some("bob".to_string()),
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
        let service = InvitationServiceApi::new(effects, authority_context.clone()).unwrap();

        let receiver_id = authority_context.authority_id();
        let result = service
            .invite_as_contact(receiver_id, None, Some("Out-of-band invite".to_string()), None)
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
        let service = InvitationServiceApi::new(effects, authority_context.clone()).unwrap();

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
        let service = InvitationServiceApi::new(effects, authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([116u8; 32]);
        let invitation = service
            .invite_to_channel(receiver_id, "channel-123".to_string(), None, None, None)
            .await
            .unwrap();

        assert!(invitation.invitation_id.as_str().starts_with("inv-"));
    }

    #[tokio::test]
    async fn test_accept_decline_flow() {
        let authority_context = create_test_authority(117);
        let effects = effects_for(&authority_context);
        let service = InvitationServiceApi::new(effects, authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([118u8; 32]);

        // Create two invitations
        let inv1 = service
            .invite_as_contact(receiver_id, None, None, None)
            .await
            .unwrap();
        let inv2 = service
            .invite_as_contact(AuthorityId::new_from_entropy([119u8; 32]), None, None, None)
            .await
            .unwrap();

        // Accept one
        let accept_result = service.accept(&inv1.invitation_id).await.unwrap();
        assert!(accept_result.success);
        assert_eq!(accept_result.new_status, Some(InvitationStatus::Accepted));

        // Decline the other
        let decline_result = service.decline(&inv2.invitation_id).await.unwrap();
        assert!(decline_result.success);
        assert_eq!(decline_result.new_status, Some(InvitationStatus::Declined));

        // Check pending is empty
        let pending = service.list_pending().await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_is_pending() {
        let authority_context = create_test_authority(120);
        let effects = effects_for(&authority_context);
        let service = InvitationServiceApi::new(effects, authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([121u8; 32]);
        let invitation = service
            .invite_as_contact(receiver_id, None, None, None)
            .await
            .unwrap();

        assert!(service.is_pending(&invitation.invitation_id).await);

        // Accept it
        service.accept(&invitation.invitation_id).await.unwrap();

        // No longer pending
        assert!(!service.is_pending(&invitation.invitation_id).await);
    }
}
