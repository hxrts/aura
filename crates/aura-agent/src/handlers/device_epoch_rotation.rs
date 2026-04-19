use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::ceremony_runner::{CeremonyCommitMetadata, CeremonyRunner};
use crate::runtime::services::{CeremonyTracker, ReconfigurationManager};
use crate::runtime::vm_host_bridge::AuraVmRoundDisposition;
use crate::runtime::{
    handle_owned_vm_round, open_owned_manifest_vm_session_admitted, RuntimeChoreographySessionId,
    SessionIngressError,
};
use crate::{AgentError, AgentResult, ThresholdSigningService};
use aura_core::crypto::tree_signing::{
    public_key_package_from_bytes, share_from_key_package_bytes,
};
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{
    PhysicalTimeEffects, SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    ThresholdSigningEffects, TransportEffects, TransportError,
};
use aura_core::threshold::{ParticipantIdentity, SigningContext};
use aura_core::tree::metadata::DeviceLeafMetadata;
use aura_core::tree::LeafRole;
use aura_core::types::identifiers::CeremonyId;
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::{hash, AttestedOp, AuthorityId, DeviceId, LeafId, LeafNode, NodeIndex, TreeOp};
use aura_protocol::effects::{ChoreographicRole, RoleIndex, TreeEffects};
use aura_sync::protocols::{
    DeviceEpochAcceptance, DeviceEpochCommit, DeviceEpochProposal, DeviceEpochRotationKind,
};
use std::collections::BTreeMap;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

const PROTOCOL_ID: &str = "aura.sync.device_epoch_rotation";
const COMMIT_STORAGE_NAMESPACE: &str = "device_epoch_rotation_commit";
const COMMIT_STATUS_POLL_MS: u64 = 100;
const COMMIT_STATUS_TIMEOUT_MS: u64 = 10_000;

#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct DeviceEpochRotationInitRequest {
    #[zeroize(skip)]
    pub ceremony_id: CeremonyId,
    #[zeroize(skip)]
    pub kind: DeviceEpochRotationKind,
    #[zeroize(skip)]
    pub pending_epoch: u64,
    #[zeroize(skip)]
    pub participant_device_id: DeviceId,
    /// Security-sensitive serialized key package. Zeroized on drop.
    pub key_package: Vec<u8>,
    /// Security-sensitive serialized threshold configuration. Zeroized on drop.
    pub threshold_config: Vec<u8>,
    /// Device-epoch public key package retained with the secret material and
    /// cleared on drop with the rest of the ceremony payload.
    pub public_key_package: Vec<u8>,
}

#[derive(Clone)]
pub struct DeviceEpochRotationService {
    authority_id: AuthorityId,
    effects: std::sync::Arc<AuraEffectSystem>,
    ceremony_tracker: CeremonyTracker,
    ceremony_runner: CeremonyRunner,
    signing_service: ThresholdSigningService,
    reconfiguration: ReconfigurationManager,
}

impl DeviceEpochRotationService {
    pub fn new(
        authority_id: AuthorityId,
        effects: std::sync::Arc<AuraEffectSystem>,
        ceremony_tracker: CeremonyTracker,
        ceremony_runner: CeremonyRunner,
        signing_service: ThresholdSigningService,
        reconfiguration: ReconfigurationManager,
    ) -> Self {
        Self {
            authority_id,
            effects,
            ceremony_tracker,
            ceremony_runner,
            signing_service,
            reconfiguration,
        }
    }

