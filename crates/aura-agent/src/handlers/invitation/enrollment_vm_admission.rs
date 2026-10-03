//! Ceremony-local control authentication. These proofs authorize only the
//! exact independently admitted enrollment; they grant no global peer identity.
use super::enrollment_manifest_admission::AdmittedEnrollmentManifest;
use super::enrollment_trust::RetainedEnrollmentVmControl;
use super::{AgentError, AgentResult, DeviceEnrollmentAcceptanceTranscript};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{
    CryptoCoreEffects, PhysicalTimeEffects, SecureStorageCapability, SecureStorageEffects,
    SecureStorageLocation, ThresholdSigningEffects,
};
use aura_core::threshold::SigningContext;
use aura_guards::GuardContextProvider;
use aura_invitation::enrollment_manifest::EnrollmentManifestError;
use aura_invitation::protocol::{
    DeviceEnrollmentAccept, DeviceEnrollmentConfirm, DeviceEnrollmentRequest,
};
use aura_signature::SecurityTranscript;
use serde::{Deserialize, Serialize};
pub(super) const MAX_CONTROL_FRAME_BYTES: usize = 1_048_576;

#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentVmAdmissionError {
    #[error("enrollment control does not match the admitted ceremony")]
    Binding,
    #[error("enrollment control signature is invalid")]
    Signature,
    #[error("enrollment setup signing identity changed after admission")]
    SigningIdentityChanged,
    #[error("enrollment confirmation issuer or target lacks current committed membership")]
    CurrentMembership,

    #[error("enrollment activation failed: {0:?}")]
    TerminalFailed(aura_app::runtime_bridge::CeremonyFailureReason),
    #[error("enrollment control stage failed")]
    Stage(#[source] Box<dyn std::error::Error + Send + Sync>),
}
pub(super) fn terminal_failure(
    reason: aura_app::runtime_bridge::CeremonyFailureReason,
) -> AgentError {
    failure(EnrollmentVmAdmissionError::TerminalFailed(reason))
}
fn failure(error: EnrollmentVmAdmissionError) -> AgentError {
    aura_core::AuraError::crypto_with_source(
        "enrollment VM admission failed",
        std::sync::Arc::new(error),
    )
    .into()
}
#[derive(Clone, Copy)]
enum StageKind {
    Invalid,
    NotFound,
    PermissionDenied,
    Crypto,
    Network,
    Serialization,
    Storage,
    Internal,
}
impl From<&aura_core::AuraError> for StageKind {
    fn from(error: &aura_core::AuraError) -> Self {
        use aura_core::AuraError;
        match error {
            AuraError::Invalid { .. } => Self::Invalid,
            AuraError::NotFound { .. } => Self::NotFound,
            AuraError::PermissionDenied { .. } => Self::PermissionDenied,
            AuraError::Crypto { .. } => Self::Crypto,
            AuraError::Network { .. } => Self::Network,
            AuraError::Serialization { .. } => Self::Serialization,
            AuraError::Storage { .. } => Self::Storage,
            AuraError::Internal { .. } | AuraError::Terminal(_) => Self::Internal,
        }
    }
}
fn stage(error: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    use aura_core::AuraError;
    use std::error::Error;
    let error: Box<dyn Error + Send + Sync> = Box::new(error);
    let mut cause: Option<&(dyn Error + 'static)> = Some(error.as_ref());
    let mut kind = StageKind::Internal;
    while let Some(source) = cause {
        if let Some(native) = source.downcast_ref::<AuraError>() {
            kind = StageKind::from(native);
            break;
        }
        if source
            .downcast_ref::<aura_core::effects::TimeError>()
            .is_some()
        {
            kind = StageKind::Internal;
            break;
        }
        if source
            .downcast_ref::<aura_core::util::serialization::SerializationError>()
            .is_some()
            || source
                .downcast_ref::<std::array::TryFromSliceError>()
                .is_some()
        {
            kind = StageKind::Serialization;
            break;
        }
        if source
            .downcast_ref::<aura_core::effects::CryptoError>()
            .is_some()
        {
            kind = StageKind::Crypto;
            break;
        }
        cause = source.source();
    }
    let message = "enrollment VM required stage failed".to_owned();
    let source: Option<std::sync::Arc<dyn Error + Send + Sync>> = Some(std::sync::Arc::new(
        EnrollmentVmAdmissionError::Stage(error),
    ));
    match kind {
        StageKind::Invalid => AuraError::Invalid { message, source },
        StageKind::NotFound => AuraError::NotFound { message, source },
        StageKind::PermissionDenied => AuraError::PermissionDenied { message, source },
        StageKind::Crypto => AuraError::Crypto { message, source },
        StageKind::Network => AuraError::Network { message, source },
        StageKind::Serialization => AuraError::Serialization { message, source },
        StageKind::Storage => AuraError::Storage { message, source },
        StageKind::Internal => AuraError::Internal { message, source },
    }
    .into()
}
fn transcript_stage(error: aura_signature::AuthenticationError) -> AgentError {
    aura_core::AuraError::Serialization {
        message: "encode enrollment control transcript".into(),
        source: Some(std::sync::Arc::new(EnrollmentVmAdmissionError::Stage(
            Box::new(error),
        ))),
    }
    .into()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
enum EnrollmentControlDecision {
    Request(DeviceEnrollmentRequest),
    Committed(DeviceEnrollmentConfirm),
    Failed {
        invitation: aura_core::InvitationId,
        ceremony: aura_core::CeremonyId,
        reason: aura_app::runtime_bridge::CeremonyFailureReason,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnrollmentControlFrame {
    version: u16,
    manifest_digest: [u8; 32],
    decision: EnrollmentControlDecision,
    signature: Vec<u8>,
    #[serde(default)]
    committed_ops: Option<Vec<aura_core::AttestedOp>>,
}
/// Non-serializable commit evidence minted only after the actual issuer-pinned
/// Committed frame verifies against the independently admitted ceremony.
/// Cached invitation status and arbitrary subject/device fields cannot mint it.
pub(crate) struct VerifiedEnrollmentConfirmation {
    frame: Box<EnrollmentControlFrame>,
    canonical_invitation: Box<super::Invitation>,
    manifest: Box<aura_invitation::enrollment_manifest::EnrollmentTrustManifest>,
    manifest_digest: [u8; 32],
    confirmed_at_ms: u64,
    committed_transition: VerifiedEnrollmentCommittedTransition,
}
/// Exact authenticated ordered baseline and post-commit suffix. Only successful
/// verification against the independent admission can construct this value.
/// It proves the original signed checkpoint, never absence of future revocation.
pub(crate) struct VerifiedEnrollmentCommittedTransition {
    ops: Vec<aura_core::AttestedOp>,
    state: Box<aura_journal::commitment_tree::state::TreeState>,
}
impl VerifiedEnrollmentCommittedTransition {
    pub(crate) fn ops(&self) -> &[aura_core::AttestedOp] {
        &self.ops
    }
    pub(crate) fn state(&self) -> &aura_journal::commitment_tree::state::TreeState {
        &self.state
    }
}

/// Verified cryptographic history; it does not prove global freshness and is
/// not an activation capability without the actual runtime tree owner lease.
pub(crate) struct VerifiedEnrollmentTreeExtension {
    state: Box<aura_journal::commitment_tree::state::TreeState>,
}
impl VerifiedEnrollmentTreeExtension {
    pub(crate) fn state(&self) -> &aura_journal::commitment_tree::state::TreeState {
        &self.state
    }
}

impl std::fmt::Debug for VerifiedEnrollmentConfirmation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedEnrollmentConfirmation")
            .field("ceremony", &self.manifest.ceremony)
            .field("device", &self.manifest.invitee_device)
            .field("pending_epoch", &self.manifest.pending_epoch)
            .finish_non_exhaustive()
    }
}
/// An authenticated negative owner decision, never signing/adoption authority.
/// Only pinned control verification mints it; fields and constructors are private.
pub(crate) struct VerifiedEnrollmentFailureCapability {
    frame: Box<EnrollmentControlFrame>,
    canonical_invitation: Box<super::Invitation>,
    manifest: Box<aura_invitation::enrollment_manifest::EnrollmentTrustManifest>,
    manifest_digest: [u8; 32],
    observed_at_ms: u64,
    reason: aura_app::runtime_bridge::CeremonyFailureReason,
}
impl VerifiedEnrollmentFailureCapability {
    pub(crate) fn manifest(
        &self,
    ) -> &aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
        &self.manifest
    }
    pub(crate) fn canonical_invitation(&self) -> &super::Invitation {
        &self.canonical_invitation
    }
    pub(crate) fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }
    pub(crate) fn reason(&self) -> aura_app::runtime_bridge::CeremonyFailureReason {
        self.reason
    }
    pub(super) fn frame(&self) -> &EnrollmentControlFrame {
        &self.frame
    }
    pub(crate) fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }
}
pub(super) enum VerifiedEnrollmentTerminal {
    Committed(VerifiedEnrollmentConfirmation),
    Failed(VerifiedEnrollmentFailureCapability),
}

