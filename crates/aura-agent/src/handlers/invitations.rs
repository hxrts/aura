//! Invitation orchestration exposed by the agent.

use crate::errors::{AuraError, Result};
use aura_invitation::{
    device_invitation::{
        shared_invitation_ledger, DeviceInvitationCoordinator, DeviceInvitationRequest,
        DeviceInvitationResponse, InvitationEnvelope,
    },
    invitation_acceptance::{InvitationAcceptance, InvitationAcceptanceCoordinator},
};
use crate::runtime::AuraEffectSystem;
use std::sync::Arc;
use tokio::sync::RwLock;

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
        // TODO: Fix coordinator creation - requires refactoring to use Arc<dyn AuraEffects>
        let _ = self.effects.read().await;
        Err(AuraError::internal(
            "Device invitation not yet implemented - requires Arc-based effect system",
        ))
    }

    /// Accept a received invitation envelope.
    pub async fn accept_invitation(
        &self,
        envelope: InvitationEnvelope,
    ) -> Result<InvitationAcceptance> {
        // TODO: Fix coordinator creation - requires refactoring to use Arc<dyn AuraEffects>
        let _ = self.effects.read().await;
        Err(AuraError::internal(
            "Invitation acceptance not yet implemented - requires Arc-based effect system",
        ))
    }
}
