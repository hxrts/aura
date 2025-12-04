//! Invitation Handlers
//!
//! Handlers for invitation-related operations including creating, accepting,
//! and declining invitations for channels, guardians, and contacts.

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::RandomEffects;
use aura_core::identifiers::AuthorityId;
use aura_invitation::facts::InvitationFact;
use aura_journal::DomainFact;
use aura_protocol::effects::EffectApiEffects;
use aura_protocol::guards::send_guard::create_send_guard;
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
    /// Convert to string type for InvitationFact
    fn as_type_string(&self) -> String {
        match self {
            InvitationType::Channel { .. } => "channel".to_string(),
            InvitationType::Guardian { .. } => "guardian".to_string(),
            InvitationType::Contact { .. } => "contact".to_string(),
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
pub struct InvitationHandler {
    context: HandlerContext,
    /// Cache of pending invitations (for quick lookup)
    pending_invitations: Arc<RwLock<HashMap<String, Invitation>>>,
}

impl InvitationHandler {
    /// Create a new invitation handler
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        Ok(Self {
            context: HandlerContext::new(authority),
            pending_invitations: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get the authority context
    pub fn authority_context(&self) -> &AuthorityContext {
        &self.context.authority
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

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                "invitation:create".to_string(),
                self.context.effect_context.context_id(),
                self.context.authority.authority_id,
                50,
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(
                    result
                        .denial_reason
                        .unwrap_or_else(|| "invitation create not authorized".to_string()),
                ));
            }
        }

        // Generate unique invitation ID
        let invitation_id = format!("inv-{}", effects.random_uuid().await.simple());

        let current_time = effects.current_timestamp().await.unwrap_or(0);
        let expires_at = expires_in_ms.map(|ms| current_time + ms);

        // Create the invitation fact
        let fact = InvitationFact::Sent {
            context_id: self.context.effect_context.context_id(),
            invitation_id: invitation_id.clone(),
            sender_id: self.context.authority.authority_id,
            receiver_id,
            invitation_type: invitation_type.as_type_string(),
            sent_at_ms: current_time,
            expires_at_ms: expires_at,
            message: message.clone(),
        };

        // Journal the invitation fact
        HandlerUtilities::append_generic_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "invitation",
            &fact.to_bytes(),
        )
        .await?;

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

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                "invitation:accept".to_string(),
                self.context.effect_context.context_id(),
                self.context.authority.authority_id,
                50,
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(
                    result
                        .denial_reason
                        .unwrap_or_else(|| "invitation accept not authorized".to_string()),
                ));
            }
        }

        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Create acceptance fact
        let fact = InvitationFact::Accepted {
            invitation_id: invitation_id.to_string(),
            acceptor_id: self.context.authority.authority_id,
            accepted_at_ms: current_time,
        };

        // Journal the acceptance
        HandlerUtilities::append_generic_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "invitation",
            &fact.to_bytes(),
        )
        .await?;

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

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                "invitation:decline".to_string(),
                self.context.effect_context.context_id(),
                self.context.authority.authority_id,
                30,
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(result.denial_reason.unwrap_or_else(
                    || "invitation decline not authorized".to_string(),
                )));
            }
        }

        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Create decline fact
        let fact = InvitationFact::Declined {
            invitation_id: invitation_id.to_string(),
            decliner_id: self.context.authority.authority_id,
            declined_at_ms: current_time,
        };

        // Journal the decline
        HandlerUtilities::append_generic_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "invitation",
            &fact.to_bytes(),
        )
        .await?;

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

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                "invitation:cancel".to_string(),
                self.context.effect_context.context_id(),
                self.context.authority.authority_id,
                30,
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(
                    result
                        .denial_reason
                        .unwrap_or_else(|| "invitation cancel not authorized".to_string()),
                ));
            }
        }

        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Create cancellation fact
        let fact = InvitationFact::Cancelled {
            invitation_id: invitation_id.to_string(),
            canceller_id: self.context.authority.authority_id,
            cancelled_at_ms: current_time,
        };

        // Journal the cancellation
        HandlerUtilities::append_generic_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "invitation",
            &fact.to_bytes(),
        )
        .await?;

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