impl VerifiedEnrollmentConfirmation {
    pub(crate) fn manifest(
        &self,
    ) -> &aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
        &self.manifest
    }
    pub(crate) fn canonical_invitation(&self) -> &super::Invitation {
        &self.canonical_invitation
    }
    pub(crate) fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }
    /// Authenticated original committed checkpoint; does not prove absence of later revocation.
    #[cfg(test)]
    pub(crate) fn committed_state(&self) -> &aura_journal::commitment_tree::state::TreeState {
        self.committed_transition.state()
    }
    pub(crate) fn committed_transition(&self) -> &VerifiedEnrollmentCommittedTransition {
        &self.committed_transition
    }
    /// Crypto verification alone; the runtime binds this result to its held
    /// actual tree decision lease before it can authorize local activation.
    pub(crate) fn verify_local_extension(
        &self,
        current: &[aura_core::AttestedOp],
    ) -> AgentResult<VerifiedEnrollmentTreeExtension> {
        let original = self.committed_transition.ops();
        if current.len() < original.len() {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        for (left, right) in current.iter().zip(original) {
            if aura_core::util::serialization::to_vec(left).map_err(stage)?
                != aura_core::util::serialization::to_vec(right).map_err(stage)?
            {
                return Err(failure(EnrollmentVmAdmissionError::Binding));
            }
        }
        let mut ops = original.to_vec();
        let mut state = self.committed_transition.state().clone();
        for op in current.iter().skip(original.len()) {
            let target = aura_core::tree::verification::extract_target_node(&op.op.op)
                .or_else(|| match &op.op.op {
                    aura_core::TreeOpKind::RemoveLeaf { leaf, .. } => {
                        state.get_remove_leaf_affected_parent(leaf)
                    }
                    _ => None,
                })
                .ok_or_else(|| failure(EnrollmentVmAdmissionError::Binding))?;
            check_admitted_node_operation(
                &self.manifest,
                &self.canonical_invitation,
                &state,
                op,
                target,
            )?;
            ops.push(op.clone());
            state = aura_journal::commitment_tree::reduce(&ops).map_err(stage)?;
            retain_admitted_node_keys(&self.manifest, &self.canonical_invitation, &mut state)?;
        }
        Ok(VerifiedEnrollmentTreeExtension {
            state: Box::new(state),
        })
    }
    pub(super) fn frame(&self) -> &EnrollmentControlFrame {
        &self.frame
    }
    pub(super) fn confirmed_at_ms(&self) -> u64 {
        self.confirmed_at_ms
    }
}

