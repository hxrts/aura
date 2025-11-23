//! Authority Authentication Protocol
//!
//! This module implements authentication for the authority-centric architecture,
//! replacing the device-centric authentication model. Authorities are opaque
//! cryptographic actors that hide internal device structure.

use aura_core::{AuraError, Authority, AuthorityId, Hash32, Result};
use aura_macros::choreography;
use aura_protocol::effects::AuraEffects;
use aura_verify::session::{SessionScope, SessionTicket};
use aura_verify::{IdentityProof, VerifiedIdentity};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn authority_to_device_id(auth_id: &AuthorityId) -> aura_core::DeviceId {
    aura_core::DeviceId::from_uuid(auth_id.0)
}

/// Authority authentication request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityAuthRequest {
    /// Authority requesting authentication
    pub authority_id: AuthorityId,
    /// Challenge nonce for replay protection
    pub nonce: [u8; 32],
    /// Current root commitment of the authority
    pub commitment: Hash32,
    /// Requested session scope
    pub requested_scope: SessionScope,
}

/// Authority authentication proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityAuthProof {
    /// Authority being authenticated
    pub authority_id: AuthorityId,
    /// Signature over the challenge nonce
    pub signature: Vec<u8>,
    /// Public key of the authority
    pub public_key: Vec<u8>,
    /// Root commitment at time of signing
    pub commitment: Hash32,
}

/// Authority authentication response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityAuthResponse {
    /// Verified identity of the authority
    pub verified_identity: Option<VerifiedIdentity>,
    /// Issued session ticket
    pub session_ticket: Option<SessionTicket>,
    /// Success indicator
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Authenticate an authority by verifying its signature
///
/// This function performs the core authentication logic, verifying
/// that the authority can sign with its claimed public key.
pub async fn authenticate_authority(
    authority: &dyn Authority,
    request: AuthorityAuthRequest,
) -> Result<AuthorityAuthProof> {
    // Verify the authority ID matches
    if authority.authority_id() != request.authority_id {
        return Err(AuraError::invalid("Authority ID mismatch"));
    }

    // Sign the challenge nonce with authority's key
    let signature = authority.sign_operation(&request.nonce).await?;

    // Get public key bytes
    let public_key = authority.public_key();
    let public_key_bytes = public_key.as_bytes().to_vec();

    Ok(AuthorityAuthProof {
        authority_id: authority.authority_id(),
        signature: signature.to_bytes().to_vec(),
        public_key: public_key_bytes,
        commitment: authority.root_commitment(),
    })
}

/// Verify an authority authentication proof
pub async fn verify_authority_proof(
    request: &AuthorityAuthRequest,
    proof: &AuthorityAuthProof,
) -> Result<bool> {
    // Verify authority ID matches
    if request.authority_id != proof.authority_id {
        return Ok(false);
    }

    // Verify commitment matches (prevents rollback attacks)
    if request.commitment != proof.commitment {
        return Ok(false);
    }

    // Verify signature over nonce
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let public_key = VerifyingKey::from_bytes(
        &proof
            .public_key
            .as_slice()
            .try_into()
            .map_err(|_| AuraError::crypto("Invalid public key"))?,
    )
    .map_err(|e| AuraError::crypto(format!("Public key error: {}", e)))?;

    let signature = Signature::from_bytes(
        &proof
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| AuraError::crypto("Invalid signature"))?,
    );

    public_key
        .verify(&request.nonce, &signature)
        .map(|_| true)
        .map_err(|e| AuraError::crypto(format!("Signature verification failed: {}", e)))
}