    pub async fn execute_initiator(
        self,
        request: DeviceEpochRotationInitRequest,
    ) -> AgentResult<()> {
        let initiator_device_id = self.effects.device_id();
        let participant_role = role(self.authority_id, request.participant_device_id, 1);
        let roles = vec![
            role(self.authority_id, initiator_device_id, 0),
            participant_role,
        ];
        let peer_roles = BTreeMap::from([("Participant".to_string(), participant_role)]);
        let session_uuid =
            device_epoch_rotation_session_id(&request.ceremony_id, request.participant_device_id);
        let manifest =
            aura_sync::protocols::device_epoch_rotation::telltale_session_types_device_epoch_rotation::vm_artifacts::composition_manifest();
        let global_type =
            aura_sync::protocols::device_epoch_rotation::telltale_session_types_device_epoch_rotation::vm_artifacts::global_type();
        let local_types =
            aura_sync::protocols::device_epoch_rotation::telltale_session_types_device_epoch_rotation::vm_artifacts::local_types();
        let proposal = DeviceEpochProposal {
            ceremony_id: request.ceremony_id.clone(),
            kind: request.kind,
            subject_authority: self.authority_id,
            pending_epoch: request.pending_epoch,
            initiator_device_id,
            participant_device_id: request.participant_device_id,
            key_package: request.key_package.clone(),
            threshold_config: request.threshold_config.clone(),
            public_key_package: request.public_key_package.clone(),
        };

        let mut session = open_owned_manifest_vm_session_admitted(
            self.effects.clone(),
            session_uuid,
            roles,
            &manifest,
            "Initiator",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(map_session_error)?;
        session.queue_send_bytes(to_vec(&proposal).map_err(map_encode_error)?);

        let mut acceptance: Option<DeviceEpochAcceptance> = None;
        loop {
            let round = session
                .advance_round("Initiator", &peer_roles)
                .await
                .map_err(map_internal_error)?;

            if let Some(blocked) = round.blocked_receive {
                let decoded: DeviceEpochAcceptance =
                    from_slice(&blocked.payload).map_err(map_decode_error)?;
                acceptance = Some(decoded.clone());
                let threshold_reached = self
                    .ceremony_runner
                    .record_response(
                        &request.ceremony_id,
                        ParticipantIdentity::device(decoded.acceptor_device_id),
                    )
                    .await
                    .map_err(map_internal_error)?;
                session
                    .inject_blocked_receive(&blocked)
                    .map_err(map_internal_error)?;

                let commit = if threshold_reached {
                    self.coordinate_commit(&request).await?
                } else {
                    self.wait_for_commit(&request.ceremony_id).await?
                };
                session.queue_send_bytes(to_vec(&commit).map_err(map_encode_error)?);
                continue;
            }

            match handle_owned_vm_round(&mut session, round, "device epoch rotation initiator VM")
                .map_err(map_internal_error)?
            {
                AuraVmRoundDisposition::Continue => {}
                AuraVmRoundDisposition::Complete => break,
            }
        }

        let _ = session.close().await;

        if acceptance.is_some() {
            self.record_native_session(session_uuid).await;
        }

        Ok(())
    }

    pub async fn process_pending_participant_sessions(&self) -> AgentResult<(usize, usize)> {
        let mut processed = 0usize;
        let mut completed = 0usize;

        loop {
            let envelope = match self.effects.receive_envelope().await {
                Ok(envelope) => envelope,
                Err(TransportError::NoMessage) => break,
                Err(error) => return Err(AgentError::internal(error.to_string())),
            };

            if !is_device_epoch_rotation_envelope(&envelope) {
                self.effects.requeue_envelope(envelope);
                break;
            }

            processed += 1;
            if self.execute_participant_from_envelope(envelope).await? {
                completed += 1;
            }
        }

        Ok((processed, completed))
    }

    async fn execute_participant_from_envelope(
        &self,
        envelope: TransportEnvelope,
    ) -> AgentResult<bool> {
        let session_uuid = envelope_session_uuid(&envelope)?;
        let initiator_device_id = envelope_source_device_id(&envelope)?;
        let participant_device_id = self.effects.device_id();
        let roles = vec![
            role(self.authority_id, initiator_device_id, 0),
            role(self.authority_id, participant_device_id, 1),
        ];
        let peer_roles = BTreeMap::from([(
            "Initiator".to_string(),
            role(self.authority_id, initiator_device_id, 0),
        )]);
        let manifest =
            aura_sync::protocols::device_epoch_rotation::telltale_session_types_device_epoch_rotation::vm_artifacts::composition_manifest();
        let global_type =
            aura_sync::protocols::device_epoch_rotation::telltale_session_types_device_epoch_rotation::vm_artifacts::global_type();
        let local_types =
            aura_sync::protocols::device_epoch_rotation::telltale_session_types_device_epoch_rotation::vm_artifacts::local_types();

        let mut session = open_owned_manifest_vm_session_admitted(
            self.effects.clone(),
            session_uuid,
            roles,
            &manifest,
            "Participant",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(|error| match error {
            SessionIngressError::SessionStart { .. } => {
                self.effects.requeue_envelope(envelope.clone());
                map_session_error(error)
            }
            other => map_session_error(other),
        })?;
        self.effects.requeue_envelope(envelope);

        let mut staged_proposal: Option<DeviceEpochProposal> = None;

        loop {
            let round = session
                .advance_round("Participant", &peer_roles)
                .await
                .map_err(map_internal_error)?;

            if let Some(blocked) = round.blocked_receive {
                if staged_proposal.is_none() {
                    let proposal: DeviceEpochProposal =
                        from_slice(&blocked.payload).map_err(map_decode_error)?;
                    self.stage_proposal(&proposal).await?;
                    let acceptance = DeviceEpochAcceptance {
                        ceremony_id: proposal.ceremony_id.clone(),
                        acceptor_device_id: participant_device_id,
                    };
                    session.queue_send_bytes(to_vec(&acceptance).map_err(map_encode_error)?);
                    staged_proposal = Some(proposal);
                } else {
                    let commit: DeviceEpochCommit =
                        from_slice(&blocked.payload).map_err(map_decode_error)?;
                    self.apply_commit(&commit).await?;
                    self.record_native_session(session_uuid).await;
                }
                session
                    .inject_blocked_receive(&blocked)
                    .map_err(map_internal_error)?;
                continue;
            }

            match handle_owned_vm_round(&mut session, round, "device epoch rotation participant VM")
                .map_err(map_internal_error)?
            {
                AuraVmRoundDisposition::Continue => {}
                AuraVmRoundDisposition::Complete => {
                    let _ = session.close().await;
                    return Ok(staged_proposal.is_some());
                }
            }
        }
    }

    async fn coordinate_commit(
        &self,
        request: &DeviceEpochRotationInitRequest,
    ) -> AgentResult<DeviceEpochCommit> {
        let commit = match request.kind {
            DeviceEpochRotationKind::Enrollment => {
                let attested_leaf_op = self.finalize_enrollment(&request.ceremony_id).await?;
                self.commit_local_rotation(&request.ceremony_id).await?;
                DeviceEpochCommit {
                    ceremony_id: request.ceremony_id.clone(),
                    new_epoch: request.pending_epoch,
                    attested_leaf_op,
                }
            }
            DeviceEpochRotationKind::Rotation | DeviceEpochRotationKind::Removal => {
                self.commit_local_rotation(&request.ceremony_id).await?;
                DeviceEpochCommit {
                    ceremony_id: request.ceremony_id.clone(),
                    new_epoch: request.pending_epoch,
                    attested_leaf_op: None,
                }
            }
        };

        self.store_commit(&commit).await?;
        self.ceremony_runner
            .commit(&request.ceremony_id, CeremonyCommitMetadata::default())
            .await
            .map_err(map_internal_error)?;

        Ok(commit)
    }

    async fn wait_for_commit(&self, ceremony_id: &CeremonyId) -> AgentResult<DeviceEpochCommit> {
        let start = self
            .effects
            .physical_time()
            .await
            .map_err(map_internal_error)?
            .ts_ms;

        loop {
            let status = self
                .ceremony_runner
                .status(ceremony_id)
                .await
                .map_err(map_internal_error)?;
            if status.is_committed() {
                return self.load_commit(ceremony_id).await;
            }
            if status.is_terminal() {
                return Err(AgentError::invalid(format!(
                    "ceremony {} reached terminal state {:?} before commit",
                    ceremony_id, status.state
                )));
            }

            let now = self
                .effects
                .physical_time()
                .await
                .map_err(map_internal_error)?
                .ts_ms;
            if now.saturating_sub(start) >= COMMIT_STATUS_TIMEOUT_MS {
                return Err(AgentError::timeout(format!(
                    "timed out waiting for ceremony {} commit publication",
                    ceremony_id
                )));
            }

            self.effects
                .sleep_ms(COMMIT_STATUS_POLL_MS)
                .await
                .map_err(map_internal_error)?;
        }
    }

    async fn stage_proposal(&self, proposal: &DeviceEpochProposal) -> AgentResult<()> {
        let participant = ParticipantIdentity::device(self.effects.device_id());
        let location = SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}/{}", proposal.subject_authority, proposal.pending_epoch),
            participant.storage_key(),
        );

        self.effects
            .secure_store(
                &location,
                &proposal.key_package,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(map_internal_error)?;

        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            proposal.subject_authority.to_string(),
            proposal.pending_epoch.to_string(),
        );
        self.effects
            .secure_store(
                &config_location,
                &proposal.threshold_config,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(map_internal_error)?;

        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            proposal.subject_authority.to_string(),
            proposal.pending_epoch.to_string(),
        );
        self.effects
            .secure_store(
                &pubkey_location,
                &proposal.public_key_package,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(map_internal_error)?;

        Ok(())
    }

    async fn apply_commit(&self, commit: &DeviceEpochCommit) -> AgentResult<()> {
        if let Some(attested_op) = commit.attested_leaf_op.clone() {
            self.effects
                .apply_attested_op(attested_op)
                .await
                .map_err(map_internal_error)?;
        }

        self.effects
            .commit_key_rotation(&self.authority_id, commit.new_epoch)
            .await
            .map_err(map_internal_error)?;
        self.signing_service
            .commit_key_rotation(&self.authority_id, commit.new_epoch)
            .await
            .map_err(map_internal_error)?;

        Ok(())
    }

    async fn finalize_enrollment(
        &self,
        ceremony_id: &CeremonyId,
    ) -> AgentResult<Option<AttestedOp>> {
        let ceremony_state = self
            .ceremony_tracker
            .get(ceremony_id)
            .await
            .map_err(map_internal_error)?;

        let Some(device_id) = ceremony_state.enrollment_device_id else {
            return Ok(None);
        };

        let tree_state = self
            .effects
            .get_current_state()
            .await
            .map_err(map_internal_error)?;

        if tree_state
            .leaves
            .values()
            .any(|leaf| leaf.device_id == device_id)
        {
            return Ok(None);
        }

        let participant = ParticipantIdentity::device(device_id);
        let key_location = SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}/{}", self.authority_id, ceremony_state.new_epoch),
            participant.storage_key(),
        );
        let key_package = self
            .effects
            .secure_retrieve(&key_location, &[SecureStorageCapability::Read])
            .await
            .map_err(map_internal_error)?;
        let share = share_from_key_package_bytes(&key_package).map_err(map_internal_error)?;

        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            self.authority_id.to_string(),
            ceremony_state.new_epoch.to_string(),
        );
        let pubkey_bytes = self
            .effects
            .secure_retrieve(&pubkey_location, &[SecureStorageCapability::Read])
            .await
            .map_err(map_internal_error)?;
        let public_key_package =
            public_key_package_from_bytes(&pubkey_bytes).map_err(map_internal_error)?;
        let public_key_bytes = public_key_package
            .signer_public_keys
            .get(&share.identifier)
            .cloned()
            .ok_or_else(|| AgentError::internal("missing verifying share for enrollment signer"))?;

        let next_leaf_id = tree_state
            .leaves
            .keys()
            .map(|leaf_id| leaf_id.0)
            .max()
            .map(|id| id + 1)
            .unwrap_or(0);
        let metadata = ceremony_state
            .enrollment_nickname_suggestion
            .as_ref()
            .map(DeviceLeafMetadata::with_nickname_suggestion)
            .unwrap_or_else(DeviceLeafMetadata::new)
            .encode()
            .map_err(map_internal_error)?;
        let leaf = LeafNode::new(
            LeafId(next_leaf_id),
            device_id,
            LeafRole::Device,
            public_key_bytes,
            metadata,
        )
        .map_err(map_internal_error)?;
        let op_kind = self
            .effects
            .add_leaf(leaf, NodeIndex(0))
            .await
            .map_err(map_internal_error)?;
        let op = TreeOp {
            parent_epoch: tree_state.epoch,
            parent_commitment: tree_state.root_commitment,
            op: op_kind,
            version: 1,
        };
        let signature = self
            .signing_service
            .sign(SigningContext::self_tree_op(self.authority_id, op.clone()))
            .await
            .map_err(map_internal_error)?;
        let attested = AttestedOp {
            op,
            agg_sig: signature.signature,
            signer_count: signature.signer_count,
        };
        self.effects
            .apply_attested_op(attested.clone())
            .await
            .map_err(map_internal_error)?;
        Ok(Some(attested))
    }

    async fn commit_local_rotation(&self, ceremony_id: &CeremonyId) -> AgentResult<()> {
        let ceremony_state = self
            .ceremony_tracker
            .get(ceremony_id)
            .await
            .map_err(map_internal_error)?;
        self.effects
            .commit_key_rotation(&self.authority_id, ceremony_state.new_epoch)
            .await
            .map_err(map_internal_error)?;
        self.signing_service
            .commit_key_rotation(&self.authority_id, ceremony_state.new_epoch)
            .await
            .map_err(map_internal_error)?;
        Ok(())
    }

    async fn store_commit(&self, commit: &DeviceEpochCommit) -> AgentResult<()> {
        let payload = to_vec(commit).map_err(map_encode_error)?;
        self.effects
            .secure_store(
                &commit_storage_location(self.authority_id, &commit.ceremony_id),
                &payload,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(map_internal_error)?;
        Ok(())
    }

    async fn load_commit(&self, ceremony_id: &CeremonyId) -> AgentResult<DeviceEpochCommit> {
        let bytes = self
            .effects
            .secure_retrieve(
                &commit_storage_location(self.authority_id, ceremony_id),
                &[SecureStorageCapability::Read],
            )
            .await
            .map_err(map_internal_error)?;
        from_slice(&bytes).map_err(map_decode_error)
    }

    async fn record_native_session(&self, session_uuid: Uuid) {
        let session_id =
            RuntimeChoreographySessionId::from_uuid(session_uuid).into_aura_session_id();
        self.reconfiguration
            .record_native_session(self.authority_id, session_id)
            .await;
    }
}

