//! Invitation Service - Public API for Invitation Operations
//!
//! Provides a clean public interface for invitation operations.
//! Wraps `InvitationHandler` with ergonomic methods and proper error handling.

use super::invitation::{
    Invitation, InvitationHandler, InvitationResult, InvitationStatus, InvitationType,
    ShareableInvitation, ShareableInvitationError,
};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::identifiers::AuthorityId;
use std::sync::Arc;

/// Invitation service
///
/// Provides invitation operations through a clean public API.
pub struct InvitationService {
    handler: InvitationHandler,
    effects: Arc<AuraEffectSystem>,
}

impl InvitationService {
    /// Create a new invitation service
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
    ) -> AgentResult<Self> {
        let handler = InvitationHandler::new(authority_context)?;
        Ok(Self { handler, effects })
    }

    /// Create an invitation to a channel/block
    ///
    /// # Arguments
    /// * `receiver_id` - Authority to invite
    /// * `block_id` - Block/channel ID to invite to
    /// * `message` - Optional message
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The created invitation
    pub async fn invite_to_channel(
        &self,
        receiver_id: AuthorityId,
        block_id: String,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        self.handler
            .create_invitation(
                &self.effects,
                receiver_id,
                InvitationType::Channel { block_id },
                message,
                expires_in_ms,
            )
            .await
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
        self.handler
            .create_invitation(
                &self.effects,
                receiver_id,
                InvitationType::Guardian { subject_authority },
                message,
                expires_in_ms,
            )
            .await
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
        self.handler
            .create_invitation(
                &self.effects,
                receiver_id,
                InvitationType::Contact { nickname },
                message,
                expires_in_ms,
            )
            .await
    }

    /// Accept an invitation
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to accept
    ///
    /// # Returns
    /// Result of the acceptance
    pub async fn accept(&self, invitation_id: &str) -> AgentResult<InvitationResult> {
        self.handler
            .accept_invitation(&self.effects, invitation_id)
            .await
    }

    /// Decline an invitation
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to decline
    ///
    /// # Returns
    /// Result of the decline
    pub async fn decline(&self, invitation_id: &str) -> AgentResult<InvitationResult> {
        self.handler
            .decline_invitation(&self.effects, invitation_id)
            .await
    }

    /// Cancel an invitation (sender only)
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to cancel
    ///
    /// # Returns
    /// Result of the cancellation
    pub async fn cancel(&self, invitation_id: &str) -> AgentResult<InvitationResult> {
        self.handler
            .cancel_invitation(&self.effects, invitation_id)
            .await
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
    pub async fn get(&self, invitation_id: &str) -> Option<Invitation> {
        self.handler.get_invitation(invitation_id).await
    }

    /// Check if an invitation is pending
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation
    ///
    /// # Returns
    /// True if the invitation exists and is pending
    pub async fn is_pending(&self, invitation_id: &str) -> bool {
        self.handler
            .get_invitation(invitation_id)
            .await
            .map(|inv| inv.status == InvitationStatus::Pending)
            .unwrap_or(false)
    }

    // =========================================================================
    // Sharing Methods (Out-of-Band Transfer)
    // =========================================================================

    /// Export an invitation as a shareable code string
    ///
    /// The code can be shared out-of-band (copy/paste, QR code, etc.)
    /// and imported by the receiver using `import_code`.
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to export
    ///
    /// # Returns
    /// A shareable code string (format: `aura:v1:<base64>`)
    ///
    /// # Errors
    /// Returns an error if the invitation is not found
    pub async fn export_code(&self, invitation_id: &str) -> AgentResult<String> {
        let invitation = self
            .handler
            .get_invitation(invitation_id)
            .await
            .ok_or_else(|| {
                aura_core::AuraError::not_found(format!("Invitation not found: {}", invitation_id))
            })?;

        let shareable = ShareableInvitation::from(&invitation);
        Ok(shareable.to_code())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::RelationalContext;
    use crate::core::AgentConfig;
    use aura_core::identifiers::ContextId;

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
    async fn test_invitation_service_creation() {
        let authority_context = create_test_authority(110);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));

        let service = InvitationService::new(effects, authority_context);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_invite_as_contact() {
        let authority_context = create_test_authority(111);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = InvitationService::new(effects, authority_context).unwrap();

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

        assert!(invitation.invitation_id.starts_with("inv-"));
        assert_eq!(invitation.receiver_id, receiver_id);
        assert_eq!(invitation.status, InvitationStatus::Pending);
    }

    #[tokio::test]
    async fn test_invite_as_guardian() {
        let authority_context = create_test_authority(113);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = InvitationService::new(effects, authority_context.clone()).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([114u8; 32]);
        let invitation = service
            .invite_as_guardian(
                receiver_id,
                authority_context.authority_id,
                Some("Please guard my identity".to_string()),
                Some(604800000), // 1 week
            )
            .await
            .unwrap();

        assert!(invitation.invitation_id.starts_with("inv-"));
        assert!(invitation.expires_at.is_some());
    }

    #[tokio::test]
    async fn test_invite_to_channel() {
        let authority_context = create_test_authority(115);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = InvitationService::new(effects, authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([116u8; 32]);
        let invitation = service
            .invite_to_channel(receiver_id, "channel-123".to_string(), None, None)
            .await
            .unwrap();

        assert!(invitation.invitation_id.starts_with("inv-"));
    }

    #[tokio::test]
    async fn test_accept_decline_flow() {
        let authority_context = create_test_authority(117);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = InvitationService::new(effects, authority_context).unwrap();

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
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let service = InvitationService::new(effects, authority_context).unwrap();

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
