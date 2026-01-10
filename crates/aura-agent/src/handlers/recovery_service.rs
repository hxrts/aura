//! Recovery Service - Public API for Recovery Operations
//!
//! Provides a clean public interface for guardian-based recovery operations.
//! Wraps `RecoveryHandler` with ergonomic methods and proper error handling.

use super::recovery::{
    GuardianApproval, RecoveryHandler, RecoveryOperation, RecoveryRequest, RecoveryResult,
    RecoveryState,
};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::choreography_adapter::{AuraProtocolAdapter, ReceivedMessage};
use crate::runtime::services::ceremony_runner::{
    CeremonyCommitMetadata, CeremonyInitRequest, CeremonyRunner,
};
use crate::runtime::services::CeremonyTracker;
use crate::runtime::AuraEffectSystem;
use aura_core::crypto::Ed25519Signature;
use aura_core::effects::{CryptoCoreEffects, PhysicalTimeEffects, RandomCoreEffects};
use aura_core::hash::hash;
use aura_core::identifiers::CeremonyId;
use aura_core::identifiers::{AuthorityId, RecoveryId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::util::serialization::from_slice;
use aura_core::TimeEffects;
use aura_protocol::effects::TreeEffects;
use aura_recovery::ceremony_runners::{
    execute_as as guardian_execute_as, AbortCeremony, CommitCeremony, GuardianCeremonyRole,
    ProposeRotation,
};
// Note: RespondCeremony is a received message type (Guardian -> Initiator) so we don't need
// to construct it - we only match on the type name suffix when processing received messages.
use aura_recovery::guardian_ceremony::{
    CeremonyAbort, CeremonyCommit, CeremonyProposal, CeremonyResponse, CeremonyResponseMsg,
    GuardianRotationOp,
};
use aura_recovery::guardian_membership::{
    ChangeCompletion, GuardianVote, MembershipChange, MembershipProposal,
};
use aura_recovery::guardian_setup::{GuardianAcceptance, GuardianInvitation, SetupCompletion};
use aura_recovery::membership_runners::{
    execute_as as membership_execute_as, GuardianMembershipChangeRole,
};
use aura_recovery::recovery_protocol::{
    GuardianApproval as ProtocolGuardianApproval, RecoveryOperation as ProtocolRecoveryOperation,
    RecoveryOutcome, RecoveryRequest as ProtocolRecoveryRequest,
};
use aura_recovery::recovery_runners::{execute_as as recovery_execute_as, RecoveryProtocolRole};
use aura_recovery::setup_runners::{execute_as as setup_execute_as, GuardianSetupRole};
use aura_recovery::types::{GuardianProfile, GuardianSet};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use uuid::Uuid;

/// Recovery service API
///
/// Provides recovery operations through a clean public API.
#[derive(Clone)]
pub struct RecoveryServiceApi {
    handler: RecoveryHandler,
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: CeremonyRunner,
}

impl std::fmt::Debug for RecoveryServiceApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryServiceApi").finish_non_exhaustive()
    }
}

