//! Recovery Service - Public API for Recovery Operations
//!
//! Provides a clean public interface for guardian-based recovery operations.
//! Wraps `RecoveryHandler` with ergonomic methods and proper error handling.

use super::recovery::{
    recovery_guardian_private_key_storage_key, recovery_guardian_public_key_storage_key,
    GuardianApproval, RecoveryHandler, RecoveryOperation, RecoveryRequest, RecoveryResult,
    RecoveryState,
};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::handlers::shared::{build_transport_metadata, map_handler_tree_read_error};
use crate::runtime::open_owned_manifest_vm_session_admitted;
use crate::runtime::services::ceremony_runner::{
    CeremonyCommitMetadata, CeremonyInitRequest, CeremonyRunner,
};
use crate::runtime::services::{
    CeremonyTracker, ReconfigurationManager, SessionDelegationTransfer, TrustedKeyResolutionService,
};
use crate::runtime::transport_boundary::send_guarded_transport_envelope;
use crate::runtime::vm_host_bridge::{AuraVmHostWaitStatus, AuraVmRoundDisposition};
use crate::runtime::{AuraEffectSystem, RuntimeChoreographySessionId, TaskSupervisor};
use aura_core::crypto::Ed25519Signature;
use aura_core::effects::{
    CryptoCoreEffects, CryptoExtendedEffects, PhysicalTimeEffects, SecureStorageCapability,
    SecureStorageEffects, SecureStorageLocation, StorageCoreEffects,
};
use aura_core::hash::hash;
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::types::identifiers::CeremonyId;
use aura_core::types::identifiers::{AuthorityId, RecoveryId};
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::{ContextId, Ed25519VerifyingKey, Hash32, TimeEffects};
use aura_journal::fact::{ProtocolRelationalFact, RelationalFact};
use aura_protocol::effects::{ChoreographicRole, RoleIndex, TreeEffects};
use aura_protocol::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};
use aura_recovery::ceremony_runners::{AbortCeremony, ProposeRotation};
// Note: RespondCeremony is a received message type (Guardian -> Initiator) so we don't need
// to construct it - we only match on the type name suffix when processing received messages.
use aura_recovery::guardian_ceremony::{
    build_guardian_ceremony_commit_certificate, encrypt_ceremony_key_package,
    verify_guardian_ceremony_response_signature, CeremonyAbort, CeremonyCommit,
    CeremonyCommitCertificate, CeremonyProposal, CeremonyResponse, CeremonyResponseMsg,
    GuardianRotationOp,
};
use aura_recovery::guardian_membership::{
    sign_guardian_vote, verify_guardian_vote_signature, ChangeCompletion, GuardianVote,
    MembershipChange, MembershipProposal,
};
use aura_recovery::guardian_setup::{
    build_setup_completion_with_material, encrypt_guardian_share, sign_guardian_setup_acceptance,
    validate_setup_inputs, verify_guardian_setup_acceptance_signature, EncryptedKeyShare,
    GuardianAcceptance, GuardianInvitation, SetupCompletion,
};
use aura_recovery::recovery_protocol::{
    GuardianApproval as ProtocolGuardianApproval, RecoveryOperation as ProtocolRecoveryOperation,
    RecoveryOutcome, RecoveryRequest as ProtocolRecoveryRequest,
};
use aura_recovery::types::{GuardianProfile, GuardianSet};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use telltale_machine::StepResult;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

const CHOREO_START_RETRY_DELAY_MS: u64 = 50;
const CHOREO_START_RETRY_LIMIT: usize = 40;
const RECOVERY_CLEANUP_INTERVAL: StdDuration = StdDuration::from_secs(5);
const RECOVERY_TIMEOUT_REASON: &str = "Recovery timed out";
const GUARDIAN_SETUP_ACCEPTANCE_MAX_SKEW_MS: u64 = 5 * 60 * 1000;

