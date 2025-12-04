//! Invitation Handlers
//!
//! Handlers for invitation-related operations including creating, accepting,
//! and declining invitations for channels, guardians, and contacts.
//!
//! This module now uses `aura_invitation::InvitationService` internally for
//! guard chain integration while maintaining the same public API.

use super::invitation_bridge::execute_guard_outcome;
use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::RandomEffects;
use aura_core::identifiers::AuthorityId;
use aura_invitation::guards::GuardSnapshot;
use aura_invitation::{InvitationConfig, InvitationService as CoreInvitationService};
use aura_protocol::effects::EffectApiEffects;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Invitation type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationType {
    /// Invitation to join a block/channel
    Channel { block_id: String },
    /// Invitation to become a guardian
    Guardian { subject_authority: AuthorityId },
    /// Invitation to become a contact
    Contact { petname: Option<String> },
}

impl InvitationType {
    /// Convert to aura_invitation::InvitationType
    fn to_core_type(&self) -> aura_invitation::InvitationType {
        match self {
            InvitationType::Channel { block_id } => aura_invitation::InvitationType::Channel {
                block_id: block_id.clone(),
            },
            InvitationType::Guardian { subject_authority } => {
                aura_invitation::InvitationType::Guardian {
                    subject_authority: *subject_authority,
                }
            }
            InvitationType::Contact { petname } => aura_invitation::InvitationType::Contact {
                petname: petname.clone(),
            },
        }
    }
}

/// Invitation status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationStatus {
    /// Invitation is pending response
    Pending,
    /// Invitation was accepted
    Accepted,
    /// Invitation was declined
    Declined,
    /// Invitation was cancelled by sender
    Cancelled,
    /// Invitation has expired
    Expired,
}

impl From<aura_invitation::InvitationStatus> for InvitationStatus {
    fn from(status: aura_invitation::InvitationStatus) -> Self {
        match status {
            aura_invitation::InvitationStatus::Pending => InvitationStatus::Pending,
            aura_invitation::InvitationStatus::Accepted => InvitationStatus::Accepted,
            aura_invitation::InvitationStatus::Declined => InvitationStatus::Declined,
            aura_invitation::InvitationStatus::Cancelled => InvitationStatus::Cancelled,
            aura_invitation::InvitationStatus::Expired => InvitationStatus::Expired,
        }
    }
}

/// Created invitation details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invitation {
    /// Unique invitation identifier
    pub invitation_id: String,
    /// Sender authority
    pub sender_id: AuthorityId,
    /// Receiver authority
    pub receiver_id: AuthorityId,
    /// Type of invitation
    pub invitation_type: InvitationType,
    /// Current status
    pub status: InvitationStatus,
    /// Creation timestamp (ms)
    pub created_at: u64,
    /// Expiration timestamp (ms), if any
    pub expires_at: Option<u64>,
    /// Optional message
    pub message: Option<String>,
}

/// Result of an invitation action
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Create a new invitation handler
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        let service = CoreInvitationService::new(authority.authority_id, InvitationConfig::default());

        Ok(Self {
            context: HandlerContext::new(authority),
            service,
            pending_invitations: Arc::new(RwLock::new(HashMap::new())),
        })
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
            ]
        } else {
            // TODO: Get capabilities from Biscuit token or capability store
            vec![
                "invitation:send".to_string(),
                "invitation:accept".to_string(),
                "invitation:decline".to_string(),
                "invitation:cancel".to_string(),
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
        let core_type = invitation_type.to_core_type();

        let outcome = self.service.prepare_send_invitation(
            &snapshot,
            receiver_id,
            core_type,
            message.clone(),
            expires_in_ms,
            invitation_id.clone(),
        );

        // Execute the outcome (handles denial and effects)
        execute_guard_outcome(outcome, &self.context.authority, effects).await?;

        let invitation = Invitation {
            invitation_id: invitation_id.clone(),
            sender_id: self.context.authority.authority_id,
            receiver_id,
            invitation_type,
            status: InvitationStatus::Pending,
            created_at: current_time,
            expires_at,
            message,
        };

        // Cache the pending invitation
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

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects).await;
        let outcome = self.service.prepare_accept_invitation(&snapshot, invitation_id);

        // Execute the outcome
        execute_guard_outcome(outcome, &self.context.authority, effects).await?;

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

    /// Decline an invitation
    pub async fn decline_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &str,
    ) -> AgentResult<InvitationResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects).await;
        let outcome = self.service.prepare_decline_invitation(&snapshot, invitation_id);

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

        // Build snapshot and prepare through service
        let snapshot = self.build_snapshot(effects).await;
        let outcome = self.service.prepare_cancel_invitation(&snapshot, invitation_id);

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

    /// Get an invitation by ID
    pub async fn get_invitation(&self, invitation_id: &str) -> Option<Invitation> {
        let cache = self.pending_invitations.read().await;
        cache.get(invitation_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::RelationalContext;
    use crate::core::AgentConfig;
    use crate::runtime::effects::AuraEffectSystem;
    use aura_core::identifiers::{AuthorityId, ContextId};
    use std::sync::Arc;
    use tokio::sync::RwLock;

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
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([92u8; 32]);
        let effects_guard = effects.read().await;

        let invitation = handler
            .create_invitation(
                &effects_guard,
                receiver_id,
                InvitationType::Contact {
                    petname: Some("alice".to_string()),
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
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([94u8; 32]);
        let effects_guard = effects.read().await;

        let invitation = handler
            .create_invitation(
                &effects_guard,
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
            .accept_invitation(&effects_guard, &invitation.invitation_id)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, Some(InvitationStatus::Accepted));
    }

    #[tokio::test]
    async fn invitation_can_be_declined() {
        let authority_context = create_test_authority(96);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([97u8; 32]);
        let effects_guard = effects.read().await;

        let invitation = handler
            .create_invitation(
                &effects_guard,
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
            .decline_invitation(&effects_guard, &invitation.invitation_id)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, Some(InvitationStatus::Declined));
    }

    #[tokio::test]
    async fn invitation_can_be_cancelled() {
        let authority_context = create_test_authority(98);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let handler = InvitationHandler::new(authority_context).unwrap();

        let receiver_id = AuthorityId::new_from_entropy([99u8; 32]);
        let effects_guard = effects.read().await;

        let invitation = handler
            .create_invitation(
                &effects_guard,
                receiver_id,
                InvitationType::Contact { petname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let result = handler
            .cancel_invitation(&effects_guard, &invitation.invitation_id)
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
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let handler = InvitationHandler::new(authority_context).unwrap();

        let effects_guard = effects.read().await;

        // Create 3 invitations
        let inv1 = handler
            .create_invitation(
                &effects_guard,
                AuthorityId::new_from_entropy([101u8; 32]),
                InvitationType::Contact { petname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let inv2 = handler
            .create_invitation(
                &effects_guard,
                AuthorityId::new_from_entropy([102u8; 32]),
                InvitationType::Contact { petname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let _inv3 = handler
            .create_invitation(
                &effects_guard,
                AuthorityId::new_from_entropy([103u8; 32]),
                InvitationType::Contact { petname: None },
                None,
                None,
            )
            .await
            .unwrap();

        // Accept one, decline another
        handler
            .accept_invitation(&effects_guard, &inv1.invitation_id)
            .await
            .unwrap();
        handler
            .decline_invitation(&effects_guard, &inv2.invitation_id)
            .await
            .unwrap();

        // Only inv3 should be pending
        let pending = handler.list_pending().await;
        assert_eq!(pending.len(), 1);
    }
}