// Authority Authentication Choreography Protocol
//
// This choreography implements the authority authentication protocol:
// - Requester sends authentication request to verifier
// - Verifier generates and returns cryptographic challenge
// - Requester submits signed proof
// - Verifier validates proof and returns authentication result
choreography! {
    #[namespace = "authority_auth"]
    protocol AuthorityAuth {
        roles: Requester, Verifier;

        // Step 1: Request authentication
        Requester[guard_capability = "request_auth", flow_cost = 50]
        -> Verifier: RequestAuth(AuthorityAuthRequest);

        // Step 2: Return challenge
        Verifier[guard_capability = "issue_challenge", flow_cost = 30]
        -> Requester: Challenge(ChallengeData);

        // Step 3: Submit proof
        Requester[guard_capability = "submit_proof", flow_cost = 50]
        -> Verifier: SubmitProof(AuthorityAuthProof);

        // Step 4: Return result
        Verifier[guard_capability = "verify_auth", flow_cost = 30]
        -> Requester: AuthResult(AuthorityAuthResponse);
    }
}

/// Challenge data for authority authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeData {
    /// Challenge nonce to sign
    pub nonce: [u8; 32],
    /// Session ID for this authentication attempt
    pub session_id: String,
    /// Timestamp of challenge issuance
    pub timestamp: u64,
}

/// Authority authentication handler
pub struct AuthorityAuthHandler<E: AuraEffects> {
    effects: Arc<E>,
}

impl<E: AuraEffects> AuthorityAuthHandler<E> {
    /// Create a new authentication handler
    pub fn new(effects: Arc<E>) -> Self {
        Self { effects }
    }

    /// Run the authentication protocol as requester
    pub async fn authenticate_as_requester(
        &self,
        authority: Arc<dyn Authority>,
        _verifier_authority: AuthorityId,
        scope: SessionScope,
    ) -> Result<AuthorityAuthResponse> {
        // Create authentication request
        let mut request = AuthorityAuthRequest {
            authority_id: authority.authority_id(),
            nonce: [0; 32], // Will be replaced by challenge
            commitment: authority.root_commitment(),
            requested_scope: scope,
        };

        // Step 1: Send authentication request
        // In a full implementation, this would use the choreography runtime
        // For now, simulate the protocol steps

        // Step 2: Receive challenge from verifier
        let challenge = self.simulate_receive_challenge(&request).await?;

        // Update request with the actual challenge nonce
        request.nonce = challenge.nonce;

        // Step 3: Generate proof by signing the challenge
        let proof = authenticate_authority(authority.as_ref(), request.clone()).await?;

        // Step 4: Receive authentication result
        let response = self.simulate_verify_proof(&request, &proof).await?;

        Ok(response)
    }

    /// Run the authentication protocol as verifier
    pub async fn authenticate_as_verifier(
        &self,
        request: AuthorityAuthRequest,
    ) -> Result<AuthorityAuthResponse> {
        // Step 1: Receive authentication request (already provided)

        // Step 2: Generate and send challenge
        let challenge = self.generate_challenge(&request).await?;

        // In a full choreography implementation, we would wait for the proof submission
        // For now, simulate receiving a proof and verify it
        let simulated_proof = AuthorityAuthProof {
            authority_id: request.authority_id,
            signature: vec![0; 64],  // Placeholder signature
            public_key: vec![0; 32], // Placeholder public key
            commitment: request.commitment,
        };

        // Step 3: Verify the submitted proof
        let verification_result = verify_authority_proof(&request, &simulated_proof).await;

        // Step 4: Return authentication result
        match verification_result {
            Ok(true) => {
                // Create session ticket
                let mut ticket_nonce = [0u8; 16];
                let ticket_nonce_bytes = self.effects.random_bytes(16).await;
                ticket_nonce.copy_from_slice(&ticket_nonce_bytes[..16]);

                let session_ticket = SessionTicket {
                    session_id: uuid::Uuid::parse_str(&challenge.session_id)
                        .unwrap_or_else(|_| uuid::Uuid::new_v4()),
                    issuer_device_id: aura_core::DeviceId::from_uuid(request.authority_id.uuid()),
                    scope: request.requested_scope,
                    issued_at: challenge.timestamp,
                    expires_at: challenge.timestamp + 3600, // 1 hour
                    nonce: ticket_nonce,
                };

                // Create identity proof - simulating a device signature for authority
                let signature = if simulated_proof.signature.len() == 64 {
                    let mut sig_bytes = [0u8; 64];
                    sig_bytes.copy_from_slice(&simulated_proof.signature);
                    aura_core::Ed25519Signature::from_bytes(&sig_bytes)
                } else {
                    aura_core::Ed25519Signature::from_bytes(&[0; 64])
                };

                let identity_proof = IdentityProof::Device {
                    device_id: aura_core::DeviceId::from_uuid(request.authority_id.uuid()),
                    signature,
                };

                // Create verified identity
                let verified_identity = VerifiedIdentity {
                    proof: identity_proof,
                    message_hash: request.nonce, // The challenge nonce that was signed
                };

                Ok(AuthorityAuthResponse {
                    verified_identity: Some(verified_identity),
                    session_ticket: Some(session_ticket),
                    success: true,
                    error: None,
                })
            }
            Ok(false) => Ok(AuthorityAuthResponse {
                verified_identity: None,
                session_ticket: None,
                success: false,
                error: Some("Authentication failed: invalid proof".to_string()),
            }),
            Err(e) => Ok(AuthorityAuthResponse {
                verified_identity: None,
                session_ticket: None,
                success: false,
                error: Some(format!("Authentication error: {}", e)),
            }),
        }
    }