impl RecoveryServiceApi {
    /// Create a new recovery service
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
    ) -> AgentResult<Self> {
        let handler = RecoveryHandler::new(authority_context)?;
        let ceremony_runner = CeremonyRunner::new(CeremonyTracker::new());
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
        })
    }

    /// Create a new recovery service with a shared ceremony runner.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
    ) -> AgentResult<Self> {
        let handler = RecoveryHandler::new(authority_context)?;
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
        })
    }

    async fn register_recovery_ceremony(
        &self,
        request: &RecoveryRequest,
    ) -> AgentResult<CeremonyId> {
        let ceremony_id = CeremonyId::new(request.recovery_id.to_string());
        let total_n = u16::try_from(request.guardians.len()).map_err(|_| {
            AgentError::config("Recovery guardian set exceeds supported size".to_string())
        })?;
        let threshold_k = u16::try_from(request.threshold).map_err(|_| {
            AgentError::config("Recovery threshold exceeds supported size".to_string())
        })?;
        let participants = request
            .guardians
            .iter()
            .copied()
            .map(aura_core::threshold::ParticipantIdentity::guardian)
            .collect::<Vec<_>>();

        let tree_state = self
            .effects
            .get_current_state()
            .await
            .map_err(|e| AgentError::effects(format!("Failed to read tree state: {e}")))?;

        let now_ms = self.effects.current_timestamp_ms().await;
        let prestate_hash = Some(aura_core::Hash32(tree_state.root_commitment));

        for old_id in self
            .ceremony_runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::Recovery,
                prestate_hash.as_ref(),
            )
            .await
        {
            let _ = self
                .ceremony_runner
                .supersede(
                    &old_id,
                    &ceremony_id,
                    aura_core::ceremony::SupersessionReason::NewerRequest,
                    now_ms,
                )
                .await;
        }

        self.ceremony_runner
            .start(CeremonyInitRequest {
                ceremony_id: ceremony_id.clone(),
                kind: aura_app::runtime_bridge::CeremonyKind::Recovery,
                initiator_id: request.account_authority,
                threshold_k,
                total_n,
                participants,
                new_epoch: tree_state.epoch.value(),
                enrollment_device_id: None,
                enrollment_nickname_suggestion: None,
                prestate_hash,
            })
            .await
            .map_err(|e| AgentError::internal(format!("Failed to register ceremony: {e}")))?;

        Ok(ceremony_id)
    }

    /// Initiate a recovery ceremony to add a new device
    ///
    /// # Arguments
    /// * `device_public_key` - Public key of the new device
    /// * `guardians` - Guardian authorities to request approval from
    /// * `threshold` - Required number of approvals
    /// * `justification` - Reason for recovery
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The recovery request details
    pub async fn add_device(
        &self,
        device_public_key: Vec<u8>,
        guardians: Vec<AuthorityId>,
        threshold: u32,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        let request = self
            .handler
            .initiate(
                &self.effects,
                RecoveryOperation::AddDevice { device_public_key },
                guardians,
                threshold,
                justification,
                expires_in_ms,
            )
            .await?;
        let _ = self.register_recovery_ceremony(&request).await?;
        self.spawn_recovery_protocol(&request).await?;
        Ok(request)
    }

    /// Initiate a recovery ceremony to remove a compromised device
    ///
    /// # Arguments
    /// * `leaf_index` - Leaf index of device to remove
    /// * `guardians` - Guardian authorities to request approval from
    /// * `threshold` - Required number of approvals
    /// * `justification` - Reason for recovery
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The recovery request details
    pub async fn remove_device(
        &self,
        leaf_index: u32,
        guardians: Vec<AuthorityId>,
        threshold: u32,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        let request = self
            .handler
            .initiate(
                &self.effects,
                RecoveryOperation::RemoveDevice { leaf_index },
                guardians,
                threshold,
                justification,
                expires_in_ms,
            )
            .await?;
        let _ = self.register_recovery_ceremony(&request).await?;
        self.spawn_recovery_protocol(&request).await?;
        Ok(request)
    }

    /// Initiate a full tree replacement recovery
    ///
    /// # Arguments
    /// * `new_public_key` - New public key for the recovered tree
    /// * `guardians` - Guardian authorities to request approval from
    /// * `threshold` - Required number of approvals
    /// * `justification` - Reason for recovery
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The recovery request details
    pub async fn replace_tree(
        &self,
        new_public_key: Vec<u8>,
        guardians: Vec<AuthorityId>,
        threshold: u32,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        let request = self
            .handler
            .initiate(
                &self.effects,
                RecoveryOperation::ReplaceTree { new_public_key },
                guardians,
                threshold,
                justification,
                expires_in_ms,
            )
            .await?;
        let _ = self.register_recovery_ceremony(&request).await?;
        self.spawn_recovery_protocol(&request).await?;
        Ok(request)
    }

    /// Initiate a guardian set update
    ///
    /// # Arguments
    /// * `new_guardians` - New guardian authorities
    /// * `new_threshold` - New threshold
    /// * `guardians` - Current guardian authorities to request approval from
    /// * `threshold` - Required number of approvals from current guardians
    /// * `justification` - Reason for update
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The recovery request details
    pub async fn update_guardians(
        &self,
        new_guardians: Vec<AuthorityId>,
        new_threshold: u32,
        guardians: Vec<AuthorityId>,
        threshold: u32,
        justification: String,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<RecoveryRequest> {
        let request = self
            .handler
            .initiate(
                &self.effects,
                RecoveryOperation::UpdateGuardians {
                    new_guardians,
                    new_threshold,
                },
                guardians,
                threshold,
                justification,
                expires_in_ms,
            )
            .await?;
        let _ = self.register_recovery_ceremony(&request).await?;
        self.spawn_recovery_protocol(&request).await?;
        Ok(request)
    }

    /// Submit a guardian approval for an active recovery
    ///
    /// # Arguments
    /// * `approval` - The guardian approval
    ///
    /// # Returns
    /// The updated recovery state
    pub async fn submit_approval(&self, approval: GuardianApproval) -> AgentResult<RecoveryState> {
        let state = self
            .handler
            .submit_approval(&self.effects, approval.clone())
            .await?;
        let ceremony_id = CeremonyId::new(approval.recovery_id.to_string());
        let _ = self
            .ceremony_runner
            .record_response(
                &ceremony_id,
                aura_core::threshold::ParticipantIdentity::guardian(approval.guardian_id),
            )
            .await
            .map_err(|e| {
                AgentError::internal(format!("Failed to record recovery approval: {e}"))
            })?;
        if let Some(recovery) = self.handler.get_recovery(&approval.recovery_id).await {
            let _ = self
                .execute_recovery_protocol_guardian(&approval, recovery.request.account_authority)
                .await;
        }
        Ok(state)
    }

    /// Complete a recovery ceremony
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery to complete
    ///
    /// # Returns
    /// The recovery result
    pub async fn complete(&self, recovery_id: &RecoveryId) -> AgentResult<RecoveryResult> {
        let result = self.handler.complete(&self.effects, recovery_id).await?;
        let ceremony_id = CeremonyId::new(recovery_id.to_string());
        let committed_at = self.effects.physical_time().await.ok();
        let _ = self
            .ceremony_runner
            .commit(
                &ceremony_id,
                CeremonyCommitMetadata {
                    committed_at,
                    consensus_id: None,
                },
            )
            .await;
        Ok(result)
    }

    /// Cancel a recovery ceremony
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery to cancel
    /// * `reason` - Reason for cancellation
    ///
    /// # Returns
    /// The recovery result
    pub async fn cancel(
        &self,
        recovery_id: &RecoveryId,
        reason: String,
    ) -> AgentResult<RecoveryResult> {
        let result = self
            .handler
            .cancel(&self.effects, recovery_id, reason.clone())
            .await?;
        let ceremony_id = CeremonyId::new(recovery_id.to_string());
        let _ = self.ceremony_runner.abort(&ceremony_id, Some(reason)).await;
        Ok(result)
    }

    /// Get the state of a recovery ceremony
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery
    ///
    /// # Returns
    /// The recovery state if found
    pub async fn get_state(&self, recovery_id: &RecoveryId) -> Option<RecoveryState> {
        self.handler.get_state(recovery_id).await
    }

    /// List all active recovery ceremonies
    ///
    /// # Returns
    /// List of (recovery_id, state) pairs
    pub async fn list_active(&self) -> Vec<(RecoveryId, RecoveryState)> {
        self.handler.list_active().await
    }

    /// Check if a recovery is pending (initiated but not complete)
    ///
    /// # Arguments
    /// * `recovery_id` - ID of the recovery
    ///
    /// # Returns
    /// True if the recovery is in Initiated or CollectingShares state
    pub async fn is_pending(&self, recovery_id: &RecoveryId) -> bool {
        matches!(
            self.handler.get_state(recovery_id).await,
            Some(RecoveryState::Initiated { .. })
                | Some(RecoveryState::CollectingShares { .. })
                | Some(RecoveryState::Reconstructing { .. })
        )
    }

    async fn spawn_recovery_protocol(&self, request: &RecoveryRequest) -> AgentResult<()> {
        let protocol_request = self.map_recovery_request(request).await?;
        let guardians = request.guardians.clone();
        let effects = self.effects.clone();
        let authority_id = self.handler.authority_context().authority_id();

        for guardian_id in guardians {
            let protocol_request = protocol_request.clone();
            let effects = effects.clone();
            tokio::spawn(async move {
                let account_fut = execute_recovery_protocol_account(
                    effects.clone(),
                    authority_id,
                    guardian_id,
                    protocol_request.clone(),
                );
                let coordinator_fut = execute_recovery_protocol_coordinator(
                    effects.clone(),
                    authority_id,
                    guardian_id,
                    protocol_request.clone(),
                );
                let _ = tokio::join!(account_fut, coordinator_fut);
            });
        }

        Ok(())
    }

    async fn map_recovery_request(
        &self,
        request: &RecoveryRequest,
    ) -> AgentResult<ProtocolRecoveryRequest> {
        use crate::core::AgentError;
        use aura_core::tree::LeafPublicKey;
        use std::collections::BTreeMap;

        let new_tree_commitment = self
            .effects
            .get_current_commitment()
            .await
            .map_err(|e| AgentError::effects(format!("get tree commitment: {e}")))?;

        let operation = match &request.operation {
            RecoveryOperation::ReplaceTree { new_public_key } => {
                let pkg = aura_core::frost::PublicKeyPackage::new(
                    new_public_key.clone(),
                    BTreeMap::new(),
                    1,
                    1,
                );
                ProtocolRecoveryOperation::ReplaceTree {
                    new_public_key: pkg,
                }
            }
            RecoveryOperation::AddDevice { device_public_key } => {
                let key = LeafPublicKey::try_from(device_public_key.clone())
                    .map_err(|e| AgentError::invalid(format!("Invalid device public key: {e}")))?;
                ProtocolRecoveryOperation::AddDevice {
                    device_public_key: key,
                }
            }
            RecoveryOperation::RemoveDevice { leaf_index } => {
                ProtocolRecoveryOperation::RemoveDevice {
                    leaf_index: *leaf_index,
                }
            }
            RecoveryOperation::UpdateGuardians {
                new_guardians,
                new_threshold,
            } => ProtocolRecoveryOperation::UpdateGuardians {
                new_guardians: new_guardians.clone(),
                new_threshold: *new_threshold,
            },
        };

        Ok(ProtocolRecoveryRequest {
            recovery_id: request.recovery_id.clone(),
            account_authority: request.account_authority,
            new_tree_commitment,
            operation,
            justification: request.justification.clone(),
        })
    }

    async fn execute_recovery_protocol_guardian(
        &self,
        approval: &GuardianApproval,
        account_authority: AuthorityId,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let signature = Ed25519Signature::try_from(approval.signature.clone())
            .map_err(|e| AgentError::invalid(format!("Invalid guardian signature: {e}")))?;

        let protocol_approval = ProtocolGuardianApproval {
            guardian_id: approval.guardian_id,
            recovery_id: approval.recovery_id.clone(),
            signature,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: approval.approved_at,
                uncertainty: None,
            }),
        };

        let mut role_map = HashMap::new();
        role_map.insert(RecoveryProtocolRole::Account, account_authority);
        role_map.insert(RecoveryProtocolRole::Coordinator, account_authority);
        role_map.insert(RecoveryProtocolRole::Guardian, approval.guardian_id);

        let approval_type = std::any::type_name::<ProtocolGuardianApproval>();

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            approval.guardian_id,
            RecoveryProtocolRole::Guardian,
            role_map,
        )
        .with_message_provider(move |request_ctx, _received| {
            if request_ctx.type_name == approval_type {
                return Some(Box::new(protocol_approval.clone()));
            }
            None
        });

        let session_id = recovery_session_id(&approval.recovery_id, &approval.guardian_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("recovery guardian start failed: {e}")))?;

        let result = recovery_execute_as(RecoveryProtocolRole::Guardian, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("recovery guardian failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    /// Prepare guardian ceremony by generating FROST threshold keys
    ///
    /// This method generates new threshold keys for the guardian configuration
    /// at a new epoch. The keys are stored but not activated until the ceremony
    /// completes successfully.
    ///
    /// # Full Ceremony Flow
    ///
    /// For complete ceremony orchestration, use `RuntimeBridge.initiate_guardian_ceremony()`
    /// via the AppCore workflow layer. That method:
    /// 1. Calls this method to generate keys
    /// 2. Creates a ceremony ID and registers with CeremonyTracker
    /// 3. Sends guardian invitations via `send_guardian_invitation`
    /// 4. Tracks responses and commits/rollbacks the key rotation
    ///
    /// This method is exposed for advanced use cases where you need direct
    /// control over key generation separate from the invitation flow.
    ///
    /// # Arguments
    /// * `threshold_k` - Minimum number of guardians required (k-of-n)
    /// * `guardian_ids` - List of guardian authority IDs
    ///
    /// # Returns
    /// A tuple of (new_epoch, key_packages, public_key_package) on success.
    /// - `new_epoch`: The epoch for the new keys (call commit/rollback with this)
    /// - `key_packages`: Encrypted key packages for each guardian
    /// - `public_key_package`: The group public key for the new configuration
    pub async fn prepare_guardian_keys(
        &self,
        threshold_k: u16,
        guardian_ids: Vec<AuthorityId>,
    ) -> AgentResult<(u64, Vec<Vec<u8>>, Vec<u8>)> {
        use crate::core::AgentError;
        use aura_core::effects::ThresholdSigningEffects;

        let total_n = guardian_ids.len() as u16;

        if threshold_k == 0 {
            return Err(AgentError::invalid(
                "Threshold must be at least 1".to_string(),
            ));
        }

        if total_n < threshold_k {
            return Err(AgentError::invalid(format!(
                "Need at least {} guardians for {}-of-{} threshold",
                threshold_k, threshold_k, total_n
            )));
        }

        // Get effect system read lock
        let authority_id = self.handler.authority_context().authority_id();

        let participants: Vec<aura_core::threshold::ParticipantIdentity> = guardian_ids
            .iter()
            .copied()
            .map(aura_core::threshold::ParticipantIdentity::guardian)
            .collect();

        // Generate new threshold keys
        let (new_epoch, key_packages, public_key) = self
            .effects
            .rotate_keys(&authority_id, threshold_k, total_n, &participants)
            .await
            .map_err(|e| {
                AgentError::internal(format!("Failed to generate threshold keys: {}", e))
            })?;

        tracing::info!(
            authority_id = %authority_id,
            new_epoch,
            threshold_k,
            total_n,
            num_key_packages = key_packages.len(),
            public_key_size = public_key.len(),
            "Generated new guardian threshold keys"
        );

        Ok((new_epoch, key_packages, public_key))
    }

    /// Commit a guardian key rotation after successful ceremony
    ///
    /// Call this after all guardians have accepted and stored their key shares.
    /// This makes the new epoch authoritative.
    pub async fn commit_guardian_keys(&self, new_epoch: u64) -> AgentResult<()> {
        use crate::core::{default_context_id_for_authority, AgentError};
        use aura_core::effects::ThresholdSigningEffects;
        use aura_core::threshold::{policy_for, CeremonyFlow, KeyGenerationPolicy};

        let authority_id = self.handler.authority_context().authority_id();
        let policy = policy_for(CeremonyFlow::GuardianSetupRotation);
        if policy.keygen == KeyGenerationPolicy::K3ConsensusDkg {
            let context_id = default_context_id_for_authority(authority_id);
            let has_commit = self
                .effects
                .has_dkg_transcript_commit(authority_id, context_id, new_epoch)
                .await
                .map_err(|e| {
                    AgentError::internal(format!("Failed to verify DKG transcript commit: {e}"))
                })?;
            if !has_commit {
                return Err(AgentError::invalid(
                    "Missing consensus DKG transcript".to_string(),
                ));
            }
        }

        self.effects
            .commit_key_rotation(&authority_id, new_epoch)
            .await
            .map_err(|e| AgentError::internal(format!("Failed to commit key rotation: {}", e)))?;

        tracing::info!(
            authority_id = %authority_id,
            epoch = new_epoch,
            "Committed guardian key rotation"
        );

        Ok(())
    }

    /// Rollback a guardian key rotation after ceremony failure
    ///
    /// Call this when the ceremony fails (guardian declined, user cancelled, or timeout).
    /// This discards the new epoch's keys and keeps the previous configuration active.
    pub async fn rollback_guardian_keys(&self, failed_epoch: u64) -> AgentResult<()> {
        use crate::core::AgentError;
        use aura_core::effects::ThresholdSigningEffects;

        let authority_id = self.handler.authority_context().authority_id();

        self.effects
            .rollback_key_rotation(&authority_id, failed_epoch)
            .await
            .map_err(|e| AgentError::internal(format!("Failed to rollback key rotation: {}", e)))?;

        tracing::info!(
            authority_id = %authority_id,
            epoch = failed_epoch,
            "Rolled back guardian key rotation"
        );

        Ok(())
    }

    /// Send a guardian invitation with key package
    ///
    /// This routes the guardian invitation through the proper aura-recovery
    /// protocol. The invitation includes an encrypted key package for the
    /// guardian to store securely.
    ///
    /// # Arguments
    /// * `guardian_id` - Contact ID of the guardian to invite
    /// * `ceremony_id` - Unique ceremony identifier
    /// * `threshold_k` - Minimum signers required
    /// * `total_n` - Total number of guardians
    /// * `key_package` - Encrypted FROST key package for this guardian
    ///
    /// # Returns
    /// Ok if invitation was sent successfully
    ///
    /// # Protocol Flow
    /// 1. Creates CeremonyProposal message with key package
    /// 2. Serializes and wraps in TransportEnvelope
    /// 3. Sends via TransportEffects to guardian's authority
    /// 4. Guardian receives via their effect system
    /// 5. Guardian processes through guard chain + journal
    /// 6. GuardianBinding fact committed when accepted
    ///
    /// # Parameters
    /// * `guardian_id` - The specific guardian to send this invitation to
    /// * `ceremony_id` - The ceremony identifier string
    /// * `threshold_k` - Minimum signers required
    /// * `total_n` - Total number of guardians
    /// * `all_guardian_ids` - All guardian authority IDs participating in the ceremony
    /// * `new_epoch` - The epoch for the new key rotation
    /// * `key_package` - Encrypted key package for this guardian
    pub async fn send_guardian_invitation(
        &self,
        guardian_authority: AuthorityId,
        ceremony_id: aura_recovery::CeremonyId,
        prestate_hash: aura_core::Hash32,
        operation: aura_recovery::GuardianRotationOp,
        key_package: &[u8],
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        use aura_core::effects::TransportEffects;
        use aura_core::ContextId;
        use aura_recovery::guardian_ceremony::CeremonyProposal;

        tracing::info!(
            guardian_id = %guardian_authority,
            ceremony_id = %ceremony_id,
            threshold_k = operation.threshold_k,
            total_n = operation.total_n,
            key_package_size = key_package.len(),
            "Sending guardian invitation through transport"
        );

        // Get our authority context for source
        let initiator_id = self.handler.authority_context().authority_id();

        // Create a context ID for guardian ceremonies
        // Use a deterministic derivation from initiator + ceremony ID
        let context_entropy = {
            let mut h = aura_core::hash::hasher();
            h.update(b"GUARDIAN_CEREMONY_CONTEXT");
            h.update(&initiator_id.to_bytes());
            h.update(&ceremony_id.0 .0);
            h.finalize()
        };
        let ceremony_context = ContextId::new_from_entropy(context_entropy);

        // Best-effort encryption envelope fields (actual key agreement is wired separately).
        let nonce_bytes = self.effects.random_bytes(12).await;
        let mut encryption_nonce = [0u8; 12];
        encryption_nonce.copy_from_slice(&nonce_bytes[..12]);
        let ephemeral_public_key = self.effects.random_bytes(32).await;

        // Create the ceremony proposal
        let proposal = CeremonyProposal {
            ceremony_id,
            initiator_id,
            prestate_hash,
            operation,
            encrypted_key_package: key_package.to_vec(),
            encryption_nonce,
            ephemeral_public_key,
        };

        // Serialize the proposal
        let payload = serde_json::to_vec(&proposal)
            .map_err(|e| AgentError::internal(format!("Failed to serialize proposal: {}", e)))?;

        // Create transport envelope
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-guardian-proposal".to_string(),
        );
        metadata.insert("protocol-version".to_string(), "1".to_string());
        metadata.insert("ceremony-id".to_string(), ceremony_id.to_string());

        let envelope = aura_core::effects::TransportEnvelope {
            destination: guardian_authority,
            source: initiator_id,
            context: ceremony_context,
            payload,
            metadata,
            receipt: None, // Receipts would be added by guard chain in production
        };

        // Send via transport effects
        self.effects
            .send_envelope(envelope)
            .await
            .map_err(|e| AgentError::effects(format!("Failed to send invitation: {}", e)))?;

        tracing::info!(
            guardian_id = %guardian_authority,
            ceremony_id = %ceremony_id,
            "Guardian invitation sent successfully"
        );

        Ok(())
    }

    /// Execute guardian ceremony as initiator using choreographic protocol.
    /// Returns the list of guardians who accepted (for recording in the tracker).
    pub async fn execute_guardian_ceremony_initiator(
        &self,
        ceremony_id: aura_recovery::CeremonyId,
        prestate_hash: aura_core::Hash32,
        operation: GuardianRotationOp,
        guardians: Vec<AuthorityId>,
        key_packages: Vec<Vec<u8>>,
    ) -> AgentResult<Vec<AuthorityId>> {
        use crate::core::AgentError;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        eprintln!(
            "[DEBUG] execute_guardian_ceremony_initiator: guardians={:?}",
            guardians.iter().map(|g| g.to_string()).collect::<Vec<_>>()
        );

        if guardians.len() != key_packages.len() {
            return Err(AgentError::invalid(
                "guardian list and key package length mismatch",
            ));
        }

        let authority_id = self.handler.authority_context().authority_id();

        // The guardian ceremony protocol supports exactly 2 guardians
        if guardians.len() != 2 {
            return Err(AgentError::invalid(format!(
                "Guardian ceremony requires exactly 2 guardians, got {}",
                guardians.len()
            )));
        }

        // Sort guardians for deterministic role assignment. Both initiator and guardians
        // must use the same ordering: sorted_guardians[0] = Guardian1, sorted_guardians[1] = Guardian2.
        // Sort pairs of (guardian, key_package) to keep them matched.
        let mut guardian_packages: Vec<_> = guardians.into_iter().zip(key_packages).collect();
        guardian_packages.sort_by_key(|(g, _)| *g);
        let (sorted_guardians, sorted_key_packages): (Vec<_>, Vec<_>) =
            guardian_packages.into_iter().unzip();

        let mut role_map = HashMap::new();
        role_map.insert(GuardianCeremonyRole::Initiator, authority_id);
        role_map.insert(GuardianCeremonyRole::Guardian1, sorted_guardians[0]);
        role_map.insert(GuardianCeremonyRole::Guardian2, sorted_guardians[1]);

        let mut proposals = VecDeque::new();
        for key_package in sorted_key_packages.iter() {
            let nonce_bytes = self.effects.random_bytes(12).await;
            let mut encryption_nonce = [0u8; 12];
            encryption_nonce.copy_from_slice(&nonce_bytes[..12]);
            let ephemeral_public_key = self.effects.random_bytes(32).await;
            proposals.push_back(CeremonyProposal {
                ceremony_id,
                initiator_id: authority_id,
                prestate_hash,
                operation: operation.clone(),
                encrypted_key_package: key_package.clone(),
                encryption_nonce,
                ephemeral_public_key,
            });
        }

        let threshold_k = operation.threshold_k as usize;
        // Shared state to capture accepted guardians for return
        let accepted_guardians: Arc<RwLock<Vec<AuthorityId>>> = Arc::new(RwLock::new(Vec::new()));
        let accepted_guardians_capture = accepted_guardians.clone();

        // The choreography macro generates wrapper types with these message labels:
        // - ProposeRotation (wraps CeremonyProposal)
        // - RespondCeremony (wraps CeremonyResponseMsg)
        // - CommitCeremony (wraps CeremonyCommit)
        // - AbortCeremony (wraps CeremonyAbort)
        // We match on the suffix since the full path varies.

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            GuardianCeremonyRole::Initiator,
            role_map,
        )
        .with_message_provider(move |request, received| {
            eprintln!(
                "[DEBUG] message_provider called for type: {}, received.len={}",
                request.type_name,
                received.len()
            );
            // Match on message labels (suffixes) since the macro generates wrapper types
            if request.type_name.ends_with("ProposeRotation") {
                return proposals.pop_front().map(|proposal| {
                    Box::new(ProposeRotation(proposal)) as Box<dyn std::any::Any + Send>
                });
            }

            if request.type_name.ends_with("CommitCeremony") {
                let mut accepted = Vec::new();
                for msg in received {
                    if msg.type_name.ends_with("RespondCeremony") {
                        if let Ok(response) = from_slice::<CeremonyResponseMsg>(&msg.bytes) {
                            if response.response == CeremonyResponse::Accept {
                                accepted.push(response.guardian_id);
                            }
                        }
                    }
                }
                // Capture accepted guardians in shared state
                if let Ok(mut guard) = accepted_guardians_capture.try_write() {
                    *guard = accepted.clone();
                }
                let commit = CeremonyCommit {
                    ceremony_id,
                    new_epoch: operation.new_epoch,
                    threshold_signature: Vec::new(),
                    participants: accepted,
                };
                return Some(Box::new(CommitCeremony(commit)));
            }

            if request.type_name.ends_with("AbortCeremony") {
                let mut declined = false;
                for msg in received {
                    if msg.type_name.ends_with("RespondCeremony") {
                        if let Ok(response) = from_slice::<CeremonyResponseMsg>(&msg.bytes) {
                            if response.response == CeremonyResponse::Decline {
                                declined = true;
                                break;
                            }
                        }
                    }
                }
                let reason = if declined {
                    "guardian_declined"
                } else {
                    "threshold_not_met"
                };
                let abort = CeremonyAbort {
                    ceremony_id,
                    reason: reason.to_string(),
                };
                return Some(Box::new(AbortCeremony(abort)));
            }

            None
        })
        .with_branch_decider(move |received| {
            let mut accepted = 0usize;
            let mut declined = 0usize;
            for msg in received {
                if msg.type_name.ends_with("RespondCeremony") {
                    if let Ok(response) = from_slice::<CeremonyResponseMsg>(&msg.bytes) {
                        match response.response {
                            CeremonyResponse::Accept => accepted += 1,
                            CeremonyResponse::Decline => declined += 1,
                            CeremonyResponse::Pending => {}
                        }
                    }
                }
            }
            if declined > 0 {
                Some("cancel".to_string())
            } else if accepted >= threshold_k {
                Some("finalize".to_string())
            } else {
                Some("cancel".to_string())
            }
        });

        let session_id = Self::ceremony_session_id(ceremony_id);
        eprintln!("[DEBUG] starting session with session_id={}", session_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("guardian ceremony start failed: {e}")))?;
        eprintln!("[DEBUG] session started, executing guardian_execute_as...");

        guardian_execute_as(GuardianCeremonyRole::Initiator, &mut adapter)
            .await
            .map_err(|e| {
                let error_str = e.to_string();
                eprintln!("[DEBUG] guardian_execute_as failed with: {}", error_str);
                // Detect peer connectivity issues and provide actionable error message
                if error_str.contains("message provider returned None")
                    || error_str.contains("Protocol violation")
                    || error_str.contains("Transport error")
                    || error_str.contains("No message available")
                {
                    AgentError::internal(
                        "guardian ceremony failed: no responses received from guardians. \
                         Ensure guardian peers are online and connected."
                            .to_string(),
                    )
                } else {
                    AgentError::internal(format!("guardian ceremony failed: {e}"))
                }
            })?;

        let _ = adapter.end_session().await;

        // Return the captured accepted guardians
        let accepted = accepted_guardians.read().await.clone();
        Ok(accepted)
    }

    /// Execute guardian ceremony as a guardian (accept/decline).
    ///
    /// The `role_index` parameter specifies which guardian role this peer plays:
    /// - 0 = Guardian1
    /// - 1 = Guardian2
    pub async fn execute_guardian_ceremony_guardian(
        &self,
        initiator_id: AuthorityId,
        ceremony_id: aura_recovery::CeremonyId,
        response: CeremonyResponse,
        role_index: usize,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.authority_context().authority_id();

        // Determine which guardian role this peer plays
        let guardian_role = match role_index {
            0 => GuardianCeremonyRole::Guardian1,
            1 => GuardianCeremonyRole::Guardian2,
            _ => {
                return Err(AgentError::invalid(format!(
                    "Invalid guardian role index: {} (must be 0 or 1)",
                    role_index
                )))
            }
        };

        let mut role_map = HashMap::new();
        role_map.insert(GuardianCeremonyRole::Initiator, initiator_id);
        role_map.insert(guardian_role, authority_id);

        let response_type = std::any::type_name::<CeremonyResponseMsg>();
        let response_msg = CeremonyResponseMsg {
            ceremony_id,
            guardian_id: authority_id,
            response,
            signature: Vec::new(),
        };

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            guardian_role,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if request.type_name == response_type {
                return Some(Box::new(response_msg.clone()));
            }
            None
        });

        let session_id = Self::ceremony_session_id(ceremony_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("guardian ceremony start failed: {e}")))?;

        let result = guardian_execute_as(guardian_role, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("guardian ceremony failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    /// Execute guardian setup ceremony as initiator using choreographic protocol.
    pub async fn execute_guardian_setup_initiator(
        &self,
        account_id: AuthorityId,
        guardians: Vec<AuthorityId>,
        threshold: u16,
    ) -> AgentResult<(String, SetupCompletion)> {
        let setup_id = self.build_guardian_setup_id(account_id).await?;
        let completion = self
            .execute_guardian_setup_initiator_with_id(&setup_id, account_id, guardians, threshold)
            .await?;
        Ok((setup_id, completion))
    }

    /// Execute guardian setup ceremony with a known setup id.
    pub async fn execute_guardian_setup_initiator_with_id(
        &self,
        setup_id: &str,
        account_id: AuthorityId,
        guardians: Vec<AuthorityId>,
        threshold: u16,
    ) -> AgentResult<SetupCompletion> {
        use crate::core::AgentError;

        validate_guardian_setup_inputs(&guardians, threshold)?;

        let authority_id = self.handler.authority_context().authority_id();
        let timestamp = self.guardian_setup_timestamp().await?;

        let mut invitations = VecDeque::new();
        for _ in 0..guardians.len() {
            invitations.push_back(GuardianInvitation {
                setup_id: setup_id.to_string(),
                account_id,
                target_guardians: guardians.clone(),
                threshold,
                timestamp: timestamp.clone(),
            });
        }

        let mut role_map = HashMap::new();
        role_map.insert(GuardianSetupRole::SetupInitiator, authority_id);
        role_map.insert(GuardianSetupRole::Guardian1, guardians[0]);
        role_map.insert(GuardianSetupRole::Guardian2, guardians[1]);
        role_map.insert(GuardianSetupRole::Guardian3, guardians[2]);

        let invitation_type = std::any::type_name::<GuardianInvitation>();
        let completion_type = std::any::type_name::<SetupCompletion>();
        let acceptance_type = std::any::type_name::<GuardianAcceptance>();

        let setup_id_owned = setup_id.to_string();
        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            GuardianSetupRole::SetupInitiator,
            role_map,
        )
        .with_message_provider(move |request, received| {
            if request.type_name == invitation_type {
                return invitations
                    .pop_front()
                    .map(|inv| Box::new(inv) as Box<dyn std::any::Any + Send>);
            }

            if request.type_name == completion_type {
                let acceptances = collect_guardian_acceptances(received, acceptance_type);
                let completion =
                    build_guardian_setup_completion(&setup_id_owned, threshold, acceptances);
                return Some(Box::new(completion));
            }

            None
        });

        let session_id = guardian_setup_session_id(setup_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("guardian setup start failed: {e}")))?;

        let result = setup_execute_as(GuardianSetupRole::SetupInitiator, &mut adapter)
            .await
            .map_err(|e| {
                let error_str = e.to_string();
                // Detect peer connectivity issues and provide actionable error message
                if error_str.contains("message provider returned None")
                    || error_str.contains("Protocol violation")
                {
                    AgentError::internal(
                        "guardian setup failed: no responses received from guardians. \
                         Ensure guardian peers are online and connected."
                            .to_string(),
                    )
                } else {
                    AgentError::internal(format!("guardian setup failed: {e}"))
                }
            });

        let acceptances =
            collect_guardian_acceptances(adapter.received_messages(), acceptance_type);
        let completion = build_guardian_setup_completion(setup_id, threshold, acceptances);

        let _ = adapter.end_session().await;
        result.map(|_| completion)
    }

    /// Execute guardian setup ceremony as a guardian (accept/decline).
    pub async fn execute_guardian_setup_guardian(
        &self,
        invitation: GuardianInvitation,
        accepted: bool,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        validate_guardian_setup_inputs(&invitation.target_guardians, invitation.threshold)?;

        let authority_id = self.handler.authority_context().authority_id();
        let guardian_index = invitation
            .target_guardians
            .iter()
            .position(|id| *id == authority_id)
            .ok_or_else(|| {
                AgentError::invalid("Guardian not listed in setup invitation".to_string())
            })?;

        let guardian_role = match guardian_index {
            0 => GuardianSetupRole::Guardian1,
            1 => GuardianSetupRole::Guardian2,
            2 => GuardianSetupRole::Guardian3,
            _ => {
                return Err(AgentError::invalid(
                    "Guardian setup requires exactly three guardians".to_string(),
                ))
            }
        };

        let mut role_map = HashMap::new();
        role_map.insert(GuardianSetupRole::SetupInitiator, invitation.account_id);
        role_map.insert(GuardianSetupRole::Guardian1, invitation.target_guardians[0]);
        role_map.insert(GuardianSetupRole::Guardian2, invitation.target_guardians[1]);
        role_map.insert(GuardianSetupRole::Guardian3, invitation.target_guardians[2]);

        let acceptance_type = std::any::type_name::<GuardianAcceptance>();
        let setup_id = invitation.setup_id.clone();
        let timestamp = self.guardian_setup_timestamp().await?;
        let (_, public_key) = self
            .effects
            .ed25519_generate_keypair()
            .await
            .map_err(|e| AgentError::internal(format!("guardian keygen failed: {e}")))?;
        let acceptance = GuardianAcceptance {
            guardian_id: authority_id,
            setup_id: setup_id.clone(),
            accepted,
            public_key,
            timestamp,
        };

        let mut adapter =
            AuraProtocolAdapter::new(self.effects.clone(), authority_id, guardian_role, role_map)
                .with_message_provider(move |request, _received| {
                    if request.type_name == acceptance_type {
                        return Some(Box::new(acceptance.clone()));
                    }
                    None
                });

        let session_id = guardian_setup_session_id(&setup_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("guardian setup start failed: {e}")))?;

        let result = setup_execute_as(guardian_role, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("guardian setup failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    fn ceremony_session_id(ceremony_id: aura_recovery::CeremonyId) -> Uuid {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&ceremony_id.0 .0[..16]);
        Uuid::from_bytes(bytes)
    }

    async fn build_guardian_setup_id(&self, account_id: AuthorityId) -> AgentResult<String> {
        let now_ms = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or_default();
        Ok(format!("setup_{}_{}", account_id, now_ms))
    }

    async fn guardian_setup_timestamp(&self) -> AgentResult<TimeStamp> {
        let now_ms = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or_default();
        Ok(TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: now_ms,
            uncertainty: None,
        }))
    }

    // =========================================================================
    // Guardian Membership Change Methods
    // =========================================================================

    /// Initiate a guardian membership change ceremony.
    ///
    /// Executes the GuardianMembershipChange choreography as the ChangeInitiator role.
    /// This is a 3-phase protocol:
    /// 1. ProposeChange: ChangeInitiator → Guardian1/2/3
    /// 2. CastVote: Guardian1/2/3 → ChangeInitiator
    /// 3. CompleteChange: ChangeInitiator → Guardian1/2/3
    ///
    /// # Arguments
    /// * `change` - The membership change to propose (AddGuardian, RemoveGuardian, UpdateGuardian)
    /// * `guardians` - Current guardian authorities (exactly 3 required for choreography)
    /// * `threshold` - Required number of approvals
    /// * `new_threshold` - Optional new threshold after the change
    ///
    /// # Returns
    /// The change completion result with the new guardian set
    pub async fn initiate_membership_change(
        &self,
        change: MembershipChange,
        guardians: Vec<AuthorityId>,
        threshold: u32,
        new_threshold: Option<u16>,
    ) -> AgentResult<ChangeCompletion> {
        use crate::core::AgentError;

        if guardians.len() != 3 {
            return Err(AgentError::invalid(
                "Guardian membership change choreography requires exactly three guardians"
                    .to_string(),
            ));
        }

        let authority_id = self.handler.authority_context().authority_id();
        let now_ms = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or_default();

        let change_id = format!("membership_{}_{}", authority_id, now_ms);

        // Build role mapping: ChangeInitiator + Guardian1/2/3
        let mut role_map = HashMap::new();
        role_map.insert(GuardianMembershipChangeRole::ChangeInitiator, authority_id);
        role_map.insert(GuardianMembershipChangeRole::Guardian1, guardians[0]);
        role_map.insert(GuardianMembershipChangeRole::Guardian2, guardians[1]);
        role_map.insert(GuardianMembershipChangeRole::Guardian3, guardians[2]);

        // Build proposals for each guardian (Phase 1)
        let mut proposals = VecDeque::new();
        for _ in 0..3 {
            proposals.push_back(MembershipProposal {
                change_id: change_id.clone(),
                account_id: authority_id, // Account is the initiator
                proposer_id: authority_id,
                change: change.clone(),
                new_threshold,
                timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                    ts_ms: now_ms,
                    uncertainty: None,
                }),
            });
        }

        // Type names for message provider
        let proposal_type = std::any::type_name::<MembershipProposal>();
        let completion_type = std::any::type_name::<ChangeCompletion>();
        let vote_type = std::any::type_name::<GuardianVote>();

        let threshold_usize = threshold as usize;
        let change_id_owned = change_id.clone();
        let new_threshold_final = new_threshold.unwrap_or(threshold as u16);

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            GuardianMembershipChangeRole::ChangeInitiator,
            role_map,
        )
        .with_message_provider(move |request, received| {
            // Phase 1: Provide proposals to guardians
            if request.type_name == proposal_type {
                return proposals
                    .pop_front()
                    .map(|p| Box::new(p) as Box<dyn std::any::Any + Send>);
            }

            // Phase 3: Build completion based on votes received
            if request.type_name == completion_type {
                let mut accepted_guardians = Vec::new();
                for msg in received {
                    if msg.type_name == vote_type {
                        if let Ok(vote) = from_slice::<GuardianVote>(&msg.bytes) {
                            if vote.approved {
                                accepted_guardians.push(vote.guardian_id);
                            }
                        }
                    }
                }

                let success = accepted_guardians.len() >= threshold_usize;
                let new_guardian_set = GuardianSet::new(
                    accepted_guardians
                        .iter()
                        .copied()
                        .map(GuardianProfile::new)
                        .collect(),
                );

                let completion = ChangeCompletion {
                    change_id: change_id_owned.clone(),
                    success,
                    new_guardian_set,
                    new_threshold: new_threshold_final,
                    change_evidence: Vec::new(),
                };
                return Some(Box::new(completion));
            }

            None
        });

        let session_id = membership_session_id(&change_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("membership change start failed: {e}")))?;

        let result =
            membership_execute_as(GuardianMembershipChangeRole::ChangeInitiator, &mut adapter)
                .await
                .map_err(|e| AgentError::internal(format!("membership change failed: {e}")));

        // Extract the completion from received messages
        let completion = {
            let vote_type_str = std::any::type_name::<GuardianVote>();
            let mut accepted_guardians = Vec::new();
            for msg in adapter.received_messages() {
                if msg.type_name == vote_type_str {
                    if let Ok(vote) = from_slice::<GuardianVote>(&msg.bytes) {
                        if vote.approved {
                            accepted_guardians.push(vote.guardian_id);
                        }
                    }
                }
            }

            ChangeCompletion {
                change_id,
                success: accepted_guardians.len() >= threshold_usize,
                new_guardian_set: GuardianSet::new(
                    accepted_guardians
                        .into_iter()
                        .map(GuardianProfile::new)
                        .collect(),
                ),
                new_threshold: new_threshold_final,
                change_evidence: Vec::new(),
            }
        };

        let _ = adapter.end_session().await;
        result.map(|_| completion)
    }

    /// Vote on a guardian membership change as a guardian.
    ///
    /// Executes the GuardianMembershipChange choreography as a Guardian role.
    ///
    /// # Arguments
    /// * `proposal` - The membership proposal to vote on
    /// * `initiator_id` - Authority of the change initiator
    /// * `guardian_index` - Index of this guardian (0, 1, or 2)
    /// * `approved` - Whether to approve the change
    /// * `rationale` - Reason for the vote
    ///
    /// # Returns
    /// The guardian's vote
    pub async fn vote_membership_change(
        &self,
        proposal: MembershipProposal,
        initiator_id: AuthorityId,
        guardian_index: usize,
        approved: bool,
        rationale: String,
    ) -> AgentResult<GuardianVote> {
        use crate::core::AgentError;

        let authority_id = self.handler.authority_context().authority_id();

        let guardian_role = match guardian_index {
            0 => GuardianMembershipChangeRole::Guardian1,
            1 => GuardianMembershipChangeRole::Guardian2,
            2 => GuardianMembershipChangeRole::Guardian3,
            _ => {
                return Err(AgentError::invalid(
                    "Guardian index must be 0, 1, or 2".to_string(),
                ))
            }
        };

        let now_ms = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or_default();

        // Create vote signature
        let mut sig_input = Vec::new();
        sig_input.extend_from_slice(&authority_id.to_bytes());
        sig_input.extend_from_slice(proposal.change_id.as_bytes());
        sig_input.push(approved as u8);
        let vote_signature = hash(&sig_input).to_vec();

        let vote = GuardianVote {
            change_id: proposal.change_id.clone(),
            guardian_id: authority_id,
            approved,
            vote_signature,
            rationale,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            }),
        };

        // Build role mapping
        let mut role_map = HashMap::new();
        role_map.insert(GuardianMembershipChangeRole::ChangeInitiator, initiator_id);
        // We only know the initiator; other guardians are resolved by transport

        let vote_type = std::any::type_name::<GuardianVote>();
        let vote_clone = vote.clone();

        let mut adapter =
            AuraProtocolAdapter::new(self.effects.clone(), authority_id, guardian_role, role_map)
                .with_message_provider(move |request, _received| {
                    if request.type_name == vote_type {
                        return Some(Box::new(vote_clone.clone()));
                    }
                    None
                });

        let session_id = membership_session_id(&proposal.change_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("membership vote start failed: {e}")))?;

        let result = membership_execute_as(guardian_role, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("membership vote failed: {e}")));

        let _ = adapter.end_session().await;
        result.map(|_| vote)
    }

    /// Process incoming guardian acceptance responses from transport
    ///
    /// This method should be called periodically to check for acceptance messages
    /// from guardians. The caller should update the ceremony tracker with the results.
    ///
    /// # Returns
    /// List of (ceremony_id, guardian_id) pairs for accepted guardians
    pub async fn process_guardian_acceptances(&self) -> AgentResult<Vec<(String, String)>> {
        use aura_core::effects::TransportEffects;

        let mut acceptances = Vec::new();

        // Poll for incoming acceptance messages
        loop {
            match self.effects.receive_envelope().await {
                Ok(envelope) => {
                    // Check if this is a guardian acceptance response
                    if envelope.metadata.get("content-type")
                        == Some(&"application/aura-guardian-acceptance".to_string())
                    {
                        if let (Some(ceremony_id), Some(guardian_id)) = (
                            envelope.metadata.get("ceremony-id"),
                            envelope.metadata.get("guardian-id"),
                        ) {
                            tracing::info!(
                                ceremony_id = %ceremony_id,
                                guardian_id = %guardian_id,
                                "Received guardian acceptance"
                            );

                            acceptances.push((ceremony_id.clone(), guardian_id.clone()));
                        }
                    }
                }
                Err(aura_core::effects::TransportError::NoMessage) => {
                    // No more messages
                    break;
                }
                Err(e) => {
                    tracing::warn!("Error receiving acceptance response: {}", e);
                    break;
                }
            }
        }

        Ok(acceptances)
    }
}

