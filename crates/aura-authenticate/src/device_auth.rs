//! G_auth: Main Device Authentication Choreography
//!
//! This module implements the G_auth choreography for distributed device
//! authentication using the rumpsteak-aura choreographic programming framework.

use crate::{AuraResult, BiscuitGuardEvaluator, ResourceScope};
use aura_core::{AccountId, DeviceId, FlowBudget};
use aura_macros::choreography;
use aura_protocol::effects::AuraEffects;
use aura_verify::session::{SessionScope, SessionTicket};
use aura_verify::{IdentityProof, KeyMaterial, VerifiedIdentity};
use aura_wot::BiscuitTokenManager;
use biscuit_auth::Biscuit;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Device authentication request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthRequest {
    /// Device requesting authentication
    pub device_id: DeviceId,
    /// Account context for authentication
    pub account_id: AccountId,
    /// Requested session scope
    pub requested_scope: SessionScope,
    /// Challenge nonce for replay protection
    pub challenge_nonce: Vec<u8>,
}

/// Device authentication response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthResponse {
    /// Authentication result
    pub verified_identity: Option<VerifiedIdentity>,
    /// Issued session ticket
    pub session_ticket: Option<SessionTicket>,
    /// Success indicator
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

// Device Authentication Choreography Protocol
//
// This choreography implements the complete device authentication protocol:
// - Requester sends challenge request to verifier
// - Verifier generates and returns cryptographic challenge
// - Requester submits signed identity proof
// - Verifier validates proof and returns authentication result
choreography! {
    #[namespace = "device_auth"]
    protocol DeviceAuthChoreography {
        roles: Requester, Verifier, Coordinator;

        // Phase 1: Challenge Request
        // Requester requests authentication challenge
        Requester[guard_capability = "request_auth",
                  flow_cost = 100,
                  journal_facts = "auth_challenge_requested"]
        -> Verifier: ChallengeRequest(ChallengeRequest);

        // Phase 2: Challenge Response
        // Verifier generates and sends cryptographic challenge
        Verifier[guard_capability = "generate_challenge",
                 flow_cost = 150,
                 journal_facts = "auth_challenge_generated"]
        -> Requester: ChallengeResponse(ChallengeResponse);

        // Phase 3: Proof Submission
        // Requester submits signed identity proof
        Requester[guard_capability = "submit_proof",
                  flow_cost = 200,
                  journal_facts = "auth_proof_submitted"]
        -> Verifier: ProofSubmission(ProofSubmission);

        // Phase 4: Authentication Result
        choice Verifier {
            success: {
                // Verifier validates proof and issues session ticket
                Verifier[guard_capability = "verify_proof",
                         flow_cost = 250,
                         journal_facts = "auth_verification_success",
                         journal_merge = true]
                -> Requester: AuthResult(AuthResult);

                // Notify coordinator of successful authentication
                Verifier[guard_capability = "notify_success",
                         flow_cost = 100,
                         journal_facts = "auth_coordinator_notified"]
                -> Coordinator: AuthenticationSuccessful(AuthenticationSuccessful);
            }
            failure: {
                // Verifier rejects authentication
                Verifier[guard_capability = "reject_auth",
                         flow_cost = 150,
                         journal_facts = "auth_verification_failed"]
                -> Requester: AuthResult(AuthResult);

                // Notify coordinator of failed authentication
                Verifier[guard_capability = "notify_failure",
                         flow_cost = 100,
                         journal_facts = "auth_coordinator_notified"]
                -> Coordinator: AuthenticationFailed(AuthenticationFailed);
            }
        }
    }
}

// Message types for device authentication choreography

/// Challenge request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeRequest {
    /// Device requesting authentication
    pub device_id: DeviceId,
    /// Account context
    pub account_id: AccountId,
    /// Requested session scope
    pub scope: SessionScope,
}

/// Challenge response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResponse {
    /// Challenge to be signed
    pub challenge: Vec<u8>,
    /// Challenge expiry timestamp
    pub expires_at: u64,
    /// Session ID for tracking
    pub session_id: String,
}