/// Recovery service API
///
/// Provides recovery operations through a clean public API.
#[derive(Clone)]
pub struct RecoveryServiceApi {
    handler: RecoveryHandler,
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: CeremonyRunner,
    reconfiguration: ReconfigurationManager,
    tasks: Arc<TaskSupervisor>,
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
        let service = Self {
            handler,
            effects,
            ceremony_runner,
            reconfiguration: ReconfigurationManager::new(),
            tasks: Arc::new(TaskSupervisor::new()),
        };
        service.spawn_cleanup_task();
        Ok(service)
    }

    #[cfg(test)]
    fn new_for_test(
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
            tasks: Arc::new(TaskSupervisor::new()),
        })
    }

    /// Create a new recovery service with a shared ceremony runner.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        reconfiguration: ReconfigurationManager,
        tasks: Arc<TaskSupervisor>,
    ) -> AgentResult<Self> {
        let handler = RecoveryHandler::new(authority_context)?;
        let service = Self {
            handler,
            effects,
            ceremony_runner,
            reconfiguration,
            tasks,
        };
        service.spawn_cleanup_task();
        Ok(service)
    }

    fn spawn_cleanup_task(&self) {
        let service = self.clone();
        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(self.effects.time_effects().clone());
        let cleanup_tasks = self.tasks.group("recovery_service.cleanup");
        let _cleanup_task_handle = cleanup_tasks.spawn_local_interval_until_named(
            "expired_recoveries",
            time_effects,
            RECOVERY_CLEANUP_INTERVAL,
            move || {
                let service = service.clone();
                async move {
                    let removed = service.cleanup_expired_recoveries_once().await;
                    if removed > 0 {
                        tracing::warn!(
                            event = "recovery_service.cleanup.expired",
                            removed,
                            "Cancelled expired recovery ceremonies"
                        );
                    }
                    true
                }
            },
        );
    }

    async fn cleanup_expired_recoveries_once(&self) -> usize {
        let now_ms = match self.effects.physical_time().await {
            Ok(now) => now.ts_ms,
            Err(error) => {
                tracing::warn!(
                    event = "recovery_service.cleanup.time_unavailable",
                    error = %error,
                    "Skipping expired recovery cleanup because time is unavailable"
                );
                return 0;
            }
        };

        let expired_ids = self.handler.expired_recovery_ids(now_ms).await;
        let mut removed = 0;

        for recovery_id in expired_ids {
            match self
                .cancel(&recovery_id, RECOVERY_TIMEOUT_REASON.to_string())
                .await
            {
                Ok(_) => {
                    removed += 1;
                }
                Err(error) => {
                    tracing::warn!(
                        event = "recovery_service.cleanup.cancel_failed",
                        recovery_id = %recovery_id,
                        error = %error,
                        "Failed to cancel expired recovery ceremony"
                    );
                }
            }
        }

        removed
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
            .map_err(map_handler_tree_read_error)?;

        let now_ms = self.effects.current_timestamp_ms().await;
        let prestate_hash = aura_core::Hash32(tree_state.root_commitment);

        for old_id in self
            .ceremony_runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::Recovery,
                &prestate_hash,
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
        let approval = self.verified_recovery_approval(approval).await?;
        let state = self
            .handler
            .submit_approval(&self.effects, approval.payload().clone())
            .await?;
        let ceremony_id = CeremonyId::new(approval.payload().recovery_id.to_string());
        let _ = self
            .ceremony_runner
            .record_verified_response(
                &ceremony_id,
                aura_core::threshold::ParticipantIdentity::guardian(approval.payload().guardian_id),
                &approval,
            )
            .await
            .map_err(|e| {
                AgentError::internal(format!("Failed to record recovery approval: {e}"))
            })?;
        if let Some(recovery) = self
            .handler
            .get_recovery(&approval.payload().recovery_id)
            .await
        {
            let _ = self
                .execute_recovery_protocol_guardian(
                    approval.payload(),
                    recovery.request.account_authority,
                )
                .await;
        }
        Ok(state)
    }

    async fn verified_recovery_approval(
        &self,
        approval: GuardianApproval,
    ) -> AgentResult<VerifiedIngress<GuardianApproval>> {
        let recovery = self
            .handler
            .get_recovery(&approval.recovery_id)
            .await
            .ok_or_else(|| {
                AgentError::runtime(format!(
                    "Recovery ceremony not found: {}",
                    approval.recovery_id
                ))
            })?;
        let guardian_is_member = recovery.request.guardians.contains(&approval.guardian_id);
        let duplicate_approval = recovery
            .approvals
            .iter()
            .any(|existing| existing.guardian_id == approval.guardian_id);
        let context =
            ContextId::new_from_entropy(hash(approval.recovery_id.to_string().as_bytes()));
        let payload_hash = Hash32::from_value(&approval)
            .map_err(|error| AgentError::internal(format!("hash recovery approval: {error}")))?;
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Authority(approval.guardian_id),
            context,
            None,
            payload_hash,
            1,
        );
        let evidence = IngressVerificationEvidence::builder(metadata)
            .peer_identity(
                guardian_is_member,
                "approval guardian must be in recovery set",
            )
            .and_then(|builder| {
                builder.envelope_authenticity(
                    !approval.signature.is_empty(),
                    "guardian approval signature must be present",
                )
            })
            .and_then(|builder| {
                builder.capability_authorization(
                    guardian_is_member,
                    "guardian must be authorized by the recovery request",
                )
            })
            .and_then(|builder| {
                builder.namespace_scope(
                    recovery.request.guardians.contains(&approval.guardian_id),
                    "recovery approval must target an active recovery guardian scope",
                )
            })
            .and_then(|builder| builder.schema_version(true, "recovery approval schema v1"))
            .and_then(|builder| {
                builder.replay_freshness(
                    !duplicate_approval,
                    "guardian approval must not duplicate an existing approval",
                )
            })
            .and_then(|builder| {
                builder.signer_membership(guardian_is_member, "guardian signer must be enrolled")
            })
            .and_then(|builder| {
                builder.proof_evidence(
                    !approval.signature.is_empty(),
                    "guardian signature evidence must be present",
                )
            })
            .and_then(|builder| builder.build())
            .map_err(|error| {
                AgentError::internal(format!("verify recovery approval ingress: {error}"))
            })?;
        DecodedIngress::new(approval, evidence.metadata().clone())
            .verify(evidence)
            .map_err(|error| AgentError::internal(format!("promote recovery approval: {error}")))
    }

    fn verified_recovery_session_payload<T: Serialize>(
        source: AuthorityId,
        context_entropy: &[u8],
        payload: T,
        signer_member: bool,
        proof_present: bool,
    ) -> AgentResult<VerifiedIngress<T>> {
        let context = ContextId::new_from_entropy(hash(context_entropy));
        let payload_hash = Hash32::from_value(&payload).map_err(|error| {
            AgentError::internal(format!("hash recovery session payload: {error}"))
        })?;
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Authority(source),
            context,
            None,
            payload_hash,
            1,
        );
        let evidence = IngressVerificationEvidence::builder(metadata)
            .peer_identity(
                signer_member,
                "recovery session sender must be an expected guardian",
            )
            .and_then(|builder| {
                builder.envelope_authenticity(
                    proof_present,
                    "recovery session payload must carry signature or key proof evidence",
                )
            })
            .and_then(|builder| {
                builder.capability_authorization(
                    signer_member,
                    "recovery session sender must be authorized for this ceremony",
                )
            })
            .and_then(|builder| {
                builder.namespace_scope(
                    !context_entropy.is_empty(),
                    "recovery session payload must be ceremony scoped",
                )
            })
            .and_then(|builder| builder.schema_version(true, "recovery session schema v1"))
            .and_then(|builder| {
                builder.replay_freshness(
                    proof_present,
                    "recovery session payload must include replay-resistant proof material",
                )
            })
            .and_then(|builder| {
                builder.signer_membership(
                    signer_member,
                    "recovery session signer must be an expected guardian",
                )
            })
            .and_then(|builder| {
                builder.proof_evidence(
                    proof_present,
                    "recovery session proof evidence must be present",
                )
            })
            .and_then(|builder| builder.build())
            .map_err(|error| {
                AgentError::internal(format!("verify recovery session ingress: {error}"))
            })?;
        DecodedIngress::new(payload, evidence.metadata().clone())
            .verify(evidence)
            .map_err(|error| {
                AgentError::internal(format!("promote recovery session payload: {error}"))
            })
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
        let tasks = self
            .tasks
            .group(format!("recovery_service.protocol.{}", request.recovery_id));

        for guardian_id in guardians {
            let protocol_request = protocol_request.clone();
            let effects = effects.clone();
            let fut = async move {
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
            };
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "wasm32")] {
                    let _task_handle =
                        tasks.spawn_local_named(format!("guardian.{guardian_id}"), fut);
                } else {
                    let _task_handle =
                        tasks.spawn_named(format!("guardian.{guardian_id}"), fut);
                }
            }
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
            prestate_hash: request.prestate_hash,
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

        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                self.effects.clone(),
                session_id,
                roles,
                &manifest,
                "Guardian",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| AgentError::internal(error.to_string()))?;
            session.queue_send_bytes(to_vec(&protocol_approval).map_err(|error| {
                AgentError::internal(format!("guardian approval encode failed: {error}"))
            })?);

            let loop_result = loop {
                let round = session
                    .advance_round("Guardian", &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
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

            let _ = session.close().await;
            loop_result
        }
        .await;
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

        use aura_core::ContextId;

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

        let proposal = self
            .build_guardian_ceremony_proposal(
                guardian_authority,
                ceremony_id,
                prestate_hash,
                &operation,
                key_package,
            )
            .await?;

        // Serialize the proposal
        let payload = serde_json::to_vec(&proposal)
            .map_err(|e| AgentError::internal(format!("Failed to serialize proposal: {}", e)))?;

        // Create transport envelope
        let metadata = build_transport_metadata(
            "application/aura-guardian-proposal",
            [
                ("protocol-version", "1".to_string()),
                ("ceremony-id", ceremony_id.to_string()),
            ],
        );

        let envelope = aura_core::effects::TransportEnvelope {
            destination: guardian_authority,
            source: initiator_id,
            context: ceremony_context,
            payload,
            metadata,
            receipt: None, // Receipts would be added by guard chain in production
        };

        send_guarded_transport_envelope(&self.effects, envelope)
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
    /// Returns verified guardian responses for tracker recording.
    pub async fn execute_guardian_ceremony_initiator(
        &self,
        ceremony_id: aura_recovery::CeremonyId,
        prestate_hash: aura_core::Hash32,
        operation: GuardianRotationOp,
        guardians: Vec<AuthorityId>,
        key_packages: Vec<Vec<u8>>,
    ) -> AgentResult<Vec<VerifiedIngress<CeremonyResponseMsg>>> {
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
        let guardian_key_resolver = self.trusted_guardian_setup_keys(&sorted_guardians).await?;
        let mut guardian_proposals = BTreeMap::new();
        for (guardian, key_package) in sorted_guardians.iter().zip(sorted_key_packages.iter()) {
            let proposal = self
                .build_guardian_ceremony_proposal(
                    *guardian,
                    ceremony_id,
                    prestate_hash,
                    &operation,
                    key_package,
                )
                .await?;
            guardian_proposals.insert(*guardian, proposal);
        }
        let session_id = Self::ceremony_session_id(ceremony_id);
        let roles = vec![
            Self::role(authority_id, 0),
            Self::role(sorted_guardians[0], 0),
            Self::role(sorted_guardians[1], 0),
        ];
        let manifest = aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::composition_manifest();
        let global_type = aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::global_type();
        let local_types = aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::local_types();
        let mut attempt = 0usize;

        let result = loop {
            let open_result = open_owned_manifest_vm_session_admitted(
                self.effects.clone(),
                session_id,
                roles.clone(),
                &manifest,
                "Initiator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await;

            let mut session = match open_result {
                Ok(session) => session,
                Err(error)
                    if matches!(
                        &error,
                        crate::runtime::SessionIngressError::SessionStart {
                            reason: crate::runtime::SessionStartFailureReason::AlreadyExists,
                            ..
                        }
                    ) =>
                {
                    if attempt >= CHOREO_START_RETRY_LIMIT {
                        break Err(AgentError::internal(
                            "guardian ceremony start failed: another session is still active"
                                .to_string(),
                        ));
                    }
                    attempt += 1;
                    sleep(Duration::from_millis(CHOREO_START_RETRY_DELAY_MS)).await;
                    continue;
                }
                Err(error) => {
                    break Err(AgentError::internal(format!(
                        "guardian ceremony start failed: {error}"
                    )));
                }
            };

            for guardian in &sorted_guardians {
                let proposal = guardian_proposals.get(guardian).ok_or_else(|| {
                    AgentError::internal(format!(
                        "missing guardian ceremony proposal for {guardian}"
                    ))
                })?;
                session.queue_send_bytes(to_vec(&ProposeRotation(proposal.clone())).map_err(
                    |error| {
                        AgentError::internal(format!("guardian proposal encode failed: {error}"))
                    },
                )?);
            }

            let peer_roles = BTreeMap::from([
                ("Guardian1".to_string(), Self::role(sorted_guardians[0], 0)),
                ("Guardian2".to_string(), Self::role(sorted_guardians[1], 0)),
            ]);
            let mut responses = Vec::new();
            let mut seen_guardians = BTreeSet::new();
            let mut branch_queued = false;

            let loop_result = loop {
                let round = session
                    .advance_round("Initiator", &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    let response: CeremonyResponseMsg =
                        from_slice(&blocked.payload).map_err(|error| {
                            AgentError::internal(format!(
                                "guardian ceremony response decode failed: {error}"
                            ))
                        })?;
                    let response_guardian = response.guardian_id;
                    let response_context = response.ceremony_id.to_string();
                    let response_member = sorted_guardians.contains(&response_guardian);
                    if !response_member {
                        return Err(AgentError::invalid(format!(
                            "guardian ceremony response from unexpected guardian {response_guardian}"
                        )));
                    }
                    if !seen_guardians.insert(response_guardian) {
                        return Err(AgentError::invalid(format!(
                            "duplicate guardian ceremony response from {response_guardian}"
                        )));
                    }
                    let proposal = guardian_proposals.get(&response_guardian).ok_or_else(|| {
                        AgentError::internal(format!(
                            "missing guardian ceremony proposal for response from {response_guardian}"
                        ))
                    })?;
                    let response_has_proof = verify_guardian_ceremony_response_signature(
                        self.effects.as_ref(),
                        proposal,
                        &response,
                        &guardian_key_resolver,
                    )
                    .await
                    .map_err(|error| {
                        AgentError::effects(format!(
                            "guardian ceremony response verification failed: {error}"
                        ))
                    })?;
                    let response = Self::verified_recovery_session_payload(
                        response_guardian,
                        response_context.as_bytes(),
                        response,
                        response_member,
                        response_has_proof,
                    )?;
                    responses.push(response);

                    if !branch_queued && responses.len() == 2 {
                        let accepted: Vec<AuthorityId> = responses
                            .iter()
                            .filter(|response| {
                                response.payload().response == CeremonyResponse::Accept
                            })
                            .map(|response| response.payload().guardian_id)
                            .collect();
                        let declined = responses.iter().any(|response| {
                            response.payload().response == CeremonyResponse::Decline
                        });
                        let finalize =
                            !declined && accepted.len() >= operation.threshold_k as usize;
                        if finalize {
                            let accepted_messages: Vec<_> = responses
                                .iter()
                                .filter(|response| {
                                    response.payload().response == CeremonyResponse::Accept
                                })
                                .map(|response| response.payload().clone())
                                .collect();
                            let commit_certificate = build_guardian_ceremony_commit_certificate(
                                self.effects.as_ref(),
                                ceremony_id,
                                authority_id,
                                prestate_hash,
                                &operation,
                                &sorted_guardians,
                                operation.threshold_k,
                                &accepted_messages,
                                &guardian_key_resolver,
                            )
                            .await
                            .map_err(|error| {
                                AgentError::effects(format!(
                                    "guardian ceremony commit certificate build failed: {error}"
                                ))
                            })?;
                            self.store_guardian_ceremony_commit_certificate(&commit_certificate)
                                .await?;
                            session.queue_choice_label("commit");
                            let commit = CeremonyCommit {
                                ceremony_id,
                                new_epoch: operation.new_epoch,
                                commit_certificate,
                                participants: accepted,
                            };
                            let payload = to_vec(&commit).map_err(|error| {
                                AgentError::internal(format!(
                                    "guardian ceremony commit encode failed: {error}"
                                ))
                            })?;
                            session.queue_send_bytes(payload.clone());
                            session.queue_send_bytes(payload);
                        } else {
                            session.queue_choice_label("cancel");
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
                            session.queue_send_bytes(payload.clone());
                            session.queue_send_bytes(payload);
                        }
                        branch_queued = true;
                    }

                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
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
                            .into_iter()
                            .filter(|response| {
                                response.payload().response == CeremonyResponse::Accept
                            })
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

            let _ = session.close().await;
            break loop_result;
        };
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
        if role_index > 1 {
            return Err(AgentError::invalid(format!(
                "Invalid guardian role index: {} (must be 0 or 1)",
                role_index
            )));
        }
        let _ = (initiator_id, ceremony_id, response);
        Err(AgentError::internal(
            "guardian ceremony response requires a guardian signature; unsigned responses are disabled",
        ))
    }

    async fn build_guardian_ceremony_proposal(
        &self,
        guardian_authority: AuthorityId,
        ceremony_id: aura_recovery::CeremonyId,
        prestate_hash: aura_core::Hash32,
        operation: &GuardianRotationOp,
        key_package: &[u8],
    ) -> AgentResult<CeremonyProposal> {
        let initiator_id = self.handler.authority_context().authority_id();
        let guardian_public_key = self
            .effects
            .retrieve(&recovery_guardian_public_key_storage_key(guardian_authority))
            .await
            .map_err(|error| {
                AgentError::effects(format!(
                    "guardian ceremony trusted key retrieval failed for {guardian_authority}: {error}"
                ))
            })?
            .ok_or_else(|| {
                AgentError::invalid(format!(
                    "missing trusted guardian ceremony key for {guardian_authority}"
                ))
            })?;
        encrypt_ceremony_key_package(
            self.effects.as_ref(),
            ceremony_id,
            initiator_id,
            prestate_hash,
            operation,
            &guardian_public_key,
            key_package,
        )
        .await
        .map_err(|error| {
            AgentError::effects(format!(
                "guardian ceremony proposal encryption failed for {guardian_authority}: {error}"
            ))
        })
    }

    async fn store_guardian_ceremony_commit_certificate(
        &self,
        certificate: &CeremonyCommitCertificate,
    ) -> AgentResult<()> {
        let encoded = to_vec(certificate).map_err(|error| {
            AgentError::internal(format!(
                "guardian ceremony commit certificate encode failed: {error}"
            ))
        })?;
        self.effects
            .store(
                &guardian_ceremony_commit_certificate_storage_key(certificate.ceremony_id),
                encoded,
            )
            .await
            .map_err(|error| {
                AgentError::effects(format!(
                    "guardian ceremony commit certificate storage failed: {error}"
                ))
            })
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
        let invitation = GuardianInvitation {
            setup_id: setup_id.to_string(),
            account_id,
            target_guardians: guardians.clone(),
            threshold,
            timestamp: timestamp.clone(),
        };
        let guardian_key_resolver = self.trusted_guardian_setup_keys(&guardians).await?;
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

        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                self.effects.clone(),
                session_id,
                roles,
                &manifest,
                "SetupInitiator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| AgentError::internal(error.to_string()))?;

            for _ in 0..guardians.len() {
                session.queue_send_bytes(to_vec(&invitation).map_err(|error| {
                    AgentError::internal(format!("guardian invitation encode failed: {error}"))
                })?);
            }

            let mut acceptances = Vec::new();
            let mut seen_guardians = BTreeSet::new();
            let mut completion_queued = false;
            let mut completion: Option<SetupCompletion> = None;

            let loop_result = loop {
                let round = session
                    .advance_round("SetupInitiator", &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    let acceptance: GuardianAcceptance =
                        from_slice(&blocked.payload).map_err(|error| {
                            AgentError::internal(format!(
                                "guardian acceptance decode failed: {error}"
                            ))
                        })?;
                    let acceptance = self
                        .verify_guardian_setup_acceptance_for_invitation(
                            &invitation,
                            &guardians,
                            &guardian_key_resolver,
                            &mut seen_guardians,
                            acceptance,
                        )
                        .await?;
                    acceptances.push(acceptance);
                    if !completion_queued && acceptances.len() == guardians.len() {
                        let built_completion = self
                            .build_verified_guardian_setup_completion(&invitation, &acceptances)
                            .await?;
                        let payload = to_vec(&built_completion).map_err(|error| {
                            AgentError::internal(format!(
                                "guardian setup completion encode failed: {error}"
                            ))
                        })?;
                        completion = Some(built_completion);
                        for _ in 0..guardians.len() {
                            session.queue_send_bytes(payload.clone());
                        }
                        completion_queued = true;
                    }
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
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
                        break completion.ok_or_else(|| {
                            AgentError::internal(
                                "guardian setup completed without a verified completion payload"
                                    .to_string(),
                            )
                        });
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

            let _ = session.close().await;
            loop_result
        }
        .await;
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

        let active_role_name = match guardian_index {
            0 => "Guardian1",
            1 => "Guardian2",
            2 => "Guardian3",
            _ => {
                return Err(AgentError::invalid(
                    "Guardian setup requires exactly three guardians".to_string(),
                ));
            }
        };

        let setup_id = invitation.setup_id.clone();
        let timestamp = self.guardian_setup_timestamp().await?;
        let signing_private_key = self
            .effects
            .retrieve(&recovery_guardian_private_key_storage_key(authority_id))
            .await
            .map_err(|error| {
                AgentError::effects(format!(
                    "guardian setup signing key retrieval failed: {error}"
                ))
            })?
            .ok_or_else(|| {
                AgentError::invalid(
                    "guardian setup acceptance requires a registered guardian signing key"
                        .to_string(),
                )
            })?;
        let (share_private_key, public_key) = self
            .effects
            .ed25519_generate_keypair()
            .await
            .map_err(|e| AgentError::internal(format!("guardian keygen failed: {e}")))?;
        let key_location = SecureStorageLocation::with_sub_key(
            "guardian_acceptance_keys",
            &setup_id,
            authority_id.to_string(),
        );
        self.effects
            .secure_store(
                &key_location,
                &share_private_key,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|error| {
                AgentError::effects(format!(
                    "guardian setup acceptance key storage failed: {error}"
                ))
            })?;
        let signature = sign_guardian_setup_acceptance(
            self.effects.as_ref(),
            &invitation,
            authority_id,
            accepted,
            &public_key,
            &timestamp,
            &signing_private_key,
        )
        .await
        .map_err(|error| {
            AgentError::effects(format!("guardian setup acceptance signing failed: {error}"))
        })?;
        let acceptance = GuardianAcceptance {
            guardian_id: authority_id,
            setup_id: setup_id.clone(),
            account_id: invitation.account_id,
            accepted,
            public_key,
            timestamp,
            signature,
        };
        let session_id = guardian_setup_session_id(&setup_id);
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

        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                self.effects.clone(),
                session_id,
                roles,
                &manifest,
                active_role_name,
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| AgentError::internal(error.to_string()))?;
            session.queue_send_bytes(to_vec(&acceptance).map_err(|error| {
                AgentError::internal(format!("guardian acceptance encode failed: {error}"))
            })?);

            let loop_result = loop {
                let round = session
                    .advance_round(active_role_name, &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
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

            let _ = session.close().await;
            loop_result
        }
        .await;
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
        guardian_verifying_keys: BTreeMap<AuthorityId, Vec<u8>>,
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
        let proposal = MembershipProposal {
            change_id: change_id.clone(),
            account_id: authority_id,
            proposer_id: authority_id,
            change: change.clone(),
            threshold: threshold as u16,
            new_threshold,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            }),
        };
        let guardian_key_resolver =
            trusted_guardian_vote_keys(&guardians, &guardian_verifying_keys)?;

        let completion = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                self.effects.clone(),
                session_id,
                roles,
                &manifest,
                "ChangeInitiator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| AgentError::internal(error.to_string()))?;

            for _ in 0..3 {
                session.queue_send_bytes(to_vec(&proposal).map_err(|error| {
                    AgentError::internal(format!("membership proposal encode failed: {error}"))
                })?);
            }

            let mut votes = Vec::new();
            let mut seen_guardians = BTreeSet::new();
            let mut completion_queued = false;

            let loop_result = loop {
                let round = session
                    .advance_round("ChangeInitiator", &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    let vote: GuardianVote = from_slice(&blocked.payload).map_err(|error| {
                        AgentError::internal(format!("guardian vote decode failed: {error}"))
                    })?;
                    let vote = self
                        .verify_membership_vote_for_proposal(
                            &proposal,
                            &guardians,
                            &guardian_key_resolver,
                            &mut seen_guardians,
                            vote,
                        )
                        .await?;
                    votes.push(vote);

                    if !completion_queued && votes.len() == 3 {
                        let accepted_guardians: Vec<AuthorityId> = votes
                            .iter()
                            .filter(|vote| vote.payload().approved)
                            .map(|vote| vote.payload().guardian_id)
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
                            session.queue_send_bytes(payload.clone());
                        }
                        completion_queued = true;
                    }

                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
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
                            .filter(|vote| vote.payload().approved)
                            .map(|vote| vote.payload().guardian_id)
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

            let _ = session.close().await;
            loop_result
        }
        .await?;

        if completion.success {
            self.apply_guardian_handoff_reconfiguration(&completion, &change)
                .await?;
        }

        Ok(completion)
    }

    async fn verify_membership_vote_for_proposal(
        &self,
        proposal: &MembershipProposal,
        guardians: &[AuthorityId],
        guardian_key_resolver: &TrustedKeyResolutionService,
        seen_guardians: &mut BTreeSet<AuthorityId>,
        vote: GuardianVote,
    ) -> AgentResult<VerifiedIngress<GuardianVote>> {
        let vote_guardian = vote.guardian_id;
        let vote_context = vote.change_id.clone();
        let vote_member = guardians.contains(&vote_guardian);
        if !seen_guardians.insert(vote_guardian) {
            return Err(AgentError::invalid(format!(
                "duplicate guardian membership vote from {vote_guardian}"
            )));
        }
        let signature_verified = verify_guardian_vote_signature(
            self.effects.as_ref(),
            proposal,
            &vote,
            guardian_key_resolver,
        )
        .await
        .map_err(|error| {
            AgentError::effects(format!("guardian vote verification failed: {error}"))
        })?;
        Self::verified_recovery_session_payload(
            vote_guardian,
            vote_context.as_bytes(),
            vote,
            vote_member,
            signature_verified,
        )
    }

    async fn trusted_guardian_setup_keys(
        &self,
        guardians: &[AuthorityId],
    ) -> AgentResult<TrustedKeyResolutionService> {
        let resolver = TrustedKeyResolutionService::new();
        for guardian in guardians {
            let key = self
                .effects
                .retrieve(&recovery_guardian_public_key_storage_key(*guardian))
                .await
                .map_err(|error| {
                    AgentError::effects(format!(
                        "trusted guardian setup key retrieval failed for {guardian}: {error}"
                    ))
                })?
                .ok_or_else(|| {
                    AgentError::invalid(format!(
                        "missing trusted guardian setup key for {guardian}"
                    ))
                })?;
            resolver
                .register_guardian_key(*guardian, key)
                .map_err(|error| {
                    AgentError::invalid(format!(
                        "invalid trusted guardian setup key for {guardian}: {error}"
                    ))
                })?;
        }
        Ok(resolver)
    }

    async fn verify_guardian_setup_acceptance_for_invitation(
        &self,
        invitation: &GuardianInvitation,
        guardians: &[AuthorityId],
        guardian_key_resolver: &TrustedKeyResolutionService,
        seen_guardians: &mut BTreeSet<AuthorityId>,
        acceptance: GuardianAcceptance,
    ) -> AgentResult<VerifiedIngress<GuardianAcceptance>> {
        let guardian_id = acceptance.guardian_id;
        if !guardians.contains(&guardian_id) {
            return Err(AgentError::invalid(format!(
                "guardian setup acceptance from unknown guardian {guardian_id}"
            )));
        }
        if !seen_guardians.insert(guardian_id) {
            return Err(AgentError::invalid(format!(
                "duplicate guardian setup acceptance from {guardian_id}"
            )));
        }
        if acceptance.setup_id != invitation.setup_id {
            return Err(AgentError::invalid(format!(
                "guardian setup acceptance has mismatched setup id: expected {}, got {}",
                invitation.setup_id, acceptance.setup_id
            )));
        }
        if acceptance.account_id != invitation.account_id {
            return Err(AgentError::invalid(format!(
                "guardian setup acceptance has mismatched account id: expected {}, got {}",
                invitation.account_id, acceptance.account_id
            )));
        }
        if acceptance.public_key.is_empty() {
            return Err(AgentError::invalid(
                "guardian setup acceptance must include a public key".to_string(),
            ));
        }
        Ed25519VerifyingKey::try_from_slice(&acceptance.public_key).map_err(|error| {
            AgentError::invalid(format!(
                "guardian setup acceptance public key is malformed: {error}"
            ))
        })?;
        let invitation_ms = physical_timestamp_ms(&invitation.timestamp)?;
        let acceptance_ms = physical_timestamp_ms(&acceptance.timestamp)?;
        let now_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|error| {
                AgentError::effects(format!(
                    "guardian setup acceptance freshness check failed: {error}"
                ))
            })?
            .ts_ms;
        if acceptance_ms < invitation_ms {
            return Err(AgentError::invalid(
                "guardian setup acceptance timestamp predates the invitation".to_string(),
            ));
        }
        if now_ms.abs_diff(acceptance_ms) > GUARDIAN_SETUP_ACCEPTANCE_MAX_SKEW_MS {
            return Err(AgentError::invalid(
                "guardian setup acceptance timestamp is stale".to_string(),
            ));
        }
        let signature_verified = verify_guardian_setup_acceptance_signature(
            self.effects.as_ref(),
            invitation,
            &acceptance,
            guardian_key_resolver,
        )
        .await
        .map_err(|error| {
            AgentError::effects(format!(
                "guardian setup acceptance verification failed: {error}"
            ))
        })?;
        let acceptance_context = acceptance.setup_id.clone();
        Self::verified_recovery_session_payload(
            guardian_id,
            acceptance_context.as_bytes(),
            acceptance,
            true,
            signature_verified,
        )
    }

    async fn build_verified_guardian_setup_completion(
        &self,
        invitation: &GuardianInvitation,
        acceptances: &[VerifiedIngress<GuardianAcceptance>],
    ) -> AgentResult<SetupCompletion> {
        let mut accepted_acceptances: Vec<GuardianAcceptance> = acceptances
            .iter()
            .filter(|acceptance| acceptance.payload().accepted)
            .map(|acceptance| acceptance.payload().clone())
            .collect();
        accepted_acceptances.sort_by_key(|acceptance| acceptance.guardian_id);

        let (encrypted_shares, public_key_package) =
            if accepted_acceptances.len() >= invitation.threshold as usize {
                self.generate_guardian_setup_completion_material(
                    invitation,
                    invitation.threshold,
                    &accepted_acceptances,
                )
                .await?
            } else {
                (Vec::new(), Vec::new())
            };

        build_setup_completion_with_material(
            &invitation.setup_id,
            invitation.threshold,
            acceptances
                .iter()
                .map(|acceptance| acceptance.payload().clone())
                .collect(),
            encrypted_shares,
            public_key_package,
        )
        .map_err(AgentError::invalid)
    }

    async fn generate_guardian_setup_completion_material(
        &self,
        invitation: &GuardianInvitation,
        threshold: u16,
        accepted_acceptances: &[GuardianAcceptance],
    ) -> AgentResult<(Vec<EncryptedKeyShare>, Vec<u8>)> {
        let accepted_count = u16::try_from(accepted_acceptances.len()).map_err(|_| {
            AgentError::invalid("guardian setup accepted guardian count exceeds u16".to_string())
        })?;
        let signing_keys = self
            .effects
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                threshold,
                accepted_count,
            )
            .await
            .map_err(|error| {
                AgentError::effects(format!("guardian setup key generation failed: {error}"))
            })?;
        if signing_keys.public_key_package.is_empty() {
            return Err(AgentError::internal(
                "guardian setup key generation returned an empty public key package".to_string(),
            ));
        }
        if signing_keys.key_packages.len() != accepted_acceptances.len() {
            return Err(AgentError::internal(format!(
                "guardian setup key generation returned {} shares for {} guardians",
                signing_keys.key_packages.len(),
                accepted_acceptances.len()
            )));
        }

        let mut encrypted_shares = Vec::with_capacity(accepted_acceptances.len());
        for (idx, acceptance) in accepted_acceptances.iter().enumerate() {
            let signer_index = u16::try_from(idx + 1).map_err(|_| {
                AgentError::internal("guardian setup signer index overflow".to_string())
            })?;
            encrypted_shares.push(
                encrypt_guardian_share(
                    self.effects.as_ref(),
                    invitation,
                    acceptance,
                    signer_index,
                    &signing_keys.key_packages[idx],
                    &signing_keys.public_key_package,
                )
                .await
                .map_err(|error| {
                    AgentError::effects(format!("guardian setup share encryption failed: {error}"))
                })?,
            );
        }

        Ok((encrypted_shares, signing_keys.public_key_package.clone()))
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
        guardian_private_key: &[u8],
    ) -> AgentResult<GuardianVote> {
        let authority_id = self.handler.authority_context().authority_id();

        let active_role_name = match guardian_index {
            0 => "Guardian1",
            1 => "Guardian2",
            2 => "Guardian3",
            _ => {
                return Err(AgentError::invalid(
                    "Guardian index must be 0, 1, or 2".to_string(),
                ));
            }
        };

        let now_ms = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or_default();

        let vote_timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: now_ms,
            uncertainty: None,
        });
        let vote_signature = sign_guardian_vote(
            self.effects.as_ref(),
            &proposal,
            authority_id,
            approved,
            &vote_timestamp,
            guardian_private_key,
        )
        .await
        .map_err(|error| AgentError::effects(format!("guardian vote signing failed: {error}")))?;

        let vote = GuardianVote {
            change_id: proposal.change_id.clone(),
            guardian_id: authority_id,
            approved,
            vote_signature,
            rationale,
            timestamp: vote_timestamp,
        };

        let session_id = membership_session_id(&proposal.change_id);
        let roles = vec![Self::role(initiator_id, 0), Self::role(authority_id, 0)];
        let peer_roles =
            BTreeMap::from([("ChangeInitiator".to_string(), Self::role(initiator_id, 0))]);
        let manifest = aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::composition_manifest();
        let global_type = aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::global_type();
        let local_types = aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::local_types();

        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                self.effects.clone(),
                session_id,
                roles,
                &manifest,
                active_role_name,
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| AgentError::internal(error.to_string()))?;
            session.queue_send_bytes(to_vec(&vote).map_err(|error| {
                AgentError::internal(format!("guardian vote encode failed: {error}"))
            })?);

            let loop_result = loop {
                let round = session
                    .advance_round(active_role_name, &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
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

            let _ = session.close().await;
            loop_result
        }
        .await;
        result.map(|_| vote)
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
                SessionDelegationTransfer::new(
                    session_id,
                    *previous_guardian,
                    new_profile.authority_id,
                    "guardian_handoff",
                )
                .with_context(context_id),
            )
            .await
            .map(|_| ())
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

