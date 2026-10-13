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
    ThresholdSigningEffects, TransportError,
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
    DeviceEnrollmentEpochCommitTranscript, DeviceEnrollmentEpochCommitTranscriptPayload,
    DeviceEpochAcceptance, DeviceEpochAcceptanceTranscript, DeviceEpochAcceptanceTranscriptPayload,
    DeviceEpochCommit, DeviceEpochCommitTranscript, DeviceEpochCommitTranscriptPayload,
    DeviceEpochProposal, DeviceEpochProposalPurpose, DeviceEpochProposalTranscript,
    EncryptedDeviceEpochKeyPackage, MAX_DEVICE_EPOCH_COMMIT_BYTES,
};
use aura_sync::protocols::DeviceEpochRotationKind;
use std::{collections::BTreeMap, fmt};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

const PROTOCOL_ID: &str = "aura.sync.device_epoch_rotation";

enum ParticipantInboundLease<'a> {
    Transport(crate::runtime::effects::InboundEnvelopeLease<'a>),
    Session(crate::runtime::effects::UnclaimedChoreographyEnvelopeLease<'a>),
}

impl ParticipantInboundLease<'_> {
    fn consume(self) -> TransportEnvelope {
        match self {
            Self::Transport(lease) => lease.consume(),
            Self::Session(lease) => lease.consume(),
        }
    }
}
const COMMIT_STORAGE_NAMESPACE: &str = "device_epoch_rotation_commit";
const COMMIT_STATUS_POLL_MS: u64 = 100;
const COMMIT_STATUS_TIMEOUT_MS: u64 = 10_000;
const PROPOSAL_SIGNING_DOMAIN: &str =
    aura_sync::protocols::device_epoch_rotation::DEVICE_EPOCH_PROPOSAL_SIGNING_DOMAIN;
const ENROLLMENT_COMMIT_SIGNING_DOMAIN: &str =
    "aura.sync.device_epoch_rotation.enrollment_commit.v2";
const COMMIT_SIGNING_DOMAIN: &str = "aura.sync.device_epoch_rotation.commit";

/// Canonical proposal data for both signing and exact local-consent matching.
/// Constructing this context grants neither approval nor signing authority.
pub(crate) fn device_epoch_proposal_signing_context(
    proposal: &DeviceEpochProposal,
) -> AgentResult<SigningContext> {
    use aura_signature::SecurityTranscript;
    let payload = DeviceEpochProposalTranscript::new(proposal)
        .transcript_bytes()
        .map_err(map_internal_error)?;
    Ok(SigningContext::message(
        proposal.subject_authority,
        PROPOSAL_SIGNING_DOMAIN.to_string(),
        payload,
    ))
}

pub(crate) struct OriginalEnrollmentRotationContext {
    pub(super) issued:
        std::sync::Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
    pub(super) original: crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
    pub(super) children: crate::task_registry::TaskGroup,
}

enum DeviceEpochSigningOwner<'a> {
    Enrollment(
        &'a crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        &'a crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
    ),
    Ordinary,
}

impl OriginalEnrollmentRotationContext {
    fn require_request(
        &self,
        effects: &AuraEffectSystem,
        request: &DeviceEpochRotationInitRequest,
    ) -> AgentResult<()> {
        self.issued.require_runtime_owner(effects)?;
        self.original.require_issued_notice_owner(&self.issued)?;
        let manifest = self.issued.manifest();
        if request.ceremony_id != manifest.ceremony
            || request.pending_epoch != manifest.pending_epoch
            || Hash32::from_bytes(&request.threshold_config)
                != manifest.pending_threshold_config_digest
            || hash::hash(&request.public_key_package) != manifest.pending_public_key_package_digest
            || request.participant_device_id == manifest.initiator_device
            || !manifest.final_inventory.iter().any(|parent| {
                parent
                    .participants
                    .contains(&ParticipantIdentity::device(request.participant_device_id))
            })
        {
            return Err(AgentError::invalid(
                "rotation request differs from original enrollment owner",
            ));
        }
        Ok(())
    }

    fn require_proposal(
        &self,
        effects: &AuraEffectSystem,
        request: &DeviceEpochRotationInitRequest,
        proposal: &DeviceEpochProposal,
    ) -> AgentResult<()> {
        self.require_request(effects, request)?;
        let manifest = self.issued.manifest();
        if proposal.purpose
            != (DeviceEpochProposalPurpose::Enrollment {
                manifest_digest: Hash32::new(self.issued.digest()),
                enrolling_device_id: manifest.invitee_device,
            })
            || proposal.subject_authority != manifest.subject
            || proposal.ceremony_id != request.ceremony_id
            || proposal.pending_epoch != request.pending_epoch
            || proposal.initiator_device_id != manifest.initiator_device
            || proposal.participant_device_id != request.participant_device_id
            || proposal.key_package_hash != Hash32::from_bytes(&request.key_package)
            || proposal.threshold_config_hash != manifest.pending_threshold_config_digest
            || proposal.public_key_package_hash
                != Hash32::new(manifest.pending_public_key_package_digest)
        {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        }
        Ok(())
    }
}

pub(crate) enum DeviceEpochRotationOrigin {
    Enrollment(OriginalEnrollmentRotationContext),
    Rotation,
    Removal,
}

impl DeviceEpochRotationOrigin {
    pub(crate) fn original_children(&self) -> Option<crate::task_registry::TaskGroup> {
        match self {
            Self::Enrollment(context) => Some(context.children.clone()),
            Self::Rotation | Self::Removal => None,
        }
    }

