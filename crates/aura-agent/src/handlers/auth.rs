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
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::{FlowCost, Hash32};
use aura_guards::chain::create_send_guard;
use aura_guards::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};
use aura_protocol::effects::EffectApiEffects;
use aura_signature::session::SessionScope;
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

const AUTH_CHALLENGE_PROTOCOL_VERSION: u16 = 1;

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
    /// Context for the authentication request
    pub context_id: ContextId,
    /// Device expected to answer the challenge
    pub device_id: DeviceId,
    /// Requested authentication scope
    pub scope: SessionScope,
    /// Authentication epoch associated with this challenge
    pub epoch: u64,
    /// Protocol version for transcript binding
    pub protocol_version: u16,
    /// Authority allowed to consume this challenge response.
    pub audience_authority_id: AuthorityId,
}

#[derive(Debug, Clone, Serialize)]
struct AuthChallengeTranscriptPayload {
    challenge_id: String,
    challenge_bytes: Vec<u8>,
    created_at: u64,
    expires_at: u64,
    authority_id: AuthorityId,
    context_id: ContextId,
    device_id: DeviceId,
    scope: SessionScope,
    epoch: u64,
    protocol_version: u16,
    audience_authority_id: AuthorityId,
    auth_method: AuthMethod,
    response_authority_id: AuthorityId,
    response_device_id: Option<DeviceId>,
    response_threshold_epoch: Option<u64>,
}

struct AuthChallengeTranscript<'a> {
    challenge: &'a AuthChallenge,
    response: &'a AuthResponse,
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
            context_id: self.challenge.context_id,
            device_id: self.challenge.device_id,
            scope: self.challenge.scope.clone(),
            epoch: self.challenge.epoch,
            protocol_version: self.challenge.protocol_version,
            audience_authority_id: self.challenge.audience_authority_id,
            auth_method: self.response.auth_method.clone(),
            response_authority_id: self.response.authority_id,
            response_device_id: self.response.device_id,
            response_threshold_epoch: self.response.threshold_epoch,
        }
    }
}

fn auth_challenge_transcript<'a>(
    challenge: &'a AuthChallenge,
    response: &'a AuthResponse,
) -> AuthChallengeTranscript<'a> {
    AuthChallengeTranscript {
        challenge,
        response,
    }
}