fn recovery_role(authority_id: AuthorityId, role_index: u16) -> ChoreographicRole {
    ChoreographicRole::for_authority(
        authority_id,
        RoleIndex::new(role_index.into()).expect("role index"),
    )
}

async fn execute_recovery_protocol_account(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    let session_id = recovery_session_id(&request.recovery_id, &guardian_id);
    let roles = vec![
        recovery_role(authority_id, 0),
        recovery_role(authority_id, 1),
        recovery_role(guardian_id, 0),
    ];
    let peer_roles = BTreeMap::from([("Coordinator".to_string(), recovery_role(authority_id, 1))]);
    let manifest =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::composition_manifest();
    let global_type =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::global_type();
    let local_types =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::local_types();

    let result = async {
        let mut session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id,
            roles,
            &manifest,
            "Account",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(|error| AgentError::internal(error.to_string()))?;
        session.queue_send_bytes(to_vec(&request).map_err(|error| {
            AgentError::internal(format!("recovery request encode failed: {error}"))
        })?);

        let loop_result = loop {
            let round = session
                .advance_round("Account", &peer_roles)
                .await
                .map_err(|error| AgentError::internal(error.to_string()))?;

            match crate::runtime::handle_owned_vm_round(&mut session, round, "recovery account VM")
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

async fn execute_recovery_protocol_coordinator(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    let session_id = recovery_session_id(&request.recovery_id, &guardian_id);
    let roles = vec![
        recovery_role(authority_id, 0),
        recovery_role(authority_id, 1),
        recovery_role(guardian_id, 0),
    ];
    let peer_roles = BTreeMap::from([
        ("Account".to_string(), recovery_role(authority_id, 0)),
        ("Guardian".to_string(), recovery_role(guardian_id, 0)),
    ]);
    let manifest =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::composition_manifest();
    let global_type =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::global_type();
    let local_types =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::local_types();

    let result = async {
        let mut session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id,
            roles,
            &manifest,
            "Coordinator",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(|error| AgentError::internal(error.to_string()))?;
        session.queue_send_bytes(to_vec(&request).map_err(|error| {
            AgentError::internal(format!("recovery request encode failed: {error}"))
        })?);
        let mut approvals = Vec::new();

        let loop_result = loop {
            let round = session
                .advance_round("Coordinator", &peer_roles)
                .await
                .map_err(|error| AgentError::internal(error.to_string()))?;

            if let Some(blocked) = round.blocked_receive {
                let approval: ProtocolGuardianApproval =
                    from_slice(&blocked.payload).map_err(|error| {
                        AgentError::internal(format!("guardian approval decode failed: {error}"))
                    })?;
                let approval_guardian = approval.guardian_id;
                let approval_context = approval.recovery_id.to_string();
                let approval_member = approval_guardian == guardian_id;
                let approval_has_proof = approval.signature.0.iter().any(|byte| *byte != 0);
                let approval = RecoveryServiceApi::verified_recovery_session_payload(
                    approval_guardian,
                    approval_context.as_bytes(),
                    approval,
                    approval_member,
                    approval_has_proof,
                )?;
                approvals.push(approval);
                let outcome_approvals: Vec<ProtocolGuardianApproval> = approvals
                    .iter()
                    .map(|approval| approval.payload().clone())
                    .collect();
                session.queue_send_bytes(
                    to_vec(&RecoveryOutcome {
                        success: true,
                        recovery_grant: None,
                        error: None,
                        approvals: outcome_approvals,
                    })
                    .map_err(|error| {
                        AgentError::internal(format!("recovery outcome encode failed: {error}"))
                    })?,
                );
                session
                    .inject_blocked_receive(&blocked)
                    .map_err(|error| AgentError::internal(error.to_string()))?;
                continue;
            }

            match round.host_wait_status {
                AuraVmHostWaitStatus::Idle => {}
                AuraVmHostWaitStatus::TimedOut => {
                    break Err(AgentError::internal(
                        "recovery coordinator VM timed out while waiting for receive".to_string(),
                    ));
                }
                AuraVmHostWaitStatus::Cancelled => {
                    break Err(AgentError::internal(
                        "recovery coordinator VM cancelled while waiting for receive".to_string(),
                    ));
                }
                AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
            }

            match round.step {
                StepResult::AllDone => break Ok(()),
                StepResult::Continue => {}
                StepResult::Stuck => {
                    break Err(AgentError::internal(
                        "recovery coordinator VM became stuck without a pending receive"
                            .to_string(),
                    ));
                }
            }
        };

        let _ = session.close().await;
        loop_result
    }
    .await;
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

fn trusted_guardian_vote_keys(
    guardians: &[AuthorityId],
    guardian_verifying_keys: &BTreeMap<AuthorityId, Vec<u8>>,
) -> AgentResult<TrustedKeyResolutionService> {
    let resolver = TrustedKeyResolutionService::new();
    for guardian in guardians {
        let key = guardian_verifying_keys.get(guardian).ok_or_else(|| {
            AgentError::invalid(format!("missing trusted guardian vote key for {guardian}"))
        })?;
        resolver
            .register_guardian_key(*guardian, key.clone())
            .map_err(|error| {
                AgentError::invalid(format!(
                    "invalid trusted guardian vote key for {guardian}: {error}"
                ))
            })?;
    }
    Ok(resolver)
}

fn validate_guardian_setup_inputs(guardians: &[AuthorityId], threshold: u16) -> AgentResult<()> {
    validate_setup_inputs(guardians, threshold).map_err(AgentError::invalid)
}

fn physical_timestamp_ms(timestamp: &TimeStamp) -> AgentResult<u64> {
    match timestamp {
        TimeStamp::PhysicalClock(physical) => Ok(physical.ts_ms),
        other => Err(AgentError::invalid(format!(
            "guardian setup requires physical timestamps, got {other:?}"
        ))),
    }
}

fn guardian_ceremony_commit_certificate_storage_key(
    ceremony_id: aura_recovery::CeremonyId,
) -> String {
    format!(
        "recovery_guardian_ceremony_commit_certificates/{}",
        hex::encode(ceremony_id.0 .0)
    )
}

#[cfg(test)]
mod tests {
    use super::super::recovery::{
        recovery_guardian_private_key_storage_key, recovery_guardian_public_key_storage_key,
    };
    use super::*;
    use crate::core::config::StorageConfig;
    use crate::core::AgentConfig;
    use aura_core::effects::StorageCoreEffects;
    use aura_recovery::guardian_ceremony::{
        build_guardian_ceremony_commit_certificate, decrypt_ceremony_key_package,
        sign_guardian_ceremony_response_with_context, verify_guardian_ceremony_commit_certificate,
        CeremonyCommitCertificate,
    };
    use aura_recovery::recovery_approval::{
        recovery_operation_hash, RecoveryApprovalTranscript, RecoveryApprovalTranscriptPayload,
    };

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
    }

    fn isolated_test_config() -> (tempfile::TempDir, AgentConfig) {
        let temp = tempfile::tempdir().unwrap();
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: temp.path().join("aura"),
                ..Default::default()
            },
            ..Default::default()
        };
        (temp, config)
    }

    async fn signed_test_approval(
        effects: &AuraEffectSystem,
        request: &RecoveryRequest,
        guardian_id: AuthorityId,
        approved_at: u64,
    ) -> GuardianApproval {
        let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
        effects
            .store(
                &recovery_guardian_public_key_storage_key(guardian_id),
                public_key,
            )
            .await
            .unwrap();
        let operation_hash = recovery_operation_hash(&request.operation).unwrap();
        let transcript = RecoveryApprovalTranscript::new(RecoveryApprovalTranscriptPayload {
            recovery_id: request.recovery_id.clone(),
            account_authority: request.account_authority,
            operation_hash,
            prestate_hash: request.prestate_hash,
            approved: true,
            approved_at_ms: approved_at,
            guardian_id,
        });
        let signature = aura_signature::sign_ed25519_transcript(effects, &transcript, &private_key)
            .await
            .unwrap();
        GuardianApproval {
            recovery_id: request.recovery_id.clone(),
            guardian_id,
            signature,
            share_data: None,
            approved_at,
            prestate_hash: request.prestate_hash,
        }
    }

    async fn store_guardian_setup_signing_keypair(
        effects: &AuraEffectSystem,
        guardian_id: AuthorityId,
    ) -> (Vec<u8>, Vec<u8>) {
        let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
        effects
            .store(
                &recovery_guardian_private_key_storage_key(guardian_id),
                private_key.clone(),
            )
            .await
            .unwrap();
        effects
            .store(
                &recovery_guardian_public_key_storage_key(guardian_id),
                public_key.clone(),
            )
            .await
            .unwrap();
        (private_key, public_key)
    }

    async fn signed_guardian_setup_acceptance(
        effects: &AuraEffectSystem,
        invitation: &GuardianInvitation,
        guardian_id: AuthorityId,
        accepted: bool,
        guardian_private_key: &[u8],
        acceptance_timestamp_ms: u64,
        public_key: Vec<u8>,
    ) -> GuardianAcceptance {
        let timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: acceptance_timestamp_ms,
            uncertainty: None,
        });
        let signature = sign_guardian_setup_acceptance(
            effects,
            invitation,
            guardian_id,
            accepted,
            &public_key,
            &timestamp,
            guardian_private_key,
        )
        .await
        .unwrap();
        GuardianAcceptance {
            guardian_id,
            setup_id: invitation.setup_id.clone(),
            account_id: invitation.account_id,
            accepted,
            public_key,
            timestamp,
            signature,
        }
    }

    #[tokio::test]
    async fn test_recovery_service_creation() {
        let authority_context = create_test_authority(150);
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let service = RecoveryServiceApi::new_for_test(effects, authority_context);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_add_device_recovery() {
        let authority_context = create_test_authority(151);
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();

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
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();

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
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects, authority_context).unwrap();

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
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects, authority_context).unwrap();

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
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();

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
        let approval = signed_test_approval(&effects, &request, guardians[0], 12345).await;
        service.submit_approval(approval).await.unwrap();

        // Complete
        let result = service.complete(&request.recovery_id).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_list_active() {
        let authority_context = create_test_authority(166);
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects, authority_context).unwrap();

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
    async fn test_expired_recovery_cleanup_cancels_and_marks_ceremony_failed() {
        let authority_context = create_test_authority(169);
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects, authority_context).unwrap();

        let guardians = vec![AuthorityId::new_from_entropy([170u8; 32])];
        let request = service
            .add_device(
                vec![0u8; 32],
                guardians,
                1,
                "Expiring recovery".to_string(),
                Some(0),
            )
            .await
            .unwrap();

        let removed = service.cleanup_expired_recoveries_once().await;
        assert_eq!(removed, 1);
        assert!(service.get_state(&request.recovery_id).await.is_none());

        let ceremony_status = service
            .ceremony_runner
            .status(&CeremonyId::new(request.recovery_id.to_string()))
            .await
            .unwrap();
        assert!(ceremony_status.is_aborted());
        match ceremony_status.state {
            aura_core::domain::CeremonyState::Aborted { reason, .. } => {
                assert_eq!(reason, RECOVERY_TIMEOUT_REASON);
            }
            state => panic!("expected aborted ceremony state, got {state:?}"),
        }
    }

    #[tokio::test]
    async fn test_membership_change_requires_three_guardians() {
        let authority_context = create_test_authority(170);
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects, authority_context).unwrap();

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
                BTreeMap::new(),
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
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects, authority_context).unwrap();

        let proposal = MembershipProposal {
            change_id: "test-change-123".to_string(),
            account_id: AuthorityId::new_from_entropy([175u8; 32]),
            proposer_id: AuthorityId::new_from_entropy([176u8; 32]),
            change: MembershipChange::AddGuardian {
                guardian: GuardianProfile::new(AuthorityId::new_from_entropy([177u8; 32])),
            },
            threshold: 2,
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
                &[],
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Guardian index must be 0, 1, or 2"));
    }

    #[tokio::test]
    async fn test_membership_vote_verification_rejects_duplicate_guardian_vote() {
        let authority_context = create_test_authority(180);
        let proposer = authority_context.authority_id();
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();
        let guardian = AuthorityId::new_from_entropy([181u8; 32]);
        let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
        let proposal = MembershipProposal {
            change_id: "test-change-duplicate".to_string(),
            account_id: AuthorityId::new_from_entropy([182u8; 32]),
            proposer_id: proposer,
            change: MembershipChange::AddGuardian {
                guardian: GuardianProfile::new(AuthorityId::new_from_entropy([183u8; 32])),
            },
            threshold: 2,
            new_threshold: None,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        };
        let vote_timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1001,
            uncertainty: None,
        });
        let signature = sign_guardian_vote(
            effects.as_ref(),
            &proposal,
            guardian,
            true,
            &vote_timestamp,
            &private_key,
        )
        .await
        .unwrap();
        let vote = GuardianVote {
            change_id: proposal.change_id.clone(),
            guardian_id: guardian,
            approved: true,
            vote_signature: signature,
            rationale: "approved".to_string(),
            timestamp: vote_timestamp,
        };
        let guardians = vec![guardian];
        let verifying_keys = BTreeMap::from([(guardian, public_key)]);
        let key_resolver =
            trusted_guardian_vote_keys(&guardians, &verifying_keys).expect("trusted key resolver");
        let mut seen = BTreeSet::new();

        service
            .verify_membership_vote_for_proposal(
                &proposal,
                &guardians,
                &key_resolver,
                &mut seen,
                vote.clone(),
            )
            .await
            .expect("first vote verifies");
        let duplicate = service
            .verify_membership_vote_for_proposal(
                &proposal,
                &guardians,
                &key_resolver,
                &mut seen,
                vote,
            )
            .await
            .unwrap_err();

        assert!(duplicate.to_string().contains("duplicate guardian"));
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
        let (_temp, config) = isolated_test_config();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, account_authority)
                .unwrap(),
        );
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();

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

    #[tokio::test]
    async fn guardian_setup_acceptance_rejects_duplicate_guardians() {
        let authority_context = create_test_authority(210);
        let account_authority = authority_context.authority_id();
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();
        let now_ms = effects.physical_time().await.unwrap().ts_ms;
        let guardians = vec![
            AuthorityId::new_from_entropy([211u8; 32]),
            AuthorityId::new_from_entropy([212u8; 32]),
            AuthorityId::new_from_entropy([213u8; 32]),
        ];
        let invitation = GuardianInvitation {
            setup_id: "setup-dup".to_string(),
            account_id: account_authority,
            target_guardians: guardians.clone(),
            threshold: 2,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: now_ms.saturating_sub(10),
                uncertainty: None,
            }),
        };
        let (guardian_private_key, _) =
            store_guardian_setup_signing_keypair(&effects, guardians[0]).await;
        let _ = store_guardian_setup_signing_keypair(&effects, guardians[1]).await;
        let _ = store_guardian_setup_signing_keypair(&effects, guardians[2]).await;
        let resolver = service
            .trusted_guardian_setup_keys(&guardians)
            .await
            .unwrap();
        let (_, valid_public_key) = effects.ed25519_generate_keypair().await.unwrap();
        let first = signed_guardian_setup_acceptance(
            &effects,
            &invitation,
            guardians[0],
            true,
            &guardian_private_key,
            now_ms,
            valid_public_key.clone(),
        )
        .await;
        let second = signed_guardian_setup_acceptance(
            &effects,
            &invitation,
            guardians[0],
            true,
            &guardian_private_key,
            now_ms,
            valid_public_key,
        )
        .await;

        let mut seen = BTreeSet::new();
        service
            .verify_guardian_setup_acceptance_for_invitation(
                &invitation,
                &guardians,
                &resolver,
                &mut seen,
                first,
            )
            .await
            .unwrap();
        let error = service
            .verify_guardian_setup_acceptance_for_invitation(
                &invitation,
                &guardians,
                &resolver,
                &mut seen,
                second,
            )
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("duplicate guardian setup acceptance"));
    }

    #[tokio::test]
    async fn guardian_setup_acceptance_rejects_forged_and_malformed_payloads() {
        let authority_context = create_test_authority(214);
        let account_authority = authority_context.authority_id();
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();
        let now_ms = effects.physical_time().await.unwrap().ts_ms;
        let guardians = vec![
            AuthorityId::new_from_entropy([215u8; 32]),
            AuthorityId::new_from_entropy([216u8; 32]),
            AuthorityId::new_from_entropy([217u8; 32]),
        ];
        let invitation = GuardianInvitation {
            setup_id: "setup-forged".to_string(),
            account_id: account_authority,
            target_guardians: guardians.clone(),
            threshold: 2,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: now_ms.saturating_sub(10),
                uncertainty: None,
            }),
        };
        let (guardian_private_key, _) =
            store_guardian_setup_signing_keypair(&effects, guardians[0]).await;
        let (other_private_key, _) =
            store_guardian_setup_signing_keypair(&effects, guardians[1]).await;
        let _ = store_guardian_setup_signing_keypair(&effects, guardians[2]).await;
        let resolver = service
            .trusted_guardian_setup_keys(&guardians)
            .await
            .unwrap();

        let forged = signed_guardian_setup_acceptance(
            &effects,
            &invitation,
            guardians[0],
            true,
            &other_private_key,
            now_ms,
            vec![8u8; 32],
        )
        .await;
        let malformed = signed_guardian_setup_acceptance(
            &effects,
            &invitation,
            guardians[0],
            true,
            &guardian_private_key,
            now_ms,
            vec![1, 2, 3],
        )
        .await;

        let mut seen = BTreeSet::new();
        assert!(service
            .verify_guardian_setup_acceptance_for_invitation(
                &invitation,
                &guardians,
                &resolver,
                &mut seen,
                forged,
            )
            .await
            .is_err());
        let mut seen = BTreeSet::new();
        let error = service
            .verify_guardian_setup_acceptance_for_invitation(
                &invitation,
                &guardians,
                &resolver,
                &mut seen,
                malformed,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("public key is malformed"));
    }

    #[tokio::test]
    async fn guardian_setup_builds_verified_completion_with_crypto_material() {
        let authority_context = create_test_authority(218);
        let account_authority = authority_context.authority_id();
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();
        let now_ms = effects.physical_time().await.unwrap().ts_ms;
        let guardians = vec![
            AuthorityId::new_from_entropy([219u8; 32]),
            AuthorityId::new_from_entropy([220u8; 32]),
            AuthorityId::new_from_entropy([221u8; 32]),
        ];
        let invitation = GuardianInvitation {
            setup_id: "setup-success".to_string(),
            account_id: account_authority,
            target_guardians: guardians.clone(),
            threshold: 2,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: now_ms.saturating_sub(10),
                uncertainty: None,
            }),
        };
        let (g1_private, _) = store_guardian_setup_signing_keypair(&effects, guardians[0]).await;
        let (g2_private, _) = store_guardian_setup_signing_keypair(&effects, guardians[1]).await;
        let (g3_private, _) = store_guardian_setup_signing_keypair(&effects, guardians[2]).await;
        let resolver = service
            .trusted_guardian_setup_keys(&guardians)
            .await
            .unwrap();
        let (_, g1_public) = effects.ed25519_generate_keypair().await.unwrap();
        let (_, g2_public) = effects.ed25519_generate_keypair().await.unwrap();
        let (_, g3_public) = effects.ed25519_generate_keypair().await.unwrap();

        let acceptance_1 = signed_guardian_setup_acceptance(
            &effects,
            &invitation,
            guardians[0],
            true,
            &g1_private,
            now_ms,
            g1_public,
        )
        .await;
        let acceptance_2 = signed_guardian_setup_acceptance(
            &effects,
            &invitation,
            guardians[1],
            true,
            &g2_private,
            now_ms,
            g2_public,
        )
        .await;
        let acceptance_3 = signed_guardian_setup_acceptance(
            &effects,
            &invitation,
            guardians[2],
            false,
            &g3_private,
            now_ms,
            g3_public,
        )
        .await;

        let mut seen = BTreeSet::new();
        let verified = vec![
            service
                .verify_guardian_setup_acceptance_for_invitation(
                    &invitation,
                    &guardians,
                    &resolver,
                    &mut seen,
                    acceptance_1,
                )
                .await
                .unwrap(),
            service
                .verify_guardian_setup_acceptance_for_invitation(
                    &invitation,
                    &guardians,
                    &resolver,
                    &mut seen,
                    acceptance_2,
                )
                .await
                .unwrap(),
            service
                .verify_guardian_setup_acceptance_for_invitation(
                    &invitation,
                    &guardians,
                    &resolver,
                    &mut seen,
                    acceptance_3,
                )
                .await
                .unwrap(),
        ];

        let completion = service
            .build_verified_guardian_setup_completion(&invitation, &verified)
            .await
            .unwrap();
        assert!(completion.success);
        assert_eq!(completion.encrypted_shares.len(), 2);
        assert!(!completion.public_key_package.is_empty());
        let share_guardians: BTreeSet<_> = completion
            .encrypted_shares
            .iter()
            .map(|share| share.guardian_id)
            .collect();
        assert_eq!(
            share_guardians,
            BTreeSet::from([guardians[0], guardians[1]])
        );
    }

    #[tokio::test]
    async fn guardian_ceremony_proposal_encryption_replaces_raw_key_packages() {
        let authority_context = create_test_authority(222);
        let initiator_id = authority_context.authority_id();
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();
        let guardian_id = AuthorityId::new_from_entropy([223u8; 32]);
        let (guardian_private_key, guardian_public_key) =
            store_guardian_setup_signing_keypair(&effects, guardian_id).await;
        let ceremony_id = aura_recovery::CeremonyId(Hash32([0x81; 32]));
        let prestate_hash = Hash32([0x82; 32]);
        let operation = GuardianRotationOp {
            threshold_k: 1,
            total_n: 1,
            guardian_ids: vec![guardian_id],
            new_epoch: 9,
        };
        let key_package = vec![0xA5; 48];

        let proposal = service
            .build_guardian_ceremony_proposal(
                guardian_id,
                ceremony_id,
                prestate_hash,
                &operation,
                &key_package,
            )
            .await
            .unwrap();

        assert_ne!(proposal.encrypted_key_package, key_package);
        assert_eq!(proposal.recipient_public_key, guardian_public_key);
        assert_eq!(proposal.initiator_id, initiator_id);
        let decrypted =
            decrypt_ceremony_key_package(effects.as_ref(), &proposal, &guardian_private_key)
                .await
                .unwrap();
        assert_eq!(decrypted, key_package);
    }

    #[tokio::test]
    async fn guardian_ceremony_commit_certificate_is_persisted_and_verifiable() {
        let authority_context = create_test_authority(224);
        let initiator_id = authority_context.authority_id();
        let (_temp, config) = isolated_test_config();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let service = RecoveryServiceApi::new_for_test(effects.clone(), authority_context).unwrap();
        let guardians = vec![
            AuthorityId::new_from_entropy([225u8; 32]),
            AuthorityId::new_from_entropy([226u8; 32]),
        ];
        let ceremony_id = aura_recovery::CeremonyId(Hash32([0x83; 32]));
        let prestate_hash = Hash32([0x84; 32]);
        let operation = GuardianRotationOp {
            threshold_k: 2,
            total_n: 2,
            guardian_ids: guardians.clone(),
            new_epoch: 10,
        };
        let (g1_private_key, _) =
            store_guardian_setup_signing_keypair(&effects, guardians[0]).await;
        let (g2_private_key, _) =
            store_guardian_setup_signing_keypair(&effects, guardians[1]).await;
        let resolver = service
            .trusted_guardian_setup_keys(&guardians)
            .await
            .unwrap();

        let response_1 = CeremonyResponseMsg {
            ceremony_id,
            guardian_id: guardians[0],
            response: CeremonyResponse::Accept,
            encrypted_key_package_hash: Hash32([0x91; 32]),
            signature: sign_guardian_ceremony_response_with_context(
                effects.as_ref(),
                ceremony_id,
                initiator_id,
                prestate_hash,
                &operation,
                guardians[0],
                CeremonyResponse::Accept,
                Hash32([0x91; 32]),
                &g1_private_key,
            )
            .await
            .unwrap(),
        };
        let response_2 = CeremonyResponseMsg {
            ceremony_id,
            guardian_id: guardians[1],
            response: CeremonyResponse::Accept,
            encrypted_key_package_hash: Hash32([0x92; 32]),
            signature: sign_guardian_ceremony_response_with_context(
                effects.as_ref(),
                ceremony_id,
                initiator_id,
                prestate_hash,
                &operation,
                guardians[1],
                CeremonyResponse::Accept,
                Hash32([0x92; 32]),
                &g2_private_key,
            )
            .await
            .unwrap(),
        };

        let certificate = build_guardian_ceremony_commit_certificate(
            effects.as_ref(),
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            &guardians,
            operation.threshold_k,
            &[response_1, response_2],
            &resolver,
        )
        .await
        .unwrap();
        service
            .store_guardian_ceremony_commit_certificate(&certificate)
            .await
            .unwrap();

        let stored = effects
            .retrieve(&guardian_ceremony_commit_certificate_storage_key(
                ceremony_id,
            ))
            .await
            .unwrap()
            .unwrap();
        let stored: CeremonyCommitCertificate = from_slice(&stored).unwrap();
        assert!(verify_guardian_ceremony_commit_certificate(
            effects.as_ref(),
            &stored,
            &guardians,
            operation.threshold_k,
            &resolver,
        )
        .await
        .unwrap());
    }
}