async fn execute_recovery_protocol_account(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    use crate::core::AgentError;

    let mut role_map = HashMap::new();
    role_map.insert(RecoveryProtocolRole::Account, authority_id);
    role_map.insert(RecoveryProtocolRole::Coordinator, authority_id);
    role_map.insert(RecoveryProtocolRole::Guardian, guardian_id);

    let request_type = std::any::type_name::<ProtocolRecoveryRequest>();

    let session_id = recovery_session_id(&request.recovery_id, &guardian_id);
    let request_clone = request.clone();
    let mut adapter = AuraProtocolAdapter::new(
        effects.clone(),
        authority_id,
        RecoveryProtocolRole::Account,
        role_map,
    )
    .with_message_provider(move |request_ctx, _received| {
        if request_ctx.type_name == request_type {
            return Some(Box::new(request_clone.clone()));
        }
        None
    });
    adapter
        .start_session(session_id)
        .await
        .map_err(|e| AgentError::internal(format!("recovery account start failed: {e}")))?;

    let result = recovery_execute_as(RecoveryProtocolRole::Account, &mut adapter)
        .await
        .map_err(|e| AgentError::internal(format!("recovery account failed: {e}")));

    let _ = adapter.end_session().await;
    result
}

async fn execute_recovery_protocol_coordinator(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    use crate::core::AgentError;

    let mut role_map = HashMap::new();
    role_map.insert(RecoveryProtocolRole::Account, authority_id);
    role_map.insert(RecoveryProtocolRole::Coordinator, authority_id);
    role_map.insert(RecoveryProtocolRole::Guardian, guardian_id);

    let request_type = std::any::type_name::<ProtocolRecoveryRequest>();
    let approval_type = std::any::type_name::<ProtocolGuardianApproval>();
    let outcome_type = std::any::type_name::<RecoveryOutcome>();

    let session_id = recovery_session_id(&request.recovery_id, &guardian_id);
    let request_clone = request.clone();
    let mut adapter = AuraProtocolAdapter::new(
        effects.clone(),
        authority_id,
        RecoveryProtocolRole::Coordinator,
        role_map,
    )
    .with_message_provider(move |request_ctx, received| {
        if request_ctx.type_name == request_type {
            return Some(Box::new(request_clone.clone()));
        }

        if request_ctx.type_name == outcome_type {
            let mut approvals = Vec::new();
            for msg in received {
                if msg.type_name == approval_type {
                    if let Ok(approval) = from_slice::<ProtocolGuardianApproval>(&msg.bytes) {
                        approvals.push(approval);
                    }
                }
            }
            let success = !approvals.is_empty();
            let outcome = RecoveryOutcome {
                success,
                recovery_grant: None,
                error: if success {
                    None
                } else {
                    Some("no approvals".to_string())
                },
                approvals,
            };
            return Some(Box::new(outcome));
        }

        None
    });

    adapter
        .start_session(session_id)
        .await
        .map_err(|e| AgentError::internal(format!("recovery coordinator start failed: {e}")))?;

    let result = recovery_execute_as(RecoveryProtocolRole::Coordinator, &mut adapter)
        .await
        .map_err(|e| AgentError::internal(format!("recovery coordinator failed: {e}")));

    let _ = adapter.end_session().await;
    result
}

