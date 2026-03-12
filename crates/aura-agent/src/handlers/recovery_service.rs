//! Recovery Service - Public API for Recovery Operations
//!
//! Provides a clean public interface for guardian-based recovery operations.
//! Wraps `RecoveryHandler` with ergonomic methods and proper error handling.

use super::recovery::{
    GuardianApproval, RecoveryHandler, RecoveryOperation, RecoveryRequest, RecoveryResult,
    RecoveryState,
};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::services::ceremony_runner::{
    CeremonyCommitMetadata, CeremonyInitRequest, CeremonyRunner,
};
use crate::runtime::services::{CeremonyTracker, ReconfigurationManager};
use crate::runtime::vm_host_bridge::{
    advance_host_bridged_vm_round, close_and_reap_vm_session, inject_vm_receive,
    open_manifest_vm_session_admitted, AuraVmHostWaitStatus,
};
use crate::runtime::{AuraEffectSystem, RuntimeChoreographySessionId};
use aura_core::crypto::Ed25519Signature;
use aura_core::effects::{CryptoCoreEffects, PhysicalTimeEffects, RandomCoreEffects};
use aura_core::hash::hash;
use aura_core::identifiers::CeremonyId;
use aura_core::identifiers::{AuthorityId, RecoveryId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::TimeEffects;
use aura_journal::fact::{ProtocolRelationalFact, RelationalFact};
use aura_protocol::effects::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, RoleIndex, TreeEffects,
};
use aura_recovery::ceremony_runners::{
    AbortCeremony, CommitCeremony, GuardianCeremonyRole, ProposeRotation,
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
use aura_recovery::membership_runners::GuardianMembershipChangeRole;
use aura_recovery::recovery_protocol::{
    GuardianApproval as ProtocolGuardianApproval, RecoveryOperation as ProtocolRecoveryOperation,
    RecoveryOutcome, RecoveryRequest as ProtocolRecoveryRequest,
};
use aura_recovery::setup_runners::GuardianSetupRole;
use aura_recovery::types::{GuardianProfile, GuardianSet};
use std::collections::BTreeMap;
use std::sync::Arc;
use telltale_vm::vm::StepResult;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

const CHOREO_START_RETRY_DELAY_MS: u64 = 50;
const CHOREO_START_RETRY_LIMIT: usize = 40;

mod ceremony_types;
mod state_machine;

/// Recovery service API
///
/// Provides recovery operations through a clean public API.
#[derive(Clone)]
pub struct RecoveryServiceApi {
    handler: RecoveryHandler,
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: CeremonyRunner,
    reconfiguration: ReconfigurationManager,
}

impl std::fmt::Debug for RecoveryServiceApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryServiceApi").finish_non_exhaustive()
    }
}

impl RecoveryServiceApi {
    fn role(authority_id: AuthorityId, role_index: u16) -> ChoreographicRole {
        ChoreographicRole::for_authority(
            authority_id,
            RoleIndex::new(role_index.into()).expect("role index"),
        )
    }

