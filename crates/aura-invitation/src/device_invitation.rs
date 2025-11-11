//! Device invitation helpers with FlowGuard integration.

use crate::{
    transport::deliver_via_rendezvous, AuraEffectSystem, InvitationError, InvitationResult,
};
use aura_core::effects::{NetworkEffects, TimeEffects};
use aura_core::{relationships::ContextId, AccountId, Cap, DeviceId};
use aura_journal::semilattice::{InvitationLedger, InvitationRecord};
use blake3::Hasher;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

static GLOBAL_INVITATION_LEDGER: Lazy<Arc<Mutex<InvitationLedger>>> =
    Lazy::new(|| Arc::new(Mutex::new(InvitationLedger::new())));

/// Device invitation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInvitationRequest {
    /// Device sending invitation
    pub inviter: DeviceId,
    /// Device being invited to join
    pub invitee: DeviceId,
    /// Account for device addition
    pub account_id: AccountId,
    /// Capabilities to grant to new device
    pub granted_capabilities: Cap,
    /// Device role description
    pub device_role: String,
    /// Invitation TTL in seconds (optional override)
    pub ttl_secs: Option<u64>,
}

/// Device invitation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInvitationResponse {
    /// Invitation envelope that was sent
    pub invitation: InvitationEnvelope,
    /// Invitation successfully broadcast
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Content-addressed invitation envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationEnvelope {
    /// Stable invitation identifier
    pub invitation_id: String,
    /// Inviter device
    pub inviter: DeviceId,
    /// Invitee device
    pub invitee: DeviceId,
    /// Account context
    pub account_id: AccountId,
    /// Capabilities being granted
    pub granted_capabilities: Cap,
    /// Role assigned to invitee
    pub device_role: String,
    /// Creation timestamp
    pub created_at: u64,
    /// Expiry timestamp
    pub expires_at: u64,
    /// Content hash for deduplication
    pub content_hash: Vec<u8>,
}

impl InvitationEnvelope {
    fn new(request: &DeviceInvitationRequest, created_at: u64, ttl_secs: u64) -> Self {
        let expires_at = created_at + ttl_secs;
        let invitation_id = format!("invitation-{}", Uuid::new_v4());
        let mut hasher = Hasher::new();
        hasher.update(invitation_id.as_bytes());
        hasher.update(request.inviter.to_string().as_bytes());
        hasher.update(request.invitee.to_string().as_bytes());
        hasher.update(request.account_id.to_string().as_bytes());
        hasher.update(&expires_at.to_be_bytes());
        let hash = hasher.finalize();

        Self {
            invitation_id,
            inviter: request.inviter,
            invitee: request.invitee,
            account_id: request.account_id,
            granted_capabilities: request.granted_capabilities.clone(),
            device_role: request.device_role.clone(),
            created_at,
            expires_at,
            content_hash: hash.as_bytes().to_vec(),
        }
    }
}

/// Device invitation coordinator with in-memory ledger.
pub struct DeviceInvitationCoordinator {
    effects: AuraEffectSystem,
    ledger: Arc<Mutex<InvitationLedger>>,
    default_ttl_secs: u64,
}

impl DeviceInvitationCoordinator {
    /// Create new device invitation coordinator.
    pub fn new(effect_system: AuraEffectSystem) -> Self {
        Self {
            effects: effect_system,
            ledger: Arc::clone(&GLOBAL_INVITATION_LEDGER),
            default_ttl_secs: 3600,
        }
    }

    /// Create coordinator with shared ledger reference.
    pub fn with_ledger(
        effect_system: AuraEffectSystem,
        ledger: Arc<Mutex<InvitationLedger>>,
    ) -> Self {
        Self {
            effects: effect_system,
            ledger,
            default_ttl_secs: 3600,
        }
    }

    /// Issue a device invitation and broadcast to the invitee.
    pub async fn invite_device(
        &self,
        request: DeviceInvitationRequest,
    ) -> InvitationResult<DeviceInvitationResponse> {
        let ttl = request.ttl_secs.unwrap_or(self.default_ttl_secs);
        if ttl == 0 {
            return Err(InvitationError::invalid(
                "invitation TTL must be greater than zero",
            ));
        }

        let created_at = self.effects.current_timestamp().await;
        let envelope = InvitationEnvelope::new(&request, created_at, ttl);
        self.record_invitation(&envelope).await?;
        self.send_invitation(&envelope).await?;

        Ok(DeviceInvitationResponse {
            invitation: envelope,
            success: true,
            error: None,
        })
    }

    async fn record_invitation(&self, envelope: &InvitationEnvelope) -> InvitationResult<()> {
        let record = InvitationRecord::pending(
            envelope.invitation_id.clone(),
            envelope.expires_at,
            envelope.created_at,
        );
        let mut ledger = self.ledger.lock().await;
        ledger.upsert(record);
        Ok(())
    }

    async fn send_invitation(&self, envelope: &InvitationEnvelope) -> InvitationResult<()> {
        let context = ContextId::hierarchical(&["invitation", &envelope.account_id.to_string()]);

        self.effects
            .set_flow_hint_components(context, envelope.invitee, 1)
            .await;

        let payload = serde_json::to_vec(envelope)
            .map_err(|err| InvitationError::serialization(err.to_string()))?;

        let ttl_window = envelope.expires_at.saturating_sub(envelope.created_at);
        deliver_via_rendezvous(
            &self.effects,
            &payload,
            envelope.inviter,
            envelope.invitee,
            ttl_window,
        )
        .await?;

        NetworkEffects::send_to_peer(&self.effects, envelope.invitee.0, payload)
            .await
            .map_err(|err| InvitationError::network(err.to_string()))?;
        
        Ok(())
    }

    /// Access the shared ledger (mainly for tests/status).
    pub async fn ledger_snapshot(&self) -> InvitationLedger {
        self.ledger.lock().await.clone()
    }
}

/// Access the shared invitation ledger used by other modules.
pub fn shared_invitation_ledger() -> Arc<Mutex<InvitationLedger>> {
    Arc::clone(&GLOBAL_INVITATION_LEDGER)
}
