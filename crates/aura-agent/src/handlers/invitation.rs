//! Invitation Handlers
//!
//! Handlers for invitation-related operations including creating, accepting,
//! and declining invitations for channels, guardians, and contacts.
//!
//! This module uses `aura_invitation::InvitationService` internally for
//! guard chain integration. Types are re-exported from `aura_invitation`.

use super::invitation_bridge::execute_guard_outcome;
use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::RandomEffects;
use aura_core::identifiers::AuthorityId;
use aura_invitation::guards::GuardSnapshot;
use aura_invitation::{InvitationConfig, InvitationService as CoreInvitationService};
use aura_protocol::effects::EffectApiEffects;
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
            // Capabilities will be derived from Biscuit token when integrated.
            // Currently uses default set for non-testing mode.
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
        let outcome = self
            .service
            .prepare_accept_invitation(&snapshot, invitation_id);

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

    /// Get an invitation by ID
    pub async fn get_invitation(&self, invitation_id: &str) -> Option<Invitation> {
        let cache = self.pending_invitations.read().await;
        cache.get(invitation_id).cloned()
    }
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
                InvitationType::Contact { nickname: None },
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
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let inv2 = handler
            .create_invitation(
                &effects_guard,
                AuthorityId::new_from_entropy([102u8; 32]),
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .unwrap();

        let _inv3 = handler
            .create_invitation(
                &effects_guard,
                AuthorityId::new_from_entropy([103u8; 32]),
                InvitationType::Contact { nickname: None },
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