/// Authentication response containing signed challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    /// Challenge identifier being responded to
    pub challenge_id: String,
    /// Signature over the challenge bytes
    pub signature: Vec<u8>,
    /// Authentication method used
    pub auth_method: AuthMethod,
    /// Authority claiming to satisfy the challenge.
    pub authority_id: AuthorityId,
    /// Device identity for device-key authentication.
    pub device_id: Option<DeviceId>,
    /// Threshold epoch for authority threshold authentication.
    pub threshold_epoch: Option<u64>,
    /// Test-only untrusted key material retained for negative tests.
    #[cfg(test)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub legacy_untrusted_public_key: Vec<u8>,
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
        let scope = SessionScope::Protocol {
            protocol_type: "authentication".to_string(),
        };

        let challenge = AuthChallenge {
            challenge_id: challenge_id.clone(),
            challenge_bytes,
            created_at: current_time,
            expires_at: current_time + 300_000, // 5 minute expiry
            authority_id: self.context.authority.authority_id(),
            context_id: self.context.authority.default_context_id(),
            device_id: self.device_id(),
            scope,
            epoch: 0,
            protocol_version: AUTH_CHALLENGE_PROTOCOL_VERSION,
            audience_authority_id: self.context.authority.authority_id(),
        };

        // Store pending challenge
        self.challenge_manager
            .cache_challenge(challenge.clone())
            .await
            .map_err(AgentError::effects)?;

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
        if response.authority_id != challenge.authority_id {
            return Err(AgentError::effects(
                "device authentication response authority does not match challenge",
            ));
        }
        let response_device_id = response.device_id.ok_or_else(|| {
            AgentError::effects("device authentication response is missing device id")
        })?;
        if response_device_id != challenge.device_id {
            return Err(AgentError::effects(
                "device authentication response device does not match challenge",
            ));
        }
        if response.threshold_epoch.is_some() {
            return Err(AgentError::effects(
                "device authentication response must not carry a threshold epoch",
            ));
        }
        let trusted_key = self
            .key_resolver
            .resolve_device_key(response_device_id)
            .map_err(|e| AgentError::effects(format!("device key resolution failed: {e}")))?;
        let transcript = auth_challenge_transcript(challenge, response);

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
        if response.authority_id != challenge.authority_id {
            return Err(AgentError::effects(
                "threshold authentication response authority does not match challenge",
            ));
        }
        if response.device_id.is_some() {
            return Err(AgentError::effects(
                "threshold authentication response must not carry a device id",
            ));
        }
        let threshold_epoch = response.threshold_epoch.ok_or_else(|| {
            AgentError::effects("threshold authentication response is missing threshold epoch")
        })?;
        if threshold_epoch != challenge.epoch {
            return Err(AgentError::effects(
                "threshold authentication response epoch does not match challenge",
            ));
        }
        // Validate input lengths
        if response.signature.len() != 64 {
            return Err(AgentError::effects(format!(
                "Invalid FROST signature length: {} (expected 64)",
                response.signature.len()
            )));
        }
        let trusted_key = self
            .key_resolver
            .resolve_authority_threshold_key(response.authority_id, threshold_epoch)
            .map_err(|e| AgentError::effects(format!("threshold key resolution failed: {e}")))?;

        let transcript = auth_challenge_transcript(challenge, response);

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

        let response = AuthResponse {
            challenge_id: challenge.challenge_id.clone(),
            signature: vec![0_u8; 64],
            auth_method: AuthMethod::DeviceKey,
            authority_id: challenge.authority_id,
            device_id: Some(challenge.device_id),
            threshold_epoch: None,
            legacy_untrusted_public_key: public_key,
        };
        let transcript = auth_challenge_transcript(challenge, &response);

        let signature = sign_ed25519_transcript(effects, &transcript, &private_key)
            .await
            .map_err(|e| AgentError::effects(format!("failed to sign challenge: {e}")))?;

        Ok(AuthResponse {
            signature,
            ..response
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
        let trusted_key = self
            .key_resolver
            .resolve_authority_threshold_key(challenge.authority_id, challenge.epoch)
            .map_err(|e| AgentError::effects(format!("threshold key resolution failed: {e}")))?;
        if trusted_key.bytes() != group_public_key.as_slice() {
            return Err(AgentError::effects(
                "threshold signing key package does not match enrolled authority threshold key",
            ));
        }
        let response = AuthResponse {
            challenge_id: challenge.challenge_id.clone(),
            signature: vec![0_u8; 64],
            auth_method: AuthMethod::ThresholdSignature,
            authority_id: challenge.authority_id,
            device_id: None,
            threshold_epoch: Some(challenge.epoch),
            #[cfg(test)]
            legacy_untrusted_public_key: group_public_key.clone(),
        };
        let transcript = auth_challenge_transcript(challenge, &response);
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
            signature,
            ..response
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
    use crate::core::{default_context_id_for_authority, AgentConfig, AuthorityContext};
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

        let config = AgentConfig {
            storage: crate::core::config::StorageConfig {
                encryption_policy: crate::core::config::StorageEncryptionPolicy::PlaintextForTests,
                ..Default::default()
            },
            ..Default::default()
        };
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        let challenge = handler.create_challenge(&effects).await.unwrap();
        assert!(!challenge.challenge_id.is_empty());
        assert_eq!(challenge.challenge_bytes.len(), 32);
        assert!(challenge.expires_at > challenge.created_at);
        assert_eq!(challenge.protocol_version, AUTH_CHALLENGE_PROTOCOL_VERSION);
    }

    #[test]
    fn auth_challenge_transcript_binds_method_authority_and_device() {
        let authority_id = AuthorityId::new_from_entropy([9; 32]);
        let challenge = AuthChallenge {
            challenge_id: "challenge-1".to_string(),
            challenge_bytes: vec![1; 32],
            created_at: 100,
            expires_at: 200,
            authority_id,
            context_id: default_context_id_for_authority(authority_id),
            device_id: DeviceId::from_uuid(uuid::Uuid::from_u128(1)),
            scope: SessionScope::Protocol {
                protocol_type: "authentication".to_string(),
            },
            epoch: 0,
            protocol_version: AUTH_CHALLENGE_PROTOCOL_VERSION,
            audience_authority_id: authority_id,
        };

        let device_response = AuthResponse {
            challenge_id: challenge.challenge_id.clone(),
            signature: vec![1; 64],
            auth_method: AuthMethod::DeviceKey,
            authority_id,
            device_id: Some(challenge.device_id),
            threshold_epoch: None,
            legacy_untrusted_public_key: vec![1; 32],
        };
        let threshold_response = AuthResponse {
            challenge_id: challenge.challenge_id.clone(),
            signature: vec![1; 64],
            auth_method: AuthMethod::ThresholdSignature,
            authority_id,
            device_id: None,
            threshold_epoch: Some(challenge.epoch),
            legacy_untrusted_public_key: vec![1; 32],
        };
        let wrong_device_response = AuthResponse {
            device_id: Some(DeviceId::from_uuid(uuid::Uuid::from_u128(2))),
            ..device_response.clone()
        };

        let device_key = auth_challenge_transcript(&challenge, &device_response)
            .transcript_bytes()
            .unwrap();
        let threshold = auth_challenge_transcript(&challenge, &threshold_response)
            .transcript_bytes()
            .unwrap();
        let wrong_device = auth_challenge_transcript(&challenge, &wrong_device_response)
            .transcript_bytes()
            .unwrap();

        assert_ne!(device_key, threshold);
        assert_ne!(device_key, wrong_device);
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
            auth_method: AuthMethod::DeviceKey,
            authority_id,
            device_id: Some(handler.device_id()),
            threshold_epoch: None,
            legacy_untrusted_public_key: vec![0u8; 32],
        };

        let response = handler.build_response_ingress(response).unwrap();
        let result = handler.verify_response(&effects, &response).await.unwrap();
        assert!(!result.authenticated);
        assert!(result.failure_reason.is_some());
    }

    #[tokio::test]
    async fn device_auth_uses_enrolled_device_key_not_response_key() {
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
        let enrolled_key = response.legacy_untrusted_public_key.clone();
        handler
            .key_resolver
            .register_device_key(challenge.device_id, enrolled_key)
            .unwrap();
        response.legacy_untrusted_public_key = vec![0xAA; 32];

        let response = handler.build_response_ingress(response).unwrap();
        let result = handler.verify_response(&effects, &response).await.unwrap();
        assert!(
            result.authenticated,
            "auth verification should use the trusted registered device key, not the response key"
        );
    }

    #[tokio::test]
    async fn stale_proof_replay_to_new_challenge_id_is_rejected() {
        let authority_id = AuthorityId::new_from_entropy([96u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        let old_challenge = handler.create_challenge(&effects).await.unwrap();
        let fresh_challenge = handler.create_challenge(&effects).await.unwrap();
        let signed_old = handler
            .sign_challenge_with_ephemeral_key_for_tests(&effects, &old_challenge)
            .await
            .unwrap();
        handler
            .key_resolver
            .register_device_key(
                old_challenge.device_id,
                signed_old.legacy_untrusted_public_key.clone(),
            )
            .unwrap();

        let replay = AuthResponse {
            challenge_id: fresh_challenge.challenge_id.clone(),
            signature: signed_old.signature,
            auth_method: signed_old.auth_method,
            authority_id: signed_old.authority_id,
            device_id: signed_old.device_id,
            threshold_epoch: signed_old.threshold_epoch,
            legacy_untrusted_public_key: signed_old.legacy_untrusted_public_key,
        };

        let replay = handler.build_response_ingress(replay).unwrap();
        let result = handler.verify_response(&effects, &replay).await.unwrap();
        assert!(!result.authenticated);
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
        let mut challenge = handler.create_challenge(&effects).await.unwrap();
        challenge.epoch = 7;
        handler
            .challenge_manager
            .remove_challenge(&challenge.challenge_id)
            .await;
        handler
            .challenge_manager
            .cache_challenge(challenge.clone())
            .await
            .unwrap();
        assert_eq!(challenge.challenge_bytes.len(), 32);
        handler
            .key_resolver
            .register_authority_threshold_key(
                authority_id,
                challenge.epoch,
                extract_group_public_key(&key_gen_result.public_key_package).unwrap(),
            )
            .unwrap();

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
        assert_eq!(response.legacy_untrusted_public_key.len(), 32); // Ed25519 public key length

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
        let mut challenge = handler.create_challenge(&effects).await.unwrap();
        challenge.epoch = 4;
        handler
            .challenge_manager
            .remove_challenge(&challenge.challenge_id)
            .await;
        handler
            .challenge_manager
            .cache_challenge(challenge.clone())
            .await
            .unwrap();

        // Create an invalid threshold signature response (wrong signature bytes)
        let response = AuthResponse {
            challenge_id: challenge.challenge_id.clone(),
            signature: vec![0u8; 64], // Invalid signature
            auth_method: AuthMethod::ThresholdSignature,
            authority_id,
            device_id: None,
            threshold_epoch: Some(challenge.epoch),
            legacy_untrusted_public_key: vec![0u8; 32], // Invalid public key
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

    #[tokio::test]
    async fn attacker_generated_device_keypair_is_rejected_without_enrollment() {
        let authority_id = AuthorityId::new_from_entropy([97u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        let challenge = handler.create_challenge(&effects).await.unwrap();
        let response = handler
            .sign_challenge_with_ephemeral_key_for_tests(&effects, &challenge)
            .await
            .unwrap();

        let response = handler.build_response_ingress(response).unwrap();
        let result = handler.verify_response(&effects, &response).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn wrong_authority_id_is_rejected() {
        let authority_id = AuthorityId::new_from_entropy([98u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        let challenge = handler.create_challenge(&effects).await.unwrap();
        let mut response = handler
            .sign_challenge_with_ephemeral_key_for_tests(&effects, &challenge)
            .await
            .unwrap();
        handler
            .key_resolver
            .register_device_key(
                challenge.device_id,
                response.legacy_untrusted_public_key.clone(),
            )
            .unwrap();
        response.authority_id = AuthorityId::new_from_entropy([99u8; 32]);

        let response = handler.build_response_ingress(response).unwrap();
        let result = handler.verify_response(&effects, &response).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn wrong_device_id_is_rejected() {
        let authority_id = AuthorityId::new_from_entropy([100u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        let challenge = handler.create_challenge(&effects).await.unwrap();
        let mut response = handler
            .sign_challenge_with_ephemeral_key_for_tests(&effects, &challenge)
            .await
            .unwrap();
        response.device_id = Some(DeviceId::new_from_entropy([101u8; 32]));

        let response = handler.build_response_ingress(response).unwrap();
        let result = handler.verify_response(&effects, &response).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn swapped_threshold_group_key_is_rejected() {
        let authority_id = AuthorityId::new_from_entropy([102u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system(&config);
        let handler = AuthHandler::new(authority_context).unwrap();

        let enrolled = effects
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                2,
                3,
            )
            .await
            .unwrap();
        let attacker = effects
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                2,
                3,
            )
            .await
            .unwrap();

        let mut challenge = handler.create_challenge(&effects).await.unwrap();
        challenge.epoch = 9;
        handler
            .challenge_manager
            .remove_challenge(&challenge.challenge_id)
            .await;
        handler
            .challenge_manager
            .cache_challenge(challenge.clone())
            .await
            .unwrap();
        handler
            .key_resolver
            .register_authority_threshold_key(
                authority_id,
                challenge.epoch,
                extract_group_public_key(&enrolled.public_key_package).unwrap(),
            )
            .unwrap();

        let result = handler
            .sign_challenge_threshold(
                &effects,
                &challenge,
                &attacker.key_packages,
                &attacker.public_key_package,
                &[1, 2],
            )
            .await;
        assert!(result.is_err());
    }
}
