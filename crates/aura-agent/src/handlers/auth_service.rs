//! Authentication Service - Public API for Authentication Operations
//!
//! Provides a clean public interface for authentication operations.
//! Wraps `AuthHandler` with ergonomic methods and proper error handling.

use super::auth::{
    AuthChallenge, AuthHandler, AuthMethod, AuthResponse, AuthResult, AuthenticationStatus,
};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::vm_host_bridge::AuraVmRoundDisposition;
use crate::runtime::{
    handle_owned_vm_round, open_owned_manifest_vm_session_admitted, AuraEffectSystem,
};
use aura_authentication::dkd::{DkdMessage, DkdSessionId};
use aura_authentication::guardian_auth_relational::{
    GuardianAuthProof, GuardianAuthRequest, GuardianAuthResponse,
};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::hash;
use aura_core::types::identifiers::{AccountId, AuthorityId, ContextId, DeviceId};
use aura_core::util::serialization::to_vec;
use aura_mpst::upstream::types::{GlobalType, LocalTypeR};
use aura_mpst::CompositionManifest;
use aura_protocol::effects::{ChoreographicRole, RoleIndex};
use std::collections::BTreeMap;
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

    /// Query the explicit runtime-owned authentication status.
    pub async fn authentication_status(&self) -> AgentResult<AuthenticationStatus> {
        self.handler.authentication_status(&self.effects).await
    }

    /// Get the device ID for this authentication service
    pub fn device_id(&self) -> DeviceId {
        self.handler.device_id()
    }

    /// Get supported authentication methods
    pub fn supported_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::DeviceKey, AuthMethod::ThresholdSignature]
    }

    fn auth_role(authority_id: AuthorityId) -> ChoreographicRole {
        ChoreographicRole::for_authority(authority_id, RoleIndex::new(0).expect("role index"))
    }

    async fn run_vm_protocol(
        &self,
        session_uuid: Uuid,
        roles: Vec<ChoreographicRole>,
        peer_roles: BTreeMap<String, ChoreographicRole>,
        active_role: &str,
        manifest: &CompositionManifest,
        global_type: &GlobalType,
        local_types: &BTreeMap<String, LocalTypeR>,
        initial_payloads: Vec<Vec<u8>>,
    ) -> AgentResult<()> {
        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                self.effects.clone(),
                session_uuid,
                roles,
                manifest,
                active_role,
                global_type,
                local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| AgentError::internal(error.to_string()))?;

            for payload in initial_payloads {
                session.queue_send_bytes(payload);
            }

            let loop_result = loop {
                let round = session
                    .advance_round(active_role, &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                match handle_owned_vm_round(&mut session, round, &format!("auth {active_role} VM"))
                    .map_err(|error| AgentError::internal(error.to_string()))?
                {
                    AuraVmRoundDisposition::Continue => {}
                    AuraVmRoundDisposition::Complete => break Ok(()),
                }
            };

            let _ = session.close().await;
            loop_result
        }
        .await;
        result
    }

    async fn execute_dkd_initiator_vm(
        &self,
        participant: AuthorityId,
    ) -> AgentResult<DkdSessionId> {
        let authority_id = self.handler.authority_context().authority_id();
        let session_id = DkdSessionId::deterministic(&authority_id.to_string());
        let timestamp = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);
        let roles = vec![Self::auth_role(authority_id), Self::auth_role(participant)];
        let peer_roles =
            BTreeMap::from([("Participant".to_string(), Self::auth_role(participant))]);
        let manifest =
            aura_authentication::dkd::telltale_session_types_dkd_protocol::vm_artifacts::composition_manifest();
        let global_type =
            aura_authentication::dkd::telltale_session_types_dkd_protocol::vm_artifacts::global_type();
        let local_types =
            aura_authentication::dkd::telltale_session_types_dkd_protocol::vm_artifacts::local_types();
        let payloads = vec![
            DkdMessage {
                session_id: session_id.clone(),
                message_type: "initiate".to_string(),
                payload: hash::hash(format!("init:{}", session_id.0).as_bytes()).to_vec(),
                sender: self.device_id(),
                timestamp,
            },
            DkdMessage {
                session_id: session_id.clone(),
                message_type: "reveal_request".to_string(),
                payload: hash::hash(format!("reveal:{}", session_id.0).as_bytes()).to_vec(),
                sender: self.device_id(),
                timestamp,
            },
            DkdMessage {
                session_id: session_id.clone(),
                message_type: "key_derived".to_string(),
                payload: hash::hash(format!("key:{}", session_id.0).as_bytes()).to_vec(),
                sender: self.device_id(),
                timestamp,
            },
        ]
        .into_iter()
        .map(|message| {
            to_vec(&message)
                .map_err(|error| AgentError::internal(format!("DKD encode failed: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;

        self.run_vm_protocol(
            dkd_session_uuid(&session_id),
            roles,
            peer_roles,
            "Initiator",
            &manifest,
            &global_type,
            &local_types,
            payloads,
        )
        .await?;

        Ok(session_id)
    }

    async fn execute_dkd_participant_vm(
        &self,
        initiator: AuthorityId,
        session_id: DkdSessionId,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();
        let timestamp = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);
        let roles = vec![Self::auth_role(initiator), Self::auth_role(authority_id)];
        let peer_roles = BTreeMap::from([("Initiator".to_string(), Self::auth_role(initiator))]);
        let manifest =
            aura_authentication::dkd::telltale_session_types_dkd_protocol::vm_artifacts::composition_manifest();
        let global_type =
            aura_authentication::dkd::telltale_session_types_dkd_protocol::vm_artifacts::global_type();
        let local_types =
            aura_authentication::dkd::telltale_session_types_dkd_protocol::vm_artifacts::local_types();
        let payloads = vec![
            DkdMessage {
                session_id: session_id.clone(),
                message_type: "commitment".to_string(),
                payload: hash::hash(format!("commit:{}", session_id.0).as_bytes()).to_vec(),
                sender: self.device_id(),
                timestamp,
            },
            DkdMessage {
                session_id: session_id.clone(),
                message_type: "reveal".to_string(),
                payload: hash::hash(format!("reveal:{}", session_id.0).as_bytes()).to_vec(),
                sender: self.device_id(),
                timestamp,
            },
        ]
        .into_iter()
        .map(|message| {
            to_vec(&message)
                .map_err(|error| AgentError::internal(format!("DKD encode failed: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;

        self.run_vm_protocol(
            dkd_session_uuid(&session_id),
            roles,
            peer_roles,
            "Participant",
            &manifest,
            &global_type,
            &local_types,
            payloads,
        )
        .await
    }

    async fn execute_guardian_auth_as_account_vm(
        &self,
        coordinator: AuthorityId,
        guardian: AuthorityId,
        context_id: ContextId,
        request: GuardianAuthRequest,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();
        let roles = vec![
            Self::auth_role(authority_id),
            Self::auth_role(guardian),
            Self::auth_role(coordinator),
        ];
        let peer_roles =
            BTreeMap::from([("Coordinator".to_string(), Self::auth_role(coordinator))]);
        let manifest = aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::composition_manifest();
        let global_type = aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::global_type();
        let local_types = aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::local_types();

        self.run_vm_protocol(
            guardian_auth_session_uuid(&context_id, &request),
            roles,
            peer_roles,
            "Account",
            &manifest,
            &global_type,
            &local_types,
            vec![to_vec(&request).map_err(|error| {
                AgentError::internal(format!("guardian auth request encode failed: {error}"))
            })?],
        )
        .await
    }

    async fn execute_guardian_auth_as_coordinator_vm(
        &self,
        account: AuthorityId,
        guardian: AuthorityId,
        context_id: ContextId,
        request: GuardianAuthRequest,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();
        let response = GuardianAuthResponse {
            success: true,
            authorized: true,
            error: None,
        };
        let roles = vec![
            Self::auth_role(account),
            Self::auth_role(guardian),
            Self::auth_role(authority_id),
        ];
        let peer_roles = BTreeMap::from([
            ("Account".to_string(), Self::auth_role(account)),
            ("Guardian".to_string(), Self::auth_role(guardian)),
        ]);
        let manifest = aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::composition_manifest();
        let global_type = aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::global_type();
        let local_types = aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::local_types();

        self.run_vm_protocol(
            guardian_auth_session_uuid(&context_id, &request),
            roles,
            peer_roles,
            "Coordinator",
            &manifest,
            &global_type,
            &local_types,
            vec![
                to_vec(&request).map_err(|error| {
                    AgentError::internal(format!("guardian auth request encode failed: {error}"))
                })?,
                to_vec(&response).map_err(|error| {
                    AgentError::internal(format!("guardian auth response encode failed: {error}"))
                })?,
            ],
        )
        .await
    }

    async fn execute_guardian_auth_as_guardian_vm(
        &self,
        account: AuthorityId,
        coordinator: AuthorityId,
        context_id: ContextId,
        request: GuardianAuthRequest,
        proof: GuardianAuthProof,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();
        let roles = vec![
            Self::auth_role(account),
            Self::auth_role(authority_id),
            Self::auth_role(coordinator),
        ];
        let peer_roles =
            BTreeMap::from([("Coordinator".to_string(), Self::auth_role(coordinator))]);
        let manifest = aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::composition_manifest();
        let global_type = aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::global_type();
        let local_types = aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::local_types();

        self.run_vm_protocol(
            guardian_auth_session_uuid(&context_id, &request),
            roles,
            peer_roles,
            "Guardian",
            &manifest,
            &global_type,
            &local_types,
            vec![to_vec(&proof).map_err(|error| {
                AgentError::internal(format!("guardian auth proof encode failed: {error}"))
            })?],
        )
        .await
    }

    // ========================================================================
    // DKD Choreography (execute_as)
    // ========================================================================

    /// Execute DKD choreography as the initiator with a single participant.
    pub async fn execute_dkd_initiator(
        &self,
        participant: AuthorityId,
    ) -> AgentResult<DkdSessionId> {
        self.execute_dkd_initiator_vm(participant).await
    }

    /// Execute DKD choreography as the participant for an existing session.
    pub async fn execute_dkd_participant(
        &self,
        initiator: AuthorityId,
        session_id: DkdSessionId,
    ) -> AgentResult<()> {
        self.execute_dkd_participant_vm(initiator, session_id).await
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
        self.execute_guardian_auth_as_account_vm(coordinator, guardian, context_id, request)
            .await
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
        self.execute_guardian_auth_as_coordinator_vm(account, guardian, context_id, request)
            .await
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
        self.execute_guardian_auth_as_guardian_vm(account, coordinator, context_id, request, proof)
            .await
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
    use aura_core::types::identifiers::AuthorityId;

    #[tokio::test]
    async fn test_auth_service_creation() {
        let authority_id = AuthorityId::new_from_entropy([80u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([82u8; 32]);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let service = AuthServiceApi::new(effects.clone(), authority_context, account_id).unwrap();

        assert!(!service.device_id().0.is_nil());
    }

    #[tokio::test]
    async fn test_authentication_status() {
        let authority_id = AuthorityId::new_from_entropy([83u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([85u8; 32]);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let service = AuthServiceApi::new(effects.clone(), authority_context, account_id).unwrap();

        let error = service
            .authentication_status()
            .await
            .expect_err("authentication status should require authorization");
        assert!(
            error.to_string().contains("Authorization denied"),
            "expected authorization denial, got: {error}"
        );
    }

    #[tokio::test]
    async fn test_challenge_response_flow() {
        let authority_id = AuthorityId::new_from_entropy([86u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([88u8; 32]);

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

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
        let effects = crate::testing::simulation_effect_system_arc(&config);

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
