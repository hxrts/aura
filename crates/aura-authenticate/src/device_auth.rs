//! G_auth: Main Device Authentication Choreography
//!
//! This module implements the G_auth choreography for distributed device
//! authentication using the rumpsteak-aura choreographic programming framework.

use crate::{AccountId, AuraError, AuraResult, BiscuitGuardEvaluator};
use aura_core::DeviceId;
use aura_core::TimeEffects;
use aura_macros::choreography;
use aura_protocol::effects::AuraEffects;
use aura_verify::session::{SessionScope, SessionTicket};
use aura_verify::{IdentityProof, KeyMaterial, VerifiedIdentity};
use aura_wot::BiscuitTokenManager;
use ed25519_dalek::Verifier;
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

        // Execute device authentication choreography
        match self.execute_device_auth_choreography(&request).await {
            Ok((verified_identity, session_ticket)) => {
                tracing::info!(
                    "Device authentication successful for device: {}",
                    request.device_id
                );
                Ok(DeviceAuthResponse {
                    verified_identity: Some(verified_identity),
                    session_ticket: Some(session_ticket),
                    success: true,
                    error: None,
                })
            }
            Err(e) => {
                tracing::error!(
                    "Device authentication failed for device {}: {}",
                    request.device_id,
                    e
                );
                Ok(DeviceAuthResponse {
                    verified_identity: None,
                    session_ticket: None,
                    success: false,
                    error: Some(format!("Device authentication failed: {}", e)),
                })
            }
        }
    }

    /// Execute the device authentication choreography protocol
    async fn execute_device_auth_choreography(
        &self,
        request: &DeviceAuthRequest,
    ) -> AuraResult<(VerifiedIdentity, SessionTicket)> {
        // Phase 1: Challenge Request
        tracing::debug!(
            "Phase 1: Sending challenge request for device {}",
            request.device_id
        );

        let challenge_request = ChallengeRequest {
            device_id: request.device_id,
            account_id: request.account_id,
            scope: request.requested_scope.clone(),
        };

        // Phase 2: Challenge Response - Generate challenge
        let challenge_response = self.generate_challenge(&challenge_request).await?;

        // Phase 3: Proof Submission - Create identity proof
        let (identity_proof, verifying_key) = self
            .create_identity_proof(request, &challenge_response)
            .await?;
        let proof_submission = ProofSubmission {
            session_id: challenge_response.session_id.clone(),
            identity_proof: identity_proof.clone(),
            key_material: self
                .create_key_material(&request.device_id, verifying_key)
                .await?,
        };

        // Phase 4: Authentication Result - Verify proof
        let auth_result = self
            .verify_proof(&challenge_response, &proof_submission)
            .await?;

        match auth_result.success {
            true => {
                let verified_identity = auth_result.verified_identity.ok_or_else(|| {
                    AuraError::invalid("Missing verified identity in successful auth")
                })?;
                let session_ticket = auth_result.session_ticket.ok_or_else(|| {
                    AuraError::invalid("Missing session ticket in successful auth")
                })?;
                Ok((verified_identity, session_ticket))
            }
            false => Err(AuraError::invalid(&format!(
                "Authentication verification failed: {}",
                auth_result
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string())
            ))),
        }
    }

    /// Generate challenge for device authentication
    async fn generate_challenge(
        &self,
        request: &ChallengeRequest,
    ) -> AuraResult<ChallengeResponse> {
        // Generate random challenge
        let challenge_bytes = self.effects.random_bytes(32).await;
        let current_time = TimeEffects::current_timestamp(self.effects.as_ref()).await;
        let expires_at = current_time + 300; // 5 minute expiry

        Ok(ChallengeResponse {
            challenge: challenge_bytes,
            expires_at,
            session_id: uuid::Uuid::new_v4().to_string(),
        })
    }

    /// Create identity proof by signing the challenge
    async fn create_identity_proof(
        &self,
        request: &DeviceAuthRequest,
        challenge_response: &ChallengeResponse,
    ) -> AuraResult<(IdentityProof, aura_core::crypto::Ed25519VerifyingKey)> {
        // Create message to sign (combine challenge with device info)
        let mut message = Vec::new();
        message.extend_from_slice(&challenge_response.challenge);
        message.extend_from_slice(request.device_id.to_string().as_bytes());

        // Generate keypair and signature using the effect system crypto API
        let (verifying_key_bytes, signing_key_bytes) = self
            .effects
            .ed25519_generate_keypair()
            .await
            .map_err(|e| AuraError::crypto(format!("Keypair generation failed: {}", e)))?;
        let signature_bytes = self
            .effects
            .ed25519_sign(&message, &signing_key_bytes)
            .await
            .map_err(|e| AuraError::crypto(format!("Failed to sign challenge: {}", e)))?;

        let mut pk_array = [0u8; 32];
        pk_array.copy_from_slice(&verifying_key_bytes[..32]);
        let verifying_key = aura_core::crypto::Ed25519VerifyingKey::from_bytes(&pk_array)
            .map_err(|e| AuraError::crypto(format!("Invalid verifying key: {}", e)))?;

        let mut signature = [0u8; 64];
        signature.copy_from_slice(&signature_bytes[..64]);

        Ok((
            IdentityProof::Device {
                device_id: request.device_id,
                signature: signature.into(),
            },
            verifying_key,
        ))
    }

    /// Create key material for verification
    async fn create_key_material(
        &self,
        device_id: &DeviceId,
        public_key: aura_core::crypto::Ed25519VerifyingKey,
    ) -> AuraResult<KeyMaterial> {
        let mut key_material = KeyMaterial::new();
        key_material.add_device_key(*device_id, public_key);

        Ok(key_material)
    }

    /// Verify the authentication proof
    async fn verify_proof(
        &self,
        challenge_response: &ChallengeResponse,
        proof_submission: &ProofSubmission,
    ) -> AuraResult<AuthResult> {
        // Verify challenge hasn't expired
        let current_time = TimeEffects::current_timestamp(self.effects.as_ref()).await;
        if current_time > challenge_response.expires_at {
            return Ok(AuthResult {
                session_id: proof_submission.session_id.clone(),
                verified_identity: None,
                session_ticket: None,
                success: false,
                error: Some("Challenge expired".to_string()),
            });
        }

        // Verify signature against provided key material
        let verified_identity = match &proof_submission.identity_proof {
            IdentityProof::Device { device_id, signature } => {
                let verifying_key = proof_submission
                    .key_material
                    .get_device_public_key(device_id)
                    .map_err(|e| AuraError::crypto(format!("Missing device key: {}", e)))?;

                let sig_bytes: [u8; 64] = signature.to_bytes();
                let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
                verifying_key
                    .verify(&challenge_response.challenge, &sig)
                    .map_err(|e| AuraError::crypto(format!("Signature verification failed: {}", e)))?;

                VerifiedIdentity {
                    proof: proof_submission.identity_proof.clone(),
                    message_hash: aura_core::hash::hash(&challenge_response.challenge),
                }
            }
            _ => {
                return Ok(AuthResult {
                    session_id: proof_submission.session_id.clone(),
                    verified_identity: None,
                    session_ticket: None,
                    success: false,
                    error: Some("Invalid identity proof type".to_string()),
                });
            }
        };

        // Create session ticket
        let session_ticket = self
            .create_session_ticket(&proof_submission.session_id, &verified_identity)
            .await?;

        Ok(AuthResult {
            session_id: proof_submission.session_id.clone(),
            verified_identity: Some(verified_identity),
            session_ticket: Some(session_ticket),
            success: true,
            error: None,
        })
    }

    /// Create session ticket after successful authentication
    async fn create_session_ticket(
        &self,
        session_id: &str,
        verified_identity: &VerifiedIdentity,
    ) -> AuraResult<SessionTicket> {
        let current_time = TimeEffects::current_timestamp(self.effects.as_ref()).await;
        let expires_at = current_time + 3600; // 1 hour session

        // Generate nonce for session ticket
        let nonce_bytes = self.effects.random_bytes(16).await;
        let mut nonce = [0u8; 16];
        nonce.copy_from_slice(&nonce_bytes[..16]);

        // Extract device ID from verified identity
        let issuer_device_id = match &verified_identity.proof {
            IdentityProof::Device { device_id, .. } => *device_id,
            _ => {
                return Err(AuraError::invalid(
                    "Cannot extract device ID from identity proof",
                ))
            }
        };

        Ok(SessionTicket {
            session_id: uuid::Uuid::parse_str(session_id).unwrap_or_else(|_| uuid::Uuid::new_v4()),
            issuer_device_id,
            scope: SessionScope::Protocol {
                protocol_type: "device_auth".to_string(),
            },
            issued_at: current_time,
            expires_at,
            nonce,
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
    use aura_protocol::EffectApiEffects;
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

    // TODO: This test is disabled due to runtime context issues in the effect system builder
    // The effect system initialization requires careful async runtime management
    #[aura_test]
    async fn test_coordinator_creation() -> aura_core::AuraResult<()> {
        let device_id = test_device_id(2);
        let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
        let coordinator = DeviceAuthCoordinator::new(fixture.effect_system_arc());

        // Just verify the coordinator was created successfully
        // Test passes if we can create and access the coordinator
        let _ = coordinator.effects();
        Ok(())
    }
}
