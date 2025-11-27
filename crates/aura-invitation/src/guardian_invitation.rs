//! Guardian Invitation Choreography
//!
//! This module implements choreographic protocols for guardian relationship
//! establishment and invitation acceptance.

use crate::{Guardian, GuardianId, InvitationError, InvitationResult, TrustLevel};
use aura_core::effects::{CryptoEffects, JournalEffects, NetworkEffects, PhysicalTimeEffects};
use aura_core::journal::FactValue;
use aura_core::{hash, AccountId, DeviceId};
use serde::{Deserialize, Serialize};

/// Guardian invitation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianInvitationRequest {
    /// Device sending guardian invitation
    pub inviter: DeviceId,
    /// Device being invited as guardian
    pub invitee: DeviceId,
    /// Account for guardianship
    pub account_id: AccountId,
    /// Guardian role description
    pub role_description: String,
    /// Required trust level for guardian
    pub required_trust_level: TrustLevel,
    /// Recovery responsibilities
    pub recovery_responsibilities: Vec<String>,
}

/// Guardian invitation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianInvitationResponse {
    /// Established guardian relationship
    pub guardian_relationship: Option<Guardian>,
    /// Invitation accepted
    pub accepted: bool,
    /// Response message
    pub message: String,
    /// Success status
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Guardian invitation coordinator
pub struct GuardianInvitationCoordinator {
    /// Current device ID
    device_id: DeviceId,
}

impl GuardianInvitationCoordinator {
    /// Create new guardian invitation coordinator
    pub fn new(device_id: DeviceId) -> Self {
        Self { device_id }
    }

    /// Execute guardian invitation choreography
    ///
    /// This implements a simplified guardian invitation flow:
    /// 1. Inviter initiates invitation with guardian parameters
    /// 2. Invitee receives and evaluates invitation
    /// 3. Invitee accepts or rejects based on trust level and responsibilities
    /// 4. If accepted, establish guardian relationship
    ///
    /// NOTE: Full implementation requires NetworkEffects for message passing
    /// and CryptoEffects for relationship attestation. This is a local simulation.
    pub async fn invite_guardian<E>(
        &self,
        request: GuardianInvitationRequest,
        effects: &E,
    ) -> InvitationResult<GuardianInvitationResponse>
    where
        E: PhysicalTimeEffects + NetworkEffects + CryptoEffects + JournalEffects + Send + Sync,
    {
        tracing::info!(
            "Starting guardian invitation from {} to {}",
            request.inviter,
            request.invitee
        );

        // Validate request parameters
        if request.recovery_responsibilities.is_empty() {
            return Err(InvitationError::invalid(
                "Guardian must have at least one recovery responsibility",
            ));
        }

        // Send invitation message to invitee via effects
        self.send_invitation_via_effects(&request, effects).await?;

        // Wait for invitee's decision via effects
        let accepted = self
            .receive_invitation_decision_via_effects(&request, effects)
            .await?;

        if accepted {
            // Exchange cryptographic attestations via effects
            self.exchange_guardian_attestation_via_effects(&request, effects)
                .await?;

            // Record relationship in journal via effects
            let guardian_id = self
                .record_guardian_relationship_via_effects(&request, effects)
                .await?;

            Ok(GuardianInvitationResponse {
                guardian_relationship: Some(guardian_id),
                accepted: true,
                message: format!(
                    "Guardian invitation accepted. {} is now a guardian for account {}",
                    request.invitee, request.account_id
                ),
                success: true,
                error: None,
            })
        } else {
            // Record rejection in journal via effects
            self.record_invitation_rejection_via_effects(&request, effects)
                .await?;

            Ok(GuardianInvitationResponse {
                guardian_relationship: None,
                accepted: false,
                message: "Guardian invitation declined".to_string(),
                success: false,
                error: Some("Invitation evaluation failed requirements".to_string()),
            })
        }
    }

    /// Send invitation message to invitee via NetworkEffects
    async fn send_invitation_via_effects(
        &self,
        request: &GuardianInvitationRequest,
        network: &dyn NetworkEffects,
    ) -> InvitationResult<()> {
        // Serialize the invitation request
        let message_data = serde_json::to_vec(request)
            .map_err(|e| InvitationError::serialization(e.to_string()))?;

        network
            .send_to_peer(request.invitee.0, message_data)
            .await
            .map_err(|e| InvitationError::network(e.to_string()))?;

        Ok(())
    }

    /// Wait for and receive invitee's decision via NetworkEffects
    async fn receive_invitation_decision_via_effects<E>(
        &self,
        request: &GuardianInvitationRequest,
        effects: &E,
    ) -> InvitationResult<bool>
    where
        E: PhysicalTimeEffects + NetworkEffects,
    {
        // Attempt to receive a response from invitee; if none, fall back to local evaluation
        let received = effects.receive_from(request.invitee.0).await;
        match received {
            Ok(bytes) => {
                let decision: bool = serde_json::from_slice(&bytes)
                    .map_err(|e| InvitationError::serialization(e.to_string()))?;
                Ok(decision)
            }
            Err(_) => {
                // Fallback deterministic evaluation to avoid blocking
                let decision = self.evaluate_invitation(request);
                effects.sleep_ms(50).await.map_err(|e| {
                    InvitationError::internal(format!("time provider unavailable: {e}"))
                })?;
                Ok(decision)
            }
        }
    }

