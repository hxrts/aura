//! Authority Authentication Protocol
//!
//! This module implements authentication for the authority-centric architecture,
//! replacing the device-centric authentication model. Authorities are opaque
//! cryptographic actors that hide internal device structure.

use aura_core::{AuraError, Authority, AuthorityId, Hash32, Result};
use aura_macros::choreography;
use aura_protocol::effects::AuraEffects;
use aura_verify::session::{SessionScope, SessionTicket};
use aura_verify::{IdentityProof, KeyMaterial, VerifiedIdentity};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
        verifier_authority: AuthorityId,
        scope: SessionScope,
    ) -> Result<AuthorityAuthResponse> {
        // Create authentication request
        let request = AuthorityAuthRequest {
            authority_id: authority.authority_id(),
            nonce: [0; 32], // Will be replaced by challenge
            commitment: authority.root_commitment(),
            requested_scope: scope,
        };

        // Run the choreography protocol
        // TODO: Integrate with rumpsteak session types

        // For now, return a placeholder
        Ok(AuthorityAuthResponse {
            verified_identity: None,
            session_ticket: None,
            success: false,
            error: Some("Not yet implemented".to_string()),
        })
    }

    /// Run the authentication protocol as verifier
    pub async fn authenticate_as_verifier(
        &self,
        request: AuthorityAuthRequest,
    ) -> Result<AuthorityAuthResponse> {
        // Generate challenge nonce
        let mut nonce = [0u8; 32];
        self.effects.random_bytes(&mut nonce).await?;

        // Create challenge data
        let challenge = ChallengeData {
            nonce,
            session_id: uuid::Uuid::new_v4().to_string(),
            timestamp: self.effects.current_timestamp().await?,
        };

        // TODO: Complete verifier implementation

        Ok(AuthorityAuthResponse {
            verified_identity: None,
            session_ticket: None,
            success: false,
            error: Some("Verifier not yet implemented".to_string()),
        })
    }
}
