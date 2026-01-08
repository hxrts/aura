//! Authentication Service - Public API for Authentication Operations
//!
//! Provides a clean public interface for authentication operations.
//! Wraps `AuthHandler` with ergonomic methods and proper error handling.

use super::auth::{AuthChallenge, AuthHandler, AuthMethod, AuthResponse, AuthResult};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::choreography_adapter::AuraProtocolAdapter;
use crate::runtime::AuraEffectSystem;
use aura_authentication::dkd::{DkdMessage, DkdSessionId};
use aura_authentication::dkd_runners::{execute_as as dkd_execute_as, DkdChoreographyRole};
use aura_authentication::guardian_auth_relational::{
    GuardianAuthProof, GuardianAuthRequest, GuardianAuthResponse,
};
use aura_authentication::guardian_auth_runners::{
    execute_as as guardian_auth_execute_as, GuardianAuthRelationalRole,
};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::hash;
use aura_core::identifiers::{AccountId, AuthorityId, ContextId, DeviceId};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use uuid::Uuid;

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

    // ========================================================================
    // DKD Choreography (execute_as)
    // ========================================================================

    /// Execute DKD choreography as the initiator with a single participant.
    pub async fn execute_dkd_initiator(
        &self,
        participant: AuthorityId,
    ) -> AgentResult<DkdSessionId> {
        use crate::core::AgentError;

        let authority_id = self.handler.authority_context().authority_id();
        // Use authority_id for deterministic session ID instead of random UUID
        let session_id = DkdSessionId::deterministic(&authority_id.to_string());
        let timestamp = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        let mut role_map = HashMap::new();
        role_map.insert(DkdChoreographyRole::Initiator, authority_id);
        role_map.insert(DkdChoreographyRole::Participant, participant);

        let initiate_type = std::any::type_name::<DkdMessage>();

        let mut outbound = VecDeque::new();
        outbound.push_back(DkdMessage {
            session_id: session_id.clone(),
            message_type: "initiate".to_string(),
            payload: hash::hash(format!("init:{}", session_id.0).as_bytes()).to_vec(),
            sender: DeviceId::from_uuid(authority_id.0),
            timestamp,
        });
        outbound.push_back(DkdMessage {
            session_id: session_id.clone(),
            message_type: "reveal_request".to_string(),
            payload: hash::hash(format!("reveal:{}", session_id.0).as_bytes()).to_vec(),
            sender: DeviceId::from_uuid(authority_id.0),
            timestamp,
        });
        outbound.push_back(DkdMessage {
            session_id: session_id.clone(),
            message_type: "key_derived".to_string(),
            payload: hash::hash(format!("key:{}", session_id.0).as_bytes()).to_vec(),
            sender: DeviceId::from_uuid(authority_id.0),
            timestamp,
        });

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            DkdChoreographyRole::Initiator,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if request.type_name == initiate_type {
                return outbound
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_uuid = dkd_session_uuid(&session_id);
        adapter
            .start_session(session_uuid)
            .await
            .map_err(|e| AgentError::internal(format!("dkd start failed: {e}")))?;

        let result = dkd_execute_as(DkdChoreographyRole::Initiator, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("dkd failed: {e}")));

        let _ = adapter.end_session().await;
        result.map(|_| session_id)
    }

    /// Execute DKD choreography as the participant for an existing session.
    pub async fn execute_dkd_participant(
        &self,
        initiator: AuthorityId,
        session_id: DkdSessionId,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.authority_context().authority_id();
        let timestamp = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        let mut role_map = HashMap::new();
        role_map.insert(DkdChoreographyRole::Initiator, initiator);
        role_map.insert(DkdChoreographyRole::Participant, authority_id);

        let message_type = std::any::type_name::<DkdMessage>();

        let mut outbound = VecDeque::new();
        outbound.push_back(DkdMessage {
            session_id: session_id.clone(),
            message_type: "commitment".to_string(),
            payload: hash::hash(format!("commit:{}", session_id.0).as_bytes()).to_vec(),
            sender: DeviceId::from_uuid(authority_id.0),
            timestamp,
        });
        outbound.push_back(DkdMessage {
            session_id: session_id.clone(),
            message_type: "reveal".to_string(),
            payload: hash::hash(format!("reveal:{}", session_id.0).as_bytes()).to_vec(),
            sender: DeviceId::from_uuid(authority_id.0),
            timestamp,
        });

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            DkdChoreographyRole::Participant,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if request.type_name == message_type {
                return outbound
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_uuid = dkd_session_uuid(&session_id);
        adapter
            .start_session(session_uuid)
            .await
            .map_err(|e| AgentError::internal(format!("dkd start failed: {e}")))?;

        let result = dkd_execute_as(DkdChoreographyRole::Participant, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("dkd failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    // ========================================================================
    // GuardianAuthRelational Choreography (execute_as)
    // ========================================================================

    /// Execute GuardianAuthRelational choreography as the Account role.
    ///
    /// The Account initiates guardian authentication by sending a request to
    /// the Coordinator, then receives the final authentication result.
    ///
    /// # Arguments
    /// * `coordinator` - AuthorityId of the coordinator
    /// * `guardian` - AuthorityId of the guardian to authenticate
    /// * `context_id` - ContextId for the guardian relationship
    /// * `request` - The guardian authentication request
    ///
    /// # Returns
    /// Ok(()) on successful protocol completion
    pub async fn execute_guardian_auth_as_account(
        &self,
        coordinator: AuthorityId,
        guardian: AuthorityId,
        context_id: ContextId,
        request: GuardianAuthRequest,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();

        let mut role_map = HashMap::new();
        role_map.insert(GuardianAuthRelationalRole::Account, authority_id);
        role_map.insert(GuardianAuthRelationalRole::Coordinator, coordinator);
        role_map.insert(GuardianAuthRelationalRole::Guardian, guardian);

        let request_type = std::any::type_name::<GuardianAuthRequest>();
        let request_clone = request.clone();

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            GuardianAuthRelationalRole::Account,
            role_map,
        )
        .with_message_provider(move |req_ctx, _received| {
            if req_ctx.type_name == request_type {
                return Some(Box::new(request_clone.clone()) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_uuid = guardian_auth_session_uuid(&context_id, &request);
        adapter
            .start_session(session_uuid)
            .await
            .map_err(|e| AgentError::internal(format!("guardian auth start failed: {e}")))?;

        guardian_auth_execute_as(GuardianAuthRelationalRole::Account, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("guardian auth failed: {e}")))?;

        let _ = adapter.end_session().await;
        Ok(())
    }

    /// Execute GuardianAuthRelational choreography as the Coordinator role.
    ///
    /// The Coordinator receives the request from Account, forwards it to Guardian,
    /// receives the proof from Guardian, and sends the result back to Account.
    ///
    /// # Arguments
    /// * `account` - AuthorityId of the account requesting authentication
    /// * `guardian` - AuthorityId of the guardian
    /// * `context_id` - ContextId for the guardian relationship
    /// * `request` - The guardian authentication request (used for session ID)
    ///
    /// # Returns
    /// Ok(()) on successful protocol coordination
    pub async fn execute_guardian_auth_as_coordinator(
        &self,
        account: AuthorityId,
        guardian: AuthorityId,
        context_id: ContextId,
        request: GuardianAuthRequest,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();

        let mut role_map = HashMap::new();
        role_map.insert(GuardianAuthRelationalRole::Account, account);
        role_map.insert(GuardianAuthRelationalRole::Coordinator, authority_id);
        role_map.insert(GuardianAuthRelationalRole::Guardian, guardian);

        // Coordinator forwards the request to guardian and then builds response
        let request_type = std::any::type_name::<GuardianAuthRequest>();
        let response_type = std::any::type_name::<GuardianAuthResponse>();
        let request_clone = request.clone();

        // Create a queue for outbound messages
        let mut outbound_requests: VecDeque<GuardianAuthRequest> = VecDeque::new();
        outbound_requests.push_back(request_clone.clone());

        let mut outbound_responses: VecDeque<GuardianAuthResponse> = VecDeque::new();
        // We'll prepare a default response; the actual response will be populated
        // after receiving the guardian's proof in a real implementation
        outbound_responses.push_back(GuardianAuthResponse {
            success: true,
            authorized: true,
            error: None,
        });

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            GuardianAuthRelationalRole::Coordinator,
            role_map,
        )
        .with_message_provider(move |req_ctx, _received| {
            if req_ctx.type_name == request_type {
                return outbound_requests
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            if req_ctx.type_name == response_type {
                return outbound_responses
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_uuid = guardian_auth_session_uuid(&context_id, &request);
        adapter
            .start_session(session_uuid)
            .await
            .map_err(|e| AgentError::internal(format!("guardian auth start failed: {e}")))?;

        guardian_auth_execute_as(GuardianAuthRelationalRole::Coordinator, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("guardian auth failed: {e}")))?;

        let _ = adapter.end_session().await;
        Ok(())
    }

    /// Execute GuardianAuthRelational choreography as the Guardian role.
    ///
    /// The Guardian receives a forwarded request from the Coordinator,
    /// validates it, creates a proof, and sends it back to the Coordinator.
    ///
    /// # Arguments
    /// * `account` - AuthorityId of the account requesting authentication
    /// * `coordinator` - AuthorityId of the coordinator
    /// * `context_id` - ContextId for the guardian relationship
    /// * `request` - The guardian authentication request (used for session ID)
    /// * `proof` - The pre-computed guardian authentication proof
    ///
    /// # Returns
    /// Ok(()) on successful proof submission
    pub async fn execute_guardian_auth_as_guardian(
        &self,
        account: AuthorityId,
        coordinator: AuthorityId,
        context_id: ContextId,
        request: GuardianAuthRequest,
        proof: GuardianAuthProof,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();

        let mut role_map = HashMap::new();
        role_map.insert(GuardianAuthRelationalRole::Account, account);
        role_map.insert(GuardianAuthRelationalRole::Coordinator, coordinator);
        role_map.insert(GuardianAuthRelationalRole::Guardian, authority_id);

        let proof_type = std::any::type_name::<GuardianAuthProof>();
        let proof_clone = proof.clone();

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            GuardianAuthRelationalRole::Guardian,
            role_map,
        )
        .with_message_provider(move |req_ctx, _received| {
            if req_ctx.type_name == proof_type {
                return Some(Box::new(proof_clone.clone()) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_uuid = guardian_auth_session_uuid(&context_id, &request);
        adapter
            .start_session(session_uuid)
            .await
            .map_err(|e| AgentError::internal(format!("guardian auth start failed: {e}")))?;

        guardian_auth_execute_as(GuardianAuthRelationalRole::Guardian, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("guardian auth failed: {e}")))?;

        let _ = adapter.end_session().await;
        Ok(())
    }
}

fn dkd_session_uuid(session_id: &DkdSessionId) -> Uuid {
    let digest = hash::hash(session_id.0.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

fn guardian_auth_session_uuid(context_id: &ContextId, request: &GuardianAuthRequest) -> Uuid {
    // Create deterministic session ID from context + guardian + account
    let key = format!(
        "guardian_auth:{}:{}:{}",
        context_id.0, request.guardian_id, request.account_id
    );
    let digest = hash::hash(key.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::identifiers::AuthorityId;

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

    #[test]
    fn test_guardian_auth_session_uuid_deterministic() {
        use aura_authentication::guardian_auth_relational::GuardianOperation;

        let context_id = ContextId(Uuid::from_bytes([1u8; 16]));
        let guardian_id = AuthorityId::new_from_entropy([50u8; 32]);
        let account_id = AuthorityId::new_from_entropy([51u8; 32]);

        let request = GuardianAuthRequest {
            context_id,
            guardian_id,
            account_id,
            operation: GuardianOperation::DenyRecovery {
                reason: "test".to_string(),
            },
        };

        let uuid1 = guardian_auth_session_uuid(&context_id, &request);
        let uuid2 = guardian_auth_session_uuid(&context_id, &request);

        // Same inputs should produce same session ID
        assert_eq!(uuid1, uuid2);

        // Different context should produce different session ID
        let context_id2 = ContextId(Uuid::from_bytes([2u8; 16]));
        let uuid3 = guardian_auth_session_uuid(&context_id2, &request);
        assert_ne!(uuid1, uuid3);
    }

    #[test]
    fn test_guardian_auth_session_uuid_different_participants() {
        use aura_authentication::guardian_auth_relational::GuardianOperation;

        let context_id = ContextId(Uuid::from_bytes([3u8; 16]));
        let guardian_id1 = AuthorityId::new_from_entropy([52u8; 32]);
        let guardian_id2 = AuthorityId::new_from_entropy([53u8; 32]);
        let account_id = AuthorityId::new_from_entropy([54u8; 32]);

        let request1 = GuardianAuthRequest {
            context_id,
            guardian_id: guardian_id1,
            account_id,
            operation: GuardianOperation::DenyRecovery {
                reason: "test".to_string(),
            },
        };

        let request2 = GuardianAuthRequest {
            context_id,
            guardian_id: guardian_id2,
            account_id,
            operation: GuardianOperation::DenyRecovery {
                reason: "test".to_string(),
            },
        };

        let uuid1 = guardian_auth_session_uuid(&context_id, &request1);
        let uuid2 = guardian_auth_session_uuid(&context_id, &request2);

        // Different guardian should produce different session ID
        assert_ne!(uuid1, uuid2);
    }
}