    fn signing_owner(&self) -> DeviceEpochSigningOwner<'_> {
        match self {
            Self::Enrollment(context) => {
                DeviceEpochSigningOwner::Enrollment(&context.issued, &context.original)
            }
            Self::Rotation | Self::Removal => DeviceEpochSigningOwner::Ordinary,
        }
    }
    fn kind(&self) -> DeviceEpochRotationKind {
        match self {
            Self::Enrollment(_) => DeviceEpochRotationKind::Enrollment,
            Self::Rotation => DeviceEpochRotationKind::Rotation,
            Self::Removal => DeviceEpochRotationKind::Removal,
        }
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct DeviceEpochRotationInitRequest {
    #[zeroize(skip)]
    pub ceremony_id: CeremonyId,
    #[zeroize(skip)]
    pub(crate) origin: DeviceEpochRotationOrigin,
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
            .field("kind", &self.origin.kind())
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

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEnrollmentActivation {
    version: u16,
    subject: AuthorityId,
    ceremony: CeremonyId,
    pending_epoch: u64,
    prestate: Hash32,
    setup_digest: [u8; 32],
    epoch_fence: AttestedOp,
    baseline: Vec<AttestedOp>,
    manifest_digest: [u8; 32],
    attested: AttestedOp,
}
#[derive(Debug, thiserror::Error)]
enum EnrollmentEpochFenceError {
    #[error("enrollment activation schema is unsupported")]
    UnsupportedSchema,
    #[error("enrollment activation differs from its owned original generation")]
    Binding,
    #[error("enrollment tree changed from the original signed activation history")]
    HistoryChanged,
    #[error("enrollment pending epoch is not the next authenticated tree epoch")]
    EpochMismatch,
    #[error("current enrollment tree lacks required physical device membership")]
    Membership,
}
fn enrollment_fence_failure(source: EnrollmentEpochFenceError) -> AgentError {
    AgentError::from(aura_core::AuraError::PermissionDenied {
        message: format!("owned enrollment epoch fence rejected: {source}"),
        source: Some(std::sync::Arc::new(source)),
    })
}
/// Actual tree custody is retained through key activation and terminal commit.
/// Serialized preparation by itself is never an activation capability.
pub(crate) struct AuthenticatedPreparedEnrollmentActivation {
    effects: std::sync::Arc<AuraEffectSystem>,
    tracker: CeremonyTracker,
    stored: StoredEnrollmentActivation,
    baseline_length: usize,
    candidate: Vec<AttestedOp>,
}
impl AuthenticatedPreparedEnrollmentActivation {
    pub(crate) fn require_original(
        &self,
        decision: &crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<'_>,
    ) -> Result<(), aura_core::AuraError> {
        decision.require_tracker(&self.tracker)?;
        decision.require_effects(self.effects.as_ref())
    }
    pub(crate) fn ceremony_id(&self) -> &CeremonyId {
        &self.stored.ceremony
    }
}

pub(crate) struct AuthenticatedPreparedEnrollmentRecovery<'a> {
    authenticated: AuthenticatedPreparedEnrollmentActivation,
    decision:
        crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<'a>,
}
impl<'a> AuthenticatedPreparedEnrollmentRecovery<'a> {
    pub(crate) fn into_original_decision(
        self,
    ) -> (
        AuthenticatedPreparedEnrollmentActivation,
        crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<'a>,
    ) {
        (self.authenticated, self.decision)
    }
}

pub(crate) struct PreparedEnrollmentActivationCapability<'a> {
    effects: &'a AuraEffectSystem,
    tracker: CeremonyTracker,
    stored: StoredEnrollmentActivation,
    _tree: aura_protocol::handlers::tree::TreeDecisionLease<'a>,
}
impl PreparedEnrollmentActivationCapability<'_> {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "PreparedEnrollmentActivationCapability", receiver_type = PreparedEnrollmentActivationCapability, family = "runtime_helper")]
    pub(crate) async fn require_pending_generation(
        &self,
        effects: &AuraEffectSystem,
        activation: &crate::runtime::services::ceremony_tracker::EnrollmentActivationCapability<'_>,
    ) -> AgentResult<()> {
        if !std::ptr::eq(self.effects, effects) || activation.ceremony_id() != &self.stored.ceremony
        {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        }
        activation.require_effects(effects)?;
        activation.require_tracker(&self.tracker)?;
        activation
            .require_generation(self.stored.subject, self.stored.pending_epoch)
            .await?;
        Ok(())
    }

    pub(crate) fn subject(&self) -> AuthorityId {
        self.stored.subject
    }
    pub(crate) fn pending_epoch(&self) -> u64 {
        self.stored.pending_epoch
    }
    pub(crate) fn prestate(&self) -> Hash32 {
        self.stored.prestate
    }
    pub(crate) fn ceremony_id(&self) -> &CeremonyId {
        &self.stored.ceremony
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "PreparedEnrollmentActivationCapability", receiver_type = PreparedEnrollmentActivationCapability, family = "runtime_helper")]
    pub(crate) async fn original_committed_ops(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> AgentResult<Vec<AttestedOp>> {
        issued.require_runtime_owner(effects)?;
        let manifest = issued.manifest();
        if !std::ptr::eq(self.effects, effects)
            || self.stored.manifest_digest != issued.digest()
            || self.stored.subject != manifest.subject
            || self.stored.ceremony != manifest.ceremony
            || self.stored.pending_epoch != manifest.pending_epoch
        {
            #[cfg(test)]
            eprintln!(
                "original activation binding: effects={} manifest={} subject={} ceremony={} epoch={}",
                std::ptr::eq(self.effects, effects),
                self.stored.manifest_digest == issued.digest(),
                self.stored.subject == manifest.subject,
                self.stored.ceremony == manifest.ceremony,
                self.stored.pending_epoch == manifest.pending_epoch,
            );
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        }
        let mut expected = self.stored.baseline.clone();
        expected.push(self.stored.attested.clone());
        expected.push(self.stored.epoch_fence.clone());
        let actual = effects
            .export_tree_ops()
            .await
            .map_err(map_internal_error)?;
        if to_vec(&actual).map_err(map_encode_error)?
            != to_vec(&expected).map_err(map_encode_error)?
        {
            return Err(enrollment_fence_failure(
                EnrollmentEpochFenceError::HistoryChanged,
            ));
        }
        let epoch = effects
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", manifest.subject.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if epoch.as_slice() != manifest.pending_epoch.to_le_bytes().as_slice() {
            return Err(enrollment_fence_failure(
                EnrollmentEpochFenceError::EpochMismatch,
            ));
        }
        for (namespace, digest) in [
            (
                "threshold_config",
                *manifest.pending_threshold_config_digest.as_bytes(),
            ),
            (
                "threshold_pubkey",
                manifest.pending_public_key_package_digest,
            ),
        ] {
            let bytes = effects
                .secure_retrieve(
                    &SecureStorageLocation::with_sub_key(
                        namespace,
                        manifest.subject.to_string(),
                        manifest.pending_epoch.to_string(),
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await?;
            let actual_digest = if namespace == "threshold_config" {
                let metadata: crate::runtime::effects::ThresholdConfigMetadata =
                    serde_json::from_slice(&bytes).map_err(|source| {
                        AgentError::from(aura_core::AuraError::Serialization {
                            message: "decode original activated signing generation".into(),
                            source: Some(std::sync::Arc::new(source)),
                        })
                    })?;
                metadata.require_activated_pending_generation(digest)?;
                digest
            } else {
                hash::hash(&bytes)
            };
            if actual_digest != digest {
                #[cfg(test)]
                eprintln!("original activation binding: stored {namespace} digest differs");
                return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
            }
        }
        Ok(vec![
            self.stored.attested.clone(),
            self.stored.epoch_fence.clone(),
        ])
    }
}
fn enrollment_activation_prestate(
    authority: AuthorityId,
    baseline: &[AttestedOp],
    participants: &std::collections::HashSet<ParticipantIdentity>,
    issuer_device: DeviceId,
    enrolling_device: DeviceId,
) -> AgentResult<Hash32> {
    let state = aura_journal::commitment_tree::reduce(baseline).map_err(map_internal_error)?;
    // Reconstruct the exact issuance order: actual issuer first, other existing
    // devices sorted by the original string key, enrolling physical device last.
    // Tracker participants are acceptance-only and exclude the actual issuer.
    // Reconstructing the signing roster must prepend that owned issuer.
    if issuer_device == enrolling_device
        || participants.contains(&ParticipantIdentity::device(issuer_device))
        || !participants.contains(&ParticipantIdentity::device(enrolling_device))
    {
        return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
    }
    let mut others = participants
        .iter()
        .map(|participant| match participant {
            ParticipantIdentity::Device(device) => Ok(*device),
            _ => Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding)),
        })
        .collect::<AgentResult<Vec<_>>>()?;
    others.retain(|device| *device != issuer_device && *device != enrolling_device);
    others.sort_by_key(|device| device.to_string());
    let mut devices = Vec::with_capacity(participants.len() + 1);
    devices.push(issuer_device);
    devices.extend(others);
    devices.push(enrolling_device);
    let input = serde_json::to_vec(&(state.epoch, state.root_commitment, devices))
        .map_err(map_encode_error)?;
    let prestate = aura_core::Prestate::new(
        vec![(authority, Hash32(state.root_commitment))],
        Hash32(hash::hash(&input)),
    )
    .map_err(map_internal_error)?;
    Ok(prestate.compute_hash())
}
fn enrollment_activation_location(ceremony: &CeremonyId) -> SecureStorageLocation {
    SecureStorageLocation::new("device_enrollment_activation_v1", ceremony.to_string())
}

impl DeviceEpochRotationService {
    pub(crate) async fn requires_original_sole_finalization(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
    ) -> AgentResult<bool> {
        issued.require_runtime_owner(self.effects.as_ref())?;
        original.require_issued_notice_owner(issued)?;
        original
            .execute(self.effects.as_ref(), || async {
                let state = self
                    .ceremony_tracker
                    .get(&issued.manifest().ceremony)
                    .await
                    .map_err(AgentError::from)?;
                let invitee = ParticipantIdentity::device(issued.manifest().invitee_device);
                if state.kind != aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
                    || state.initiator_id != issued.manifest().subject
                    || state.new_epoch != issued.manifest().pending_epoch
                    || state.enrollment_device_id != Some(issued.manifest().invitee_device)
                    || !state.participants.contains(&invitee)
                {
                    return Err(AgentError::invalid(
                        "original finalizer roster binding differs",
                    ));
                }
                Ok(state.participants.len() == 1)
            })
            .await
            .map_err(|source| original.map_run_error("select original finalizer role", source))
    }
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
        if let DeviceEpochRotationOrigin::Enrollment(context) = &request.origin {
            context.require_request(self.effects.as_ref(), &request)?;
        }
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

