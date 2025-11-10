//! Invitation acceptance helpers that interact with the shared ledger.

use crate::{
    device_invitation::{shared_invitation_ledger, InvitationEnvelope},
    transport::deliver_via_rendezvous,
    InvitationError, InvitationResult,
};
use aura_core::{relationships::ContextId, DeviceId};
use aura_journal::semilattice::InvitationLedger;
use aura_protocol::effects::{AuraEffectSystem, NetworkEffects, TimeEffects};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Result of accepting an invitation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationAcceptance {
    /// Invitation identifier
    pub invitation_id: String,
    /// Invitee device
    pub invitee: DeviceId,
    /// Timestamp of acceptance
    pub accepted_at: u64,
}

/// Coordinator used by invitees to accept pending invitations.
pub struct InvitationAcceptanceCoordinator {
    effects: AuraEffectSystem,
    ledger: Arc<Mutex<InvitationLedger>>,
}

impl InvitationAcceptanceCoordinator {
    /// Create a new acceptance coordinator.
    pub fn new(effect_system: AuraEffectSystem) -> Self {
        Self {
            effects: effect_system,
            ledger: shared_invitation_ledger(),
        }
    }

    /// Create with explicit ledger reference (useful for tests).
    pub fn with_ledger(
        effect_system: AuraEffectSystem,
        ledger: Arc<Mutex<InvitationLedger>>,
    ) -> Self {
        Self {
            effects: effect_system,
            ledger,
        }
    }

    /// Accept an invitation envelope if it is still valid.
    pub async fn accept_invitation(
        &self,
        envelope: InvitationEnvelope,
    ) -> InvitationResult<InvitationAcceptance> {
        let now = self.effects.current_timestamp().await;
        if now > envelope.expires_at {
            let mut ledger = self.ledger.lock().await;
            ledger.mark_expired(&envelope.invitation_id, now);
            return Err(InvitationError::invalid("invitation has expired"));
        }

        {
            let mut ledger = self.ledger.lock().await;
            ledger.mark_accepted(&envelope.invitation_id, now);
        }

        self.send_ack(&envelope).await?;

        Ok(InvitationAcceptance {
            invitation_id: envelope.invitation_id,
            invitee: envelope.invitee,
            accepted_at: now,
        })
    }

    async fn send_ack(&self, envelope: &InvitationEnvelope) -> InvitationResult<()> {
        let context =
            ContextId::hierarchical(&["invitation-ack", &envelope.account_id.to_string()]);
        self.effects
            .set_flow_hint_components(context, envelope.inviter, 1)
            .await;

        let ack = serde_json::json!({
            "invitation_id": envelope.invitation_id,
            "invitee": envelope.invitee,
            "accepted_at": self.effects.current_timestamp().await,
        });
        let payload = serde_json::to_vec(&ack)
            .map_err(|err| InvitationError::serialization(err.to_string()))?;

        let ttl_window = envelope.expires_at.saturating_sub(envelope.created_at);
        deliver_via_rendezvous(
            &self.effects,
            &payload,
            envelope.invitee,
            envelope.inviter,
            ttl_window,
        )
        .await?;

        NetworkEffects::send_to_peer(&self.effects, envelope.inviter.0, payload)
            .await
            .map_err(|err| InvitationError::network(err.to_string()))
    }
}