#[derive(Clone, Serialize)]
struct EnrollmentControlTranscript {
    version: u16,
    manifest_digest: [u8; 32],
    decision: EnrollmentControlDecision,
    committed_ops: Option<Vec<aura_core::AttestedOp>>,
}
impl SecurityTranscript for EnrollmentControlTranscript {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.device-enrollment-control.v2";
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}
impl EnrollmentControlFrame {
    pub(super) fn decode(bytes: &[u8]) -> AgentResult<Self> {
        if bytes.len() > MAX_CONTROL_FRAME_BYTES {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        aura_core::util::serialization::from_slice(bytes).map_err(stage)
    }
    fn transcript(&self) -> EnrollmentControlTranscript {
        EnrollmentControlTranscript {
            version: self.version,
            manifest_digest: self.manifest_digest,
            decision: self.decision.clone(),
            committed_ops: self.committed_ops.clone(),
        }
    }
    async fn verify(
        &self,
        effects: &AuraEffectSystem,
        admitted: &AdmittedEnrollmentManifest,
    ) -> AgentResult<()> {
        self.verify_at(
            effects,
            admitted,
            effects.physical_time().await.map_err(stage)?.ts_ms,
        )
        .await
    }
    async fn verify_at(
        &self,
        effects: &AuraEffectSystem,
        admitted: &AdmittedEnrollmentManifest,
        now: u64,
    ) -> AgentResult<()> {
        if self.version != 2
            || self.manifest_digest != admitted.manifest_digest()
            || self.signature.len() != 64
        {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        if now >= admitted.manifest().expires_at_ms {
            return Err(stage(EnrollmentManifestError::Expired));
        }
        let bytes = self
            .transcript()
            .transcript_bytes()
            .map_err(transcript_stage)?;
        if !effects
            .ed25519_verify(
                &bytes,
                &self.signature,
                &admitted.manifest().initiator_confirmation_verifier,
            )
            .await
            .map_err(stage)?
        {
            return Err(failure(EnrollmentVmAdmissionError::Signature));
        }
        Ok(())
    }
    pub(super) async fn verify_request(
        &self,
        effects: &AuraEffectSystem,
        admitted: &AdmittedEnrollmentManifest,
    ) -> AgentResult<DeviceEnrollmentRequest> {
        self.verify(effects, admitted).await?;
        let EnrollmentControlDecision::Request(request) = &self.decision else {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        };
        request.validate_against(&expected_request(admitted))?;
        if self.committed_ops.is_some() {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        Ok(request.clone())
    }
    pub(super) async fn verify_confirmation(
        &self,
        effects: &AuraEffectSystem,
        admitted: &AdmittedEnrollmentManifest,
        request: &DeviceEnrollmentRequest,
    ) -> AgentResult<VerifiedEnrollmentConfirmation> {
        let now = effects.physical_time().await.map_err(stage)?.ts_ms;
        self.verify_confirmation_at(effects, admitted, request, now)
            .await
    }
    pub(super) async fn verify_terminal(
        &self,
        effects: &AuraEffectSystem,
        admitted: &AdmittedEnrollmentManifest,
        request: &DeviceEnrollmentRequest,
    ) -> AgentResult<VerifiedEnrollmentTerminal> {
        match &self.decision {
            EnrollmentControlDecision::Request(_) => {
                Err(failure(EnrollmentVmAdmissionError::Binding))
            }
            EnrollmentControlDecision::Committed(_) => self
                .verify_confirmation(effects, admitted, request)
                .await
                .map(VerifiedEnrollmentTerminal::Committed),
            EnrollmentControlDecision::Failed { .. } => {
                let now = effects.physical_time().await.map_err(stage)?.ts_ms;
                self.verify_failure_at(effects, admitted, now)
                    .await
                    .map(VerifiedEnrollmentTerminal::Failed)
            }
        }
    }
    async fn verify_failure_at(
        &self,
        effects: &AuraEffectSystem,
        admitted: &AdmittedEnrollmentManifest,
        observed_at_ms: u64,
    ) -> AgentResult<VerifiedEnrollmentFailureCapability> {
        self.verify_at(effects, admitted, observed_at_ms).await?;
        let EnrollmentControlDecision::Failed {
            invitation,
            ceremony,
            reason,
        } = &self.decision
        else {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        };
        let manifest = admitted.manifest();
        if invitation != &manifest.invitation
            || ceremony != &manifest.ceremony
            || effects.device_id() != manifest.invitee_device
        {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        Ok(VerifiedEnrollmentFailureCapability {
            frame: Box::new(self.clone()),
            canonical_invitation: Box::new(admitted.canonical_invitation().clone()),
            manifest: Box::new(manifest.clone()),
            manifest_digest: admitted.manifest_digest(),
            observed_at_ms,
            reason: *reason,
        })
    }

    async fn verify_confirmation_at(
        &self,
        effects: &AuraEffectSystem,
        admitted: &AdmittedEnrollmentManifest,
        request: &DeviceEnrollmentRequest,
        confirmed_at_ms: u64,
    ) -> AgentResult<VerifiedEnrollmentConfirmation> {
        self.verify_at(effects, admitted, confirmed_at_ms).await?;
        request.validate_against(&expected_request(admitted))?;
        let EnrollmentControlDecision::Committed(confirm) = &self.decision else {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        };
        confirm.validate_against(request)?;
        if effects.device_id() != admitted.manifest().invitee_device {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        let committed_transition = verify_committed_ops(admitted, self.committed_ops.as_deref())?;
        Ok(VerifiedEnrollmentConfirmation {
            frame: Box::new(self.clone()),
            canonical_invitation: Box::new(admitted.canonical_invitation().clone()),
            manifest: Box::new(admitted.manifest().clone()),
            manifest_digest: admitted.manifest_digest(),
            confirmed_at_ms,
            committed_transition,
        })
    }
}
pub(super) fn expected_request(admitted: &AdmittedEnrollmentManifest) -> DeviceEnrollmentRequest {
    let manifest = admitted.manifest();
    DeviceEnrollmentRequest {
        invitation_id: manifest.invitation.clone(),
        subject_authority: manifest.subject,
        ceremony_id: manifest.ceremony.clone(),
        pending_epoch: manifest.pending_epoch,
        device_id: manifest.invitee_device,
    }
}
/// Required producer guard over an authenticated snapshot. This does not mint
/// a receiver freshness witness or serialize signing against later revocation.
async fn require_current_confirmation_membership(
    effects: &AuraEffectSystem,
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
) -> AgentResult<()> {
    let ops = effects.export_tree_ops().await.map_err(stage)?;
    // Verify every actual local parent and attestation before reduction. A
    // cached tree projection or retained terminal receipt is insufficient.
    effects
        .collect_enrollment_parent_inventory(&ops)
        .await
        .map_err(stage)?;
    let state = aura_journal::commitment_tree::reduce(&ops).map_err(stage)?;
    let metadata = effects
        .require_threshold_config_metadata(&manifest.subject, manifest.pending_epoch)
        .await
        .map_err(stage)?;
    let raw_epoch = effects
        .secure_retrieve(
            &SecureStorageLocation::new("epoch_state", manifest.subject.to_string()),
            &[SecureStorageCapability::Read],
        )
        .await
        .map_err(stage)?;
    let epoch: [u8; 8] = raw_epoch.as_slice().try_into().map_err(stage)?;
    if effects.authority_id() != manifest.subject
        || effects.device_id() != manifest.initiator_device
        || u64::from_le_bytes(epoch) != manifest.pending_epoch
        || state.epoch.value() != manifest.pending_epoch
        || !current_confirmation_membership(
            &state,
            manifest.initiator_device,
            manifest.invitee_device,
        )
        || !metadata.contains_participant(
            &aura_core::types::participants::ParticipantIdentity::device(manifest.initiator_device),
        )
        || !metadata.contains_participant(
            &aura_core::types::participants::ParticipantIdentity::device(manifest.invitee_device),
        )
    {
        return Err(failure(EnrollmentVmAdmissionError::CurrentMembership));
    }
    Ok(())
}

fn current_confirmation_membership(
    state: &aura_journal::commitment_tree::state::TreeState,
    issuer: aura_core::DeviceId,
    invitee: aura_core::DeviceId,
) -> bool {
    [issuer, invitee].iter().all(|device| {
        state
            .leaves
            .values()
            .any(|leaf| leaf.device_id == *device && leaf.role == aura_core::LeafRole::Device)
    })
}

async fn export_committed_ops(
    effects: &AuraEffectSystem,
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
) -> AgentResult<Vec<aura_core::AttestedOp>> {
    let ops = effects.export_tree_ops().await.map_err(stage)?;
    let baseline_count = manifest.baseline_count as usize;
    if ops.len() <= baseline_count || ops.len() > 4096 {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let prefix: Vec<Vec<u8>> = ops[..baseline_count]
        .iter()
        .map(aura_core::util::serialization::to_vec)
        .collect::<Result<_, _>>()
        .map_err(stage)?;
    let canonical = aura_core::util::serialization::to_vec(&prefix).map_err(stage)?;
    if aura_core::hash::hash(&canonical) != manifest.baseline_digest {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    Ok(ops[baseline_count..].to_vec())
}

/// Construct only the verification view for an independently admitted history.
/// Same-node keys remain pinned through verified same-epoch operations; a new
/// pending root epoch is bound to its exact signed package and provisional policy.
fn retain_admitted_node_keys(
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    canonical: &super::Invitation,
    state: &mut aura_journal::commitment_tree::state::TreeState,
) -> AgentResult<()> {
    fn group(
        mode: aura_core::crypto::single_signer::SigningMode,
        package: &[u8],
    ) -> AgentResult<[u8; 32]> {
        match mode {
            aura_core::crypto::single_signer::SigningMode::SingleSigner => {
                aura_core::crypto::single_signer::SingleSignerPublicKeyPackage::from_bytes(package)
                    .map_err(stage)?
                    .verifying_key()
                    .try_into()
                    .map_err(stage)
            }
            aura_core::crypto::single_signer::SigningMode::Threshold => {
                aura_core::crypto::tree_signing::public_key_package_from_bytes(package)
                    .map_err(stage)?
                    .group_public_key
                    .as_slice()
                    .try_into()
                    .map_err(stage)
            }
        }
    }
    let mut pinned = std::collections::BTreeMap::new();
    for parent in &manifest.parents {
        if parent.epoch != state.epoch.value() {
            continue;
        }
        let key = group(parent.mode, &parent.public_key_package)?;
        if let Some(previous) = pinned.insert(parent.signing_node, key) {
            if previous != key {
                return Err(failure(EnrollmentVmAdmissionError::Binding));
            }
        }
    }
    if state.epoch.value() == manifest.pending_epoch {
        let super::InvitationType::DeviceEnrollment {
            public_key_package,
            threshold_config,
            ..
        } = &canonical.invitation_type
        else {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        };
        if aura_core::hash::hash(public_key_package) != manifest.pending_public_key_package_digest
            || aura_core::hash::hash(threshold_config)
                != *manifest.pending_threshold_config_digest.as_bytes()
        {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        let policy =
            aura_invitation::enrollment_manifest::EnrollmentTrustManifest::decode_pending_policy(
                threshold_config,
            )
            .map_err(stage)?;
        let root = match state.root_node() {
            Some(root) => root,
            None if !state.leaves.is_empty()
                && state.leaves.keys().all(|leaf| {
                    state.get_leaf_parent(*leaf) == Some(aura_core::tree::NodeIndex(0))
                }) =>
            {
                aura_core::tree::NodeIndex(0)
            }
            None => return Err(failure(EnrollmentVmAdmissionError::Binding)),
        };
        // Issuance explicitly represents this exact root package. It cannot be
        // assigned to a branch whose independent verifier inventory is absent.
        if root != aura_core::tree::NodeIndex(0) {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        let actual = state
            .get_policy(&root)
            .map(|actual| actual.required_signers(authenticated_node_child_count(state, root)))
            .transpose()
            .map_err(stage)?;
        if actual.is_some_and(|required| required > policy.threshold()) {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        let key = group(policy.signing_mode(), public_key_package)?;
        if pinned.get(&root).is_some_and(|old| old != &key) {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        pinned.insert(root, key);
    }
    for (node, key) in pinned {
        state.set_signing_key(
            node,
            aura_core::tree::BranchSigningKey::new(key, state.epoch),
        );
    }
    Ok(())
}

/// Counts actual authenticated branch and leaf edges without journal mutation.
fn authenticated_node_child_count(
    state: &aura_journal::commitment_tree::state::TreeState,
    node: aura_core::tree::NodeIndex,
) -> usize {
    // Topology stores branch children and leaf-parent edges separately. Count
    // both kinds from actual authenticated reduced history; never substitute
    // signed participant totals for the node's observed child topology.
    state.get_children(node).len()
        + state
            .leaves
            .keys()
            .filter(|leaf| state.get_leaf_parent(**leaf) == Some(node))
            .count()
}
/// Verification-only policy projection from independently admitted exact node
/// inventory. It does not create journal branches or authorize UI metadata.
struct AdmittedNodeVerificationView<'a> {
    state: &'a aura_journal::commitment_tree::state::TreeState,
    policies: std::collections::BTreeMap<aura_core::tree::NodeIndex, aura_core::tree::Policy>,
}
impl aura_core::tree::verification::TreeStateView for AdmittedNodeVerificationView<'_> {
    fn get_signing_key(
        &self,
        node: aura_core::tree::NodeIndex,
    ) -> Option<&aura_core::tree::BranchSigningKey> {
        self.state.get_signing_key(&node)
    }
    fn get_policy(&self, node: aura_core::tree::NodeIndex) -> Option<&aura_core::tree::Policy> {
        self.policies.get(&node)
    }
    fn child_count(&self, node: aura_core::tree::NodeIndex) -> usize {
        authenticated_node_child_count(self.state, node)
    }
    fn current_epoch(&self) -> aura_core::Epoch {
        self.state.epoch
    }
    fn current_commitment(&self) -> [u8; 32] {
        self.state.root_commitment
    }
}
fn check_admitted_node_operation(
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    canonical: &super::Invitation,
    state: &aura_journal::commitment_tree::state::TreeState,
    op: &aura_core::AttestedOp,
    target: aura_core::tree::NodeIndex,
) -> AgentResult<()> {
    let mut policies = std::collections::BTreeMap::new();
    let mut signing_rosters = std::collections::BTreeMap::new();
    for parent in &manifest.parents {
        if parent.epoch != state.epoch.value() {
            continue;
        }
        let roster_total = u16::try_from(parent.participants.len()).map_err(stage)?;
        let topology_total =
            u16::try_from(authenticated_node_child_count(state, parent.signing_node))
                .map_err(stage)?;
        // Signing participants remain the original pinned quorum; topology has
        // already acquired the new leaf before the original-key epoch fence.
        let policy =
            aura_core::tree::Policy::threshold(parent.threshold, topology_total).map_err(stage)?;
        if signing_rosters
            .insert(parent.signing_node, roster_total)
            .is_some_and(|old| old != roster_total)
        {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        if let Some(previous) = policies.insert(parent.signing_node, policy) {
            if previous != policy {
                return Err(failure(EnrollmentVmAdmissionError::Binding));
            }
        }
    }
    if state.epoch.value() == manifest.pending_epoch {
        let super::InvitationType::DeviceEnrollment {
            threshold_config, ..
        } = &canonical.invitation_type
        else {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        };
        let pending =
            aura_invitation::enrollment_manifest::EnrollmentTrustManifest::decode_pending_policy(
                threshold_config,
            )
            .map_err(stage)?;
        let root = aura_core::tree::NodeIndex(0);
        let topology_total =
            u16::try_from(authenticated_node_child_count(state, root)).map_err(stage)?;
        let policy = aura_core::tree::Policy::threshold(pending.threshold(), topology_total)
            .map_err(stage)?;
        if signing_rosters
            .insert(root, pending.total_participants())
            .is_some_and(|old| old != pending.total_participants())
        {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        if let Some(previous) = policies.insert(aura_core::tree::NodeIndex(0), policy) {
            if previous != policy {
                return Err(failure(EnrollmentVmAdmissionError::Binding));
            }
        }
    }
    for (node, pinned) in &policies {
        // Actual reduced policies remain an independent minimum. The signed
        // inventory cannot weaken a branch policy already established by ops.
        if let Some(actual) = state.get_policy(node) {
            let children = authenticated_node_child_count(state, *node);
            if actual.required_signers(children).map_err(stage)?
                > pinned.required_signers(children).map_err(stage)?
            {
                return Err(failure(EnrollmentVmAdmissionError::Binding));
            }
        }
    }
    let Some(n) = signing_rosters.get(&target) else {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    };
    if op.signer_count > *n {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let view = AdmittedNodeVerificationView { state, policies };

    aura_core::tree::verification::check_attested_op(&view, op, target).map_err(|source| {
        AgentError::from(aura_core::AuraError::crypto_with_source(
            "verify independently admitted exact-node enrollment operation",
            std::sync::Arc::new(source),
        ))
    })
}

fn verify_committed_ops(
    admitted: &AdmittedEnrollmentManifest,
    suffix: Option<&[aura_core::AttestedOp]>,
) -> AgentResult<VerifiedEnrollmentCommittedTransition> {
    let suffix = suffix.ok_or_else(|| failure(EnrollmentVmAdmissionError::Binding))?;
    if suffix.is_empty() || suffix.len() > 4096 {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let encoded = aura_core::util::serialization::to_vec(&suffix).map_err(stage)?;
    if encoded.len() > 1_000_000 {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let mut ops = admitted.baseline().ops().to_vec();
    let mut state = aura_journal::commitment_tree::reduce(&ops).map_err(stage)?;
    retain_admitted_node_keys(
        admitted.manifest(),
        admitted.canonical_invitation(),
        &mut state,
    )?;
    for op in suffix {
        let target = aura_core::tree::verification::extract_target_node(&op.op.op)
            .or_else(|| match &op.op.op {
                aura_core::TreeOpKind::RemoveLeaf { leaf, .. } => {
                    state.get_remove_leaf_affected_parent(leaf)
                }
                _ => None,
            })
            .ok_or_else(|| failure(EnrollmentVmAdmissionError::Binding))?;
        // Both signing key and threshold derive exclusively from the already
        // admitted state, never from a key included by this incoming frame.
        check_admitted_node_operation(
            admitted.manifest(),
            admitted.canonical_invitation(),
            &state,
            op,
            target,
        )?;
        ops.push(op.clone());
        state = aura_journal::commitment_tree::reduce(&ops).map_err(stage)?;
        retain_admitted_node_keys(
            admitted.manifest(),
            admitted.canonical_invitation(),
            &mut state,
        )?;
    }
    let manifest = admitted.manifest();
    if state.epoch.value() != manifest.pending_epoch
        || !current_confirmation_membership(
            &state,
            manifest.initiator_device,
            manifest.invitee_device,
        )
    {
        return Err(failure(EnrollmentVmAdmissionError::CurrentMembership));
    }
    Ok(VerifiedEnrollmentCommittedTransition {
        ops,
        state: Box::new(state),
    })
}

async fn sign_control(
    effects: &AuraEffectSystem,
    retained: &RetainedEnrollmentVmControl,
    decision: EnrollmentControlDecision,
) -> AgentResult<EnrollmentControlFrame> {
    retained
        .require_runtime_owner(effects)
        .map_err(AgentError::from)?;
    let manifest = retained.manifest();
    // Shared signing-material custody precedes tree custody. All production
    // signing lifecycle mutations must acquire this same generation gate.
    let _generation_owner = effects.enrollment_retirement_generation_guard().await;
    let _decision_owner = effects.lock_tree_decision().await;
    if matches!(&decision, EnrollmentControlDecision::Committed(_)) {
        require_current_confirmation_membership(effects, manifest).await?;
    }
    let (private, public) =
        crate::handlers::rendezvous_identity::require_identity_keys(effects, &manifest.subject)
            .await
            .map_err(stage)?;
    if public.as_slice() != manifest.initiator_confirmation_verifier.as_slice()
        || effects.device_id() != manifest.initiator_device
    {
        return Err(failure(EnrollmentVmAdmissionError::SigningIdentityChanged));
    }
    let committed_ops = if matches!(&decision, EnrollmentControlDecision::Committed(_)) {
        Some(export_committed_ops(effects, manifest).await?)
    } else {
        None
    };
    let transcript = EnrollmentControlTranscript {
        version: 2,
        manifest_digest: retained.digest(),
        decision,
        committed_ops,
    };
    let bytes = transcript.transcript_bytes().map_err(transcript_stage)?;
    if bytes.len() > 1_000_000 {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let signature = effects
        .ed25519_sign(&bytes, &private)
        .await
        .map_err(stage)?;
    Ok(EnrollmentControlFrame {
        version: transcript.version,
        manifest_digest: transcript.manifest_digest,
        decision: transcript.decision,
        committed_ops: transcript.committed_ops,
        signature,
    })
}
pub(super) async fn sign_request(
    effects: &AuraEffectSystem,
    retained: &RetainedEnrollmentVmControl,
) -> AgentResult<EnrollmentControlFrame> {
    let manifest = retained.manifest();
    let request = DeviceEnrollmentRequest {
        invitation_id: manifest.invitation.clone(),
        subject_authority: manifest.subject,
        ceremony_id: manifest.ceremony.clone(),
        pending_epoch: manifest.pending_epoch,
        device_id: manifest.invitee_device,
    };
    sign_control(
        effects,
        retained,
        EnrollmentControlDecision::Request(request),
    )
    .await
}
pub(super) async fn sign_committed_confirmation(
    effects: &AuraEffectSystem,
    retained: &RetainedEnrollmentVmControl,
    runner: &crate::runtime::services::ceremony_runner::CeremonyRunner,
) -> AgentResult<EnrollmentControlFrame> {
    use aura_app::runtime_bridge::CeremonyTerminalOutcome;
    match runner
        .await_enrollment_terminal_outcome(&retained.manifest().ceremony)
        .await
        .map_err(AgentError::from)?
    {
        CeremonyTerminalOutcome::Committed => {}
        CeremonyTerminalOutcome::Failed(reason) => {
            return Err(failure(EnrollmentVmAdmissionError::TerminalFailed(reason)))
        }
    }
    let manifest = retained.manifest();
    let confirm = DeviceEnrollmentConfirm {
        invitation_id: manifest.invitation.clone(),
        ceremony_id: manifest.ceremony.clone(),
        established: true,
        new_epoch: Some(manifest.pending_epoch),
    };
    sign_control(
        effects,
        retained,
        EnrollmentControlDecision::Committed(confirm),
    )
    .await
}
pub(super) async fn sign_terminal_confirmation(
    effects: &AuraEffectSystem,
    retained: &RetainedEnrollmentVmControl,
    runner: &crate::runtime::services::ceremony_runner::CeremonyRunner,
) -> AgentResult<EnrollmentControlFrame> {
    match runner
        .await_enrollment_terminal_outcome(&retained.manifest().ceremony)
        .await
        .map_err(AgentError::from)?
    {
        aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed => {
            sign_committed_confirmation(effects, retained, runner).await
        }
        aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(reason) => {
            let manifest = retained.manifest();
            sign_control(
                effects,
                retained,
                EnrollmentControlDecision::Failed {
                    invitation: manifest.invitation.clone(),
                    ceremony: manifest.ceremony.clone(),
                    reason,
                },
            )
            .await
        }
    }
}

/// This function cannot accept a digest, raw invitation, or arbitrary transcript.
/// The current provisional signer must still equal the independently retained
/// setup signer. There is no historical-key signing fallback.
pub(super) async fn sign_acceptance_for_request(
    effects: &AuraEffectSystem,
    admitted: &AdmittedEnrollmentManifest,
    request: &DeviceEnrollmentRequest,
) -> AgentResult<DeviceEnrollmentAccept> {
    request.validate_against(&expected_request(admitted))?;
    let manifest = admitted.manifest();
    let setup = admitted.local_setup().statement();
    if setup.device != effects.device_id()
        || setup.authority != manifest.invitee_authority
        || setup.device != manifest.invitee_device
        || setup.nonce != manifest.setup.nonce
        || admitted.local_setup().digest() != manifest.setup.digest
    {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let now = effects.physical_time().await.map_err(stage)?.ts_ms;
    if now >= manifest.expires_at_ms || now >= setup.expires_at_ms {
        return Err(stage(EnrollmentManifestError::Expired));
    }
    require_current_setup_epoch(effects, setup).await?;
    let transcript = DeviceEnrollmentAcceptanceTranscript {
        manifest_digest: admitted.manifest_digest(),
        invitation: admitted.canonical_invitation(),
        acceptor_id: setup.authority,
        subject_authority: manifest.subject,
        ceremony_id: manifest.ceremony.clone(),
        device_id: setup.device,
    };
    let payload = transcript.transcript_bytes().map_err(transcript_stage)?;
    let signature = effects
        .sign(SigningContext::message(
            setup.authority,
            DeviceEnrollmentAcceptanceTranscript::DOMAIN_SEPARATOR.to_string(),
            payload,
        ))
        .await
        .map_err(stage)?;
    require_current_setup_epoch(effects, setup).await?;
    if signature.epoch != setup.signing_epoch
        || signature.public_key_package != setup.public_key_package
        || signature.signer_count < setup.threshold
        || signature.signer_count > setup.participants
        || signature.signers.len() != usize::from(signature.signer_count)
        || signature
            .signers
            .iter()
            .any(|index| *index == 0 || *index > setup.participants)
        || signature.signers.windows(2).any(|pair| pair[0] >= pair[1])
        || (setup.signing_mode == aura_core::crypto::single_signer::SigningMode::SingleSigner
            && (signature.signer_count != 1 || signature.signers != [1]))
    {
        return Err(failure(EnrollmentVmAdmissionError::SigningIdentityChanged));
    }
    Ok(DeviceEnrollmentAccept {
        invitation_id: manifest.invitation.clone(),
        ceremony_id: manifest.ceremony.clone(),
        device_id: setup.device,
        acceptor_id: setup.authority,
        signature,
        manifest_digest: Some(admitted.manifest_digest()),
    })
}

pub(super) async fn sign_refusal_for_request(
    effects: &AuraEffectSystem,
    admitted: &AdmittedEnrollmentManifest,
    request: &DeviceEnrollmentRequest,
) -> AgentResult<aura_invitation::protocol::DeviceEnrollmentRefusal> {
    request.validate_against(&expected_request(admitted))?;
    let manifest = admitted.manifest();
    let setup = admitted.local_setup().statement();
    if setup.device != effects.device_id()
        || setup.authority != manifest.invitee_authority
        || setup.device != manifest.invitee_device
        || setup.nonce != manifest.setup.nonce
        || admitted.local_setup().digest() != manifest.setup.digest
    {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let now = effects.physical_time().await.map_err(stage)?.ts_ms;
    if now >= manifest.expires_at_ms || now >= setup.expires_at_ms {
        return Err(stage(EnrollmentManifestError::Expired));
    }
    require_current_setup_epoch(effects, setup).await?;
    let transcript =
        super::DeviceEnrollmentRefusalTranscript(DeviceEnrollmentAcceptanceTranscript {
            manifest_digest: admitted.manifest_digest(),
            invitation: admitted.canonical_invitation(),
            acceptor_id: setup.authority,
            subject_authority: manifest.subject,
            ceremony_id: manifest.ceremony.clone(),
            device_id: setup.device,
        });
    let payload = transcript.transcript_bytes().map_err(transcript_stage)?;
    let signature = effects
        .sign(SigningContext::message(
            setup.authority,
            super::DeviceEnrollmentRefusalTranscript::DOMAIN_SEPARATOR.to_string(),
            payload,
        ))
        .await
        .map_err(stage)?;
    require_current_setup_epoch(effects, setup).await?;
    if signature.epoch != setup.signing_epoch
        || signature.public_key_package != setup.public_key_package
        || signature.signer_count < setup.threshold
        || signature.signer_count > setup.participants
        || signature.signers.len() != usize::from(signature.signer_count)
        || signature
            .signers
            .iter()
            .any(|index| *index == 0 || *index > setup.participants)
        || signature.signers.windows(2).any(|pair| pair[0] >= pair[1])
        || (setup.signing_mode == aura_core::crypto::single_signer::SigningMode::SingleSigner
            && (signature.signer_count != 1 || signature.signers != [1]))
    {
        return Err(failure(EnrollmentVmAdmissionError::SigningIdentityChanged));
    }
    Ok(aura_invitation::protocol::DeviceEnrollmentRefusal {
        binding: DeviceEnrollmentAccept {
            invitation_id: manifest.invitation.clone(),
            ceremony_id: manifest.ceremony.clone(),
            device_id: setup.device,
            acceptor_id: setup.authority,
            signature,
            manifest_digest: Some(admitted.manifest_digest()),
        },
    })
}

async fn require_current_setup_epoch(
    effects: &AuraEffectSystem,
    setup: &aura_invitation::enrollment_setup::DeviceEnrollmentSetupStatement,
) -> AgentResult<()> {
    let bytes = effects
        .secure_retrieve(
            &SecureStorageLocation::new("epoch_state", setup.authority.to_string()),
            &[SecureStorageCapability::Read],
        )
        .await
        .map_err(stage)?;
    let epoch: [u8; 8] = bytes.as_slice().try_into().map_err(stage)?;
    if u64::from_le_bytes(epoch) != setup.signing_epoch {
        return Err(failure(EnrollmentVmAdmissionError::SigningIdentityChanged));
    }
    Ok(())
}

/// An arbitrary timestamp or deserialized frame cannot call historical verify.
/// Only the secure-storage owner may produce this non-serializable input.
pub(super) async fn verify_retained_confirmation(
    effects: &AuraEffectSystem,
    retained: super::enrollment_manifest_admission::RetainedConfirmationAdmission,
) -> AgentResult<VerifiedEnrollmentConfirmation> {
    let request = expected_request(retained.admitted());
    retained
        .frame()
        .verify_confirmation_at(
            effects,
            retained.admitted(),
            &request,
            retained.confirmed_at_ms(),
        )
        .await
}

pub(super) async fn verify_retained_failure(
    effects: &AuraEffectSystem,
    retained: super::enrollment_manifest_admission::RetainedFailureAdmissionCapability,
) -> AgentResult<VerifiedEnrollmentFailureCapability> {
    retained
        .frame()
        .verify_failure_at(effects, retained.admitted(), retained.observed_at_ms())
        .await
}

#[cfg(test)]
pub(crate) async fn actual_invalid_control_rejection_for_test(case: &str) -> AgentError {
    let (issuer, invitee, invitation, _start, _accept, _witness) =
        Box::pin(super::tests::actual_pinned_device_enrollment_fixture(case)).await;
    let issuer_effects = issuer.runtime().effects();
    let invitee_effects = invitee.runtime().effects();
    let admitted = super::enrollment_manifest_admission::load_admitted_baseline(
        invitee_effects.as_ref(),
        invitee.authority_id(),
        &invitation,
    )
    .await
    .expect("real independently transferred enrollment admission");
    let retained = RetainedEnrollmentVmControl::load(issuer_effects.clone(), &invitation)
        .await
        .unwrap();
    let request = expected_request(&admitted);
    match case {
        "wrong-request" => {
            let mut wrong = request;
            wrong.device_id = issuer.context().device_id();
            let signed = sign_control(
                issuer_effects.as_ref(),
                &retained,
                EnrollmentControlDecision::Request(wrong),
            )
            .await
            .unwrap();
            signed
                .verify_request(invitee_effects.as_ref(), &admitted)
                .await
                .expect_err("authentic issuer cannot change admitted physical device")
        }
        "negative-confirmation" => {
            let negative = DeviceEnrollmentConfirm {
                invitation_id: request.invitation_id.clone(),
                ceremony_id: request.ceremony_id.clone(),
                established: false,
                new_epoch: Some(request.pending_epoch),
            };
            let signed = sign_control(
                issuer_effects.as_ref(),
                &retained,
                EnrollmentControlDecision::Committed(negative),
            )
            .await
            .unwrap();
            signed
                .verify_confirmation(invitee_effects.as_ref(), &admitted, &request)
                .await
                .expect_err("authenticated negative decision cannot establish enrollment")
        }
        _ => panic!("unsupported actual negative fixture"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn required_vm_stage_keeps_storage_clock_and_codec_categories() {
        use std::error::Error;
        let storage = stage(aura_core::AuraError::Storage {
            message: "actual denied read".into(),
            source: Some(std::sync::Arc::new(std::io::Error::from(
                std::io::ErrorKind::PermissionDenied,
            ))),
        });
        let AgentError::Aura(aura_core::AuraError::Storage {
            source: Some(source),
            ..
        }) = storage
        else {
            panic!("required storage failure must remain storage");
        };
        let mut cause: Option<&(dyn Error + 'static)> = Some(source.as_ref());
        let mut io = None;
        while let Some(error) = cause {
            if let Some(error) = error.downcast_ref::<std::io::Error>() {
                io = Some(error.kind());
                break;
            }
            cause = error.source();
        }
        assert_eq!(io, Some(std::io::ErrorKind::PermissionDenied));
        let time = stage(aura_core::effects::TimeError::ServiceUnavailable);
        assert!(matches!(
            time,
            AgentError::Aura(aura_core::AuraError::Internal {
                source: Some(_),
                ..
            })
        ));
        let codec = stage(
            aura_core::util::serialization::SerializationError::InvalidFormat(
                "invalid control".into(),
            ),
        );
        assert!(matches!(
            codec,
            AgentError::Aura(aura_core::AuraError::Serialization {
                source: Some(_),
                ..
            })
        ));
    }

    #[test]
    fn actual_admitted_vm_control_and_acceptance_reject_foreign_decisions() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (issuer, invitee, invitation, _start, _accept, _verified) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "sealed-vm-control",
                ),
            )
            .await;
            let issuer_effects = issuer.runtime().effects();
            let invitee_effects = invitee.runtime().effects();
            let admitted = super::super::enrollment_manifest_admission::load_admitted_baseline(
                invitee_effects.as_ref(),
                invitation.receiver_id,
                &invitation,
            )
            .await
            .expect("actual retained independent transfer");
            let retained = RetainedEnrollmentVmControl::load(issuer_effects.clone(), &invitation)
                .await
                .expect("actual retained issuer manifest");
            let frame = sign_request(issuer_effects.as_ref(), &retained)
                .await
                .expect("actual control signer");
            let request = frame
                .verify_request(invitee_effects.as_ref(), &admitted)
                .await
                .expect("admitted actual request");
            let acceptance =
                sign_acceptance_for_request(invitee_effects.as_ref(), &admitted, &request)
                    .await
                    .expect("actual retained setup signer");
            assert_eq!(acceptance.manifest_digest, Some(admitted.manifest_digest()));
            assert_eq!(
                acceptance.device_id,
                admitted.local_setup().statement().device
            );
            assert_eq!(
                acceptance.signature.public_key_package,
                admitted.local_setup().statement().public_key_package
            );
            assert!(EnrollmentControlFrame::decode(&vec![0; 4097]).is_err());
            let mut wrong_digest = frame.clone();
            wrong_digest.manifest_digest[0] ^= 1;
            assert!(wrong_digest
                .verify_request(invitee_effects.as_ref(), &admitted)
                .await
                .is_err());
            let mut modified = frame.clone();
            if let EnrollmentControlDecision::Request(request) = &mut modified.decision {
                request.pending_epoch += 1;
            }
            assert!(modified
                .verify_request(invitee_effects.as_ref(), &admitted)
                .await
                .is_err());
            let mut wrong_key = frame.clone();
            // Separate testing runtimes share the same initial deterministic crypto
            // stream. Generate a genuinely distinct real key rather than assuming
            // their first bootstrap keys differ by authority ID.
            let (other_private, other_public) = invitee_effects
                .ed25519_generate_keypair()
                .await
                .expect("actual distinct crypto keypair");
            assert_ne!(
                other_public,
                admitted.manifest().initiator_confirmation_verifier
            );
            let bytes = wrong_key
                .transcript()
                .transcript_bytes()
                .expect("actual control transcript");
            wrong_key.signature = invitee_effects
                .ed25519_sign(&bytes, &other_private)
                .await
                .expect("actual other signer");
            assert!(wrong_key
                .verify_request(invitee_effects.as_ref(), &admitted)
                .await
                .is_err());
            let mut other_request = request.clone();
            other_request.pending_epoch += 1;
            assert!(sign_acceptance_for_request(
                invitee_effects.as_ref(),
                &admitted,
                &other_request
            )
            .await
            .is_err());
            assert!(
                sign_acceptance_for_request(issuer_effects.as_ref(), &admitted, &request)
                    .await
                    .is_err()
            );
            assert!(frame
                .verify_confirmation(invitee_effects.as_ref(), &admitted, &request)
                .await
                .is_err());
            // A real owner failure wakes the same-session waiter and remains a
            // permanent typed outcome; pending state never fabricates a timeout.
            let runner = issuer.runtime().ceremony_runner();
            let (confirmation, completion) = tokio::join!(
                sign_committed_confirmation(issuer_effects.as_ref(), &retained, runner),
                runner.complete(
                    &retained.manifest().ceremony,
                    aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                        aura_app::runtime_bridge::CeremonyFailureReason::RuntimeFailed
                    )
                ),
            );
            completion.expect("actual owner terminal failure");
            let error = confirmation.expect_err("failed activation must not sign confirmation");
            assert!(!error.is_timeout());
            let AgentError::Aura(aura_core::AuraError::Crypto {
                source: Some(source),
                ..
            }) = error
            else {
                panic!("failure must retain native admission cause");
            };
            assert!(matches!(
                source.downcast_ref::<EnrollmentVmAdmissionError>(),
                Some(EnrollmentVmAdmissionError::TerminalFailed(
                    aura_app::runtime_bridge::CeremonyFailureReason::RuntimeFailed
                ))
            ));
        });
    }
}

#[cfg(test)]
mod committed_receipt_tests {
    use super::*;
    use aura_core::effects::{StorageCoreEffects, ThresholdSigningEffects};
    #[test]
    fn real_committed_confirmation_is_durable_and_reverified_before_activation_capability() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (issuer, invitee, invitation, start, _accept, verified) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "confirmed-import-receipt",
                ),
            )
            .await;
            let issuer_effects = issuer.runtime().effects();
            let invitee_effects = invitee.runtime().effects();
            let retained = RetainedEnrollmentVmControl::load(issuer_effects.clone(), &invitation)
                .await
                .unwrap();
            let admitted = super::super::enrollment_manifest_admission::load_admitted_baseline(
                invitee_effects.as_ref(),
                invitation.receiver_id,
                &invitation,
            )
            .await
            .unwrap();
            crate::runtime::services::enrollment_import::install_admitted_generation(
                invitee_effects.as_ref(),
                &admitted,
            )
            .await
            .expect("actual owned pending generation installation");
            assert!(invitee_effects
                .commit_key_rotation(
                    &admitted.manifest().subject,
                    admitted.manifest().pending_epoch
                )
                .await
                .is_err());
            assert!(invitee
                .runtime()
                .threshold_signing()
                .commit_key_rotation(
                    &admitted.manifest().subject,
                    admitted.manifest().pending_epoch
                )
                .await
                .is_err());
            let runner = issuer.runtime().ceremony_runner();
            runner
                .record_verified_enrollment_response(verified)
                .await
                .unwrap();
            let frame = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                sign_committed_confirmation(issuer_effects.as_ref(), &retained, runner),
            )
            .await
            .expect("actual finalizer publishes authoritative terminal outcome");
            let frame = match frame {
                Ok(frame) => frame,
                Err(error) => {
                    let retained_failure = issuer
                        .runtime()
                        .ceremony_tracker()
                        .get(&start.ceremony_id)
                        .await
                        .expect("actual retained ceremony state");
                    panic!(
                        "actual confirmation failed: {error:?}; retained failure: {:?}; supervisor: {:?}",
                        retained_failure.error_message,
                        issuer.runtime().tasks().terminal_failure()
                    );
                }
            };
            let request = expected_request(&admitted);
            let verified = frame
                .verify_confirmation(invitee_effects.as_ref(), &admitted, &request)
                .await
                .unwrap();
            // The genuine original signer roster stays one while AddLeaf creates
            // two authenticated root children before the original-key epoch fence.
            let history = verified.committed_transition().ops();
            let fence = history.last().expect("real signed epoch fence");
            let mut before_fence =
                aura_journal::commitment_tree::reduce(&history[..history.len() - 1]).unwrap();
            let parent = admitted
                .manifest()
                .parents
                .iter()
                .find(|parent| {
                    parent.epoch == before_fence.epoch.value()
                        && parent.signing_node == aura_core::tree::NodeIndex(0)
                })
                .expect("independently pinned original root roster");
            assert_eq!(parent.participants.len(), 1);
            assert_eq!(
                authenticated_node_child_count(&before_fence, aura_core::tree::NodeIndex(0)),
                2
            );
            retain_admitted_node_keys(
                admitted.manifest(),
                admitted.canonical_invitation(),
                &mut before_fence,
            )
            .unwrap();
            check_admitted_node_operation(
                admitted.manifest(),
                admitted.canonical_invitation(),
                &before_fence,
                fence,
                aura_core::tree::NodeIndex(0),
            )
            .unwrap();
            let mut inflated_roster_claim = fence.clone();
            inflated_roster_claim.signer_count = 2;
            assert!(check_admitted_node_operation(
                admitted.manifest(),
                admitted.canonical_invitation(),
                &before_fence,
                &inflated_roster_claim,
                aura_core::tree::NodeIndex(0)
            )
            .is_err());
            let window =
                crate::runtime::services::enrollment_window::EnrollmentWindowCapability::admitted(
                    invitee_effects.clone(),
                    &admitted,
                )
                .await
                .unwrap();
            let acknowledged = window
                .acknowledge_confirmation(invitee_effects.as_ref())
                .await
                .unwrap();
            let durable =
                super::super::enrollment_manifest_admission::retain_verified_confirmation(
                    invitee_effects.as_ref(),
                    verified,
                    acknowledged,
                )
                .await
                .unwrap();
            assert_eq!(
                durable.confirmation().manifest_digest(),
                admitted.manifest_digest()
            );
            let recovered = super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                invitee_effects.as_ref(),
                invitation.receiver_id,
                &invitation.invitation_id,
            )
            .await
            .unwrap();
            assert_eq!(
                recovered.confirmation().manifest_digest(),
                durable.confirmation().manifest_digest()
            );
            let package_location = SecureStorageLocation::with_sub_key(
                "threshold_pubkey",
                admitted.manifest().subject.to_string(),
                admitted.manifest().pending_epoch.to_string(),
            );
            let original_package = invitee_effects
                .secure_retrieve(&package_location, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let mut substituted = original_package.clone();
            substituted[0] ^= 1;
            invitee_effects
                .secure_store(
                    &package_location,
                    &substituted,
                    &[SecureStorageCapability::Write],
                )
                .await
                .unwrap();
            assert!(invitee
                .runtime()
                .threshold_signing()
                .activate_confirmed_enrollment(recovered)
                .await
                .is_err());
            invitee_effects
                .secure_store(
                    &package_location,
                    &original_package,
                    &[SecureStorageCapability::Write],
                )
                .await
                .unwrap();
            let recovered = super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                invitee_effects.as_ref(),
                invitation.receiver_id,
                &invitation.invitation_id,
            )
            .await
            .unwrap();
            let original_account = serde_json::json!({
                "authority_id": admitted.manifest().invitee_authority,
                "context_id": crate::core::context::default_context_id_for_authority(admitted.manifest().invitee_authority),
                "nickname_suggestion": "preserve-this-name",
            });
            invitee_effects
                .store(
                    "account.json",
                    serde_json::to_vec(&original_account).unwrap(),
                )
                .await
                .unwrap();
            crate::runtime::services::enrollment_profile::complete_confirmed_handoff(
                invitee_effects.as_ref(),
                &invitee.runtime().threshold_signing(),
                recovered,
            )
            .await
            .unwrap();
            let projected: serde_json::Value = serde_json::from_slice(
                &invitee_effects
                    .retrieve("account.json")
                    .await
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                projected["authority_id"],
                serde_json::to_value(admitted.manifest().subject).unwrap()
            );
            assert_eq!(projected["nickname_suggestion"], "preserve-this-name");
            let recovered = super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                invitee_effects.as_ref(),
                invitation.receiver_id,
                &invitation.invitation_id,
            )
            .await
            .unwrap();
            crate::runtime::services::enrollment_profile::complete_confirmed_handoff(
                invitee_effects.as_ref(),
                &invitee.runtime().threshold_signing(),
                recovered,
            )
            .await
            .unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(
                    &invitee_effects
                        .retrieve("account.json")
                        .await
                        .unwrap()
                        .unwrap()
                )
                .unwrap(),
                projected
            );

            let epoch = invitee_effects
                .secure_retrieve(
                    &SecureStorageLocation::new(
                        "epoch_state",
                        admitted.manifest().subject.to_string(),
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await
                .unwrap();
            assert_eq!(epoch, admitted.manifest().pending_epoch.to_le_bytes());
            assert!(invitee
                .runtime()
                .threshold_signing()
                .public_key_package(&admitted.manifest().subject)
                .await
                .is_some());

            let adopted_share = invitee
                .runtime()
                .threshold_signing()
                .participant_key_package(
                    &admitted.manifest().subject,
                    admitted.manifest().pending_epoch,
                    &aura_core::threshold::ParticipantIdentity::device(invitee_effects.device_id()),
                )
                .await
                .unwrap();
            assert_eq!(
                aura_core::hash::hash(&adopted_share),
                admitted.manifest().pending_share_digest,
                "only the exact real issued share is readable through the adopted service envelope"
            );
            // Replaying the exact admitted import after activation may not overwrite
            // the encrypted share envelope or provisional-to-final config transition.
            let share_location = SecureStorageLocation::with_sub_key(
                "participant_shares",
                format!(
                    "{}:{}",
                    admitted.manifest().subject,
                    admitted.manifest().pending_epoch
                ),
                aura_core::threshold::ParticipantIdentity::device(invitee_effects.device_id())
                    .storage_key(),
            );
            let config_location = SecureStorageLocation::with_sub_key(
                "threshold_config",
                admitted.manifest().subject.to_string(),
                admitted.manifest().pending_epoch.to_string(),
            );
            let before_share = invitee_effects
                .secure_retrieve(&share_location, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let before_config = invitee_effects
                .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            crate::runtime::services::enrollment_import::install_admitted_generation(
                invitee_effects.as_ref(),
                &admitted,
            )
            .await
            .unwrap();
            assert_eq!(
                before_share,
                invitee_effects
                    .secure_retrieve(&share_location, &[SecureStorageCapability::Read])
                    .await
                    .unwrap()
            );
            assert_eq!(
                before_config,
                invitee_effects
                    .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
                    .await
                    .unwrap()
            );
            // The actual pinned signer still cannot bind another epoch by changing
            // a cached confirmation payload; signature and exact admission both hold.
            let mut changed = frame.clone();
            if let EnrollmentControlDecision::Committed(confirm) = &mut changed.decision {
                confirm.new_epoch = Some(admitted.manifest().pending_epoch + 1);
            }
            assert!(changed
                .verify_confirmation(invitee_effects.as_ref(), &admitted, &request)
                .await
                .is_err());
            let key = SecureStorageLocation::with_sub_key(
                "device_enrollment_confirmed_import_v1",
                invitation.receiver_id.to_string(),
                invitation.invitation_id.to_string(),
            );
            let bytes = invitee_effects
                .secure_retrieve(&key, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            invitee_effects
                .secure_store(&key, b"corrupt", &[SecureStorageCapability::Write])
                .await
                .unwrap();
            assert!(
                super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                    invitee_effects.as_ref(),
                    invitation.receiver_id,
                    &invitation.invitation_id,
                )
                .await
                .is_err()
            );
            invitee_effects
                .secure_store(&key, &bytes, &[SecureStorageCapability::Write])
                .await
                .unwrap();
            invitee_effects
                .secure_delete(&key, &[SecureStorageCapability::Delete])
                .await
                .unwrap();
            assert!(
                super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                    invitee_effects.as_ref(),
                    invitation.receiver_id,
                    &invitation.invitation_id,
                )
                .await
                .is_err()
            );
        });
    }
}

#[cfg(test)]
mod current_membership_tests {
    use super::*;
    #[test]
    fn same_epoch_removal_and_guardian_role_cannot_authorize_confirmation() {
        let issuer = aura_core::DeviceId::new_from_entropy([181; 32]);
        let invitee = aura_core::DeviceId::new_from_entropy([182; 32]);
        let mut state = aura_journal::commitment_tree::state::TreeState::new();
        state.add_leaf(
            aura_core::LeafNode::new_device(aura_core::LeafId(1), issuer, vec![1; 32])
                .expect("device leaf"),
        );
        state.add_leaf(
            aura_core::LeafNode::new_device(aura_core::LeafId(2), invitee, vec![2; 32])
                .expect("device leaf"),
        );
        assert!(current_confirmation_membership(&state, issuer, invitee));
        let epoch = state.epoch;
        state.remove_leaf(&aura_core::LeafId(1));
        assert_eq!(state.epoch, epoch);
        assert!(!current_confirmation_membership(&state, issuer, invitee));
        let mut leaf = aura_core::LeafNode::new_device(aura_core::LeafId(1), issuer, vec![1; 32])
            .expect("leaf");
        leaf.role = aura_core::LeafRole::Guardian;
        state.add_leaf(leaf);
        assert!(!current_confirmation_membership(&state, issuer, invitee));
    }
}

#[cfg(test)]
mod committed_transition_tests {
    use super::*;
    #[tokio::test]
    async fn independently_admitted_baseline_rejects_absent_empty_and_replayed_transition() {
        let (_issuer, invitee, invitation, _start, _acceptance, _witness) = Box::pin(
            super::super::tests::actual_pinned_device_enrollment_fixture(
                "committed-transition-replay",
            ),
        )
        .await;
        let admitted = super::super::enrollment_manifest_admission::load_admitted_baseline(
            invitee.runtime().effects().as_ref(),
            invitee.authority_id(),
            &invitation,
        )
        .await
        .expect("actual independent manifest admission");
        assert!(verify_committed_ops(&admitted, None).is_err());
        assert!(verify_committed_ops(&admitted, Some(&[])).is_err());
        let baseline = admitted.baseline().ops();
        assert!(!baseline.is_empty());
        assert!(verify_committed_ops(&admitted, Some(&baseline[..1])).is_err());
    }
}

#[cfg(test)]
mod actual_committed_checkpoint_tests {
    use super::*;
    use aura_app::runtime_bridge::CeremonyTerminalOutcome;
    #[test]
    fn actual_finalizer_signed_confirmation_retains_authenticated_receiver_history() {
        super::super::tests::run_async_test_on_large_stack(async {
            let (issuer, invitee, invitation, start, _acceptance, witness) = Box::pin(
                super::super::tests::actual_pinned_device_enrollment_fixture(
                    "actual-committed-checkpoint",
                ),
            )
            .await;
            let issuer_effects = issuer.runtime().effects();
            let invitee_effects = invitee.runtime().effects();
            let admitted = super::super::enrollment_manifest_admission::load_admitted_baseline(
                invitee_effects.as_ref(),
                invitee.authority_id(),
                &invitation,
            )
            .await
            .expect("actual independently admitted manifest");
            let retained = RetainedEnrollmentVmControl::load(issuer_effects.clone(), &invitation)
                .await
                .expect("actual issuer-retained original manifest");
            let runner = issuer.runtime().ceremony_runner();
            runner
                .record_verified_enrollment_response(witness)
                .await
                .expect("actual signed acceptance witness");
            let service = crate::handlers::device_epoch_rotation::DeviceEpochRotationService::new(
                issuer.authority_id(),
                issuer_effects.clone(),
                issuer.runtime().ceremony_tracker().clone(),
                runner.clone(),
                issuer.runtime().threshold_signing(),
                issuer.runtime().reconfiguration().clone(),
            );
            service
                .finalize_sole_device_enrollment(&start.ceremony_id)
                .await
                .expect("actual owned finalizer commits");
            assert_eq!(
                runner
                    .terminal_outcome(&start.ceremony_id)
                    .await
                    .expect("terminal read"),
                Some(CeremonyTerminalOutcome::Committed)
            );
            let committed_history = issuer_effects
                .export_tree_ops()
                .await
                .expect("actual activation history");
            let committed_state = aura_journal::commitment_tree::reduce(&committed_history)
                .expect("actual attested epoch fence reduces");
            assert_eq!(
                committed_state.epoch.value(),
                retained.manifest().pending_epoch
            );
            assert_eq!(committed_history.len(), admitted.baseline().ops().len() + 2);
            assert!(
                matches!(committed_history.last().map(|op|&op.op.op),Some(aura_core::TreeOpKind::RotateEpoch {affected}) if affected.as_slice()==[aura_core::NodeIndex(0)])
            );
            let frame = sign_committed_confirmation(issuer_effects.as_ref(), &retained, runner)
                .await
                .expect("actual committed signature under custody");
            let wire = aura_core::util::serialization::to_vec(&frame).expect("real frame encoding");
            let decoded =
                EnrollmentControlFrame::decode(&wire).expect("bounded canonical control decode");
            let confirmation = decoded
                .verify_confirmation(
                    invitee_effects.as_ref(),
                    &admitted,
                    &expected_request(&admitted),
                )
                .await
                .expect("independent receiver validates real attested extension");
            assert_eq!(
                confirmation.committed_transition().ops(),
                issuer_effects
                    .export_tree_ops()
                    .await
                    .expect("actual committed history")
            );
            assert!(current_confirmation_membership(
                confirmation.committed_state(),
                retained.manifest().initiator_device,
                start.device_id
            ));
            let mut changed = decoded.clone();
            changed
                .committed_ops
                .as_mut()
                .expect("real committed suffix")[0]
                .agg_sig[0] ^= 1;
            assert!(verify_committed_ops(&admitted, changed.committed_ops.as_deref()).is_err());
            assert!(changed
                .verify_confirmation(
                    invitee_effects.as_ref(),
                    &admitted,
                    &expected_request(&admitted)
                )
                .await
                .is_err());
        });
    }
}
