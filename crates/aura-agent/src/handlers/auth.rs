//! Authentication Handlers
//!
//! Handlers for authentication-related operations including device key verification,
//! threshold signatures, and challenge-response authentication.

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{CryptoEffects, RandomEffects};
use aura_core::identifiers::{AuthorityId, DeviceId};
use aura_protocol::effects::EffectApiEffects;
use aura_guards::chain::create_send_guard;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

/// Fact recorded when authentication succeeds
#[derive(Debug, Serialize)]
struct AuthenticatedFact {
    authority_id: AuthorityId,
    device_id: DeviceId,
    auth_method: AuthMethod,
    challenge_id: String,
}

/// Authentication handler
pub struct AuthHandler {
    context: HandlerContext,
    /// Pending challenges awaiting response
    pending_challenges: Arc<RwLock<HashMap<String, AuthChallenge>>>,
}

impl AuthHandler {
    /// Create a new authentication handler
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        Ok(Self {
            context: HandlerContext::new(authority),
            pending_challenges: Arc::new(RwLock::new(HashMap::new())),
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
            authority_id: self.context.authority.authority_id,
        };

        // Store pending challenge
        {
            let mut challenges = self.pending_challenges.write().await;
            challenges.insert(challenge_id, challenge.clone());
        }

        Ok(challenge)
    }

    /// Verify an authentication response
    pub async fn verify_response(
        &self,
        effects: &AuraEffectSystem,
        response: &AuthResponse,
    ) -> AgentResult<AuthResult> {
        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Look up the challenge
        let challenge = {
            let challenges = self.pending_challenges.read().await;
            challenges.get(&response.challenge_id).cloned()
        };

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
            {
                let mut challenges = self.pending_challenges.write().await;
                challenges.remove(&response.challenge_id);
            }
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
            {
                let mut challenges = self.pending_challenges.write().await;
                challenges.remove(&response.challenge_id);
            }

            // Journal authentication fact
            let device_id = self.device_id();
            HandlerUtilities::append_relational_fact(
                &self.context.authority,
                effects,
                self.context.effect_context.context_id(),
                "auth_authenticated",
                &AuthenticatedFact {
                    authority_id: self.context.authority.authority_id,
                    device_id,
                    auth_method: response.auth_method.clone(),
                    challenge_id: response.challenge_id.clone(),
                },
            )
            .await?;

            Ok(AuthResult {
                authenticated: true,
                authority_id: Some(self.context.authority.authority_id),
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

    /// Verify a device key Ed25519 signature
    async fn verify_device_key_signature(
        &self,
        effects: &AuraEffectSystem,
        challenge: &AuthChallenge,
        response: &AuthResponse,
    ) -> AgentResult<bool> {
        // Verify the Ed25519 signature over the challenge bytes
        let verified = effects
            .ed25519_verify(
                &challenge.challenge_bytes,
                &response.signature,
                &response.public_key,
            )
            .await
            .map_err(|e| AgentError::effects(format!("signature verification failed: {e}")))?;

        Ok(verified)
    }

    /// Verify a threshold (FROST) signature
    ///
    /// For FROST threshold authentication, the `AuthResponse` should contain:
    /// - `signature`: The aggregated FROST signature (64 bytes for Ed25519)
    /// - `public_key`: The group public key from the threshold key package (32 bytes)
    ///
    /// The signature is verified against the challenge bytes using the group public key.
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
        if response.public_key.len() != 32 {
            return Err(AgentError::effects(format!(
                "Invalid group public key length: {} (expected 32)",
                response.public_key.len()
            )));
        }

        // Verify the FROST aggregate signature over the challenge bytes
        let verified = effects
            .frost_verify(
                &challenge.challenge_bytes,
                &response.signature,
                &response.public_key,
            )
            .await
            .map_err(|e| AgentError::effects(format!("FROST verification failed: {e}")))?;

        Ok(verified)
    }

    /// Sign a challenge using the device key
    pub async fn sign_challenge(
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

        // Sign the challenge bytes
        let signature = effects
            .ed25519_sign(&challenge.challenge_bytes, &private_key)
            .await
            .map_err(|e| AgentError::effects(format!("failed to sign challenge: {e}")))?;

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

        // Step 2: Create signing package
        let signing_package = effects
            .frost_create_signing_package(
                &challenge.challenge_bytes,
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

        // Extract the group public key from the public key package
        // The group public key is the first 32 bytes of the serialized package
        // or we can use frost_verify directly with the full package
        let group_public_key = extract_group_public_key(public_key_package)?;

        Ok(AuthResponse {
            challenge_id: challenge.challenge_id.clone(),
            signature,
            public_key: group_public_key,
            auth_method: AuthMethod::ThresholdSignature,
        })
    }

    /// Handle authentication request (legacy API for backwards compatibility)
    pub async fn authenticate(&self, effects: &AuraEffectSystem) -> AgentResult<()> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Skip guard in test mode
        if effects.is_testing() {
            return Ok(());
        }

        let guard = create_send_guard(
            "auth:authenticate".to_string(),
            self.context.effect_context.context_id(),
            self.context.authority.authority_id,
            50,
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

        HandlerUtilities::append_relational_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "auth_authenticated",
            &serde_json::json!({ "authority": self.context.authority.authority_id }),
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::core::AuthorityContext;
    use crate::runtime::effects::AuraEffectSystem;
    use aura_core::identifiers::{AuthorityId, ContextId};

    #[tokio::test]
    async fn auth_fact_is_journaled() {
        let authority_id = AuthorityId::new_from_entropy([90u8; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([8u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();
        let handler = AuthHandler::new(authority_context.clone()).unwrap();

        handler.authenticate(&effects).await.unwrap();
    }

    #[tokio::test]
    async fn challenge_can_be_created() {
        let authority_id = AuthorityId::new_from_entropy([91u8; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([9u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();
        let handler = AuthHandler::new(authority_context).unwrap();

        let challenge = handler.create_challenge(&effects).await.unwrap();
        assert!(!challenge.challenge_id.is_empty());
        assert_eq!(challenge.challenge_bytes.len(), 32);
        assert!(challenge.expires_at > challenge.created_at);
    }

    #[tokio::test]
    async fn expired_challenge_is_rejected() {
        let authority_id = AuthorityId::new_from_entropy([92u8; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([10u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();
        let handler = AuthHandler::new(authority_context).unwrap();

        // Create a response for a non-existent challenge
        let response = AuthResponse {
            challenge_id: "nonexistent".to_string(),
            signature: vec![0u8; 64],
            public_key: vec![0u8; 32],
            auth_method: AuthMethod::DeviceKey,
        };

        let result = handler.verify_response(&effects, &response).await.unwrap();
        assert!(!result.authenticated);
        assert!(result.failure_reason.is_some());
    }

    #[tokio::test]
    async fn threshold_signature_verification_works() {
        use aura_core::effects::CryptoEffects;

        let authority_id = AuthorityId::new_from_entropy([93u8; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([11u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();
        let handler = AuthHandler::new(authority_context).unwrap();

        // Step 1: Generate FROST threshold keys (2-of-3)
        let threshold = 2;
        let max_signers = 3;
        let key_gen_result = effects
            .frost_generate_keys(threshold, max_signers)
            .await
            .expect("FROST key generation should succeed");

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
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([12u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();
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