/// Proof submission message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofSubmission {
    /// Session ID from challenge
    pub session_id: String,
    /// Identity proof (signature, etc.)
    pub identity_proof: IdentityProof,
    /// Key material for verification
    pub key_material: KeyMaterial,
}

/// Authentication result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResult {
    /// Session ID
    pub session_id: String,
    /// Verification result
    pub verified_identity: Option<VerifiedIdentity>,
    /// Session ticket if successful
    pub session_ticket: Option<SessionTicket>,
    /// Success status
    pub success: bool,
    /// Error details if failed
    pub error: Option<String>,
}

/// Authentication success notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationSuccessful {
    /// Device that was authenticated
    pub device_id: DeviceId,
    /// Session ID
    pub session_id: String,
    /// Verification timestamp
    pub verified_at: u64,
}

/// Authentication failure notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationFailed {
    /// Device that failed authentication
    pub device_id: DeviceId,
    /// Session ID
    pub session_id: String,
    /// Failure reason
    pub reason: String,
    /// Failure timestamp
    pub failed_at: u64,
}

// Placeholder function for device auth choreography access
// The choreography macro will generate the appropriate types and functions
pub fn get_device_auth_choreography() {
    // The choreography macro will generate the appropriate types and functions
}

/// Device authentication coordinator using choreographic protocol
pub struct DeviceAuthCoordinator<E>
where
    E: AuraEffects + ?Sized,
{
    /// Shared effect system implementing AuraEffects
    effects: Arc<E>,
    /// Biscuit token manager for authorization
    token_manager: Option<BiscuitTokenManager>,
    /// Biscuit guard evaluator for permission checks
    guard_evaluator: Option<BiscuitGuardEvaluator>,
}

impl<E> DeviceAuthCoordinator<E>
where
    E: AuraEffects + ?Sized,
{
    /// Create a new device auth coordinator
    pub fn new(effect_system: Arc<E>) -> Self {
        Self {
            effects: effect_system,
            token_manager: None,
            guard_evaluator: None,
        }
    }

    /// Create a new device auth coordinator with Biscuit authorization
    pub fn new_with_biscuit(
        effect_system: Arc<E>,
        token_manager: BiscuitTokenManager,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        Self {
            effects: effect_system,
            token_manager: Some(token_manager),
            guard_evaluator: Some(guard_evaluator),
        }
    }

    /// Execute device authentication using the choreographic protocol
    pub async fn authenticate_device(
        &mut self,
        request: DeviceAuthRequest,
    ) -> AuraResult<DeviceAuthResponse> {
        tracing::info!(
            "Starting choreographic device authentication for device: {}",
            request.device_id
        );

        // TODO: Execute the choreographic protocol using the generated DeviceAuthChoreography
        // This is a placeholder until the choreography macro is fully integrated

        // For now, return a basic response
        Ok(DeviceAuthResponse {
            verified_identity: None,
            session_ticket: None,
            success: false,
            error: Some(
                "Choreographic device authentication not yet fully implemented".to_string(),
            ),
        })
    }

    /// Get the current effect system
    pub fn effects(&self) -> &Arc<E> {
        &self.effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::test_utils::test_device_id;
    use aura_core::{AccountId, DeviceId};
    use aura_macros::aura_test;
    use aura_verify::session::SessionScope;

    #[test]
    fn test_choreography_creation() {
        get_device_auth_choreography();
        // Test that we can create the choreography instance successfully
        // The macro generates a struct with the protocol name
    }

    #[test]
    fn test_challenge_request_serialization() {
        let request = ChallengeRequest {
            device_id: test_device_id(1),
            account_id: AccountId::new(),
            scope: SessionScope::Protocol {
                protocol_type: "device_auth".to_string(),
            },
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: ChallengeRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.device_id, deserialized.device_id);
        assert_eq!(request.account_id, deserialized.account_id);
    }

    // Note: This test is disabled due to runtime context issues in the effect system builder
    // The effect system initialization requires careful async runtime management
    #[aura_test]
    async fn test_coordinator_creation() -> aura_core::AuraResult<()> {
        let device_id = test_device_id(2);
        let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
        let coordinator = DeviceAuthCoordinator::new(fixture.effect_system());

        assert_eq!(coordinator.effects().device_id(), device_id);
        Ok(())
    }
}