    /// Simulate receiving challenge from verifier (for choreography integration)
    async fn simulate_receive_challenge(
        &self,
        request: &AuthorityAuthRequest,
    ) -> Result<ChallengeData> {
        self.generate_challenge(request).await
    }

    /// Generate challenge data for authentication
    async fn generate_challenge(&self, _request: &AuthorityAuthRequest) -> Result<ChallengeData> {
        // Generate challenge nonce
        let mut nonce = [0u8; 32];
        let nonce_bytes = self.effects.random_bytes(32).await;
        nonce.copy_from_slice(&nonce_bytes[..32]);

        Ok(ChallengeData {
            nonce,
            session_id: uuid::Uuid::new_v4().to_string(),
            timestamp: aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await,
        })
    }

    /// Simulate verifying proof and returning result (for choreography integration)
    async fn simulate_verify_proof(
        &self,
        request: &AuthorityAuthRequest,
        proof: &AuthorityAuthProof,
    ) -> Result<AuthorityAuthResponse> {
        // Verify the proof
        let verification_result = verify_authority_proof(request, proof).await;

        match verification_result {
            Ok(true) => {
                let timestamp =
                    aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await;

                // Create session ticket
                let mut nonce = [0u8; 16];
                let nonce_bytes = self.effects.random_bytes(16).await;
                nonce.copy_from_slice(&nonce_bytes[..16]);

                let session_ticket = SessionTicket {
                    session_id: uuid::Uuid::new_v4(),
                    issuer_device_id: authority_to_device_id(&request.authority_id),
                    scope: request.requested_scope.clone(),
                    issued_at: timestamp,
                    expires_at: timestamp + 3600, // 1 hour
                    nonce,
                };

                // Create identity proof - simulating a device signature for authority
                let signature = if proof.signature.len() == 64 {
                    let mut sig_bytes = [0u8; 64];
                    sig_bytes.copy_from_slice(&proof.signature);
                    aura_core::Ed25519Signature::from_bytes(&sig_bytes)
                } else {
                    aura_core::Ed25519Signature::from_bytes(&[0; 64])
                };

                let identity_proof = IdentityProof::Device {
                    device_id: authority_to_device_id(&request.authority_id),
                    signature,
                };

                // Create verified identity
                let verified_identity = VerifiedIdentity {
                    proof: identity_proof,
                    message_hash: request.nonce, // The challenge nonce that was signed
                };

                Ok(AuthorityAuthResponse {
                    verified_identity: Some(verified_identity),
                    session_ticket: Some(session_ticket),
                    success: true,
                    error: None,
                })
            }
            Ok(false) => Ok(AuthorityAuthResponse {
                verified_identity: None,
                session_ticket: None,
                success: false,
                error: Some("Authentication failed: invalid proof".to_string()),
            }),
            Err(e) => Ok(AuthorityAuthResponse {
                verified_identity: None,
                session_ticket: None,
                success: false,
                error: Some(format!("Authentication error: {}", e)),
            }),
        }
    }
}
