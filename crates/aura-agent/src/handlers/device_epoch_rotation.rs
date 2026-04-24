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
use aura_core::{
    hash, AttestedOp, AuthorityId, DeviceId, Hash32, LeafId, LeafNode, NodeIndex, TreeOp,
    TrustedKeyDomain, TrustedPublicKey,
};
use aura_protocol::effects::{ChoreographicRole, RoleIndex, TreeEffects};
use aura_protocol::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};
use aura_sync::protocols::device_epoch_rotation::{
    decrypt_device_epoch_key_package, device_epoch_commit_attested_op_hash,
    device_epoch_proposal_hash, encrypt_device_epoch_key_package,
    verify_device_epoch_authority_signature, verify_device_epoch_proposal_hashes,
    DeviceEpochAcceptance, DeviceEpochCommit, DeviceEpochCommitTranscript, DeviceEpochProposal,
    DeviceEpochProposalTranscript, EncryptedDeviceEpochKeyPackage,
};
use aura_sync::protocols::DeviceEpochRotationKind;
use std::{collections::BTreeMap, fmt};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

const PROTOCOL_ID: &str = "aura.sync.device_epoch_rotation";
const COMMIT_STORAGE_NAMESPACE: &str = "device_epoch_rotation_commit";
const COMMIT_STATUS_POLL_MS: u64 = 100;
const COMMIT_STATUS_TIMEOUT_MS: u64 = 10_000;
const PROPOSAL_SIGNING_DOMAIN: &str = "aura.sync.device_epoch_rotation.proposal";
const COMMIT_SIGNING_DOMAIN: &str = "aura.sync.device_epoch_rotation.commit";

#[derive(Zeroize, ZeroizeOnDrop)]
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
    // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md runtime-local ceremony handoff until request envelopes move to SigningShareBytes.
    pub key_package: Vec<u8>,
    /// Security-sensitive serialized threshold configuration. Zeroized on drop.
    // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md runtime-local ceremony handoff until request envelopes move to SecretBytes.
    pub threshold_config: Vec<u8>,
    /// Device-epoch public key package retained with the secret material and
    /// cleared on drop with the rest of the ceremony payload.
    pub public_key_package: Vec<u8>,
}

