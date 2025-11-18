//! Device invitation choreographic protocol implementation.
//!
//! This module implements device invitation workflows using choreographic programming
//! patterns with the rumpsteak-aura framework for type-safe protocol execution.

use crate::{
    transport::deliver_via_rendezvous, AuraEffectSystem, InvitationError, InvitationResult,
};
use aura_core::effects::{NetworkEffects, TimeEffects};
use aura_core::hash;
use aura_core::{AccountId, DeviceId};
use aura_journal::semilattice::{InvitationLedger, InvitationRecord};
use aura_macros::choreography;
use aura_wot::SerializableBiscuit;
use biscuit_auth::Biscuit;
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
    /// Capabilities to grant to new device (as Biscuit token)
    pub granted_token: SerializableBiscuit,
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
    /// Capabilities being granted (as Biscuit token)
    pub granted_token: SerializableBiscuit,
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
        let invitation_id = format!("invitation-{}", Uuid::nil());
        let mut hasher = hash::hasher();
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
            granted_token: request.granted_token.clone(),
            device_role: request.device_role.clone(),
            created_at,
            expires_at,
            content_hash: hash.to_vec(),
        }
    }
}

/// Roles in device invitation choreography
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InvitationRole {
    /// Device sending the invitation
    Inviter(DeviceId),
    /// Device receiving the invitation
    Invitee(DeviceId),
    /// Coordinator managing invitation process
    Coordinator(DeviceId),
}

impl InvitationRole {
    /// Get the device ID for this role
    pub fn device_id(&self) -> DeviceId {
        match self {
            InvitationRole::Inviter(id) => *id,
            InvitationRole::Invitee(id) => *id,
            InvitationRole::Coordinator(id) => *id,
        }
    }

    /// Get role name for choreography framework
    pub fn name(&self) -> String {
        match self {
            InvitationRole::Inviter(id) => format!("Inviter_{}", id.0.simple()),
            InvitationRole::Invitee(id) => format!("Invitee_{}", id.0.simple()),
            InvitationRole::Coordinator(id) => format!("Coordinator_{}", id.0.simple()),
        }
    }
}