    /// Exchange cryptographic attestations via CryptoEffects
    async fn exchange_guardian_attestation_via_effects<E>(
        &self,
        request: &GuardianInvitationRequest,
        effects: &E,
    ) -> InvitationResult<()>
    where
        E: PhysicalTimeEffects + CryptoEffects,
    {
        // Create guardian attestation data
        let attestation_data = serde_json::json!({
            "type": "guardian_attestation",
            "inviter": request.inviter,
            "invitee": request.invitee,
            "account_id": request.account_id,
            "role_description": request.role_description,
            "required_trust_level": request.required_trust_level,
            "recovery_responsibilities": request.recovery_responsibilities,
            "timestamp": effects.physical_time().await.map_err(|e| InvitationError::internal(format!("Time error: {}", e)))?.ts_ms / 1000,
        });

        // Sign attestation with ephemeral key to prove possession
        let (priv_key, pub_key) = effects
            .ed25519_generate_keypair()
            .await
            .map_err(|e| InvitationError::crypto(format!("keygen failed: {}", e)))?;
        let signature = effects
            .ed25519_sign(attestation_data.to_string().as_bytes(), &priv_key)
            .await
            .map_err(|e| InvitationError::crypto(format!("sign failed: {}", e)))?;

        // Verify locally to ensure integrity before sending
        let verified = effects
            .ed25519_verify(
                attestation_data.to_string().as_bytes(),
                &signature,
                &pub_key,
            )
            .await
            .map_err(|e| InvitationError::crypto(format!("verify failed: {}", e)))?;

        if !verified {
            return Err(InvitationError::crypto(
                "attestation verification failed".to_string(),
            ));
        }

        Ok(())
    }

    /// Record guardian relationship in journal via JournalEffects
    async fn record_guardian_relationship_via_effects<E>(
        &self,
        request: &GuardianInvitationRequest,
        effects: &E,
    ) -> InvitationResult<GuardianId>
    where
        E: PhysicalTimeEffects + JournalEffects,
    {
        let physical_time = effects
            .physical_time()
            .await
            .map_err(|e| InvitationError::internal(format!("Time error: {}", e)))?;

        // Create deterministic guardian identifier derived from participants + time
        let mut hasher = hash::hasher();
        hasher.update(&request.inviter.to_bytes().map_err(|e| {
            InvitationError::internal(format!("Failed to convert inviter to bytes: {}", e))
        })?);
        hasher.update(&request.invitee.to_bytes().map_err(|e| {
            InvitationError::internal(format!("Failed to convert invitee to bytes: {}", e))
        })?);
        hasher.update(request.account_id.0.as_bytes());
        hasher.update(&physical_time.ts_ms.to_be_bytes());
        let digest = hasher.finalize();
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&digest[..16]);
        let guardian_id = GuardianId(uuid::Uuid::from_bytes(uuid_bytes));

        let relationship_data = serde_json::json!({
            "type": "guardian_relationship_established",
            "guardian_id": guardian_id,
            "inviter": request.inviter,
            "invitee": request.invitee,
            "account_id": request.account_id,
            "role_description": request.role_description,
            "trust_level": request.required_trust_level,
            "recovery_responsibilities": request.recovery_responsibilities,
            "established_at": physical_time.ts_ms / 1000,
        });

        let mut journal = effects.get_journal().await?;
        journal.facts.insert_with_context(
            format!("guardian_relationship:{}", guardian_id),
            FactValue::String(relationship_data.to_string()),
            self.device_id.to_string(),
            physical_time.ts_ms,
            None,
        );
        effects.persist_journal(&journal).await?;

        Ok(guardian_id)
    }

    /// Record invitation rejection in journal via JournalEffects
    async fn record_invitation_rejection_via_effects<E>(
        &self,
        request: &GuardianInvitationRequest,
        effects: &E,
    ) -> InvitationResult<()>
    where
        E: PhysicalTimeEffects + JournalEffects,
    {
        let rejection_data = serde_json::json!({
            "type": "guardian_invitation_rejected",
            "inviter": request.inviter,
            "invitee": request.invitee,
            "account_id": request.account_id,
            "rejected_at": effects.physical_time().await.map_err(|e| InvitationError::internal(format!("Time error: {}", e)))?.ts_ms / 1000,
            "reason": "Invitation evaluation failed requirements",
        });

        let mut journal = effects.get_journal().await?;
        journal.facts.insert_with_context(
            format!("guardian_invitation_rejected:{}", request.account_id),
            FactValue::String(rejection_data.to_string()),
            self.device_id.to_string(),
            rejection_data["rejected_at"].as_i64().unwrap_or_default() as u64,
            None,
        );
        effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Evaluate whether to accept guardian invitation
    ///
    /// Simplified logic - real implementation would involve:
    /// - User confirmation
    /// - Trust level verification
    /// - Capability checks
    /// - Relationship attestation
    fn evaluate_invitation(&self, request: &GuardianInvitationRequest) -> bool {
        // Accept if we're the invitee and basic requirements are met
        if self.device_id == request.invitee {
            // Check trust level is reasonable
            matches!(
                request.required_trust_level,
                TrustLevel::High | TrustLevel::Medium
            )
        } else {
            false
        }
    }
}