    /// Create a new recovery service
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
    ) -> AgentResult<Self> {
        let handler = RecoveryHandler::new(authority_context)?;
        let time_effects: Arc<dyn PhysicalTimeEffects> = Arc::new(effects.time_effects().clone());
        let ceremony_runner = CeremonyRunner::new(CeremonyTracker::new(time_effects));
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
            reconfiguration: ReconfigurationManager::new(),
        })
    }

    /// Create a new recovery service with a shared ceremony runner.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        reconfiguration: ReconfigurationManager,
    ) -> AgentResult<Self> {
        let handler = RecoveryHandler::new(authority_context)?;
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
            reconfiguration,
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
        let session_id = recovery_session_id(&approval.recovery_id, &approval.guardian_id);
        let roles = vec![
            Self::role(account_authority, 0),
            Self::role(account_authority, 1),
            Self::role(approval.guardian_id, 0),
        ];
        let peer_roles =
            BTreeMap::from([("Coordinator".to_string(), Self::role(account_authority, 1))]);
        let manifest =
            aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::composition_manifest();
        let global_type = aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::global_type();
        let local_types = aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::local_types();

        self.effects
            .start_session(session_id, roles)
            .await
            .map_err(|error| {
                AgentError::internal(format!("recovery guardian VM start failed: {error}"))
            })?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                self.effects.as_ref(),
                &manifest,
                "Guardian",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;
            handler.push_send_bytes(to_vec(&protocol_approval).map_err(|error| {
                AgentError::internal(format!("guardian approval encode failed: {error}"))
            })?);

            let loop_result = loop {
                let round = advance_host_bridged_vm_round(
                    self.effects.as_ref(),
                    &mut engine,
                    handler.as_ref(),
                    vm_sid,
                    "Guardian",
                    &peer_roles,
                )
                .await
                .map_err(AgentError::internal)?;

                if let Some(blocked) = round.blocked_receive {
                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "recovery guardian VM timed out while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "recovery guardian VM cancelled while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "recovery guardian VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            };

            let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            loop_result
        }
        .await;

        let _ = self.effects.end_session().await;
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
        let session_id = Self::ceremony_session_id(ceremony_id);
        let mut attempt = 0usize;
        let roles = vec![
            Self::role(authority_id, 0),
            Self::role(sorted_guardians[0], 0),
            Self::role(sorted_guardians[1], 0),
        ];
        loop {
            match self.effects.start_session(session_id, roles.clone()).await {
                Ok(()) => break,
                Err(ChoreographyError::SessionAlreadyExists { .. }) => {
                    if attempt >= CHOREO_START_RETRY_LIMIT {
                        return Err(AgentError::internal(
                            "guardian ceremony start failed: another session is still active"
                                .to_string(),
                        ));
                    }
                    attempt += 1;
                    sleep(Duration::from_millis(CHOREO_START_RETRY_DELAY_MS)).await;
                }
                Err(e) => {
                    return Err(AgentError::internal(format!(
                        "guardian ceremony start failed: {e}"
                    )))
                }
            }
        }

        let result = async {
            let manifest = aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::composition_manifest();
            let global_type = aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::global_type();
            let local_types = aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::local_types();
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                self.effects.as_ref(),
                &manifest,
                "Initiator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;

            for key_package in &sorted_key_packages {
                let nonce_bytes = self.effects.random_bytes(12).await;
                let mut encryption_nonce = [0u8; 12];
                encryption_nonce.copy_from_slice(&nonce_bytes[..12]);
                let ephemeral_public_key = self.effects.random_bytes(32).await;
                handler.push_send_bytes(
                    to_vec(&ProposeRotation(CeremonyProposal {
                        ceremony_id,
                        initiator_id: authority_id,
                        prestate_hash,
                        operation: operation.clone(),
                        encrypted_key_package: key_package.clone(),
                        encryption_nonce,
                        ephemeral_public_key,
                    }))
                    .map_err(|error| {
                        AgentError::internal(format!("guardian proposal encode failed: {error}"))
                    })?,
                );
            }

            let peer_roles = BTreeMap::from([
                ("Guardian1".to_string(), Self::role(sorted_guardians[0], 0)),
                ("Guardian2".to_string(), Self::role(sorted_guardians[1], 0)),
            ]);
            let mut responses = Vec::new();
            let mut branch_queued = false;

            let loop_result = loop {
                let round = advance_host_bridged_vm_round(
                    self.effects.as_ref(),
                    &mut engine,
                    handler.as_ref(),
                    vm_sid,
                    "Initiator",
                    &peer_roles,
                )
                .await
                .map_err(AgentError::internal)?;

                if let Some(blocked) = round.blocked_receive {
                    let response: CeremonyResponseMsg =
                        from_slice(&blocked.payload).map_err(|error| {
                            AgentError::internal(format!(
                                "guardian ceremony response decode failed: {error}"
                            ))
                        })?;
                    responses.push(response);

                    if !branch_queued && responses.len() == 2 {
                        let accepted: Vec<AuthorityId> = responses
                            .iter()
                            .filter(|response| response.response == CeremonyResponse::Accept)
                            .map(|response| response.guardian_id)
                            .collect();
                        let declined = responses
                            .iter()
                            .any(|response| response.response == CeremonyResponse::Decline);
                        let finalize = !declined && accepted.len() >= operation.threshold_k as usize;
                        handler.push_choice_label(if finalize { "finalize" } else { "cancel" });
                        if finalize {
                            let commit = CommitCeremony(CeremonyCommit {
                                ceremony_id,
                                new_epoch: operation.new_epoch,
                                threshold_signature: Vec::new(),
                                participants: accepted,
                            });
                            let payload = to_vec(&commit).map_err(|error| {
                                AgentError::internal(format!(
                                    "guardian ceremony commit encode failed: {error}"
                                ))
                            })?;
                            handler.push_send_bytes(payload.clone());
                            handler.push_send_bytes(payload);
                        } else {
                            let reason = if declined {
                                "guardian_declined"
                            } else {
                                "threshold_not_met"
                            };
                            let abort = AbortCeremony(CeremonyAbort {
                                ceremony_id,
                                reason: reason.to_string(),
                            });
                            let payload = to_vec(&abort).map_err(|error| {
                                AgentError::internal(format!(
                                    "guardian ceremony abort encode failed: {error}"
                                ))
                            })?;
                            handler.push_send_bytes(payload.clone());
                            handler.push_send_bytes(payload);
                        }
                        branch_queued = true;
                    }

                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "guardian ceremony initiator VM timed out while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "guardian ceremony initiator VM cancelled while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => {
                        let accepted = responses
                            .iter()
                            .filter(|response| response.response == CeremonyResponse::Accept)
                            .map(|response| response.guardian_id)
                            .collect();
                        break Ok(accepted);
                    }
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "guardian ceremony initiator VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            };

            let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            loop_result
        }
        .await;

        let _ = self.effects.end_session().await;
        result
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

        let response_msg = CeremonyResponseMsg {
            ceremony_id,
            guardian_id: authority_id,
            response,
            signature: Vec::new(),
        };
        let session_id = Self::ceremony_session_id(ceremony_id);
        let mut attempt = 0usize;
        let active_role_name = match guardian_role {
            GuardianCeremonyRole::Guardian1 => "Guardian1",
            GuardianCeremonyRole::Guardian2 => "Guardian2",
            GuardianCeremonyRole::Initiator => unreachable!(),
        };
        let roles = vec![Self::role(initiator_id, 0), Self::role(authority_id, 0)];
        loop {
            match self.effects.start_session(session_id, roles.clone()).await {
                Ok(()) => break,
                Err(ChoreographyError::SessionAlreadyExists { .. }) => {
                    if attempt >= CHOREO_START_RETRY_LIMIT {
                        return Err(AgentError::internal(
                            "guardian ceremony start failed: another session is still active"
                                .to_string(),
                        ));
                    }
                    attempt += 1;
                    sleep(Duration::from_millis(CHOREO_START_RETRY_DELAY_MS)).await;
                }
                Err(e) => {
                    return Err(AgentError::internal(format!(
                        "guardian ceremony start failed: {e}"
                    )))
                }
            }
        }

        let result = async {
            let manifest = aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::composition_manifest();
            let global_type = aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::global_type();
            let local_types = aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::local_types();
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                self.effects.as_ref(),
                &manifest,
                active_role_name,
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;
            handler.push_send_bytes(
                to_vec(&response_msg).map_err(|error| {
                    AgentError::internal(format!("guardian ceremony response encode failed: {error}"))
                })?,
            );
            let peer_roles =
                BTreeMap::from([("Initiator".to_string(), Self::role(initiator_id, 0))]);

            let loop_result = loop {
                let round = advance_host_bridged_vm_round(
                    self.effects.as_ref(),
                    &mut engine,
                    handler.as_ref(),
                    vm_sid,
                    active_role_name,
                    &peer_roles,
                )
                .await
                .map_err(AgentError::internal)?;

                if let Some(blocked) = round.blocked_receive {
                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "guardian ceremony VM timed out while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "guardian ceremony VM cancelled while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "guardian ceremony VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            };

            let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            loop_result
        }
        .await;

        let _ = self.effects.end_session().await;
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
        validate_guardian_setup_inputs(&guardians, threshold)?;

        let authority_id = self.handler.authority_context().authority_id();
        let timestamp = self.guardian_setup_timestamp().await?;
        let session_id = guardian_setup_session_id(setup_id);
        let roles = vec![
            Self::role(authority_id, 0),
            Self::role(guardians[0], 0),
            Self::role(guardians[1], 0),
            Self::role(guardians[2], 0),
        ];
        let peer_roles = BTreeMap::from([
            ("Guardian1".to_string(), Self::role(guardians[0], 0)),
            ("Guardian2".to_string(), Self::role(guardians[1], 0)),
            ("Guardian3".to_string(), Self::role(guardians[2], 0)),
        ]);
        let manifest =
            aura_recovery::guardian_setup::telltale_session_types_guardian_setup::vm_artifacts::composition_manifest();
        let global_type =
            aura_recovery::guardian_setup::telltale_session_types_guardian_setup::vm_artifacts::global_type();
        let local_types =
            aura_recovery::guardian_setup::telltale_session_types_guardian_setup::vm_artifacts::local_types();

        self.effects
            .start_session(session_id, roles)
            .await
            .map_err(|error| {
                AgentError::internal(format!("guardian setup VM start failed: {error}"))
            })?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                self.effects.as_ref(),
                &manifest,
                "SetupInitiator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;

            for _ in 0..guardians.len() {
                handler.push_send_bytes(
                    to_vec(&GuardianInvitation {
                        setup_id: setup_id.to_string(),
                        account_id,
                        target_guardians: guardians.clone(),
                        threshold,
                        timestamp: timestamp.clone(),
                    })
                    .map_err(|error| {
                        AgentError::internal(format!("guardian invitation encode failed: {error}"))
                    })?,
                );
            }

            let mut acceptances = Vec::new();
            let mut completion_queued = false;

            let loop_result = loop {
                let round = advance_host_bridged_vm_round(
                    self.effects.as_ref(),
                    &mut engine,
                    handler.as_ref(),
                    vm_sid,
                    "SetupInitiator",
                    &peer_roles,
                )
                .await
                .map_err(AgentError::internal)?;

                if let Some(blocked) = round.blocked_receive {
                    let acceptance: GuardianAcceptance =
                        from_slice(&blocked.payload).map_err(|error| {
                            AgentError::internal(format!(
                                "guardian acceptance decode failed: {error}"
                            ))
                        })?;
                    acceptances.push(acceptance);
                    if !completion_queued && acceptances.len() == guardians.len() {
                        let completion = build_guardian_setup_completion(
                            setup_id,
                            threshold,
                            acceptances.clone(),
                        );
                        let payload = to_vec(&completion).map_err(|error| {
                            AgentError::internal(format!(
                                "guardian setup completion encode failed: {error}"
                            ))
                        })?;
                        for _ in 0..guardians.len() {
                            handler.push_send_bytes(payload.clone());
                        }
                        completion_queued = true;
                    }
                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "guardian setup initiator VM timed out while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "guardian setup initiator VM cancelled while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => {
                        break Ok(build_guardian_setup_completion(
                            setup_id,
                            threshold,
                            acceptances.clone(),
                        ))
                    }
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "guardian setup initiator VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            };

            let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            loop_result
        }
        .await;

        let _ = self.effects.end_session().await;
        result
    }

    /// Execute guardian setup ceremony as a guardian (accept/decline).
    pub async fn execute_guardian_setup_guardian(
        &self,
        invitation: GuardianInvitation,
        accepted: bool,
    ) -> AgentResult<()> {
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
        let session_id = guardian_setup_session_id(&setup_id);
        let active_role_name = match guardian_role {
            GuardianSetupRole::Guardian1 => "Guardian1",
            GuardianSetupRole::Guardian2 => "Guardian2",
            GuardianSetupRole::Guardian3 => "Guardian3",
            GuardianSetupRole::SetupInitiator => unreachable!(),
        };
        let roles = vec![
            Self::role(invitation.account_id, 0),
            Self::role(authority_id, 0),
        ];
        let peer_roles = BTreeMap::from([(
            "SetupInitiator".to_string(),
            Self::role(invitation.account_id, 0),
        )]);
        let manifest =
            aura_recovery::guardian_setup::telltale_session_types_guardian_setup::vm_artifacts::composition_manifest();
        let global_type =
            aura_recovery::guardian_setup::telltale_session_types_guardian_setup::vm_artifacts::global_type();
        let local_types =
            aura_recovery::guardian_setup::telltale_session_types_guardian_setup::vm_artifacts::local_types();

        self.effects
            .start_session(session_id, roles)
            .await
            .map_err(|error| {
                AgentError::internal(format!("guardian setup VM start failed: {error}"))
            })?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                self.effects.as_ref(),
                &manifest,
                active_role_name,
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;
            handler.push_send_bytes(to_vec(&acceptance).map_err(|error| {
                AgentError::internal(format!("guardian acceptance encode failed: {error}"))
            })?);

            let loop_result = loop {
                let round = advance_host_bridged_vm_round(
                    self.effects.as_ref(),
                    &mut engine,
                    handler.as_ref(),
                    vm_sid,
                    active_role_name,
                    &peer_roles,
                )
                .await
                .map_err(AgentError::internal)?;

                if let Some(blocked) = round.blocked_receive {
                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "guardian setup VM timed out while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "guardian setup VM cancelled while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "guardian setup VM became stuck without a pending receive".to_string(),
                        ));
                    }
                }
            };

            let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            loop_result
        }
        .await;

        let _ = self.effects.end_session().await;
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
        let threshold_usize = threshold as usize;
        let new_threshold_final = new_threshold.unwrap_or(threshold as u16);
        let session_id = membership_session_id(&change_id);
        let roles = vec![
            Self::role(authority_id, 0),
            Self::role(guardians[0], 0),
            Self::role(guardians[1], 0),
            Self::role(guardians[2], 0),
        ];
        let peer_roles = BTreeMap::from([
            ("Guardian1".to_string(), Self::role(guardians[0], 0)),
            ("Guardian2".to_string(), Self::role(guardians[1], 0)),
            ("Guardian3".to_string(), Self::role(guardians[2], 0)),
        ]);
        let manifest = aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::composition_manifest();
        let global_type = aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::global_type();
        let local_types = aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::local_types();

        self.effects
            .start_session(session_id, roles)
            .await
            .map_err(|error| {
                AgentError::internal(format!("membership change VM start failed: {error}"))
            })?;

        let completion = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                self.effects.as_ref(),
                &manifest,
                "ChangeInitiator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;

            for _ in 0..3 {
                handler.push_send_bytes(
                    to_vec(&MembershipProposal {
                        change_id: change_id.clone(),
                        account_id: authority_id,
                        proposer_id: authority_id,
                        change: change.clone(),
                        new_threshold,
                        timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                            ts_ms: now_ms,
                            uncertainty: None,
                        }),
                    })
                    .map_err(|error| {
                        AgentError::internal(format!("membership proposal encode failed: {error}"))
                    })?,
                );
            }

            let mut votes = Vec::new();
            let mut completion_queued = false;

            let loop_result = loop {
                let round = advance_host_bridged_vm_round(
                    self.effects.as_ref(),
                    &mut engine,
                    handler.as_ref(),
                    vm_sid,
                    "ChangeInitiator",
                    &peer_roles,
                )
                .await
                .map_err(AgentError::internal)?;

                if let Some(blocked) = round.blocked_receive {
                    let vote: GuardianVote = from_slice(&blocked.payload).map_err(|error| {
                        AgentError::internal(format!("guardian vote decode failed: {error}"))
                    })?;
                    votes.push(vote);

                    if !completion_queued && votes.len() == 3 {
                        let accepted_guardians: Vec<AuthorityId> = votes
                            .iter()
                            .filter(|vote| vote.approved)
                            .map(|vote| vote.guardian_id)
                            .collect();
                        let completion = ChangeCompletion {
                            change_id: change_id.clone(),
                            success: accepted_guardians.len() >= threshold_usize,
                            new_guardian_set: GuardianSet::new(
                                accepted_guardians
                                    .iter()
                                    .copied()
                                    .map(GuardianProfile::new)
                                    .collect(),
                            ),
                            new_threshold: new_threshold_final,
                            change_evidence: Vec::new(),
                        };
                        let payload = to_vec(&completion).map_err(|error| {
                            AgentError::internal(format!(
                                "membership completion encode failed: {error}"
                            ))
                        })?;
                        for _ in 0..3 {
                            handler.push_send_bytes(payload.clone());
                        }
                        completion_queued = true;
                    }

                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "membership change initiator VM timed out while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "membership change initiator VM cancelled while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => {
                        let accepted_guardians: Vec<AuthorityId> = votes
                            .iter()
                            .filter(|vote| vote.approved)
                            .map(|vote| vote.guardian_id)
                            .collect();
                        break Ok(ChangeCompletion {
                            change_id: change_id.clone(),
                            success: accepted_guardians.len() >= threshold_usize,
                            new_guardian_set: GuardianSet::new(
                                accepted_guardians
                                    .into_iter()
                                    .map(GuardianProfile::new)
                                    .collect(),
                            ),
                            new_threshold: new_threshold_final,
                            change_evidence: Vec::new(),
                        });
                    }
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "membership change initiator VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            };

            let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            loop_result
        }
        .await?;

        let _ = self.effects.end_session().await;

        if completion.success {
            self.apply_guardian_handoff_reconfiguration(&completion, &change)
                .await?;
        }

        Ok(completion)
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

        let session_id = membership_session_id(&proposal.change_id);
        let active_role_name = match guardian_role {
            GuardianMembershipChangeRole::Guardian1 => "Guardian1",
            GuardianMembershipChangeRole::Guardian2 => "Guardian2",
            GuardianMembershipChangeRole::Guardian3 => "Guardian3",
            GuardianMembershipChangeRole::ChangeInitiator => unreachable!(),
        };
        let roles = vec![Self::role(initiator_id, 0), Self::role(authority_id, 0)];
        let peer_roles =
            BTreeMap::from([("ChangeInitiator".to_string(), Self::role(initiator_id, 0))]);
        let manifest = aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::composition_manifest();
        let global_type = aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::global_type();
        let local_types = aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::local_types();

        self.effects
            .start_session(session_id, roles)
            .await
            .map_err(|error| {
                AgentError::internal(format!("membership vote VM start failed: {error}"))
            })?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                self.effects.as_ref(),
                &manifest,
                active_role_name,
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;
            handler.push_send_bytes(to_vec(&vote).map_err(|error| {
                AgentError::internal(format!("guardian vote encode failed: {error}"))
            })?);

            let loop_result = loop {
                let round = advance_host_bridged_vm_round(
                    self.effects.as_ref(),
                    &mut engine,
                    handler.as_ref(),
                    vm_sid,
                    active_role_name,
                    &peer_roles,
                )
                .await
                .map_err(AgentError::internal)?;

                if let Some(blocked) = round.blocked_receive {
                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "membership vote VM timed out while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "membership vote VM cancelled while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "membership vote VM became stuck without a pending receive".to_string(),
                        ));
                    }
                }
            };

            let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            loop_result
        }
        .await;

        let _ = self.effects.end_session().await;
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