        // A verified refusal ends the enrollment ceremony without a rotation
        // acceptance. Retire this original child and its VM session so the
        // issuer can acknowledge disposal of its exact child subtree.
        let enrollment_origin = matches!(&request.origin, DeviceEpochRotationOrigin::Enrollment(_));
        let terminal_runner = self.ceremony_runner.clone();
        let terminal_ceremony = request.ceremony_id.clone();
        let rejected = async move {
            if enrollment_origin {
                let outcome = terminal_runner
                    .await_enrollment_terminal_outcome(&terminal_ceremony)
                    .await?;
                if outcome
                    == aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                        aura_app::runtime_bridge::CeremonyFailureReason::Rejected,
                    )
                {
                    return Ok::<(), aura_core::AuraError>(());
                }
            }
            futures::future::pending().await
        };
        tokio::pin!(rejected);

        let mut acceptance: Option<VerifiedIngress<DeviceEpochAcceptance>> = None;
        loop {
            let round = tokio::select! {
                biased;
                result = &mut rejected => {
                    let close = session.close().await.map_err(map_internal_error);
                    close?;
                    result.map_err(map_internal_error)?;
                    return Ok(());
                }
                round = session.advance_round("Initiator", &peer_roles) => {
                    round.map_err(map_internal_error)?
                }
            };

            if let Some(blocked) = round.blocked_receive {
                let decoded: DeviceEpochAcceptance =
                    from_slice(&blocked.payload).map_err(map_decode_error)?;
                let verified_acceptance = self
                    .verified_device_epoch_acceptance(&request, &proposal, decoded)
                    .await?;
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
                    .inject_blocked_receive(blocked)
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
            purpose: match &request.origin {
                DeviceEpochRotationOrigin::Enrollment(context) => {
                    DeviceEpochProposalPurpose::Enrollment {
                        manifest_digest: Hash32::new(context.issued.digest()),
                        enrolling_device_id: context.issued.manifest().invitee_device,
                    }
                }
                DeviceEpochRotationOrigin::Rotation => DeviceEpochProposalPurpose::Rotation,
                DeviceEpochRotationOrigin::Removal => DeviceEpochProposalPurpose::Removal,
            },
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
        if let DeviceEpochRotationOrigin::Enrollment(context) = &request.origin {
            context.require_proposal(self.effects.as_ref(), request, &proposal)?;
        }
        proposal.authority_signature = self
            .sign_rotation_context(
                &request.origin.signing_owner(),
                device_epoch_proposal_signing_context(&proposal)?,
            )
            .await?;
        Ok(proposal)
    }

    async fn build_signed_commit(
        &self,
        owner: &DeviceEpochSigningOwner<'_>,
        proposal: &DeviceEpochProposal,
        attested_leaf_op: Option<AttestedOp>,
        attested_epoch_op: Option<AttestedOp>,
    ) -> AgentResult<DeviceEpochCommit> {
        let committed_at_ms = self
            .effects
            .physical_time()
            .await
            .map_err(map_internal_error)?
            .ts_ms;
        let proposal_hash = device_epoch_proposal_hash(proposal).map_err(map_internal_error)?;
        let attested_leaf_op_hash = attested_leaf_op
            .as_ref()
            .map(Hash32::from_value)
            .transpose()
            .map_err(map_internal_error)?;
        let attested_epoch_op_hash = attested_epoch_op
            .as_ref()
            .map(Hash32::from_value)
            .transpose()
            .map_err(map_internal_error)?;
        let authority_signature = match proposal.purpose.kind() {
            DeviceEpochRotationKind::Enrollment => {
                let transcript = DeviceEnrollmentEpochCommitTranscript::new(
                    DeviceEnrollmentEpochCommitTranscriptPayload {
                        ceremony_id: proposal.ceremony_id.clone(),
                        new_epoch: proposal.pending_epoch,
                        proposal_hash,
                        committed_at_ms,
                        attested_leaf_op_hash: attested_leaf_op_hash.ok_or_else(|| {
                            enrollment_fence_failure(EnrollmentEpochFenceError::Binding)
                        })?,
                        attested_epoch_op_hash: attested_epoch_op_hash.ok_or_else(|| {
                            enrollment_fence_failure(EnrollmentEpochFenceError::Binding)
                        })?,
                    },
                );
                self.sign_authority_transcript(owner, ENROLLMENT_COMMIT_SIGNING_DOMAIN, &transcript)
                    .await?
            }
            DeviceEpochRotationKind::Rotation | DeviceEpochRotationKind::Removal => {
                if attested_epoch_op.is_some() {
                    return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
                }
                let transcript =
                    DeviceEpochCommitTranscript::from_payload(DeviceEpochCommitTranscriptPayload {
                        ceremony_id: proposal.ceremony_id.clone(),
                        new_epoch: proposal.pending_epoch,
                        proposal_hash,
                        committed_at_ms,
                        attested_leaf_op_hash,
                    });
                self.sign_authority_transcript(owner, COMMIT_SIGNING_DOMAIN, &transcript)
                    .await?
            }
        };
        Ok(DeviceEpochCommit {
            ceremony_id: proposal.ceremony_id.clone(),
            new_epoch: proposal.pending_epoch,
            proposal_hash,
            committed_at_ms,
            attested_leaf_op_hash,
            attested_epoch_op_hash,
            authority_signature,
            attested_leaf_op,
            attested_epoch_op,
        })
    }

    async fn sign_authority_transcript<T: aura_signature::SecurityTranscript + ?Sized>(
        &self,
        owner: &DeviceEpochSigningOwner<'_>,
        domain: &str,
        transcript: &T,
    ) -> AgentResult<aura_core::threshold::ThresholdSignature> {
        let payload = transcript.transcript_bytes().map_err(map_internal_error)?;
        self.sign_rotation_context(
            owner,
            SigningContext::message(self.authority_id, domain.to_string(), payload),
        )
        .await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "DeviceEpochSigningOwner",
        family = "runtime_helper"
    )]
    async fn sign_rotation_context(
        &self,
        owner: &DeviceEpochSigningOwner<'_>,
        context: SigningContext,
    ) -> AgentResult<aura_core::threshold::ThresholdSignature> {
        let DeviceEpochSigningOwner::Enrollment(issued, original) = owner else {
            return self
                .signing_service
                .sign_with_device_quorum(context)
                .await
                .map_err(AgentError::from);
        };
        if !issued.original_conditional_signing_required(self.effects.as_ref(), original)? {
            return original
                .execute(self.effects.as_ref(), || async {
                    self.signing_service
                        .sign(context)
                        .await
                        .map_err(AgentError::from)
                })
                .await
                .map_err(|source| {
                    original.map_run_error("original single-signer rotation", source)
                });
        }
        let observed = original
            .acquire_original_conditional_signing(self.effects.as_ref(), issued)
            .await?;
        issued.retain_original_conditional_acquisition(&observed)?;
        observed.validation.map_err(|source| {
            aura_core::AuraError::crypto_with_source(
                "original quorum signing-state acquisition validation",
                std::sync::Arc::new(source),
            )
        })?;
        let mut state = observed.operation.ok_or_else(|| {
            aura_core::AuraError::invalid("original quorum signing-state acquisition interrupted")
        })??;
        state
            .sign_original_device_quorum(&self.signing_service, issued, original, context)
            .await
            .map_err(AgentError::from)
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

    fn verify_device_epoch_proposal_route(
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
        Ok(())
    }

    async fn verify_device_epoch_proposal(
        &self,
        proposal: &DeviceEpochProposal,
        initiator_device_id: DeviceId,
        expected_session_uuid: Uuid,
    ) -> AgentResult<()> {
        self.verify_device_epoch_proposal_route(
            proposal,
            initiator_device_id,
            expected_session_uuid,
        )?;
        let local_device_public_key = self
            .resolve_device_leaf_public_key(self.effects.device_id())
            .await?;
        if proposal.encrypted_key_package.recipient_public_key != local_device_public_key {
            return Err(AgentError::invalid(
                "device epoch proposal recipient key does not match the enrolled device key"
                    .to_string(),
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
        let verified = match proposal.purpose.kind() {
            DeviceEpochRotationKind::Enrollment => {
                let transcript = DeviceEnrollmentEpochCommitTranscript::from_commit(commit)
                    .map_err(map_internal_error)?;
                verify_device_epoch_authority_signature::<AuraEffectSystem, _>(
                    self.effects.as_ref(),
                    self.authority_id,
                    ENROLLMENT_COMMIT_SIGNING_DOMAIN,
                    &transcript,
                    &commit.authority_signature,
                    &trusted_public_key_package,
                    expected_epoch,
                )
                .await
                .map_err(map_internal_error)?
            }
            DeviceEpochRotationKind::Rotation | DeviceEpochRotationKind::Removal => {
                if commit.attested_epoch_op.is_some() || commit.attested_epoch_op_hash.is_some() {
                    return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
                }
                verify_device_epoch_authority_signature::<AuraEffectSystem, _>(
                    self.effects.as_ref(),
                    self.authority_id,
                    COMMIT_SIGNING_DOMAIN,
                    &DeviceEpochCommitTranscript::new(commit),
                    &commit.authority_signature,
                    &trusted_public_key_package,
                    expected_epoch,
                )
                .await
                .map_err(map_internal_error)?
            }
        };
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
            // Take only rotation envelopes; sync and other traffic stay queued for
            // their own consumers instead of blocking a proposal behind them.
            let (envelope, return_lease) = match self
                .effects
                .take_inbound_envelope_retained(is_device_epoch_rotation_envelope)
            {
                Ok(lease) => {
                    // VM startup awaits before it reads this frame. Keep the
                    // original return slot reserved until the VM is ready;
                    // cancellation restores the frame at its original index.
                    let verified = match verified_device_epoch_envelope((*lease).clone()) {
                        Ok(verified) => verified,
                        Err(error) => {
                            // Rejected wire input was consumed by the old receive
                            // path; do not restore an invalid frame forever.
                            let _ = lease.consume();
                            return Err(error);
                        }
                    };
                    (verified, Some(ParticipantInboundLease::Transport(lease)))
                }
                // Choreography envelopes are buffered per session on arrival; a
                // rotation this device only participates in waits there.
                Err(TransportError::NoMessage) => {
                    match self.effects.take_unclaimed_choreography_envelope_retained(
                        is_device_epoch_rotation_envelope,
                    ) {
                        Some(lease) => {
                            let verified = match verified_device_epoch_envelope((*lease).clone()) {
                                Ok(verified) => verified,
                                Err(error) => {
                                    let _ = lease.consume();
                                    return Err(error);
                                }
                            };
                            (verified, Some(ParticipantInboundLease::Session(lease)))
                        }
                        None => break,
                    }
                }
                Err(error) => return Err(AgentError::internal(error.to_string())),
            };

            processed += 1;
            if self
                .execute_participant_from_envelope(envelope, return_lease)
                .await?
            {
                completed += 1;
            }
        }

        Ok((processed, completed))
    }

    async fn execute_participant_from_envelope(
        &self,
        envelope: VerifiedIngress<TransportEnvelope>,
        return_lease: Option<ParticipantInboundLease<'_>>,
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

        let opened = open_owned_manifest_vm_session_admitted(
            self.effects.clone(),
            session_uuid,
            roles,
            &manifest,
            "Participant",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await;
        let mut session = match opened {
            Ok(session) => session,
            Err(error @ SessionIngressError::SessionStart { .. }) => {
                // A failed VM start has not taken ownership of the packet.
                // The retained branch returns it at its original slot.
                return Err(map_session_error(error));
            }
            Err(other) => {
                // Later VM admission failures may have taken ownership. Keep
                // the prior consume-on-error policy rather than replaying it.
                if let Some(lease) = return_lease {
                    let _ = lease.consume();
                }
                return Err(map_session_error(other));
            }
        };
        drop(return_lease);

        let mut staged_proposal: Option<DeviceEpochProposal> = None;
        let mut enrollment_peer: Option<
            crate::runtime::services::threshold_signing::OriginalEnrollmentPeerAdmission,
        > = None;

        loop {
            let round = session
                .advance_round("Participant", &peer_roles)
                .await
                .map_err(map_internal_error)?;

            if let Some(blocked) = round.blocked_receive {
                if let Some(proposal) = staged_proposal.as_ref() {
                    if blocked.payload.len() > MAX_DEVICE_EPOCH_COMMIT_BYTES {
                        return Err(AgentError::invalid("oversized device epoch commit"));
                    }
                    let commit: DeviceEpochCommit =
                        from_slice(&blocked.payload).map_err(map_decode_error)?;
                    if let Some(original) = enrollment_peer.as_ref() {
                        #[cfg(not(test))]
                        original
                            .apply_verified_commit(proposal, &commit)
                            .await
                            .map_err(AgentError::from)?;
                        #[cfg(test)]
                        original
                            .apply_verified_commit_with_postread_drop_test(proposal, &commit)
                            .await
                            .map_err(AgentError::from)?;
                    } else {
                        self.apply_commit(proposal, &commit).await?;
                    }
                    self.record_native_session(session_uuid).await;
                    // Deliver the commit to the VM; the session completes
                    // through its own protocol end.
                    session
                        .inject_blocked_receive(blocked)
                        .map_err(map_internal_error)?;
                    continue;
                }
                let proposal: DeviceEpochProposal =
                    from_slice(&blocked.payload).map_err(map_decode_error)?;
                let acceptance = if proposal.purpose.kind() == DeviceEpochRotationKind::Enrollment {
                    self.verify_device_epoch_proposal_route(
                        &proposal,
                        initiator_device_id,
                        session_uuid,
                    )?;
                    let original = self
                        .signing_service
                        .admit_original_enrollment_peer(&proposal)
                        .await
                        .map_err(AgentError::from)?;
                    // Retain the finite admission before its first staging await.
                    // Its native state remains in the original task's peer cell.
                    enrollment_peer = Some(original);
                    enrollment_peer
                        .as_ref()
                        .ok_or_else(|| {
                            enrollment_fence_failure(EnrollmentEpochFenceError::Binding)
                        })?
                        .prepare_acceptance(&proposal)
                        .await
                        .map_err(AgentError::from)?
                } else {
                    self.verify_device_epoch_proposal(&proposal, initiator_device_id, session_uuid)
                        .await?;
                    self.stage_proposal(&proposal).await?;
                    self.build_signed_acceptance(&proposal).await?
                };
                session
                    .inject_blocked_receive(blocked)
                    .map_err(map_internal_error)?;
                session.queue_send_bytes(to_vec(&acceptance).map_err(map_encode_error)?);
                staged_proposal = Some(proposal);
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
        let enrollment = match &request.origin {
            DeviceEpochRotationOrigin::Enrollment(context) => {
                context.require_proposal(self.effects.as_ref(), request, proposal)?;
                Some(context)
            }
            DeviceEpochRotationOrigin::Rotation | DeviceEpochRotationOrigin::Removal => None,
        };
        let activation = if enrollment.is_some() {
            self.ceremony_tracker
                .begin_enrollment_activation(&request.ceremony_id)
                .await
                .map_err(AgentError::from)?
        } else {
            None
        };
        if enrollment.is_some() && activation.is_none() {
            return self.load_commit(&request.ceremony_id).await;
        }
        if let (Some(context), Some(activation)) = (enrollment, activation.as_ref()) {
            self.retire_original_conditional_signing(
                &context.issued,
                &context.original,
                activation,
            )
            .await?;
            crate::handlers::invitation::enrollment_trust::restore_original_pending_signing_generation(
                self.effects.as_ref(),
                &self.signing_service,
                &context.issued,
                &context.original,
                activation,
            )
            .await?;
        }
        let prepared_activation = if enrollment.is_some() {
            Some(
                self.finalize_enrollment(
                    &request.origin.signing_owner(),
                    activation.as_ref().ok_or_else(|| {
                        enrollment_fence_failure(EnrollmentEpochFenceError::Binding)
                    })?,
                )
                .await?,
            )
        } else {
            None
        };
        let commit = match request.origin.kind() {
            DeviceEpochRotationKind::Enrollment => {
                let owned = prepared_activation
                    .as_ref()
                    .ok_or_else(|| enrollment_fence_failure(EnrollmentEpochFenceError::Binding))?;
                self.build_signed_commit(
                    &request.origin.signing_owner(),
                    proposal,
                    Some(owned.stored.attested.clone()),
                    Some(owned.stored.epoch_fence.clone()),
                )
                .await?
            }
            DeviceEpochRotationKind::Rotation | DeviceEpochRotationKind::Removal => {
                self.build_signed_commit(&request.origin.signing_owner(), proposal, None, None)
                    .await?
            }
        };

        if let (Some(context), Some(activation)) = (enrollment, activation.as_ref()) {
            self.commit_original_local_rotation(&context.issued, &context.original, activation)
                .await?;
        } else {
            self.commit_ordinary_local_rotation(&request.ceremony_id)
                .await?;
        }
        self.store_commit(&commit).await?;
        if let Some(activation) = activation {
            let context = enrollment
                .ok_or_else(|| enrollment_fence_failure(EnrollmentEpochFenceError::Binding))?;
            let prepared = prepared_activation
                .as_ref()
                .ok_or_else(|| enrollment_fence_failure(EnrollmentEpochFenceError::Binding))?;
            activation
                .commit_original(&context.issued, &context.original, prepared)
                .await
                .map_err(AgentError::from)?;
        } else {
            self.ceremony_runner
                .commit(&request.ceremony_id, CeremonyCommitMetadata::default())
                .await
                .map_err(map_internal_error)?;
        }

        Ok(commit)
    }

    /// Finalize an enrollment into an account whose only device is this one.
    ///
    /// With no other devices there is no device-epoch rotation session to run
    /// `coordinate_commit`, so the initiator waits for the new device's verified
    /// acceptance and then adds its leaf and commits the rotation itself.
    pub(crate) async fn finalize_sole_device_enrollment(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
    ) -> AgentResult<()> {
        issued.require_runtime_owner(self.effects.as_ref())?;
        original.require_issued_notice_owner(issued)?;
        #[cfg(test)]
        eprintln!("sole activation: entering original executor");
        let result = original
            .execute(self.effects.as_ref(), || async {
                let registered = self
                    .ceremony_tracker
                    .get(&issued.manifest().ceremony)
                    .await
                    .map_err(AgentError::from)?;
                // Existing devices own their rotation protocol. The dedicated sole
                // device finalizer only owns the exact one-invitee roster.
                if registered.participants.len() != 1
                    || registered.participants.iter().any(|participant| {
                        participant
                            != &ParticipantIdentity::device(issued.manifest().invitee_device)
                    })
                {
                    return Err(AgentError::invalid(
                        "sole finalizer requires exact original invitee roster",
                    ));
                }
                self.ceremony_tracker
                    .await_original_enrollment_acceptance(issued, original)
                    .await
                    .map_err(AgentError::from)?;
                let ceremony_id = &issued.manifest().ceremony;
                #[cfg(test)]
                eprintln!("sole activation: acceptance observed; acquiring activation");
                let Some(activation) = self
                    .ceremony_tracker
                    .begin_enrollment_activation(ceremony_id)
                    .await
                    .map_err(AgentError::from)?
                else {
                    return Ok(());
                };
                self.retire_original_conditional_signing(issued, original, &activation)
                    .await?;
                #[cfg(test)]
                eprintln!("sole activation: conditional retirement complete; restoring generation");
                crate::handlers::invitation::enrollment_trust::restore_original_pending_signing_generation(
                    self.effects.as_ref(),
                    &self.signing_service,
                    issued,
                    original,
                    &activation,
                )
                .await?;
                #[cfg(test)]
                eprintln!("sole activation: generation restored; preparing signed leaf and fence");
                let prepared_activation = self
                    .finalize_enrollment(
                        &DeviceEpochSigningOwner::Enrollment(issued, original),
                        &activation,
                    )
                    .await?;
                #[cfg(test)]
                eprintln!("sole activation: leaf and fence prepared; committing local rotation");
                self.commit_original_local_rotation(issued, original, &activation)
                    .await?;
                #[cfg(test)]
                eprintln!(
                    "sole activation: local rotation committed; publishing original decision"
                );
                activation
                    .commit_original(issued, original, &prepared_activation)
                    .await
                    .map_err(AgentError::from)?;
                Ok(())
            })
            .await
            .map_err(|source| original.map_run_error("finalize original issuer", source));
        #[cfg(test)]
        eprintln!("sole activation: original executor result {result:?}");
        result
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentActivationCapability",
        family = "runtime_helper"
    )]
    async fn retire_original_conditional_signing(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        activation: &crate::runtime::services::ceremony_tracker::EnrollmentActivationCapability<'_>,
    ) -> AgentResult<()> {
        if !issued.original_conditional_signing_required(self.effects.as_ref(), original)? {
            return Ok(());
        }
        let observed = original
            .acquire_original_conditional_signing(self.effects.as_ref(), issued)
            .await?;
        issued.retain_original_conditional_acquisition(&observed)?;
        observed.validation.map_err(|source| {
            AgentError::from(aura_core::AuraError::crypto_with_source(
                "original activation signing-state acquisition validation",
                std::sync::Arc::new(source),
            ))
        })?;
        let mut state = observed.operation.ok_or_else(|| {
            aura_core::AuraError::invalid(
                "original activation signing-state acquisition interrupted",
            )
        })??;
        state
            .retire_original_conditional_for_activation(issued, original, activation)
            .await?;
        Ok(())
    }

    /// Startup bounds only acquisition and reconciliation of an authenticated
    /// prepared decision. It never restores Pending execution or signs anew.
    pub(crate) async fn recover_prepared_enrollment(
        &self,
        context: &crate::runtime::services::RuntimeServiceContext,
        ceremony_id: &CeremonyId,
    ) -> AgentResult<()> {
        let selected: std::sync::Arc<dyn PhysicalTimeEffects + Send + Sync> = self.effects.clone();
        if !std::sync::Arc::ptr_eq(&selected, &context.time_effects()) {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        }
        aura_core::time::timeout::execute_with_timeout_budget(
            self.effects.as_ref(),
            context.startup_window(),
            || async {
                let decision = self
                    .ceremony_tracker
                    .acquire_enrollment_generation_decision(self.effects.as_ref())
                    .await
                    .map_err(AgentError::from)?;
                let ceremony = self
                    .ceremony_tracker
                    .get(ceremony_id)
                    .await
                    .map_err(AgentError::from)?;
                if ceremony.kind != aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
                    || ceremony.is_superseded
                    || matches!(
                        ceremony.terminal_outcome,
                        Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(_))
                    )
                {
                    return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
                }
                let authenticated = self
                    .authenticate_prepared_enrollment_activation(&ceremony, &decision)
                    .await?
                    .ok_or_else(|| enrollment_fence_failure(EnrollmentEpochFenceError::Binding))?;
                let tree = self.effects.lock_tree_decision().await;
                self.install_prepared_enrollment(&authenticated, &tree, &decision)
                    .await?;
                // A previously committed decision is observationally idempotent
                // only after the same protected record and tree were verified.
                if ceremony.terminal_outcome
                    == Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed)
                {
                    return Ok(());
                }
                let recovery = AuthenticatedPreparedEnrollmentRecovery {
                    authenticated,
                    decision,
                };
                let (activation, authenticated) = self
                    .ceremony_tracker
                    .retain_prepared_enrollment_activation(recovery)
                    .map_err(AgentError::from)?;
                let prepared = PreparedEnrollmentActivationCapability {
                    effects: self.effects.as_ref(),
                    tracker: self.ceremony_tracker.clone(),
                    stored: authenticated.stored,
                    _tree: tree,
                };
                let generation = crate::handlers::invitation::enrollment_trust::restore_prepared_pending_signing_generation(
                    self.effects.as_ref(), &self.signing_service, &prepared, &activation,
                ).await?;
                self.signing_service.commit_verified_pending_generation(&generation, &activation).await?;
                activation.commit().await.map_err(AgentError::from)
            },
        )
        .await
        .map_err(|source| {
            AgentError::from(aura_core::AuraError::Internal {
                message: "reconcile authenticated prepared enrollment".into(),
                source: Some(std::sync::Arc::new(source)),
            })
        })
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
        self.signing_service
            .stage_rotated_device_share(
                &proposal.subject_authority,
                proposal.pending_epoch,
                &key_package,
            )
            .await
            .map_err(AgentError::from)?;

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
        // Existing-device peer activation requires its own held signing/tree
        // custody. The old generic path must never discard the signed fence.
        if proposal.purpose.kind() == DeviceEpochRotationKind::Enrollment {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        }
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

    // The exact original activation owner and tree lease exclude local mutation.
    async fn authenticate_prepared_enrollment_activation(
        &self,
        ceremony: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
        decision: &crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<'_>,
    ) -> AgentResult<Option<AuthenticatedPreparedEnrollmentActivation>> {
        decision
            .require_tracker(&self.ceremony_tracker)
            .map_err(AgentError::from)?;
        decision
            .require_effects(self.effects.as_ref())
            .map_err(AgentError::from)?;
        let location = enrollment_activation_location(&ceremony.ceremony_id);
        if !self
            .effects
            .secure_exists(&location)
            .await
            .map_err(AgentError::from)?
        {
            return Ok(None);
        }
        let bytes = self
            .effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > 1_048_576 {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        }
        let raw: StoredEnrollmentActivation = from_slice(&bytes).map_err(map_decode_error)?;
        if raw.version != 3 {
            return Err(enrollment_fence_failure(
                EnrollmentEpochFenceError::UnsupportedSchema,
            ));
        }
        let fence = &raw.epoch_fence;
        let baseline = &raw.baseline;
        let proof = self
            .ceremony_tracker
            .verified_enrollment_response(&ceremony.ceremony_id)
            .await
            .map_err(AgentError::from)?;
        let aura_core::TreeOpKind::AddLeaf { leaf, under } = &raw.attested.op.op else {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        };
        let aura_core::TreeOpKind::RotateEpoch { affected } = &fence.op.op else {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        };
        if raw.subject != self.authority_id
            || raw.ceremony != ceremony.ceremony_id
            || raw.pending_epoch != ceremony.new_epoch
            || raw.prestate != ceremony.prestate_hash
            || raw.setup_digest != proof.setup_digest()
            || raw.manifest_digest != proof.acceptance().manifest_digest
            || Some(leaf.device_id) != ceremony.enrollment_device_id
            || leaf.role != LeafRole::Device
            || *under != NodeIndex(0)
            || affected.as_slice() != [NodeIndex(0)]
            || enrollment_activation_prestate(
                self.authority_id,
                baseline,
                &ceremony.participants,
                self.effects.device_id(),
                leaf.device_id,
            )? != raw.prestate
        {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        }
        let before = aura_journal::commitment_tree::reduce(baseline).map_err(map_internal_error)?;
        if before.epoch.value().checked_add(1) != Some(raw.pending_epoch) {
            return Err(enrollment_fence_failure(
                EnrollmentEpochFenceError::EpochMismatch,
            ));
        }
        let mut candidate = baseline.clone();
        candidate.push(raw.attested.clone());
        candidate.push(fence.clone());
        // The actual original parent packages verify both saved signatures. No
        // historical private-key signing or new epoch inference is used here.
        self.effects
            .collect_enrollment_parent_inventory(&candidate)
            .await
            .map_err(map_internal_error)?;
        let state =
            aura_journal::commitment_tree::reduce(&candidate).map_err(map_internal_error)?;
        if state.epoch.value() != raw.pending_epoch
            || ![self.effects.device_id(), leaf.device_id]
                .iter()
                .all(|device| {
                    state
                        .leaves
                        .values()
                        .any(|entry| entry.device_id == *device && entry.role == LeafRole::Device)
                })
        {
            return Err(enrollment_fence_failure(
                EnrollmentEpochFenceError::Membership,
            ));
        }
        Ok(Some(AuthenticatedPreparedEnrollmentActivation {
            effects: self.effects.clone(),
            tracker: self.ceremony_tracker.clone(),
            baseline_length: baseline.len(),
            candidate,
            stored: raw,
        }))
    }

    // The caller holds the original generation decision and tree lease through
    // authentication, installation, generation activation and terminal commit.
    async fn recover_prepared_enrollment_activation(
        &self,
        ceremony: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
        tree: &aura_protocol::handlers::tree::TreeDecisionLease<'_>,
        decision: &crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<'_>,
    ) -> AgentResult<Option<StoredEnrollmentActivation>> {
        let Some(authenticated) = self
            .authenticate_prepared_enrollment_activation(ceremony, decision)
            .await?
        else {
            return Ok(None);
        };
        self.install_prepared_enrollment(&authenticated, tree, decision)
            .await?;
        Ok(Some(authenticated.stored))
    }
    async fn install_prepared_enrollment(
        &self,
        authenticated: &AuthenticatedPreparedEnrollmentActivation,
        tree: &aura_protocol::handlers::tree::TreeDecisionLease<'_>,
        decision: &crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<'_>,
    ) -> AgentResult<()> {
        authenticated
            .require_original(decision)
            .map_err(AgentError::from)?;
        decision
            .require_tracker(&self.ceremony_tracker)
            .map_err(AgentError::from)?;
        decision
            .require_effects(self.effects.as_ref())
            .map_err(AgentError::from)?;
        let current = tree
            .install_authenticated_extension(
                authenticated.baseline_length,
                &authenticated.candidate,
            )
            .await
            .map_err(AgentError::from)?;
        self.effects
            .collect_enrollment_parent_inventory(&current)
            .await
            .map_err(map_internal_error)?;
        let state = aura_journal::commitment_tree::reduce(&current).map_err(map_internal_error)?;
        let aura_core::TreeOpKind::AddLeaf { leaf, .. } = &authenticated.stored.attested.op.op
        else {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        };
        if state.epoch.value() != authenticated.stored.pending_epoch
            || ![self.effects.device_id(), leaf.device_id]
                .iter()
                .all(|device| {
                    state
                        .leaves
                        .values()
                        .any(|entry| entry.device_id == *device && entry.role == LeafRole::Device)
                })
        {
            return Err(enrollment_fence_failure(
                EnrollmentEpochFenceError::Membership,
            ));
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "enrollment_activation",
        capability_type = EnrollmentActivationCapability,
        family = "runtime_helper"
    )]
    async fn finalize_enrollment<'a>(
        &'a self,
        owner: &DeviceEpochSigningOwner<'_>,
        activation: &crate::runtime::services::ceremony_tracker::EnrollmentActivationCapability<'_>,
    ) -> AgentResult<PreparedEnrollmentActivationCapability<'a>> {
        activation
            .require_tracker(&self.ceremony_tracker)
            .map_err(AgentError::from)?;
        activation
            .require_effects(self.effects.as_ref())
            .map_err(AgentError::from)?;
        let ceremony_id = activation.ceremony_id();
        let tree = self.effects.lock_tree_decision().await;
        self.ceremony_tracker
            .require_verified_enrollment_response(ceremony_id)
            .await
            .map_err(map_internal_error)?;
        let ceremony_state = self
            .ceremony_tracker
            .get(ceremony_id)
            .await
            .map_err(map_internal_error)?;

        activation
            .require_generation(self.authority_id, ceremony_state.new_epoch)
            .await
            .map_err(AgentError::from)?;
        if let Some(attested) = self
            .recover_prepared_enrollment_activation(&ceremony_state, &tree, activation.decision())
            .await?
        {
            return Ok(PreparedEnrollmentActivationCapability {
                effects: self.effects.as_ref(),
                tracker: self.ceremony_tracker.clone(),
                stored: attested,
                _tree: tree,
            });
        }
        let Some(device_id) = ceremony_state.enrollment_device_id else {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        };

        let baseline = self
            .effects
            .export_tree_ops()
            .await
            .map_err(map_internal_error)?;
        self.effects
            .collect_enrollment_parent_inventory(&baseline)
            .await
            .map_err(map_internal_error)?;
        let tree_state =
            aura_journal::commitment_tree::reduce(&baseline).map_err(map_internal_error)?;
        if enrollment_activation_prestate(
            self.authority_id,
            &baseline,
            &ceremony_state.participants,
            self.effects.device_id(),
            device_id,
        )? != ceremony_state.prestate_hash
        {
            return Err(enrollment_fence_failure(
                EnrollmentEpochFenceError::HistoryChanged,
            ));
        }
        if tree_state.epoch.value().checked_add(1) != Some(ceremony_state.new_epoch) {
            return Err(enrollment_fence_failure(
                EnrollmentEpochFenceError::EpochMismatch,
            ));
        }

        if tree_state
            .leaves
            .values()
            .any(|leaf| leaf.device_id == device_id)
        {
            return Err(AgentError::invalid(
                "enrollment device exists without owned activation evidence",
            ));
        }

        let participant = ParticipantIdentity::device(device_id);
        // Stored shares are encrypted envelopes; the signing service decrypts them.
        let key_package = self
            .signing_service
            .participant_key_package(&self.authority_id, ceremony_state.new_epoch, &participant)
            .await
            .map_err(map_internal_error)?;
        // A threshold-1 epoch holds single-signer Ed25519 packages, not FROST shares.
        let public_key_bytes = match share_from_key_package_bytes(&key_package) {
            Ok(share) => {
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
                public_key_package
                    .signer_public_keys
                    .get(&share.identifier)
                    .cloned()
                    .ok_or_else(|| {
                        AgentError::internal("missing verifying share for enrollment signer")
                    })?
            }
            Err(_) => aura_core::crypto::SingleSignerKeyPackage::import_from_secure_storage(
                &key_package,
                aura_core::secrets::SecretExportContext::secure_storage(
                    "aura-agent::device_epoch_rotation::finalize_enrollment",
                ),
            )
            .map_err(map_internal_error)?
            .verifying_key()
            .to_vec(),
        };

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
            .sign_rotation_context(
                owner,
                SigningContext::self_tree_op(self.authority_id, op.clone()),
            )
            .await
            .map_err(map_internal_error)?;
        let attested = AttestedOp {
            op,
            agg_sig: signature.signature,
            signer_count: signature.signer_count,
        };
        let mut candidate = baseline.clone();
        candidate.push(attested.clone());
        let after_leaf =
            aura_journal::commitment_tree::reduce(&candidate).map_err(map_internal_error)?;
        let fence_op = TreeOp {
            parent_epoch: after_leaf.epoch,
            parent_commitment: after_leaf.root_commitment,
            op: aura_core::TreeOpKind::RotateEpoch {
                affected: vec![NodeIndex(0)],
            },
            version: 1,
        };
        // The original active signing context signs this exact parent before
        // crypto activation. Quorum signing follows the actual signing service.
        let fence_signature = self
            .sign_rotation_context(
                owner,
                SigningContext::self_tree_op(self.authority_id, fence_op.clone()),
            )
            .await
            .map_err(map_internal_error)?;
        let epoch_fence = AttestedOp {
            op: fence_op,
            agg_sig: fence_signature.signature,
            signer_count: fence_signature.signer_count,
        };
        candidate.push(epoch_fence.clone());
        self.effects
            .collect_enrollment_parent_inventory(&candidate)
            .await
            .map_err(map_internal_error)?;
        let proof = self
            .ceremony_tracker
            .verified_enrollment_response(ceremony_id)
            .await
            .map_err(AgentError::from)?;
        let prepared = StoredEnrollmentActivation {
            version: 3,
            subject: self.authority_id,
            ceremony: ceremony_id.clone(),
            pending_epoch: ceremony_state.new_epoch,
            prestate: ceremony_state.prestate_hash,
            setup_digest: proof.setup_digest(),
            attested: attested.clone(),
            epoch_fence,
            baseline,
            manifest_digest: proof.acceptance().manifest_digest,
        };
        let bytes = to_vec(&prepared).map_err(map_encode_error)?;
        if bytes.len() > 1_048_576 {
            return Err(AgentError::invalid("oversized enrollment activation"));
        }
        // Required durable preparation precedes the irreversible tree mutation.
        let location = enrollment_activation_location(ceremony_id);
        let outcome = self
            .effects
            .secure_store_immutable(
                &location,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(AgentError::from)?;
        if outcome == aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists
            && self
                .effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await?
                != bytes
        {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        }

        #[cfg(all(test, not(target_arch = "wasm32")))]
        inject_activation_fault(self.effects.as_ref(), ceremony_id, false).await?;

        tree.install_authenticated_extension(prepared.baseline.len(), &candidate)
            .await
            .map_err(AgentError::from)?;
        #[cfg(all(test, not(target_arch = "wasm32")))]
        inject_activation_fault(self.effects.as_ref(), ceremony_id, true).await?;
        Ok(PreparedEnrollmentActivationCapability {
            effects: self.effects.as_ref(),
            tracker: self.ceremony_tracker.clone(),
            stored: prepared,
            _tree: tree,
        })
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RetainedEnrollmentVmControl", capability_type = crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl, family = "runtime_helper")]
    async fn commit_original_local_rotation(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        activation: &crate::runtime::services::ceremony_tracker::EnrollmentActivationCapability<'_>,
    ) -> AgentResult<()> {
        let generation = crate::handlers::invitation::enrollment_trust::restore_original_pending_signing_generation(
            self.effects.as_ref(), &self.signing_service, issued, original, activation,
        ).await?;
        self.signing_service
            .commit_verified_pending_generation(&generation, activation)
            .await?;
        Ok(())
    }

    async fn commit_ordinary_local_rotation(&self, ceremony_id: &CeremonyId) -> AgentResult<()> {
        let ceremony_state = self
            .ceremony_tracker
            .get(ceremony_id)
            .await
            .map_err(map_internal_error)?;
        if ceremony_state.kind == aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment {
            return Err(enrollment_fence_failure(EnrollmentEpochFenceError::Binding));
        }
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
        if payload.len() > MAX_DEVICE_EPOCH_COMMIT_BYTES {
            return Err(AgentError::invalid("oversized device epoch commit"));
        }
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
        if bytes.len() > MAX_DEVICE_EPOCH_COMMIT_BYTES {
            return Err(AgentError::invalid("oversized device epoch commit"));
        }
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

impl DeviceEpochRotationService {
    /// This device's acceptance of `proposal`, proven with its current share
    /// of the authority (the initiator checks it against that device's
    /// verifying share in its own retained package).
    async fn build_signed_acceptance(
        &self,
        proposal: &DeviceEpochProposal,
    ) -> AgentResult<DeviceEpochAcceptance> {
        use aura_signature::SecurityTranscript;
        let accepted_at_ms = self
            .effects
            .physical_time()
            .await
            .map_err(map_internal_error)?
            .ts_ms;
        let payload = DeviceEpochAcceptanceTranscriptPayload {
            ceremony_id: proposal.ceremony_id.clone(),
            acceptor_device_id: self.effects.device_id(),
            proposal_hash: device_epoch_proposal_hash(proposal).map_err(map_internal_error)?,
            accepted_at_ms,
        };
        let message = DeviceEpochAcceptanceTranscript::from_payload(payload.clone())
            .transcript_bytes()
            .map_err(map_internal_error)?;
        let possession = self
            .signing_service
            .prove_device_possession(&self.authority_id, &message)
            .await
            .map_err(AgentError::from)?;
        Ok(DeviceEpochAcceptance {
            ceremony_id: payload.ceremony_id,
            acceptor_device_id: payload.acceptor_device_id,
            proposal_hash: payload.proposal_hash,
            accepted_at_ms: payload.accepted_at_ms,
            signing_epoch: Some(possession.epoch),
            signing_mode: Some(aura_core::crypto::single_signer::SigningMode::Threshold),
            signing_index: Some(possession.index),
            signing_package_digest: Some(possession.package_digest),
            signature: possession.proof,
        })
    }

    /// Admit a participant's acceptance of `proposal`: it names this
    /// ceremony, the requested participant device and the exact proposal, and
    /// carries a valid proof of that device's current share.
    async fn verified_device_epoch_acceptance(
        &self,
        request: &DeviceEpochRotationInitRequest,
        proposal: &DeviceEpochProposal,
        acceptance: DeviceEpochAcceptance,
    ) -> AgentResult<VerifiedIngress<DeviceEpochAcceptance>> {
        use aura_signature::SecurityTranscript;
        if let DeviceEpochRotationOrigin::Enrollment(context) = &request.origin {
            context.require_proposal(self.effects.as_ref(), request, proposal)?;
        }
        if acceptance.ceremony_id != request.ceremony_id
            || acceptance.acceptor_device_id != request.participant_device_id
            || acceptance.proposal_hash
                != device_epoch_proposal_hash(proposal).map_err(map_internal_error)?
        {
            return Err(AgentError::invalid(
                "device epoch acceptance does not bind this ceremony, device and proposal",
            ));
        }
        let (
            Some(aura_core::crypto::single_signer::SigningMode::Threshold),
            Some(epoch),
            Some(index),
            Some(package_digest),
        ) = (
            acceptance.signing_mode,
            acceptance.signing_epoch,
            acceptance.signing_index,
            acceptance.signing_package_digest,
        )
        else {
            return Err(AgentError::invalid(
                "device epoch acceptance lacks its participant-device proof",
            ));
        };
        let message = DeviceEpochAcceptanceTranscript::new(&acceptance)
            .transcript_bytes()
            .map_err(map_internal_error)?;
        let claimed = crate::runtime::services::threshold_signing::DevicePossessionProof {
            epoch,
            index,
            package_digest,
            proof: acceptance.signature.clone(),
        };
        match &request.origin {
            DeviceEpochRotationOrigin::Enrollment(context) => {
                context
                    .original
                    .execute(self.effects.as_ref(), || async {
                        self.signing_service
                            .verify_original_enrollment_peer_possession(
                                &context.issued,
                                &context.original,
                                acceptance.acceptor_device_id,
                                &claimed,
                                &message,
                            )
                            .map_err(AgentError::from)
                    })
                    .await
                    .map_err(|source| {
                        context
                            .original
                            .map_run_error("verify original enrollment peer acceptance", source)
                    })?;
            }
            DeviceEpochRotationOrigin::Rotation | DeviceEpochRotationOrigin::Removal => {
                self.signing_service
                    .verify_device_possession(
                        &self.authority_id,
                        acceptance.acceptor_device_id,
                        &claimed,
                        &message,
                    )
                    .await
                    .map_err(AgentError::from)?;
            }
        }
        let encoded = to_vec(&acceptance).map_err(map_encode_error)?;
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Device(acceptance.acceptor_device_id),
            aura_core::types::identifiers::ContextId::new_from_entropy(hash::hash(
                request.ceremony_id.as_str().as_bytes(),
            )),
            None,
            Hash32::from_bytes(&encoded),
            aura_protocol::messages::WIRE_FORMAT_VERSION,
        );
        let evidence = IngressVerificationEvidence::builder(metadata)
            .peer_identity(true, "acceptance names the requested participant device")
            .and_then(|builder| {
                builder.envelope_authenticity(true, "acceptance carries a verified share proof")
            })
            .and_then(|builder| {
                builder.capability_authorization(true, "acceptor is in the retained device roster")
            })
            .and_then(|builder| builder.namespace_scope(true, "acceptance binds this ceremony"))
            .and_then(|builder| builder.schema_version(true, "acceptance schema"))
            .and_then(|builder| builder.replay_freshness(true, "acceptance binds this proposal"))
            .and_then(|builder| {
                builder.signer_membership(true, "proof verifies against the device's share")
            })
            .and_then(|builder| builder.proof_evidence(true, "participant possession proof"))
            .and_then(|builder| builder.build())
            .map_err(|error| {
                AgentError::internal(format!("verify device epoch acceptance: {error}"))
            })?;
        DecodedIngress::new(acceptance, evidence.metadata().clone())
            .verify(evidence)
            .map_err(|error| {
                AgentError::internal(format!("promote device epoch acceptance: {error}"))
            })
    }
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

fn map_internal_error(error: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    AgentError::from(aura_core::AuraError::Internal {
        message: "device epoch rotation internal".into(),
        source: Some(std::sync::Arc::new(error)),
    })
}

fn map_encode_error(error: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    AgentError::from(aura_core::AuraError::Internal {
        message: "device epoch rotation encode".into(),
        source: Some(std::sync::Arc::new(error)),
    })
}

fn map_decode_error(error: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    AgentError::from(aura_core::AuraError::Internal {
        message: "device epoch rotation decode".into(),
        source: Some(std::sync::Arc::new(error)),
    })
}

fn map_session_error(error: SessionIngressError) -> AgentError {
    AgentError::from(aura_core::AuraError::Internal {
        message: "device epoch rotation session failed".into(),
        source: Some(std::sync::Arc::new(error)),
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) struct OriginalPreparedActivationInterruption {
    profile: std::path::PathBuf,
    ceremony: CeremonyId,
    pub(crate) after_tree: bool,
    pub(crate) source: std::sync::Arc<std::io::Error>,
}
#[cfg(all(test, not(target_arch = "wasm32")))]
impl OriginalPreparedActivationInterruption {
    pub(crate) fn require_original(
        &self,
        effects: &AuraEffectSystem,
        ceremony: &CeremonyId,
    ) -> Result<(), aura_core::AuraError> {
        if self.profile != effects.config().storage.base_path || &self.ceremony != ceremony {
            return Err(aura_core::AuraError::invalid(
                "foreign original prepared interruption",
            ));
        }
        Ok(())
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
struct PreparedActivationFault {
    after_tree: bool,
    readiness: Option<tokio::sync::oneshot::Sender<OriginalPreparedActivationInterruption>>,
}
#[cfg(all(test, not(target_arch = "wasm32")))]
fn activation_faults() -> &'static async_lock::Mutex<
    std::collections::HashMap<(std::path::PathBuf, CeremonyId), PreparedActivationFault>,
> {
    static FAULTS: std::sync::OnceLock<
        async_lock::Mutex<
            std::collections::HashMap<(std::path::PathBuf, CeremonyId), PreparedActivationFault>,
        >,
    > = std::sync::OnceLock::new();
    FAULTS.get_or_init(Default::default)
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) async fn observe_activation_fault_for_test(
    effects: &AuraEffectSystem,
    ceremony: CeremonyId,
    after_tree: bool,
) -> tokio::sync::oneshot::Receiver<OriginalPreparedActivationInterruption> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    activation_faults().lock().await.insert(
        (effects.config().storage.base_path.clone(), ceremony),
        PreparedActivationFault {
            after_tree,
            readiness: Some(sender),
        },
    );
    receiver
}
#[cfg(all(test, not(target_arch = "wasm32")))]
async fn inject_activation_fault(
    effects: &AuraEffectSystem,
    ceremony: &CeremonyId,
    after_tree: bool,
) -> AgentResult<()> {
    let owner = (effects.config().storage.base_path.clone(), ceremony.clone());
    let fault = {
        let mut faults = activation_faults().lock().await;
        if faults
            .get(&owner)
            .is_some_and(|fault| fault.after_tree == after_tree)
        {
            faults.remove(&owner)
        } else {
            None
        }
    };
    if let Some(fault) = fault {
        let source = std::sync::Arc::new(std::io::Error::from(std::io::ErrorKind::Interrupted));
        if let Some(readiness) = fault.readiness {
            let _ = readiness.send(OriginalPreparedActivationInterruption {
                profile: owner.0,
                ceremony: owner.1,
                after_tree,
                source: source.clone(),
            });
        }
        return Err(AgentError::from(aura_core::AuraError::Internal {
            message: "injected activation interruption".into(),
            source: Some(source),
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::effects::{SecureStorageEffects, ThresholdSigningEffects};
    use aura_core::threshold::ThresholdConfig;
    use std::sync::Arc;

    #[tokio::test]
    async fn retained_participant_admission_keeps_selected_start_fault_source() {
        use crate::runtime::subsystems::transport::QueueEnvelopeOutcome;
        use std::error::Error;

        let selected = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let service = test_service_with_provider(0xe7, Some(Arc::new(selected.clone()))).await;
        let session_id = uuid::Uuid::from_bytes([0xe8; 16]).to_string();
        let original = TransportEnvelope {
            source: service.authority_id,
            destination: service.authority_id,
            context: aura_core::ContextId::new_from_entropy([0xe7; 32]),
            payload: vec![0xa5],
            metadata: std::collections::HashMap::from([
                (
                    "content-type".into(),
                    "application/aura-choreography".into(),
                ),
                ("session-id".into(), session_id),
                ("protocol-id".into(), PROTOCOL_ID.into()),
                (
                    "aura-source-device-id".into(),
                    test_device(0xe9).to_string(),
                ),
            ]),
            receipt: None,
        };
        assert_eq!(
            service.effects.queue_runtime_envelope(original.clone()),
            QueueEnvelopeOutcome::Queued
        );
        selected
            .fail_next_observation(aura_core::effects::TimeError::OperationFailed {
                reason: "participant selected start read fault".into(),
            })
            .await;
        let error = service
            .process_pending_participant_sessions()
            .await
            .expect_err("actual retained participant must propagate selected start fault");
        let mut cursor: &(dyn Error + 'static) = &error;
        let mut ingress = false;
        let mut choreography = false;
        let mut physical = false;
        loop {
            ingress |= cursor.is::<SessionIngressError>();
            choreography |= cursor.is::<aura_protocol::effects::ChoreographyError>();
            physical |= matches!(
                cursor.downcast_ref::<aura_core::effects::TimeError>(),
                Some(aura_core::effects::TimeError::OperationFailed { reason })
                    if reason == "participant selected start read fault"
            );
            let Some(source) = cursor.source() else { break };
            cursor = source;
        }
        assert!(ingress && choreography && physical);
        let restored = service.effects.peek_queued_choreography_envelopes();
        assert_eq!(restored.len(), 1, "exactly the original packet returns");
        assert_eq!(restored[0].payload, original.payload);
        assert_eq!(restored[0].metadata, original.metadata);
        assert_eq!(restored[0].source, original.source);
        assert_eq!(restored[0].destination, original.destination);
        assert_eq!(restored[0].context, original.context);
        assert!(restored[0].receipt.is_none());
        assert!(service.effects.vm_fragment_snapshot().is_empty());
    }

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_device(seed: u8) -> DeviceId {
        DeviceId::new_from_entropy([seed; 32])
    }

    async fn test_service(seed: u8) -> DeviceEpochRotationService {
        test_service_with_provider(seed, None).await
    }

    async fn test_service_with_provider(
        seed: u8,
        provider: Option<Arc<dyn PhysicalTimeEffects>>,
    ) -> DeviceEpochRotationService {
        let authority_id = test_authority(seed);
        let config = AgentConfig {
            device_id: test_device(seed.wrapping_add(1)),
            ..Default::default()
        };
        let effects = AuraEffectSystem::simulation_for_test_for_authority_with_salt(
            &config,
            authority_id,
            u64::from(seed),
        )
        .expect("effect system");
        let effects = Arc::new(match provider {
            Some(provider) => effects.with_physical_time_provider(provider),
            None => effects,
        });
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

    #[tokio::test]
    async fn malformed_selected_rotation_frame_is_consumed_without_replay() {
        use crate::runtime::subsystems::transport::QueueEnvelopeOutcome;
        let service = test_service(79).await;
        let frame = TransportEnvelope {
            source: test_authority(80),
            destination: service.authority_id,
            context: aura_core::ContextId::new_from_entropy([79; 32]),
            payload: vec![1],
            metadata: std::collections::HashMap::from([
                (
                    "content-type".to_string(),
                    "application/aura-choreography".to_string(),
                ),
                ("protocol-id".to_string(), PROTOCOL_ID.to_string()),
            ]),
            receipt: None,
        };
        assert_eq!(
            service.effects.queue_runtime_envelope(frame),
            QueueEnvelopeOutcome::Queued
        );
        let error = service
            .process_pending_participant_sessions()
            .await
            .expect_err("missing session and device evidence must be rejected");
        assert!(error.to_string().contains("aura-source-device-id"));
        assert!(matches!(
            service
                .effects
                .take_inbound_envelope(is_device_epoch_rotation_envelope),
            Err(TransportError::NoMessage)
        ));
    }

    struct HeldVmOpenClock {
        selected: aura_testkit::time::ManualPhysicalClock,
        hold: tokio::sync::Mutex<
            Option<(
                tokio::sync::oneshot::Sender<()>,
                tokio::sync::oneshot::Receiver<()>,
            )>,
        >,
    }

    impl HeldVmOpenClock {
        fn new() -> Self {
            Self {
                selected: aura_testkit::time::ManualPhysicalClock::new(5_000),
                hold: tokio::sync::Mutex::new(None),
            }
        }

        async fn hold_next_observation(
            &self,
        ) -> (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ) {
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            *self.hold.lock().await = Some((entered_tx, release_rx));
            (entered_rx, release_tx)
        }
    }

    #[async_trait::async_trait]
    impl PhysicalTimeEffects for HeldVmOpenClock {
        async fn physical_time(
            &self,
        ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
            if let Some((entered, release)) = self.hold.lock().await.take() {
                let _ = entered.send(());
                let _ = release.await;
            }
            self.selected.physical_time().await
        }

        async fn sleep_ms(&self, ms: u64) -> Result<(), aura_core::effects::TimeError> {
            self.selected.sleep_ms(ms).await
        }

        async fn wait_until_physical_deadline(
            &self,
            deadline: aura_core::types::window::WindowPosition<
                aura_core::types::window::PhysicalMillis,
            >,
        ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
            self.selected.wait_until_physical_deadline(deadline).await
        }
    }

    #[tokio::test]
    async fn participant_vm_open_cancel_restores_original_session_slot() {
        use crate::runtime::subsystems::transport::QueueEnvelopeOutcome;
        let clock = Arc::new(HeldVmOpenClock::new());
        let service = Arc::new(test_service_with_provider(77, Some(clock.clone())).await);
        let effects = service.effects.clone();
        let (entered, _release) = clock.hold_next_observation().await;
        let session_id = Uuid::from_bytes([7; 16]).to_string();
        for index in 0..256usize {
            let mut metadata = std::collections::HashMap::from([
                (
                    "content-type".to_string(),
                    "application/aura-choreography".to_string(),
                ),
                ("session-id".to_string(), session_id.clone()),
            ]);
            if index == 7 {
                metadata.insert("protocol-id".into(), PROTOCOL_ID.into());
                metadata.insert("aura-source-device-id".into(), test_device(79).to_string());
            }
            let frame = TransportEnvelope {
                source: test_authority(77),
                destination: service.authority_id,
                context: aura_core::ContextId::new_from_entropy([77; 32]),
                payload: vec![index as u8],
                metadata,
                receipt: None,
            };
            assert_eq!(
                effects.queue_runtime_envelope(frame),
                QueueEnvelopeOutcome::Queued
            );
        }
        let participant =
            tokio::spawn(async move { service.process_pending_participant_sessions().await });
        entered
            .await
            .expect("selected provider reached actual VM session start");
        let extra = TransportEnvelope {
            source: test_authority(77),
            destination: test_authority(77),
            context: aura_core::ContextId::new_from_entropy([77; 32]),
            payload: vec![0xEE],
            metadata: std::collections::HashMap::from([
                (
                    "content-type".to_string(),
                    "application/aura-choreography".to_string(),
                ),
                ("session-id".to_string(), session_id),
            ]),
            receipt: None,
        };
        assert_eq!(
            effects.queue_runtime_envelope(extra),
            QueueEnvelopeOutcome::Queued,
            "session-local ingress is not the bounded transport inbox"
        );
        participant.abort();
        assert!(participant
            .await
            .expect_err("actual participant cancellation")
            .is_cancelled());
        let restored = effects.peek_queued_choreography_envelopes();
        assert_eq!(restored.len(), 257);
        for (index, envelope) in restored.iter().take(256).enumerate() {
            assert_eq!(envelope.payload, [index as u8]);
        }
        assert_eq!(restored[256].payload, [0xEE]);
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
            origin: DeviceEpochRotationOrigin::Rotation,
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
            signing_epoch: None,
            signing_mode: None,
            signing_index: None,
            signing_package_digest: None,
            signature: vec![1; 64],
        };

        let error = service
            .verified_device_epoch_acceptance(&request, &proposal, acceptance)
            .await
            .expect_err("acceptance without a participant-device proof must fail closed");
        assert!(error
            .to_string()
            .contains("lacks its participant-device proof"));
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
            .build_signed_commit(&DeviceEpochSigningOwner::Ordinary, &proposal, None, None)
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
            .build_signed_commit(&DeviceEpochSigningOwner::Ordinary, &proposal, None, None)
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
    #[test]
    fn enrollment_activation_requires_fence_baseline_and_manifest_before_resume() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            for field in ["epoch_fence", "baseline", "manifest_digest"] {
                for null in [false, true] {
                    let label = format!("activation-required-{field}-{null}");
                    let (issuer, _invitee, _invitation, start, _accept, proof) = Box::pin(
                        crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                            &label,
                        ),
                    )
                    .await;
                    let effects = issuer.runtime().effects();
                    let tracker = issuer.runtime().ceremony_tracker();
                    let state = tracker
                        .get(&start.ceremony_id)
                        .await
                        .expect("actual registration");
                    let original = effects
                        .export_tree_ops()
                        .await
                        .expect("actual original history");
                    // Actual exported attestations exercise the persisted codec only;
                    // this record is never admitted as an approved activation.
                    let attested = original.first().expect("bootstrap attestation").clone();
                    let current = StoredEnrollmentActivation {
                        version: 3,
                        subject: issuer.authority_id(),
                        ceremony: start.ceremony_id.clone(),
                        pending_epoch: state.new_epoch,
                        prestate: state.prestate_hash,
                        setup_digest: proof.setup_digest(),
                        attested: attested.clone(),
                        epoch_fence: attested,
                        baseline: original.clone(),
                        manifest_digest: proof.acceptance().manifest_digest,
                    };
                    let mut wire = serde_json::to_value(current).expect("current persisted fields");
                    if null {
                        wire[field] = serde_json::Value::Null;
                    } else {
                        wire.as_object_mut().expect("record").remove(field);
                    }
                    let bytes = to_vec(&wire).expect("canonical incomplete record");
                    assert!(from_slice::<StoredEnrollmentActivation>(&bytes).is_err());
                    let location = enrollment_activation_location(&start.ceremony_id);
                    effects
                        .secure_store_immutable(
                            &location,
                            &bytes,
                            &[
                                SecureStorageCapability::Read,
                                SecureStorageCapability::Write,
                            ],
                        )
                        .await
                        .expect("actual persisted malformed record");
                    let service = DeviceEpochRotationService::new(
                        issuer.authority_id(),
                        effects.clone(),
                        tracker.clone(),
                        issuer.runtime().ceremony_runner().clone(),
                        issuer.runtime().threshold_signing(),
                        issuer.runtime().reconfiguration().clone(),
                    );
                    let acquisition =
                        crate::runtime::services::RuntimeServiceContext::test_original(
                            issuer.runtime().tasks(),
                            effects.clone(),
                        )
                        .await;
                    let error = service
                        .recover_prepared_enrollment(&acquisition, &state.ceremony_id)
                        .await
                        .expect_err("missing or null persisted field cannot resume activation");
                    let mut cause: &(dyn std::error::Error + 'static) = &error;
                    loop {
                        if cause
                            .downcast_ref::<aura_core::util::serialization::SerializationError>()
                            .is_some()
                        {
                            break;
                        }
                        cause = cause.source().expect(
                            "actual persisted decoder source must survive bounded recovery",
                        );
                    }
                    assert_eq!(
                        effects
                            .secure_retrieve(&location, &[SecureStorageCapability::Read])
                            .await
                            .expect("unchanged activation"),
                        bytes
                    );
                    assert_eq!(
                        effects
                            .export_tree_ops()
                            .await
                            .expect("unchanged original history"),
                        original
                    );
                    assert_eq!(
                        issuer
                            .runtime()
                            .ceremony_runner()
                            .terminal_outcome(&start.ceremony_id)
                            .await
                            .expect("original ceremony"),
                        None
                    );
                }
            }
        });
    }
}
