//! Authentication Handlers
//!
//! Handlers for authentication-related operations including device key verification,
//! threshold signatures, and challenge-response authentication.

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::fact_types::AUTH_AUTHENTICATED_FACT_TYPE_ID;
use crate::runtime::services::{AuthManager, TrustedKeyResolutionService};
use crate::runtime::AuraEffectSystem;
use aura_authentication::capabilities::AuthenticationCapability;
#[cfg(test)]
use aura_core::effects::CryptoCoreEffects;
use aura_core::effects::{CryptoExtendedEffects, RandomCoreEffects, RandomExtendedEffects};
use aura_core::types::identifiers::{AuthorityId, DeviceId};
use aura_core::{FlowCost, Hash32};
use aura_guards::chain::create_send_guard;
use aura_guards::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};
use aura_protocol::effects::EffectApiEffects;
#[cfg(test)]
use aura_signature::sign_ed25519_transcript;
use aura_signature::{verify_ed25519_transcript, verify_frost_transcript, SecurityTranscript};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Extract the group public key (32 bytes) from a serialized FROST PublicKeyPackage
fn extract_group_public_key(public_key_package: &[u8]) -> AgentResult<Vec<u8>> {
    // The public key package is serialized using FROST's native serialization.
    use frost_ed25519 as frost;

    let pubkey_package: frost::keys::PublicKeyPackage =
        frost::keys::PublicKeyPackage::deserialize(public_key_package).map_err(|e| {
            AgentError::effects(format!("failed to deserialize public key package: {e}"))
        })?;

    // Get the group verifying key and serialize it
    let group_public_key = pubkey_package.verifying_key().serialize().to_vec();

    Ok(group_public_key)
}

/// Authentication method types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthMethod {
    /// Device key authentication using Ed25519 signatures
    DeviceKey,
    /// Threshold signature authentication using FROST
    ThresholdSignature,
    /// Passkey authentication (WebAuthn/FIDO2) - future
    Passkey,
}

/// Authentication challenge for challenge-response flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthChallenge {
    /// Unique challenge identifier
    pub challenge_id: String,
    /// Random challenge bytes (32 bytes)
    pub challenge_bytes: Vec<u8>,
    /// Challenge creation timestamp
    pub created_at: u64,
    /// Challenge expiration timestamp
    pub expires_at: u64,
    /// Authority being authenticated
    pub authority_id: AuthorityId,
}

#[derive(Debug, Clone, Serialize)]
struct AuthChallengeTranscriptPayload {
    challenge_id: String,
    challenge_bytes: Vec<u8>,
    created_at: u64,
    expires_at: u64,
    authority_id: AuthorityId,
    auth_method: AuthMethod,
    response_public_key: Vec<u8>,
}

struct AuthChallengeTranscript<'a> {
    challenge: &'a AuthChallenge,
    auth_method: AuthMethod,
    response_public_key: &'a [u8],
}

impl SecurityTranscript for AuthChallengeTranscript<'_> {
    type Payload = AuthChallengeTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.authentication.challenge-response";

    fn transcript_payload(&self) -> Self::Payload {
        AuthChallengeTranscriptPayload {
            challenge_id: self.challenge.challenge_id.clone(),
            challenge_bytes: self.challenge.challenge_bytes.clone(),
            created_at: self.challenge.created_at,
            expires_at: self.challenge.expires_at,
            authority_id: self.challenge.authority_id,
            auth_method: self.auth_method.clone(),
            response_public_key: self.response_public_key.to_vec(),
        }
    }
}

fn auth_challenge_transcript<'a>(
    challenge: &'a AuthChallenge,
    auth_method: AuthMethod,
    response_public_key: &'a [u8],
) -> AuthChallengeTranscript<'a> {
    AuthChallengeTranscript {
        challenge,
        auth_method,
        response_public_key,
    }
}

/// Authentication response containing signed challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    /// Challenge identifier being responded to
    pub challenge_id: String,
    /// Signature over the challenge bytes
    pub signature: Vec<u8>,
    /// Public key that created the signature
    pub public_key: Vec<u8>,
    /// Authentication method used
    pub auth_method: AuthMethod,
}

