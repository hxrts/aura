//! Invitation orchestration exposed by the agent.

use crate::errors::{AuraError, Result};
use crate::runtime::AuraEffectSystem;
use aura_core::hash;
use aura_core::{RelationshipId, TrustLevel};
use aura_invitation::{
    device_invitation::{
        shared_invitation_registry, DeviceInvitationCoordinator, DeviceInvitationRequest,
        DeviceInvitationResponse, InvitationEnvelope,
    },
    invitation_acceptance::{InvitationAcceptance, InvitationAcceptanceCoordinator},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Invitation operations available to higher layers.
pub struct InvitationOperations {
    effects: Arc<RwLock<AuraEffectSystem>>,
}

impl InvitationOperations {
    /// Create new invitation operations handler.
    pub fn new(effects: Arc<RwLock<AuraEffectSystem>>) -> Self {
        Self { effects }
    }

    /// Create a device invitation envelope.
    pub async fn create_device_invitation(
        &self,
        request: DeviceInvitationRequest,
    ) -> Result<DeviceInvitationResponse> {
        // Validate the request
        if let Err(validation_error) = self.validate_invitation_request(&request).await {
            return Ok(DeviceInvitationResponse {
                invitation: self.create_placeholder_envelope(&request).await,
                success: false,
                error: Some(validation_error),
            });
        }

        // Create the invitation envelope
        let invitation = self.create_invitation_envelope(&request).await?;

        // Simulate sending the invitation
        let send_result = self.send_invitation(&invitation).await;

        match send_result {
            Ok(()) => Ok(DeviceInvitationResponse {
                invitation,
                success: true,
                error: None,
            }),
            Err(send_error) => Ok(DeviceInvitationResponse {
                invitation,
                success: false,
                error: Some(format!("Failed to send invitation: {}", send_error)),
            }),
        }
    }

    /// Accept a received invitation envelope.
    pub async fn accept_invitation(
        &self,
        envelope: InvitationEnvelope,
    ) -> Result<InvitationAcceptance> {
        // Validate the invitation envelope
        if let Err(validation_error) = self.validate_invitation_envelope(&envelope).await {
            return Ok(InvitationAcceptance {
                invitation_id: envelope.invitation_id,
                invitee: envelope.invitee,
                inviter: envelope.inviter,
                account_id: envelope.account_id,
                granted_token: envelope.granted_token,
                device_role: envelope.device_role,
                accepted_at: chrono::Utc::now().timestamp() as u64,
                relationship_id: None,
                success: false,
                error_message: Some(validation_error),
            });
        }

        // Process the invitation acceptance
        let acceptance = self.process_invitation_acceptance(envelope).await?;

        Ok(acceptance)
    }

    /// Validate an invitation request
    async fn validate_invitation_request(
        &self,
        request: &DeviceInvitationRequest,
    ) -> std::result::Result<(), String> {
        // Check if inviter and invitee are different
        if request.inviter == request.invitee {
            return Err("Cannot invite yourself".to_string());
        }

        // Validate TTL
        let ttl_secs = request.ttl_secs.unwrap_or(86400); // Default 24 hours
        if ttl_secs == 0 || ttl_secs > 604800 {
            // Max 1 week
            return Err("Invalid TTL: must be between 1 second and 1 week".to_string());
        }

        // Check if device role is valid
        if request.device_role.trim().is_empty() {
            return Err("Device role cannot be empty".to_string());
        }

        Ok(())
    }

    /// Create invitation envelope from request
    async fn create_invitation_envelope(
        &self,
        request: &DeviceInvitationRequest,
    ) -> Result<InvitationEnvelope> {
        let created_at = chrono::Utc::now().timestamp() as u64;
        let ttl_secs = request.ttl_secs.unwrap_or(86400); // Default 24 hours
        let expires_at = created_at + ttl_secs;
        let invitation_id = format!("invitation-{}", Uuid::new_v4());

        // Create content hash
        let mut hasher = hash::hasher();
        hasher.update(invitation_id.as_bytes());
        hasher.update(request.inviter.0.as_bytes());
        hasher.update(request.invitee.0.as_bytes());
        hasher.update(request.account_id.0.as_bytes());
        hasher.update(&expires_at.to_be_bytes());
        hasher.update(request.device_role.as_bytes());
        let content_hash = hasher.finalize().to_vec();

        Ok(InvitationEnvelope {
            invitation_id,
            inviter: request.inviter,
            invitee: request.invitee,
            account_id: request.account_id,
            granted_token: request.granted_token.clone(),
            device_role: request.device_role.clone(),
            created_at,
            expires_at,
            content_hash,
        })
    }

    /// Create placeholder envelope for error cases
    async fn create_placeholder_envelope(
        &self,
        request: &DeviceInvitationRequest,
    ) -> InvitationEnvelope {
        let created_at = chrono::Utc::now().timestamp() as u64;

        InvitationEnvelope {
            invitation_id: "error-invitation".to_string(),
            inviter: request.inviter,
            invitee: request.invitee,
            account_id: request.account_id,
            granted_token: request.granted_token.clone(),
            device_role: request.device_role.clone(),
            created_at,
            expires_at: created_at,
            content_hash: vec![],
        }
    }

    /// Simulate sending invitation
    async fn send_invitation(&self, _invitation: &InvitationEnvelope) -> Result<()> {
        // In a real implementation, this would:
        // 1. Store the invitation in the shared effect API
        // 2. Send the invitation through the transport layer
        // 3. Handle delivery confirmation

        // For now, simulate successful sending
        Ok(())
    }

    /// Validate invitation envelope
    async fn validate_invitation_envelope(
        &self,
        envelope: &InvitationEnvelope,
    ) -> std::result::Result<(), String> {
        let current_time = chrono::Utc::now().timestamp() as u64;

        // Check if invitation has expired
        if current_time > envelope.expires_at {
            return Err("Invitation has expired".to_string());
        }

        // Validate content hash integrity
        let mut hasher = hash::hasher();
        hasher.update(envelope.invitation_id.as_bytes());
        hasher.update(envelope.inviter.0.as_bytes());
        hasher.update(envelope.invitee.0.as_bytes());
        hasher.update(envelope.account_id.0.as_bytes());
        hasher.update(&envelope.expires_at.to_be_bytes());
        hasher.update(envelope.device_role.as_bytes());
        let expected_hash = hasher.finalize().to_vec();

        if expected_hash != envelope.content_hash {
            return Err("Invalid invitation: content hash mismatch".to_string());
        }

        Ok(())
    }

    /// Process invitation acceptance
    async fn process_invitation_acceptance(
        &self,
        envelope: InvitationEnvelope,
    ) -> Result<InvitationAcceptance> {
        let accepted_at = chrono::Utc::now().timestamp() as u64;

        // In a real implementation, this would:
        // 1. Create device relationship in the journal
        // 2. Add capabilities to the device
        // 3. Update the invitation effect API
        // 4. Send confirmation back to inviter

        // Simulate relationship creation
        let uuid = Uuid::new_v4();
        let uuid_bytes = uuid.as_bytes();
        let mut relationship_bytes = [0u8; 32];
        relationship_bytes[..16].copy_from_slice(uuid_bytes);
        let relationship_id = Some(RelationshipId(relationship_bytes));

        // Simulate successful acceptance
        let acceptance = InvitationAcceptance {
            invitation_id: envelope.invitation_id,
            invitee: envelope.invitee,
            inviter: envelope.inviter,
            account_id: envelope.account_id,
            granted_token: envelope.granted_token,
            device_role: envelope.device_role,
            accepted_at,
            relationship_id,
            success: true,
            error_message: None,
        };

        Ok(acceptance)
    }
}