impl RecoveryServiceApi {
    async fn apply_guardian_handoff_reconfiguration(
        &self,
        completion: &ChangeCompletion,
        change: &MembershipChange,
    ) -> AgentResult<()> {
        use crate::core::default_context_id_for_authority;
        use crate::core::AgentError;
        use aura_core::Hash32;

        let MembershipChange::UpdateGuardian {
            guardian_id: previous_guardian,
            new_profile,
        } = change
        else {
            return Ok(());
        };

        let session_id =
            RuntimeChoreographySessionId::from_uuid(membership_session_id(&completion.change_id))
                .into_aura_session_id();
        let account_authority = self.handler.authority_context().authority_id();
        let context_id = default_context_id_for_authority(account_authority);

        self.reconfiguration
            .record_native_session(*previous_guardian, session_id)
            .await;
        self.reconfiguration
            .delegate_session(
                &self.effects,
                Some(context_id),
                session_id,
                *previous_guardian,
                new_profile.authority_id,
                Some("guardian_handoff".to_string()),
            )
            .await
            .map_err(|e| {
                AgentError::internal(format!("guardian handoff delegation failed: {e}"))
            })?;

        let binding_fact = RelationalFact::Protocol(ProtocolRelationalFact::GuardianBinding {
            account_id: account_authority,
            guardian_id: new_profile.authority_id,
            binding_hash: Hash32::default(),
        });
        self.effects
            .commit_relational_facts(vec![binding_fact])
            .await
            .map_err(|e| {
                AgentError::internal(format!(
                    "failed to persist guardian handoff binding context: {e}"
                ))
            })?;

        Ok(())
    }
}