fn role(authority_id: AuthorityId, device_id: DeviceId, role_index: u16) -> ChoreographicRole {
    ChoreographicRole::new(
        device_id,
        authority_id,
        RoleIndex::new(role_index.into()).expect("role index"),
    )
}

fn device_epoch_rotation_session_id(
    ceremony_id: &CeremonyId,
    participant_device_id: DeviceId,
) -> Uuid {
    let mut hasher = hash::hasher();
    hasher.update(PROTOCOL_ID.as_bytes());
    hasher.update(ceremony_id.as_str().as_bytes());
    hasher.update(participant_device_id.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

fn commit_storage_location(
    authority_id: AuthorityId,
    ceremony_id: &CeremonyId,
) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key(
        COMMIT_STORAGE_NAMESPACE,
        authority_id.to_string(),
        ceremony_id.to_string(),
    )
}

fn is_device_epoch_rotation_envelope(envelope: &TransportEnvelope) -> bool {
    envelope
        .metadata
        .get("content-type")
        .is_some_and(|value| value == "application/aura-choreography")
        && envelope
            .metadata
            .get("protocol-id")
            .is_some_and(|value| value == PROTOCOL_ID)
}

fn envelope_session_uuid(envelope: &TransportEnvelope) -> AgentResult<Uuid> {
    let session_id = envelope.metadata.get("session-id").ok_or_else(|| {
        AgentError::internal("missing session-id on device epoch rotation envelope")
    })?;
    Uuid::parse_str(session_id).map_err(map_internal_error)
}

fn envelope_source_device_id(envelope: &TransportEnvelope) -> AgentResult<DeviceId> {
    let source = envelope
        .metadata
        .get("aura-source-device-id")
        .ok_or_else(|| {
            AgentError::internal("missing aura-source-device-id on device epoch rotation envelope")
        })?;
    source.parse().map_err(map_internal_error)
}

fn map_internal_error(error: impl std::fmt::Display) -> AgentError {
    AgentError::internal(error.to_string())
}

fn map_encode_error(error: impl std::fmt::Display) -> AgentError {
    AgentError::internal(format!("device epoch rotation encode failed: {error}"))
}

fn map_decode_error(error: impl std::fmt::Display) -> AgentError {
    AgentError::internal(format!("device epoch rotation decode failed: {error}"))
}

fn map_session_error(error: SessionIngressError) -> AgentError {
    AgentError::internal(format!("device epoch rotation session failed: {error}"))
}
