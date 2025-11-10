//! Invitation orchestration exposed by the agent.

use crate::errors::{AuraError, Result};
use aura_invitation::{
    device_invitation::{
        shared_invitation_ledger, DeviceInvitationCoordinator, DeviceInvitationRequest,
        DeviceInvitationResponse, InvitationEnvelope,
    },
    invitation_acceptance::{InvitationAcceptance, InvitationAcceptanceCoordinator},
};
use aura_protocol::effects::AuraEffectSystem;
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
        let effects = self.effects.read().await;
        let coordinator =
            DeviceInvitationCoordinator::with_ledger(effects.clone(), shared_invitation_ledger());
        coordinator
            .invite_device(request)
            .await
            .map_err(|err| AuraError::internal(err.to_string()))
    }

    /// Accept a received invitation envelope.
    pub async fn accept_invitation(
        &self,
        envelope: InvitationEnvelope,
    ) -> Result<InvitationAcceptance> {
        let effects = self.effects.read().await;
        let coordinator = InvitationAcceptanceCoordinator::new(effects.clone());
        coordinator
            .accept_invitation(envelope)
            .await
            .map_err(|err| AuraError::internal(err.to_string()))
    }
}