async fn execute_recovery_protocol_account(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    state_machine::execute_recovery_protocol_account(effects, authority_id, guardian_id, request)
        .await
}

async fn execute_recovery_protocol_coordinator(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    state_machine::execute_recovery_protocol_coordinator(
        effects,
        authority_id,
        guardian_id,
        request,
    )
    .await
}

fn recovery_session_id(recovery_id: &RecoveryId, guardian_id: &AuthorityId) -> Uuid {
    state_machine::recovery_session_id(recovery_id, guardian_id)
}

fn guardian_setup_session_id(setup_id: &str) -> Uuid {
    state_machine::guardian_setup_session_id(setup_id)
}

fn membership_session_id(change_id: &str) -> Uuid {
    state_machine::membership_session_id(change_id)
}

fn validate_guardian_setup_inputs(guardians: &[AuthorityId], threshold: u16) -> AgentResult<()> {
    ceremony_types::validate_guardian_setup_inputs(guardians, threshold)
}

fn build_guardian_setup_completion(
    setup_id: &str,
    threshold: u16,
    acceptances: Vec<GuardianAcceptance>,
) -> SetupCompletion {
    ceremony_types::build_guardian_setup_completion(setup_id, threshold, acceptances)
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());

        let service = RecoveryServiceApi::new(effects, authority_context);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_add_device_recovery() {
        let authority_context = create_test_authority(151);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
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

    #[tokio::test]
    async fn test_guardian_handoff_reconfiguration_emits_audit_facts() {
        let authority_context = create_test_authority(179);
        let account_authority = authority_context.authority_id();
        let context_id = crate::core::default_context_id_for_authority(account_authority);
        let config = AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, account_authority)
                .unwrap(),
        );
        let service = RecoveryServiceApi::new(effects.clone(), authority_context).unwrap();

        let previous_guardian = AuthorityId::new_from_entropy([180u8; 32]);
        let replacement_guardian = AuthorityId::new_from_entropy([181u8; 32]);
        let change = MembershipChange::UpdateGuardian {
            guardian_id: previous_guardian,
            new_profile: GuardianProfile::new(replacement_guardian),
        };
        let completion = ChangeCompletion {
            change_id: "membership_handoff_test".to_string(),
            success: true,
            new_guardian_set: GuardianSet::new(vec![GuardianProfile::new(replacement_guardian)]),
            new_threshold: 1,
            change_evidence: vec![],
        };

        service
            .apply_guardian_handoff_reconfiguration(&completion, &change)
            .await
            .unwrap_or_else(|_| panic!(
                "guardian handoff reconfiguration failed: authority_id={account_authority}, reconfiguration_type=UpdateGuardian, change_id={}, previous_guardian={previous_guardian}, replacement_guardian={replacement_guardian}",
                completion.change_id
            ));

        let expected_fact_types = "SessionDelegation(guardian_handoff), GuardianBinding";
        let facts = effects
            .load_committed_facts(account_authority)
            .await
            .unwrap_or_else(|_| panic!(
                "load committed facts failed: context_id={context_id}, authority_id={account_authority}, expected_fact_types={expected_fact_types}, reason=verify guardian handoff audit facts persisted"
            ));

        assert!(facts.iter().any(|fact| {
            matches!(
                &fact.content,
                aura_journal::fact::FactContent::Relational(RelationalFact::Protocol(
                    ProtocolRelationalFact::SessionDelegation(delegation),
                )) if delegation.bundle_id.as_deref() == Some("guardian_handoff")
                    && delegation.from_authority == previous_guardian
                    && delegation.to_authority == replacement_guardian
            )
        }));

        assert!(facts.iter().any(|fact| {
            matches!(
                &fact.content,
                aura_journal::fact::FactContent::Relational(RelationalFact::Protocol(
                    ProtocolRelationalFact::GuardianBinding {
                        account_id,
                        guardian_id,
                        ..
                    },
                )) if *account_id == account_authority && *guardian_id == replacement_guardian
            )
        }));
    }
}