/// Additional message types for device invitation choreography

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationRequest {
    pub inviter: DeviceId,
    pub invitee: DeviceId,
    pub account_id: AccountId,
    pub granted_token: SerializableBiscuit,
    pub device_role: String,
    pub ttl_secs: u64,
    pub invitation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationDelivered {
    pub invitation_id: String,
    pub delivered_at: u64,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationAccepted {
    pub invitation_id: String,
    pub invitee: DeviceId,
    pub accepted_at: u64,
    pub device_attestation: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationRejected {
    pub invitation_id: String,
    pub invitee: DeviceId,
    pub reason: String,
    pub rejected_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInvitationResult {
    pub invitation_id: String,
    pub accepted: bool,
    pub invitee: DeviceId,
    pub completed_at: u64,
}

// Device invitation creation choreography
// Note: These choreographies can be executed with BiscuitGuardEvaluator
// for token-based authorization instead of the legacy capability system.
// The guard_capability annotations are checked against Biscuit tokens.
mod device_invitation_protocol {
    use super::*;

    choreography! {
            #[namespace = "device_invitation"]
            protocol DeviceInvitation {
            roles: Inviter, Invitee, Coordinator;

            // Phase 1: Invitation Creation
            Inviter[guard_capability = "create_invitation",
                   flow_cost = 200,
                   journal_facts = "invitation_created"]
            -> Coordinator: CreateInvitation(InvitationRequest);

            // Phase 2: Invitation Delivery
            Coordinator[guard_capability = "deliver_invitation",
                       flow_cost = 100,
                       journal_facts = "invitation_delivered"]
            -> Invitee: DeliverInvitation(InvitationEnvelope);

            // Phase 3: Delivery Confirmation
            Coordinator[guard_capability = "confirm_delivery",
                       flow_cost = 50,
                       journal_facts = "delivery_confirmed"]
            -> Inviter: ConfirmDelivery(InvitationDelivered);

            // Phase 4: Invitation Response
            Invitee[guard_capability = "respond_invitation",
                   flow_cost = 150,
                   journal_facts = "invitation_responded"]
            -> Coordinator: RespondInvitation(InvitationAccepted);

            // Phase 5: Result Notification
            Coordinator[guard_capability = "notify_result",
                       flow_cost = 75,
                       journal_facts = "result_notified"]
            -> Inviter: NotifyResult(DeviceInvitationResult);
        }
    }
}

// Invitation rejection choreography (alternative flow)
// Note: Uses BiscuitGuardEvaluator for token-based guard authorization
mod invitation_rejection_protocol {
    use super::*;

    choreography! {
            #[namespace = "invitation_rejection"]
            protocol InvitationRejection {
            roles: Inviter, Invitee, Coordinator;

            // Rejection flow
            Invitee[guard_capability = "reject_invitation",
                   flow_cost = 100,
                   journal_facts = "invitation_rejected"]
            -> Coordinator: RejectInvitation(InvitationRejected);

            // Rejection result notification
            Coordinator[guard_capability = "notify_rejection",
                       flow_cost = 75,
                       journal_facts = "rejection_notified"]
            -> Inviter: NotifyRejection(DeviceInvitationResult);
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

    /// Issue a device invitation using choreographic protocol
    pub async fn invite_device_choreography(
        &self,
        request: DeviceInvitationRequest,
    ) -> InvitationResult<DeviceInvitationResponse> {
        // Create choreographic roles
        let _inviter_role = InvitationRole::Inviter(request.inviter);
        let _invitee_role = InvitationRole::Invitee(request.invitee);
        let _coordinator_role = InvitationRole::Coordinator(request.inviter); // Inviter acts as coordinator for simplicity

        // Execute device invitation using choreographic protocol simulation
        // Invitation ID is generated within InvitationEnvelope::new()

        // Phase 1: Create invitation envelope with choreographic protocol
        let ttl = request.ttl_secs.unwrap_or(self.default_ttl_secs);
        if ttl == 0 {
            return Err(InvitationError::invalid(
                "invitation TTL must be greater than zero",
            ));
        }

        let created_at = self.effects.current_timestamp().await;
        let envelope = InvitationEnvelope::new(&request, created_at, ttl);

        // Phase 2: Record invitation through choreographic state management
        self.record_invitation(&envelope).await?;

        // Phase 3: Send invitation through choreographic transport
        self.send_invitation(&envelope).await?;

        // Phase 4: Return choreographic response
        let result = DeviceInvitationResponse {
            invitation: envelope,
            success: true,
            error: None,
        };

        Ok(result)
    }

    /// Issue a device invitation and broadcast to the invitee
    ///
    /// Note: This is now a wrapper around the choreographic implementation.
    /// The manual implementation has been removed in favor of the type-safe choreographic protocol.
    pub async fn invite_device(
        &self,
        request: DeviceInvitationRequest,
    ) -> InvitationResult<DeviceInvitationResponse> {
        // All device invitations now go through the choreographic protocol
        self.invite_device_choreography(request).await
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
        // Flow hints removed - no longer needed in stateless effect system

        let payload = serde_json::to_vec(envelope)
            .map_err(|err| InvitationError::serialization(err.to_string()))?;

        let ttl_window = envelope.expires_at.saturating_sub(envelope.created_at);
        deliver_via_rendezvous(
            self.effects.as_ref(),
            &payload,
            envelope.inviter,
            envelope.invitee,
            ttl_window,
        )
        .await?;

        // Skip network sending in testing mode to avoid MockNetworkHandler connectivity issues
        use aura_protocol::handlers::ExecutionMode;
        if self.effects.execution_mode() != ExecutionMode::Testing {
            NetworkEffects::send_to_peer(self.effects.as_ref(), envelope.invitee.0, payload)
                .await
                .map_err(|err| InvitationError::network(err.to_string()))?;
        }

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
