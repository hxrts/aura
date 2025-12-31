//! Authentication Service - Public API for Authentication Operations
//!
//! Provides a clean public interface for authentication operations.
//! Wraps `AuthHandler` with ergonomic methods and proper error handling.

use super::auth::{AuthChallenge, AuthHandler, AuthMethod, AuthResponse, AuthResult};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::identifiers::{AccountId, DeviceId};
use std::sync::Arc;

/// Authentication service API
///
/// Provides authentication operations through a clean public API.
#[derive(Clone)]
pub struct AuthServiceApi {
    handler: AuthHandler,
    effects: Arc<AuraEffectSystem>,
}

impl std::fmt::Debug for AuthServiceApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthServiceApi").finish_non_exhaustive()
    }
}

impl AuthServiceApi {
    /// Create a new authentication service
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        _account_id: AccountId,
    ) -> AgentResult<Self> {
        let handler = AuthHandler::new(authority_context)?;
        Ok(Self { handler, effects })
    }

    /// Create an authentication challenge for challenge-response auth
    ///
    /// # Returns
    /// An `AuthChallenge` that must be signed by the authenticating party
    pub async fn create_challenge(&self) -> AgentResult<AuthChallenge> {
        self.handler.create_challenge(&self.effects).await
    }

    /// Verify an authentication response
    ///
    /// # Arguments
    /// * `response` - The signed challenge response
    ///
    /// # Returns
    /// An `AuthResult` indicating whether authentication succeeded
    pub async fn verify(&self, response: &AuthResponse) -> AgentResult<AuthResult> {
        self.handler.verify_response(&self.effects, response).await
    }

    /// Authenticate using device key (convenience method)
    ///
    /// Creates a challenge, signs it with the device key, and verifies.
    /// This is primarily useful for self-authentication scenarios.
    ///
    /// # Returns
    /// An `AuthResult` indicating whether authentication succeeded
    pub async fn authenticate_with_device_key(&self) -> AgentResult<AuthResult> {
        // Create challenge
        let challenge = self.handler.create_challenge(&self.effects).await?;

        // Sign with device key
        let response = self
            .handler
            .sign_challenge(&self.effects, &challenge)
            .await?;

        // Verify the response
        self.handler.verify_response(&self.effects, &response).await
    }

    /// Check if the agent is authenticated
    ///
    /// This is a simple check using the legacy authenticate method.
    ///
    /// # Returns
    /// `true` if authentication passes, `false` otherwise
    pub async fn is_authenticated(&self) -> bool {
        self.handler.authenticate(&self.effects).await.is_ok()
    }

    /// Get the device ID for this authentication service
    pub fn device_id(&self) -> DeviceId {
        self.handler.device_id()
    }

    /// Get supported authentication methods
    pub fn supported_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::DeviceKey, AuthMethod::ThresholdSignature]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::identifiers::{AuthorityId};

    #[tokio::test]
    async fn test_auth_service_creation() {
        let authority_id = AuthorityId::new_from_entropy([80u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([82u8; 32]);

        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = AuthServiceApi::new(effects, authority_context, account_id).unwrap();

        assert!(!service.device_id().0.is_nil());
    }

    #[tokio::test]
    async fn test_is_authenticated() {
        let authority_id = AuthorityId::new_from_entropy([83u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([85u8; 32]);

        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = AuthServiceApi::new(effects, authority_context, account_id).unwrap();

        // In test mode, is_authenticated should return true
        assert!(service.is_authenticated().await);
    }

    #[tokio::test]
    async fn test_challenge_response_flow() {
        let authority_id = AuthorityId::new_from_entropy([86u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([88u8; 32]);

        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = AuthServiceApi::new(effects, authority_context, account_id).unwrap();

        // Create a challenge
        let challenge = service.create_challenge().await.unwrap();
        assert!(!challenge.challenge_id.is_empty());
        assert_eq!(challenge.challenge_bytes.len(), 32);
    }

    #[tokio::test]
    async fn test_supported_methods() {
        let authority_id = AuthorityId::new_from_entropy([89u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([91u8; 32]);

        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = AuthServiceApi::new(effects, authority_context, account_id).unwrap();

        let methods = service.supported_methods();
        assert!(methods.contains(&AuthMethod::DeviceKey));
        assert!(methods.contains(&AuthMethod::ThresholdSignature));
    }
}