fn recovery_session_id(recovery_id: &RecoveryId, guardian_id: &AuthorityId) -> Uuid {
    let mut material = Vec::new();
    material.extend_from_slice(recovery_id.as_str().as_bytes());
    material.extend_from_slice(&guardian_id.to_bytes());
    let digest = hash(&material);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

fn guardian_setup_session_id(setup_id: &str) -> Uuid {
    let digest = hash(setup_id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

fn membership_session_id(change_id: &str) -> Uuid {
    let digest = hash(change_id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

fn validate_guardian_setup_inputs(guardians: &[AuthorityId], threshold: u16) -> AgentResult<()> {
    use crate::core::AgentError;

    if guardians.len() != 3 {
        return Err(AgentError::invalid(
            "Guardian setup requires exactly three guardians".to_string(),
        ));
    }

    if threshold == 0 {
        return Err(AgentError::invalid(
            "Guardian setup threshold must be at least 1".to_string(),
        ));
    }

    if threshold as usize > guardians.len() {
        return Err(AgentError::invalid(format!(
            "Guardian setup threshold {} exceeds guardian count {}",
            threshold,
            guardians.len()
        )));
    }

    Ok(())
}

fn collect_guardian_acceptances(
    received: &[ReceivedMessage],
    acceptance_type: &'static str,
) -> Vec<GuardianAcceptance> {
    let mut acceptances = Vec::new();
    for msg in received {
        if msg.type_name == acceptance_type {
            if let Ok(acceptance) = from_slice::<GuardianAcceptance>(&msg.bytes) {
                acceptances.push(acceptance);
            }
        }
    }
    acceptances
}

fn build_guardian_setup_completion(
    setup_id: &str,
    threshold: u16,
    acceptances: Vec<GuardianAcceptance>,
) -> SetupCompletion {
    let accepted_guardians: Vec<AuthorityId> = acceptances
        .iter()
        .filter(|acceptance| acceptance.accepted)
        .map(|acceptance| acceptance.guardian_id)
        .collect();

    let guardian_set = GuardianSet::new(
        accepted_guardians
            .iter()
            .copied()
            .map(GuardianProfile::new)
            .collect(),
    );

    SetupCompletion {
        setup_id: setup_id.to_string(),
        success: accepted_guardians.len() >= threshold as usize,
        guardian_set,
        threshold,
        encrypted_shares: Vec::new(),
        public_key_package: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
    }

    #[tokio::test]
    async fn test_recovery_service_creation() {
        let authority_context = create_test_authority(150);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = RecoveryServiceApi::new(effects, authority_context);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_add_device_recovery() {
        let authority_context = create_test_authority(151);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

        let guardians = vec![
            AuthorityId::new_from_entropy([152u8; 32]),
            AuthorityId::new_from_entropy([153u8; 32]),
        ];

        let request = service
            .add_device(
                vec![0u8; 32],
                guardians,
                2,
                "Adding backup device".to_string(),
                None,
            )
            .await
            .unwrap();

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
        assert_eq!(request.threshold, 2);
    }

    #[tokio::test]
    async fn test_remove_device_recovery() {
        let authority_context = create_test_authority(154);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

        let guardians = vec![AuthorityId::new_from_entropy([155u8; 32])];

        let request = service
            .remove_device(0, guardians, 1, "Device compromised".to_string(), None)
            .await
            .unwrap();

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
    }

    #[tokio::test]
    async fn test_replace_tree_recovery() {
        let authority_context = create_test_authority(156);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

        let guardians = vec![
            AuthorityId::new_from_entropy([157u8; 32]),
            AuthorityId::new_from_entropy([158u8; 32]),
            AuthorityId::new_from_entropy([159u8; 32]),
        ];

        let request = service
            .replace_tree(
                vec![0u8; 32],
                guardians,
                2,
                "Full recovery after device loss".to_string(),
                Some(604800000), // 1 week
            )
            .await
            .unwrap();

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
        assert!(request.expires_at.is_some());
    }

    #[tokio::test]
    async fn test_update_guardians_recovery() {
        let authority_context = create_test_authority(160);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

        let current_guardians = vec![AuthorityId::new_from_entropy([161u8; 32])];
        let new_guardians = vec![
            AuthorityId::new_from_entropy([162u8; 32]),
            AuthorityId::new_from_entropy([163u8; 32]),
        ];

        let request = service
            .update_guardians(
                new_guardians,
                2, // new threshold
                current_guardians,
                1, // current threshold
                "Upgrading guardian set".to_string(),
                None,
            )
            .await
            .unwrap();

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
    }

    #[tokio::test]
    async fn test_full_recovery_flow() {
        let authority_context = create_test_authority(164);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

        let guardians = vec![AuthorityId::new_from_entropy([165u8; 32])];

        // Initiate
        let request = service
            .add_device(
                vec![0u8; 32],
                guardians.clone(),
                1,
                "Test".to_string(),
                None,
            )
            .await
            .unwrap();

        // Check pending
        assert!(service.is_pending(&request.recovery_id).await);

        // Submit approval
        let approval = GuardianApproval {
            recovery_id: request.recovery_id.clone(),
            guardian_id: guardians[0],
            signature: vec![0u8; 64],
            share_data: None,
            approved_at: 12345,
        };
        service.submit_approval(approval).await.unwrap();

        // Complete
        let result = service.complete(&request.recovery_id).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_list_active() {
        let authority_context = create_test_authority(166);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

        // Initially empty
        let active = service.list_active().await;
        assert!(active.is_empty());

        // Create recoveries
        let guardians = vec![
            AuthorityId::new_from_entropy([167u8; 32]),
            AuthorityId::new_from_entropy([168u8; 32]),
        ];

        service
            .add_device(
                vec![0u8; 32],
                guardians.clone(),
                2,
                "Test 1".to_string(),
                None,
            )
            .await
            .unwrap();

        service
            .remove_device(0, guardians, 2, "Test 2".to_string(), None)
            .await
            .unwrap();

        // Should have 2 active
        let active = service.list_active().await;
        assert_eq!(active.len(), 2);
    }

    #[tokio::test]
    async fn test_membership_change_requires_three_guardians() {
        let authority_context = create_test_authority(170);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

        // Two guardians should fail
        let guardians = vec![
            AuthorityId::new_from_entropy([171u8; 32]),
            AuthorityId::new_from_entropy([172u8; 32]),
        ];

        let result = service
            .initiate_membership_change(
                MembershipChange::AddGuardian {
                    guardian: GuardianProfile::new(AuthorityId::new_from_entropy([173u8; 32])),
                },
                guardians,
                2,
                None,
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exactly three guardians"));
    }

    #[tokio::test]
    async fn test_membership_change_invalid_guardian_index() {
        let authority_context = create_test_authority(174);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let service = RecoveryServiceApi::new(effects, authority_context).unwrap();

        let proposal = MembershipProposal {
            change_id: "test-change-123".to_string(),
            account_id: AuthorityId::new_from_entropy([175u8; 32]),
            proposer_id: AuthorityId::new_from_entropy([176u8; 32]),
            change: MembershipChange::AddGuardian {
                guardian: GuardianProfile::new(AuthorityId::new_from_entropy([177u8; 32])),
            },
            new_threshold: None,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        };

        // Invalid guardian index (3 - only 0, 1, 2 valid)
        let result = service
            .vote_membership_change(
                proposal,
                AuthorityId::new_from_entropy([178u8; 32]),
                3, // Invalid
                true,
                "Test vote".to_string(),
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Guardian index must be 0, 1, or 2"));
    }

    #[tokio::test]
    async fn test_membership_session_id_deterministic() {
        let change_id = "test-membership-change-001";
        let session_id1 = membership_session_id(change_id);
        let session_id2 = membership_session_id(change_id);
        assert_eq!(session_id1, session_id2);

        // Different change_id should produce different session
        let session_id3 = membership_session_id("different-change-id");
        assert_ne!(session_id1, session_id3);
    }
}