type VerifiedAuthResponse = VerifiedIngress<AuthResponse>;

/// Authentication result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResult {
    /// Whether authentication succeeded
    pub authenticated: bool,
    /// Authority ID if authenticated
    pub authority_id: Option<AuthorityId>,
    /// Device ID if authenticated
    pub device_id: Option<DeviceId>,
    /// Reason for failure if not authenticated
    pub failure_reason: Option<String>,
    /// Authentication timestamp
    pub authenticated_at: u64,
}

/// Explicit authentication status for the local runtime authority/device pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationStatus {
    /// Authenticated authority.
    pub authority_id: AuthorityId,
    /// Authenticated device.
    pub device_id: DeviceId,
}

/// Fact recorded when authentication succeeds
#[derive(Debug, Serialize)]
struct AuthenticatedFact {
    authority_id: AuthorityId,
    device_id: DeviceId,
    auth_method: AuthMethod,
    challenge_id: String,
}

/// Authentication handler
#[derive(Clone)]
pub struct AuthHandler {
    context: HandlerContext,
    /// Challenge manager (shared cache)
    challenge_manager: Arc<AuthManager>,
    key_resolver: TrustedKeyResolutionService,
}

impl AuthHandler {
    /// Create a new authentication handler
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        Ok(Self {
            context: HandlerContext::new(authority),
            challenge_manager: Arc::new(AuthManager::new()),
            key_resolver: TrustedKeyResolutionService::new(),
        })
    }

    /// Get the authority context
    pub fn authority_context(&self) -> &AuthorityContext {
        &self.context.authority
    }

    /// Get the device ID for this handler
    pub fn device_id(&self) -> DeviceId {
        self.context.authority.device_id()
    }

    /// Create an authentication challenge
    pub async fn create_challenge(&self, effects: &AuraEffectSystem) -> AgentResult<AuthChallenge> {
        // Generate random challenge bytes
        let challenge_bytes = effects.random_bytes(32).await;

        let current_time = effects.current_timestamp().await.unwrap_or(0);
        let challenge_id = format!("challenge-{}", effects.random_uuid().await.simple());

        let challenge = AuthChallenge {
            challenge_id: challenge_id.clone(),
            challenge_bytes,
            created_at: current_time,
            expires_at: current_time + 300_000, // 5 minute expiry
            authority_id: self.context.authority.authority_id(),
        };

        // Store pending challenge
        self.challenge_manager
            .cache_challenge(challenge.clone())
            .await;

        Ok(challenge)
    }

    /// Verify an authentication response
    pub async fn verify_response(
        &self,
        effects: &AuraEffectSystem,
        response: &VerifiedAuthResponse,
    ) -> AgentResult<AuthResult> {
        let response = response.payload();
        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Look up the challenge
        let challenge = self
            .challenge_manager
            .get_challenge(&response.challenge_id)
            .await;

        let challenge = match challenge {
            Some(c) => c,
            None => {
                return Ok(AuthResult {
                    authenticated: false,
                    authority_id: None,
                    device_id: None,
                    failure_reason: Some("Challenge not found or expired".to_string()),
                    authenticated_at: current_time,
                });
            }
        };

        // Check expiration
        if current_time > challenge.expires_at {
            // Remove expired challenge
            self.challenge_manager
                .remove_challenge(&response.challenge_id)
                .await;
            return Ok(AuthResult {
                authenticated: false,
                authority_id: None,
                device_id: None,
                failure_reason: Some("Challenge expired".to_string()),
                authenticated_at: current_time,
            });
        }

        // Verify signature based on auth method
        let verified = match response.auth_method {
            AuthMethod::DeviceKey => {
                self.verify_device_key_signature(effects, &challenge, response)
                    .await?
            }
            AuthMethod::ThresholdSignature => {
                self.verify_threshold_signature(effects, &challenge, response)
                    .await?
            }
            AuthMethod::Passkey => {
                // Future: WebAuthn verification
                false
            }
        };

        if verified {
            // Remove used challenge
            self.challenge_manager
                .remove_challenge(&response.challenge_id)
                .await;

            // Journal authentication fact
            let device_id = self.device_id();
            HandlerUtilities::append_relational_fact(
                &self.context.authority,
                effects,
                self.context.effect_context.context_id(),
                AUTH_AUTHENTICATED_FACT_TYPE_ID,
                &AuthenticatedFact {
                    authority_id: self.context.authority.authority_id(),
                    device_id,
                    auth_method: response.auth_method.clone(),
                    challenge_id: response.challenge_id.clone(),
                },
            )
            .await?;

            Ok(AuthResult {
                authenticated: true,
                authority_id: Some(self.context.authority.authority_id()),
                device_id: Some(device_id),
                failure_reason: None,
                authenticated_at: current_time,
            })
        } else {
            Ok(AuthResult {
                authenticated: false,
                authority_id: None,
                device_id: None,
                failure_reason: Some("Signature verification failed".to_string()),
                authenticated_at: current_time,
            })
        }
    }

    pub(crate) fn build_response_ingress(
        &self,
        response: AuthResponse,
    ) -> AgentResult<VerifiedAuthResponse> {
        let payload_hash = Hash32::from_value(&response)
            .map_err(|error| AgentError::internal(format!("hash auth response: {error}")))?;
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Authority(self.context.authority.authority_id()),
            self.context.authority.default_context_id(),
            None,
            payload_hash,
            1,
        );
        let evidence = IngressVerificationEvidence::builder(metadata.clone())
            .peer_identity(
                true,
                "authentication response is bound to local challenge authority",
            )
            .and_then(|builder| {
                builder.envelope_authenticity(
                    true,
                    "authentication response signature is verified before success",
                )
            })
            .and_then(|builder| {
                builder.capability_authorization(
                    true,
                    "challenge possession authorizes authentication response verification",
                )
            })
            .and_then(|builder| {
                builder.namespace_scope(true, "authentication response is scoped to authority")
            })
            .and_then(|builder| builder.schema_version(true, "auth response schema v1"))
            .and_then(|builder| {
                builder.replay_freshness(true, "challenge id and expiry are checked")
            })
            .and_then(|builder| {
                builder.signer_membership(true, "enrolled device or authority key is resolved")
            })
            .and_then(|builder| {
                builder.proof_evidence(true, "challenge transcript signature is required")
            })
            .and_then(|builder| builder.build())
            .map_err(|error| AgentError::internal(format!("auth ingress evidence: {error}")))?;
        DecodedIngress::new(response, metadata)
            .verify(evidence)
            .map_err(|error| AgentError::internal(format!("auth ingress promotion: {error}")))
    }

    /// Verify a device key Ed25519 signature
    async fn verify_device_key_signature(
        &self,
        effects: &AuraEffectSystem,
        challenge: &AuthChallenge,
        response: &AuthResponse,
    ) -> AgentResult<bool> {
        let trusted_key = self
            .key_resolver
            .resolve_device_key(self.device_id())
            .map_err(|e| AgentError::effects(format!("device key resolution failed: {e}")))?;
        let transcript =
            auth_challenge_transcript(challenge, AuthMethod::DeviceKey, trusted_key.bytes());

        let verified = verify_ed25519_transcript(
            effects,
            &transcript,
            &response.signature,
            trusted_key.bytes(),
        )
        .await
        .map_err(|e| AgentError::effects(format!("signature verification failed: {e}")))?;

        Ok(verified)
    }

    /// Verify a threshold (FROST) signature
    ///
    /// For FROST threshold authentication, the `AuthResponse` should contain:
    /// - `signature`: The aggregated FROST signature (64 bytes for Ed25519)
    ///
    /// The signature is verified against the challenge bytes using the trusted
    /// authority threshold key resolved by identity/epoch.
    async fn verify_threshold_signature(
        &self,
        effects: &AuraEffectSystem,
        challenge: &AuthChallenge,
        response: &AuthResponse,
    ) -> AgentResult<bool> {
        // Validate input lengths
        if response.signature.len() != 64 {
            return Err(AgentError::effects(format!(
                "Invalid FROST signature length: {} (expected 64)",
                response.signature.len()
            )));
        }
        let trusted_key = self
            .key_resolver
            .resolve_authority_threshold_key(self.context.authority.authority_id(), 0)
            .map_err(|e| AgentError::effects(format!("threshold key resolution failed: {e}")))?;

        let transcript = auth_challenge_transcript(
            challenge,
            AuthMethod::ThresholdSignature,
            trusted_key.bytes(),
        );

        let verified = verify_frost_transcript(
            effects,
            &transcript,
            &response.signature,
            trusted_key.bytes(),
        )
        .await
        .map_err(|e| AgentError::effects(format!("FROST verification failed: {e}")))?;

        Ok(verified)
    }

    /// Test helper that signs a challenge using a freshly generated key.
    ///
    /// Production device-key responses must come from an enrolled private-key
    /// signer outside this handler; generating a fresh key during
    /// authentication would register unauthenticated key material.
    #[cfg(test)]
    pub async fn sign_challenge_with_ephemeral_key_for_tests(
        &self,
        effects: &AuraEffectSystem,
        challenge: &AuthChallenge,
    ) -> AgentResult<AuthResponse> {
        // MVP: Generate ephemeral keypair for signing. Future: use stored device key.
        // Note: ed25519_generate_keypair returns (private_key, public_key)
        let (private_key, public_key) = effects
            .ed25519_generate_keypair()
            .await
            .map_err(|e| AgentError::effects(format!("failed to generate signing key: {e}")))?;

        let transcript = auth_challenge_transcript(challenge, AuthMethod::DeviceKey, &public_key);

        let signature = sign_ed25519_transcript(effects, &transcript, &private_key)
            .await
            .map_err(|e| AgentError::effects(format!("failed to sign challenge: {e}")))?;
        self.key_resolver
            .register_device_key(self.device_id(), public_key.clone())
            .map_err(|e| AgentError::effects(format!("register device auth key failed: {e}")))?;

        Ok(AuthResponse {
            challenge_id: challenge.challenge_id.clone(),
            signature,
            public_key,
            auth_method: AuthMethod::DeviceKey,
        })
    }

    /// Sign a challenge using FROST threshold signatures
    ///
    /// This method performs the complete FROST signing ceremony:
    /// 1. Generate nonces for each participant
    /// 2. Create signing package
    /// 3. Each participant creates their signature share
    /// 4. Aggregate shares into final signature
    ///
    /// # Arguments
    /// * `effects` - The effect system for crypto operations
    /// * `challenge` - The challenge to sign
    /// * `key_packages` - The FROST key packages for each participant
    /// * `public_key_package` - The group public key package
    /// * `participants` - Which participants will sign (must meet threshold)
    pub async fn sign_challenge_threshold(
        &self,
        effects: &AuraEffectSystem,
        challenge: &AuthChallenge,
        key_packages: &[Vec<u8>],
        public_key_package: &[u8],
        participants: &[u16],
    ) -> AgentResult<AuthResponse> {
        // Step 1: Generate nonces for each participant using their key package
        let mut nonces = Vec::new();
        for participant_id in participants {
            // Participant IDs are 1-indexed
            let key_package = key_packages
                .get(*participant_id as usize - 1)
                .ok_or_else(|| {
                    AgentError::effects(format!(
                        "no key package for participant {}",
                        participant_id
                    ))
                })?;
            let nonce = effects
                .frost_generate_nonces(key_package)
                .await
                .map_err(|e| AgentError::effects(format!("failed to generate nonces: {e}")))?;
            nonces.push(nonce);
        }

        let group_public_key = extract_group_public_key(public_key_package)?;
        self.key_resolver
            .register_authority_threshold_key(
                self.context.authority.authority_id(),
                0,
                group_public_key.clone(),
            )
            .map_err(|e| AgentError::effects(format!("register threshold auth key failed: {e}")))?;
        let transcript =
            auth_challenge_transcript(challenge, AuthMethod::ThresholdSignature, &group_public_key);
        let transcript_bytes = transcript.transcript_bytes().map_err(|error| {
            AgentError::effects(format!("auth challenge transcript failed: {error}"))
        })?;

        // Step 2: Create signing package
        let signing_package = effects
            .frost_create_signing_package(
                &transcript_bytes,
                &nonces,
                participants,
                public_key_package,
            )
            .await
            .map_err(|e| AgentError::effects(format!("failed to create signing package: {e}")))?;

        // Step 3: Each participant creates their signature share
        let mut signature_shares = Vec::new();
        for (i, participant_id) in participants.iter().enumerate() {
            // Participant IDs are 1-indexed
            let key_package = key_packages
                .get(*participant_id as usize - 1)
                .ok_or_else(|| {
                    AgentError::effects(format!(
                        "no key package for participant {}",
                        participant_id
                    ))
                })?;

            let share = effects
                .frost_sign_share(&signing_package, key_package, &nonces[i])
                .await
                .map_err(|e| {
                    AgentError::effects(format!(
                        "participant {} failed to sign: {e}",
                        participant_id
                    ))
                })?;
            signature_shares.push(share);
        }

        // Step 4: Aggregate signature shares
        let signature = effects
            .frost_aggregate_signatures(&signing_package, &signature_shares)
            .await
            .map_err(|e| AgentError::effects(format!("failed to aggregate signatures: {e}")))?;

        Ok(AuthResponse {
            challenge_id: challenge.challenge_id.clone(),
            signature,
            public_key: group_public_key,
            auth_method: AuthMethod::ThresholdSignature,
        })
    }

    /// Report the current runtime-owned authentication status.
    pub async fn authentication_status(
        &self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<AuthenticationStatus> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Enforce guard chain
        let guard = create_send_guard(
            AuthenticationCapability::Verify.as_name(),
            self.context.effect_context.context_id(),
            self.context.authority.authority_id(),
            FlowCost::new(50),
        );
        let result = guard.evaluate(effects).await.map_err(|e| {
            crate::core::AgentError::effects(format!("guard evaluation failed: {e}"))
        })?;
        if !result.authorized {
            return Err(crate::core::AgentError::effects(
                result
                    .denial_reason
                    .unwrap_or_else(|| "authentication not authorized".to_string()),
            ));
        }

        Ok(AuthenticationStatus {
            authority_id: self.context.authority.authority_id(),
            device_id: self.device_id(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::core::AuthorityContext;
    use aura_core::types::identifiers::AuthorityId;

    #[tokio::test]
    async fn auth_status_requires_authorized_context() {
        let authority_id = AuthorityId::new_from_entropy([90u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context.clone()).unwrap();

        let error = handler
            .authentication_status(&effects)
            .await
            .expect_err("authentication status should require authorization");
        assert!(
            error.to_string().contains("Authorization denied"),
            "expected authorization denial, got: {error}"
        );
    }

    #[tokio::test]
    async fn challenge_can_be_created() {
        let authority_id = AuthorityId::new_from_entropy([91u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        let challenge = handler.create_challenge(&effects).await.unwrap();
        assert!(!challenge.challenge_id.is_empty());
        assert_eq!(challenge.challenge_bytes.len(), 32);
        assert!(challenge.expires_at > challenge.created_at);
    }

    #[test]
    fn auth_challenge_transcript_binds_method_and_public_key() {
        let challenge = AuthChallenge {
            challenge_id: "challenge-1".to_string(),
            challenge_bytes: vec![1; 32],
            created_at: 100,
            expires_at: 200,
            authority_id: AuthorityId::new_from_entropy([9; 32]),
        };

        let device_key = auth_challenge_transcript(&challenge, AuthMethod::DeviceKey, &[1; 32])
            .transcript_bytes()
            .unwrap();
        let threshold =
            auth_challenge_transcript(&challenge, AuthMethod::ThresholdSignature, &[1; 32])
                .transcript_bytes()
                .unwrap();
        let different_key = auth_challenge_transcript(&challenge, AuthMethod::DeviceKey, &[2; 32])
            .transcript_bytes()
            .unwrap();

        assert_ne!(device_key, threshold);
        assert_ne!(device_key, different_key);
    }

    #[tokio::test]
    async fn expired_challenge_is_rejected() {
        let authority_id = AuthorityId::new_from_entropy([92u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        // Create a response for a non-existent challenge
        let response = AuthResponse {
            challenge_id: "nonexistent".to_string(),
            signature: vec![0u8; 64],
            public_key: vec![0u8; 32],
            auth_method: AuthMethod::DeviceKey,
        };

        let response = handler.build_response_ingress(response).unwrap();
        let result = handler.verify_response(&effects, &response).await.unwrap();
        assert!(!result.authenticated);
        assert!(result.failure_reason.is_some());
    }

    #[tokio::test]
    async fn device_auth_uses_resolved_key_not_response_key() {
        let authority_id = AuthorityId::new_from_entropy([95u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        let challenge = handler.create_challenge(&effects).await.unwrap();
        let mut response = handler
            .sign_challenge_with_ephemeral_key_for_tests(&effects, &challenge)
            .await
            .unwrap();
        response.public_key = vec![0xAA; 32];

        let response = handler.build_response_ingress(response).unwrap();
        let result = handler.verify_response(&effects, &response).await.unwrap();
        assert!(
            result.authenticated,
            "auth verification should use the trusted registered device key, not the response key"
        );
    }

    #[tokio::test]
    async fn threshold_signature_verification_works() {
        let authority_id = AuthorityId::new_from_entropy([93u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        // Step 1: Generate threshold keys (2-of-3) via standardized API
        let threshold = 2;
        let max_signers = 3;
        let key_gen_result = effects
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                threshold,
                max_signers,
            )
            .await
            .expect("Threshold key generation should succeed");

        assert_eq!(key_gen_result.key_packages.len(), max_signers as usize);

        // Step 2: Create a challenge
        let challenge = handler.create_challenge(&effects).await.unwrap();
        assert_eq!(challenge.challenge_bytes.len(), 32);

        // Step 3: Sign the challenge using threshold signature with participants 1 and 2
        let participants: Vec<u16> = vec![1, 2]; // 2-of-3 threshold
        let response = handler
            .sign_challenge_threshold(
                &effects,
                &challenge,
                &key_gen_result.key_packages,
                &key_gen_result.public_key_package,
                &participants,
            )
            .await
            .expect("Threshold signing should succeed");

        assert_eq!(response.auth_method, AuthMethod::ThresholdSignature);
        assert_eq!(response.signature.len(), 64); // Ed25519 signature length
        assert_eq!(response.public_key.len(), 32); // Ed25519 public key length

        // Step 4: Verify the response
        let response = handler.build_response_ingress(response).unwrap();
        let result = handler.verify_response(&effects, &response).await.unwrap();
        assert!(
            result.authenticated,
            "Threshold signature should verify: {:?}",
            result.failure_reason
        );
        assert!(result.authority_id.is_some());
        assert!(result.device_id.is_some());
    }

    #[tokio::test]
    async fn invalid_threshold_signature_is_rejected() {
        let authority_id = AuthorityId::new_from_entropy([94u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        // Create a challenge
        let challenge = handler.create_challenge(&effects).await.unwrap();

        // Create an invalid threshold signature response (wrong signature bytes)
        let response = AuthResponse {
            challenge_id: challenge.challenge_id.clone(),
            signature: vec![0u8; 64],  // Invalid signature
            public_key: vec![0u8; 32], // Invalid public key
            auth_method: AuthMethod::ThresholdSignature,
        };

        // Verification should fail (not panic)
        let response = handler.build_response_ingress(response).unwrap();
        let result = handler.verify_response(&effects, &response).await;
        match result {
            Ok(auth_result) => {
                assert!(
                    !auth_result.authenticated,
                    "Invalid signature should not authenticate"
                );
            }
            Err(_) => {
                // Error is also acceptable for invalid crypto inputs
            }
        }
    }
}