impl fmt::Debug for DeviceEpochRotationInitRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceEpochRotationInitRequest")
            .field("ceremony_id", &self.ceremony_id)
            .field("kind", &self.kind)
            .field("pending_epoch", &self.pending_epoch)
            .field("participant_device_id", &self.participant_device_id)
            .field("key_package_len", &self.key_package.len())
            .field("key_package", &"<redacted>")
            .field("threshold_config_len", &self.threshold_config.len())
            .field("threshold_config", &"<redacted>")
            .field("public_key_package_len", &self.public_key_package.len())
            .finish()
    }
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
        let proposal = self
            .build_signed_proposal(&request, initiator_device_id)
            .await?;

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

        let mut acceptance: Option<VerifiedIngress<DeviceEpochAcceptance>> = None;
        loop {
            let round = session
                .advance_round("Initiator", &peer_roles)
                .await
                .map_err(map_internal_error)?;

            if let Some(blocked) = round.blocked_receive {
                let decoded: DeviceEpochAcceptance =
                    from_slice(&blocked.payload).map_err(map_decode_error)?;
                let verified_acceptance =
                    verified_device_epoch_acceptance(&request, &proposal, decoded)?;
                acceptance = Some(verified_acceptance.clone());
                let threshold_reached = self
                    .ceremony_runner
                    .record_verified_response(
                        &request.ceremony_id,
                        ParticipantIdentity::device(
                            verified_acceptance.payload().acceptor_device_id,
                        ),
                        &verified_acceptance,
                    )
                    .await
                    .map_err(map_internal_error)?;
                session
                    .inject_blocked_receive(&blocked)
                    .map_err(map_internal_error)?;

                let commit = if threshold_reached {
                    self.coordinate_commit(&request, &proposal).await?
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

    async fn build_signed_proposal(
        &self,
        request: &DeviceEpochRotationInitRequest,
        initiator_device_id: DeviceId,
    ) -> AgentResult<DeviceEpochProposal> {
        let proposed_at_ms = self
            .effects
            .physical_time()
            .await
            .map_err(map_internal_error)?
            .ts_ms;
        let recipient_public_key = self
            .resolve_device_leaf_public_key(request.participant_device_id)
            .await?;
        let mut proposal = DeviceEpochProposal {
            ceremony_id: request.ceremony_id.clone(),
            kind: request.kind,
            subject_authority: self.authority_id,
            pending_epoch: request.pending_epoch,
            initiator_device_id,
            participant_device_id: request.participant_device_id,
            key_package_hash: Hash32::from_bytes(&request.key_package),
            threshold_config_hash: Hash32::from_bytes(&request.threshold_config),
            public_key_package_hash: Hash32::from_bytes(&request.public_key_package),
            proposed_at_ms,
            authority_signature: aura_core::threshold::ThresholdSignature::single_signer(
                Vec::new(),
                Vec::new(),
                0,
            ),
            encrypted_key_package: EncryptedDeviceEpochKeyPackage {
                protocol_version: 1,
                recipient_device_id: request.participant_device_id,
                recipient_public_key: recipient_public_key.clone(),
                ephemeral_public_key: Vec::new(),
                nonce: [0u8; 12],
                ciphertext: Vec::new(),
                binding_hash: Hash32::from_bytes(&[]),
            },
            threshold_config: request.threshold_config.clone(),
            public_key_package: request.public_key_package.clone(),
        };
        proposal.encrypted_key_package = encrypt_device_epoch_key_package(
            self.effects.as_ref(),
            &proposal,
            &recipient_public_key,
            &request.key_package,
        )
        .await
        .map_err(map_internal_error)?;
        let transcript = DeviceEpochProposalTranscript::new(&proposal);
        proposal.authority_signature = self
            .sign_authority_transcript(PROPOSAL_SIGNING_DOMAIN, &transcript)
            .await?;
        Ok(proposal)
    }

    async fn build_signed_commit(
        &self,
        proposal: &DeviceEpochProposal,
        attested_leaf_op: Option<AttestedOp>,
    ) -> AgentResult<DeviceEpochCommit> {
        let committed_at_ms = self
            .effects
            .physical_time()
            .await
            .map_err(map_internal_error)?
            .ts_ms;
        let mut commit = DeviceEpochCommit {
            ceremony_id: proposal.ceremony_id.clone(),
            new_epoch: proposal.pending_epoch,
            proposal_hash: device_epoch_proposal_hash(proposal).map_err(map_internal_error)?,
            committed_at_ms,
            attested_leaf_op_hash: None,
            authority_signature: aura_core::threshold::ThresholdSignature::single_signer(
                Vec::new(),
                Vec::new(),
                0,
            ),
            attested_leaf_op,
        };
        commit.attested_leaf_op_hash =
            device_epoch_commit_attested_op_hash(&commit).map_err(map_internal_error)?;
        let transcript = DeviceEpochCommitTranscript::new(&commit);
        commit.authority_signature = self
            .sign_authority_transcript(COMMIT_SIGNING_DOMAIN, &transcript)
            .await?;
        Ok(commit)
    }

    async fn sign_authority_transcript<T: aura_signature::SecurityTranscript + ?Sized>(
        &self,
        domain: &str,
        transcript: &T,
    ) -> AgentResult<aura_core::threshold::ThresholdSignature> {
        let payload = transcript.transcript_bytes().map_err(map_internal_error)?;
        self.signing_service
            .sign(SigningContext::message(
                self.authority_id,
                domain.to_string(),
                payload,
            ))
            .await
            .map_err(map_internal_error)
    }

    async fn current_authority_signature_material(&self) -> AgentResult<(u64, TrustedPublicKey)> {
        let state = self
            .signing_service
            .threshold_state(&self.authority_id)
            .await
            .ok_or_else(|| {
                AgentError::internal(
                    "missing current authority threshold state for device epoch rotation",
                )
            })?;
        let public_key_package = self
            .signing_service
            .public_key_package(&self.authority_id)
            .await
            .ok_or_else(|| {
                AgentError::internal(
                    "missing current authority public key package for device epoch rotation",
                )
            })?;
        let trusted_key = TrustedPublicKey::active(
            TrustedKeyDomain::AuthorityThreshold,
            Some(state.epoch),
            public_key_package.clone(),
            Hash32::from_bytes(&public_key_package),
        );
        Ok((state.epoch, trusted_key))
    }

    async fn verify_device_epoch_proposal(
        &self,
        proposal: &DeviceEpochProposal,
        initiator_device_id: DeviceId,
        expected_session_uuid: Uuid,
    ) -> AgentResult<()> {
        if proposal.subject_authority != self.authority_id {
            return Err(AgentError::invalid(format!(
                "device epoch proposal authority mismatch: expected {}, got {}",
                self.authority_id, proposal.subject_authority
            )));
        }
        if proposal.initiator_device_id != initiator_device_id {
            return Err(AgentError::invalid(format!(
                "device epoch proposal initiator mismatch: expected {}, got {}",
                initiator_device_id, proposal.initiator_device_id
            )));
        }
        if proposal.participant_device_id != self.effects.device_id() {
            return Err(AgentError::invalid(format!(
                "device epoch proposal participant mismatch: expected {}, got {}",
                self.effects.device_id(),
                proposal.participant_device_id
            )));
        }
        let local_device_public_key = self
            .resolve_device_leaf_public_key(self.effects.device_id())
            .await?;
        if proposal.encrypted_key_package.recipient_public_key != local_device_public_key {
            return Err(AgentError::invalid(
                "device epoch proposal recipient key does not match the enrolled device key"
                    .to_string(),
            ));
        }
        if device_epoch_rotation_session_id(&proposal.ceremony_id, proposal.participant_device_id)
            != expected_session_uuid
        {
            return Err(AgentError::invalid(
                "device epoch proposal ceremony/session binding mismatch".to_string(),
            ));
        }
        if !verify_device_epoch_proposal_hashes(proposal) {
            return Err(AgentError::invalid(
                "device epoch proposal hashes do not match key material".to_string(),
            ));
        }
        let current_tree_state = self
            .effects
            .get_current_state()
            .await
            .map_err(map_internal_error)?;
        if proposal.pending_epoch <= current_tree_state.epoch.value() {
            return Err(AgentError::invalid(format!(
                "device epoch proposal pending epoch {} must advance past current epoch {}",
                proposal.pending_epoch,
                current_tree_state.epoch.value()
            )));
        }

        let (expected_epoch, trusted_public_key_package) =
            self.current_authority_signature_material().await?;
        let verified: bool = verify_device_epoch_authority_signature::<AuraEffectSystem, _>(
            self.effects.as_ref(),
            self.authority_id,
            PROPOSAL_SIGNING_DOMAIN,
            &DeviceEpochProposalTranscript::new(proposal),
            &proposal.authority_signature,
            &trusted_public_key_package,
            expected_epoch,
        )
        .await
        .map_err(map_internal_error)?;
        if !verified {
            return Err(AgentError::invalid(
                "device epoch proposal authority signature verification failed".to_string(),
            ));
        }
        Ok(())
    }

    async fn verify_device_epoch_commit(
        &self,
        proposal: &DeviceEpochProposal,
        commit: &DeviceEpochCommit,
    ) -> AgentResult<()> {
        if commit.ceremony_id != proposal.ceremony_id {
            return Err(AgentError::invalid(
                "device epoch commit ceremony id mismatch".to_string(),
            ));
        }
        if commit.new_epoch != proposal.pending_epoch {
            return Err(AgentError::invalid(
                "device epoch commit pending epoch mismatch".to_string(),
            ));
        }
        if commit.proposal_hash
            != device_epoch_proposal_hash(proposal).map_err(map_internal_error)?
        {
            return Err(AgentError::invalid(
                "device epoch commit proposal hash mismatch".to_string(),
            ));
        }
        if commit.attested_leaf_op_hash
            != device_epoch_commit_attested_op_hash(commit).map_err(map_internal_error)?
        {
            return Err(AgentError::invalid(
                "device epoch commit attested-op hash mismatch".to_string(),
            ));
        }

        let (expected_epoch, trusted_public_key_package) =
            self.current_authority_signature_material().await?;
        let verified: bool = verify_device_epoch_authority_signature::<AuraEffectSystem, _>(
            self.effects.as_ref(),
            self.authority_id,
            COMMIT_SIGNING_DOMAIN,
            &DeviceEpochCommitTranscript::new(commit),
            &commit.authority_signature,
            &trusted_public_key_package,
            expected_epoch,
        )
        .await
        .map_err(map_internal_error)?;
        if !verified {
            return Err(AgentError::invalid(
                "device epoch commit authority signature verification failed".to_string(),
            ));
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

            let envelope = verified_device_epoch_envelope(envelope)?;

            processed += 1;
            if self.execute_participant_from_envelope(envelope).await? {
                completed += 1;
            }
        }

        Ok((processed, completed))
    }

    async fn execute_participant_from_envelope(
        &self,
        envelope: VerifiedIngress<TransportEnvelope>,
    ) -> AgentResult<bool> {
        let (envelope, _) = envelope.into_parts();
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

        let staged_proposal: Option<DeviceEpochProposal> = None;

        loop {
            let round = session
                .advance_round("Participant", &peer_roles)
                .await
                .map_err(map_internal_error)?;

            if let Some(blocked) = round.blocked_receive {
                if staged_proposal.is_none() {
                    let proposal: DeviceEpochProposal =
                        from_slice(&blocked.payload).map_err(map_decode_error)?;
                    self.verify_device_epoch_proposal(&proposal, initiator_device_id, session_uuid)
                        .await?;
                    self.stage_proposal(&proposal).await?;
                    let _ = session.close().await;
                    return Err(AgentError::internal(
                        "device epoch acceptance requires signed participant device proof; unsigned acceptances are disabled".to_string(),
                    ));
                } else {
                    let commit: DeviceEpochCommit =
                        from_slice(&blocked.payload).map_err(map_decode_error)?;
                    let proposal = staged_proposal
                        .as_ref()
                        .ok_or_else(|| AgentError::internal("missing staged proposal"))?;
                    self.apply_commit(proposal, &commit).await?;
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
        proposal: &DeviceEpochProposal,
    ) -> AgentResult<DeviceEpochCommit> {
        let commit = match request.kind {
            DeviceEpochRotationKind::Enrollment => {
                let attested_leaf_op = self.finalize_enrollment(&request.ceremony_id).await?;
                self.build_signed_commit(proposal, attested_leaf_op).await?
            }
            DeviceEpochRotationKind::Rotation | DeviceEpochRotationKind::Removal => {
                self.build_signed_commit(proposal, None).await?
            }
        };

        self.commit_local_rotation(&request.ceremony_id).await?;
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
        let recipient_public_key = self
            .resolve_device_leaf_public_key(self.effects.device_id())
            .await?;
        let recipient_private_key = self
            .signing_service
            .current_local_key_agreement_secret(&self.authority_id)
            .await
            .map_err(map_internal_error)?;
        let key_package = decrypt_device_epoch_key_package(
            self.effects.as_ref(),
            proposal,
            &recipient_public_key,
            &recipient_private_key,
        )
        .await
        .map_err(map_internal_error)?;
        let location = SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}:{}", proposal.subject_authority, proposal.pending_epoch),
            participant.storage_key(),
        );

        self.effects
            .secure_store(
                &location,
                &key_package,
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

    async fn apply_commit(
        &self,
        proposal: &DeviceEpochProposal,
        commit: &DeviceEpochCommit,
    ) -> AgentResult<()> {
        self.verify_device_epoch_commit(proposal, commit).await?;
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
            format!("{}:{}", self.authority_id, ceremony_state.new_epoch),
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

    async fn resolve_device_leaf_public_key(&self, device_id: DeviceId) -> AgentResult<Vec<u8>> {
        let tree_state = self
            .effects
            .get_current_state()
            .await
            .map_err(map_internal_error)?;
        tree_state
            .leaves
            .values()
            .find(|leaf| leaf.role == LeafRole::Device && leaf.device_id == device_id)
            .map(|leaf| Vec::from(&leaf.public_key))
            .ok_or_else(|| {
                AgentError::invalid(format!(
                    "missing enrolled device leaf public key for {}",
                    device_id
                ))
            })
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

fn verified_device_epoch_acceptance(
    request: &DeviceEpochRotationInitRequest,
    proposal: &DeviceEpochProposal,
    acceptance: DeviceEpochAcceptance,
) -> AgentResult<VerifiedIngress<DeviceEpochAcceptance>> {
    let _ = (request, proposal, acceptance);
    Err(AgentError::internal(
        "device epoch acceptance verification requires signed participant-device proofs; unsigned acceptances are disabled".to_string(),
    ))
}

fn verified_device_epoch_envelope(
    envelope: TransportEnvelope,
) -> AgentResult<VerifiedIngress<TransportEnvelope>> {
    let source = envelope_source_device_id(&envelope)?;
    let session_id = envelope_session_uuid(&envelope)?;
    let schema_version = envelope
        .metadata
        .get("wire-format-version")
        .and_then(|version| version.parse::<u16>().ok())
        .unwrap_or(aura_protocol::messages::WIRE_FORMAT_VERSION);
    let metadata = VerifiedIngressMetadata::new(
        IngressSource::Device(source),
        envelope.context,
        Some(aura_core::SessionId::from_uuid(session_id)),
        aura_core::Hash32::from_bytes(&envelope.payload),
        schema_version,
    );
    let evidence = IngressVerificationEvidence::builder(metadata)
        .peer_identity(
            envelope.metadata.contains_key("aura-source-device-id"),
            "device epoch envelope must carry source device identity",
        )
        .and_then(|builder| {
            builder.envelope_authenticity(
                !envelope.payload.is_empty(),
                "device epoch envelope payload must be present",
            )
        })
        .and_then(|builder| {
            builder.capability_authorization(
                is_device_epoch_rotation_envelope(&envelope),
                "device epoch envelope must use the device epoch protocol namespace",
            )
        })
        .and_then(|builder| {
            builder.namespace_scope(
                envelope
                    .metadata
                    .get("protocol-id")
                    .is_some_and(|value| value == PROTOCOL_ID),
                "device epoch protocol id must match",
            )
        })
        .and_then(|builder| {
            builder.schema_version(
                schema_version <= aura_protocol::messages::WIRE_FORMAT_VERSION,
                "unsupported device epoch schema",
            )
        })
        .and_then(|builder| {
            builder.replay_freshness(
                envelope.metadata.contains_key("session-id"),
                "device epoch envelope must carry session freshness",
            )
        })
        .and_then(|builder| {
            builder.signer_membership(
                envelope.metadata.contains_key("aura-source-device-id"),
                "device epoch envelope must carry participant device evidence",
            )
        })
        .and_then(|builder| {
            builder.proof_evidence(
                envelope.metadata.contains_key("session-id") && !envelope.payload.is_empty(),
                "device epoch envelope must bind session and payload evidence",
            )
        })
        .and_then(|builder| builder.build())
        .map_err(|error| AgentError::internal(format!("verify device epoch ingress: {error}")))?;
    DecodedIngress::new(envelope, evidence.metadata().clone())
        .verify(evidence)
        .map_err(|error| AgentError::internal(format!("promote device epoch ingress: {error}")))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::effects::{SecureStorageEffects, ThresholdSigningEffects};
    use aura_core::threshold::ThresholdConfig;
    use std::sync::Arc;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_device(seed: u8) -> DeviceId {
        DeviceId::new_from_entropy([seed; 32])
    }

    async fn test_service(seed: u8) -> DeviceEpochRotationService {
        let authority_id = test_authority(seed);
        let config = AgentConfig {
            device_id: test_device(seed.wrapping_add(1)),
            ..Default::default()
        };
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority_with_salt(
                &config,
                authority_id,
                u64::from(seed),
            )
            .expect("effect system"),
        );
        let signing_service = ThresholdSigningService::new(effects.clone());
        signing_service
            .bootstrap_authority(&authority_id)
            .await
            .expect("bootstrap authority");
        let time_effects: Arc<dyn PhysicalTimeEffects> = Arc::new(effects.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects);
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        DeviceEpochRotationService::new(
            authority_id,
            effects,
            ceremony_tracker,
            ceremony_runner,
            signing_service,
            ReconfigurationManager::new(),
        )
    }

    fn test_request(
        ceremony_id: &'static str,
        pending_epoch: u64,
        participant_device_id: DeviceId,
        key_package: Vec<u8>,
        public_key_package: Vec<u8>,
    ) -> DeviceEpochRotationInitRequest {
        DeviceEpochRotationInitRequest {
            ceremony_id: CeremonyId::new(ceremony_id),
            kind: DeviceEpochRotationKind::Rotation,
            pending_epoch,
            participant_device_id,
            key_package,
            threshold_config: to_vec(&ThresholdConfig {
                threshold: 1,
                total_participants: 1,
            })
            .expect("encode threshold config"),
            public_key_package,
        }
    }

    #[tokio::test]
    async fn device_epoch_proposal_verification_rejects_tampered_key_material() {
        let service = test_service(81).await;
        let initiator_device_id = test_device(82);
        let request = test_request(
            "device-epoch-proposal-tamper",
            1,
            service.effects.device_id(),
            vec![1, 2, 3, 4],
            vec![9; 32],
        );

        let mut proposal = service
            .build_signed_proposal(&request, initiator_device_id)
            .await
            .expect("signed proposal");
        proposal.encrypted_key_package.ciphertext[0] ^= 0xAA;
        let expected_session_uuid =
            device_epoch_rotation_session_id(&proposal.ceremony_id, proposal.participant_device_id);

        let error = service
            .verify_device_epoch_proposal(&proposal, initiator_device_id, expected_session_uuid)
            .await
            .expect_err("tampered key material must be rejected");
        assert!(error.to_string().contains("hashes do not match"));

        let share_location = SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}:{}", service.authority_id, request.pending_epoch),
            ParticipantIdentity::device(service.effects.device_id()).storage_key(),
        );
        assert!(
            service
                .effects
                .secure_retrieve(&share_location, &[SecureStorageCapability::Read])
                .await
                .is_err(),
            "proposal verification failure must not stage participant key material"
        );
    }

    #[tokio::test]
    async fn device_epoch_proposal_verification_rejects_wrong_participant_device() {
        let service = test_service(83).await;
        let initiator_device_id = test_device(84);
        let request = test_request(
            "device-epoch-wrong-participant",
            1,
            service.effects.device_id(),
            vec![5, 6, 7, 8],
            vec![7; 32],
        );

        let mut proposal = service
            .build_signed_proposal(&request, initiator_device_id)
            .await
            .expect("signed proposal");
        proposal.participant_device_id = test_device(85);
        let expected_session_uuid =
            device_epoch_rotation_session_id(&request.ceremony_id, request.participant_device_id);

        let error = service
            .verify_device_epoch_proposal(&proposal, initiator_device_id, expected_session_uuid)
            .await
            .expect_err("wrong participant must be rejected");
        assert!(error.to_string().contains("participant mismatch"));
    }

    #[tokio::test]
    async fn device_epoch_acceptance_verification_is_fail_closed_without_device_signer() {
        let service = test_service(86).await;
        let initiator_device_id = test_device(87);
        let request = test_request(
            "device-epoch-acceptance-disabled",
            1,
            service.effects.device_id(),
            vec![9, 10, 11],
            vec![8; 32],
        );
        let proposal = service
            .build_signed_proposal(&request, initiator_device_id)
            .await
            .expect("signed proposal");
        let acceptance = DeviceEpochAcceptance {
            ceremony_id: request.ceremony_id.clone(),
            acceptor_device_id: service.effects.device_id(),
            proposal_hash: device_epoch_proposal_hash(&proposal).expect("proposal hash"),
            accepted_at_ms: 1,
            signature: vec![1; 64],
        };

        let error = verified_device_epoch_acceptance(&request, &proposal, acceptance)
            .expect_err("unsigned participant-device acceptance must fail closed");
        assert!(error
            .to_string()
            .contains("unsigned acceptances are disabled"));
    }

    #[tokio::test]
    async fn device_epoch_commit_verification_rejects_forged_signature() {
        let service = test_service(88).await;
        let initiator_device_id = test_device(89);
        let request = test_request(
            "device-epoch-commit-forged",
            1,
            service.effects.device_id(),
            vec![12, 13, 14],
            vec![6; 32],
        );
        let proposal = service
            .build_signed_proposal(&request, initiator_device_id)
            .await
            .expect("signed proposal");
        let mut commit = service
            .build_signed_commit(&proposal, None)
            .await
            .expect("signed commit");
        commit.authority_signature.signature[0] ^= 0x55;

        let error = service
            .verify_device_epoch_commit(&proposal, &commit)
            .await
            .expect_err("forged commit signature must fail");
        assert!(error
            .to_string()
            .contains("authority signature verification failed"));
    }

    #[tokio::test]
    async fn valid_signed_device_epoch_commit_activates_new_epoch_and_rejects_replay() {
        let service = test_service(90).await;
        let initiator_device_id = test_device(91);
        let participants = vec![ParticipantIdentity::device(service.effects.device_id())];
        let (pending_epoch, key_packages, public_key_package) = service
            .effects
            .rotate_keys(&service.authority_id, 1, 1, &participants)
            .await
            .expect("rotate keys");
        let request = test_request(
            "device-epoch-valid-commit",
            pending_epoch,
            service.effects.device_id(),
            key_packages[0].clone(),
            public_key_package,
        );
        let proposal = service
            .build_signed_proposal(&request, initiator_device_id)
            .await
            .expect("signed proposal");
        let expected_session_uuid =
            device_epoch_rotation_session_id(&proposal.ceremony_id, proposal.participant_device_id);
        service
            .verify_device_epoch_proposal(&proposal, initiator_device_id, expected_session_uuid)
            .await
            .expect("proposal should verify");

        let commit = service
            .build_signed_commit(&proposal, None)
            .await
            .expect("signed commit");
        service
            .apply_commit(&proposal, &commit)
            .await
            .expect("valid signed commit should activate the epoch");

        let threshold_state = service
            .signing_service
            .threshold_state(&service.authority_id)
            .await
            .expect("committed threshold state");
        assert_eq!(threshold_state.epoch, pending_epoch);

        let replay_error = service
            .apply_commit(&proposal, &commit)
            .await
            .expect_err("replayed commit must fail");
        assert!(replay_error
            .to_string()
            .contains("authority signature verification failed"));
    }
}
