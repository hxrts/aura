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
use std::sync::Arc;
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

    #[error("enrollment VM completed without its original signed terminal receipt")]
    MissingTerminalReceipt,
    #[error("original issuer terminal observer was lost")]
    TerminalObserverLost,
    #[error("enrollment activation failed: {0:?}")]
    TerminalFailed(aura_app::runtime_bridge::CeremonyFailureReason),
    #[error("enrollment control stage failed")]
    Stage(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[cfg(test)]
mod original_k1_return_registration_tests {
    use super::*;
    use aura_core::effects::{
        SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    };

    #[test]
    fn original_k1_return_registration_uses_pinned_root_and_refuses_mode_substitution() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
            let (issuer, invitee, invitation, start, _, _) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_clock(
                    "original-k1-return-registration", Arc::new(clock.clone()),
                ).await;
            let admissions =
                super::super::enrollment_manifest_admission::load_recoverable_negative_admissions(
                    invitee.runtime().effects().as_ref(),
                )
                .await
                .expect("genuine original K1 import retained its baseline");
            let admitted = admissions
                .iter()
                .find(|candidate| candidate.manifest().invitation == invitation.invitation_id)
                .expect("exact original K1 admission");
            let manifest = admitted.manifest();
            assert!(
                start.manifest_transfer.is_some(),
                "K1 original has an independently pinned transfer"
            );
            let digest = aura_core::hash::hash(&manifest.transcript_bytes().unwrap());
            let location = SecureStorageLocation::new(
                "issuer_return_mailbox_registration_v1",
                format!(
                    "{}:{}:{}",
                    manifest.subject,
                    manifest.invitation,
                    hex::encode(digest)
                ),
            );
            let bytes = issuer
                .runtime()
                .effects()
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await
                .expect("K1 original retained its signed return proof before export");
            let keys = issuer
                .runtime()
                .effects()
                .secure_list_keys(
                    "issuer_return_mailbox_registration_v1",
                    &[SecureStorageCapability::List],
                )
                .await
                .expect("original return registration inventory is readable");
            assert_eq!(keys.as_slice(), std::slice::from_ref(&location.key));
            let registration: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration =
                aura_core::util::serialization::from_slice(&bytes).unwrap();
            let credential_location = SecureStorageLocation::with_sub_key(
                "issuer_return_mailbox_credential_v1",
                manifest.subject.to_string(),
                format!("{}:{}", manifest.invitation, hex::encode(digest)),
            );
            let setup_location = SecureStorageLocation::with_sub_key(
                "issuer_return_mailbox_setup_v1",
                manifest.subject.to_string(),
                format!("{}:{}", manifest.invitation, hex::encode(digest)),
            );
            let retained_credential = issuer
                .runtime()
                .effects()
                .secure_retrieve(&credential_location, &[SecureStorageCapability::Read])
                .await
                .expect("original retrieval credential retained");
            let retained_setup = issuer
                .runtime()
                .effects()
                .secure_retrieve(&setup_location, &[SecureStorageCapability::Read])
                .await
                .expect("original setup retained");
            let root_bytes = issuer
                .runtime()
                .effects()
                .secure_retrieve(
                    &crate::core::agent::issuer_return_mailbox::retirement_root_location(
                        &registration.statement,
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("original retirement root sealed before export");
            let root: crate::core::agent::issuer_return_mailbox::IssuerReturnRetirementRoot =
                aura_core::util::serialization::from_slice(&root_bytes).unwrap();
            assert!(root.verifies(
                &registration.statement,
                &bytes,
                &retained_credential,
                &retained_setup
            ));
            let mut foreign_credential = retained_credential.clone();
            foreign_credential[0] ^= 1;
            assert!(!root.verifies(
                &registration.statement,
                &bytes,
                &foreign_credential,
                &retained_setup
            ));
            let mut foreign_registration = bytes.clone();
            foreign_registration[0] ^= 1;
            assert!(!root.verifies(
                &registration.statement,
                &foreign_registration,
                &retained_credential,
                &retained_setup
            ));
            assert_eq!(registration.statement.setup_digest(), manifest.setup.digest);
            let mut foreign_setup = serde_json::to_value(&registration).unwrap();
            foreign_setup["statement"]["setup_digest"] = serde_json::json!(vec![0_u8; 32]);
            let foreign_setup: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration =
                serde_json::from_value(foreign_setup).unwrap();
            assert!(matches!(
                foreign_setup
                    .verify_for_admitted_original(
                        invitee.runtime().effects().as_ref(),
                        admitted.baseline(),
                        admitted.local_setup(),
                    )
                    .await,
                Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::Binding)
            ));
            assert_eq!(
                registration.mode,
                aura_core::crypto::single_signer::SigningMode::SingleSigner
            );
            assert!(registration.proof.is_single_signer());
            registration
                .clone()
                .verify_for_admitted_original(
                    invitee.runtime().effects().as_ref(),
                    admitted.baseline(),
                    admitted.local_setup(),
                )
                .await
                .expect("independently admitted K1 root verifies distinct return domain");
            let mut cross_mode = registration.clone();
            cross_mode.mode = aura_core::crypto::single_signer::SigningMode::Threshold;
            assert!(matches!(cross_mode.verify_for_admitted_original(
                invitee.runtime().effects().as_ref(), admitted.baseline(), admitted.local_setup(),
            ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::SigningPolicy)));
            let mut foreign_key = registration.clone();
            foreign_key.proof.public_key_package[0] ^= 1;
            assert!(foreign_key
                .verify_for_admitted_original(
                    invitee.runtime().effects().as_ref(),
                    admitted.baseline(),
                    admitted.local_setup(),
                )
                .await
                .is_err());
            let mut stale_epoch = registration.clone();
            stale_epoch.proof.epoch = stale_epoch.proof.epoch.wrapping_add(1);
            assert!(matches!(stale_epoch.verify_for_admitted_original(
                invitee.runtime().effects().as_ref(), admitted.baseline(), admitted.local_setup(),
            ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::SigningPolicy)));
            for field in ["session", "retrieval_credential_commitment"] {
                let mut foreign = serde_json::to_value(&registration).unwrap();
                foreign["statement"][field] = if field == "session" {
                    serde_json::json!(uuid::Uuid::from_bytes([91; 16]))
                } else {
                    serde_json::json!(vec![91_u8; 32])
                };
                let foreign: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration =
                    serde_json::from_value(foreign).unwrap();
                let result = foreign
                    .verify_for_admitted_original(
                        invitee.runtime().effects().as_ref(),
                        admitted.baseline(),
                        admitted.local_setup(),
                    )
                    .await;
                assert!(
                    if field == "session" {
                        matches!(result, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::Binding))
                    } else {
                        matches!(result, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::InvalidSignature))
                    },
                    "foreign {field} cannot select the original K1 registration"
                );
            }
            let mut reordered = registration.clone();
            reordered.proof.signers = vec![2, 1];
            assert!(matches!(reordered.verify_untrusted_broker_registration(
                invitee.runtime().effects().as_ref(), invitee.runtime().effects().as_ref(),
            ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::SigningPolicy)));
            assert!(matches!(reordered.verify_for_admitted_original(
                invitee.runtime().effects().as_ref(), admitted.baseline(), admitted.local_setup(),
            ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::SigningPolicy)));
            let mut forged = registration.clone();
            forged.proof.signature[0] ^= 1;
            assert!(matches!(forged.verify_for_admitted_original(
                invitee.runtime().effects().as_ref(), admitted.baseline(), admitted.local_setup(),
            ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::InvalidSignature)));
            let verified = registration
                .verify_for_admitted_original(
                    invitee.runtime().effects().as_ref(),
                    admitted.baseline(),
                    admitted.local_setup(),
                )
                .await
                .expect("original K1 return route is independently admitted");
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "aura-message-id".into(),
                uuid::Uuid::from_bytes([71; 16]).to_string(),
            );
            metadata.insert(
                "session-id".into(),
                verified.statement().session().to_string(),
            );
            metadata.insert(
                "content-type".into(),
                "application/aura-choreography".into(),
            );
            metadata.insert(
                "aura-destination-device-id".into(),
                manifest.initiator_device.to_string(),
            );
            metadata.insert(
                "aura-source-device-id".into(),
                manifest.invitee_device.to_string(),
            );
            let frame = aura_core::effects::transport::TransportEnvelope {
                source: manifest.invitee_authority,
                destination: manifest.subject,
                context: invitation.context_id,
                payload: vec![3, 8, 13],
                metadata,
                receipt: Some(aura_core::effects::transport::TransportReceipt {
                    context: invitation.context_id,
                    src: manifest.invitee_authority,
                    dst: manifest.subject,
                    epoch: manifest.final_epoch,
                    cost: 1,
                    nonce: 71,
                    prev: [0; 32],
                    sig: vec![1],
                }),
            };
            let signed = invitee
                .threshold_signing()
                .sign_enrollment_return_envelope_authorization(
                    &verified,
                    admitted.baseline(),
                    admitted.local_setup(),
                    &frame,
                )
                .await
                .expect("actual Bob setup key signs one return VM frame");
            signed
                .clone()
                .verify_for_admitted_original(
                    invitee.runtime().effects().as_ref(),
                    &verified,
                    admitted.baseline(),
                    admitted.local_setup(),
                    &frame,
                )
                .await
                .expect("exact reverse frame and setup key verify");
            let mut wrong_direction = frame.clone();
            wrong_direction.metadata.insert(
                "aura-destination-device-id".into(),
                manifest.invitee_device.to_string(),
            );
            assert!(signed
                .clone()
                .verify_for_admitted_original(
                    invitee.runtime().effects().as_ref(),
                    &verified,
                    admitted.baseline(),
                    admitted.local_setup(),
                    &wrong_direction,
                )
                .await
                .is_err());
            let mut foreign_source = frame.clone();
            foreign_source.source = manifest.subject;
            foreign_source.receipt.as_mut().unwrap().src = manifest.subject;
            assert!(signed
                .clone()
                .verify_for_admitted_original(
                    invitee.runtime().effects().as_ref(),
                    &verified,
                    admitted.baseline(),
                    admitted.local_setup(),
                    &foreign_source,
                )
                .await
                .is_err());
            let mut foreign_source_device = frame.clone();
            foreign_source_device.metadata.insert(
                "aura-source-device-id".into(),
                manifest.initiator_device.to_string(),
            );
            assert!(signed
                .clone()
                .verify_for_admitted_original(
                    invitee.runtime().effects().as_ref(),
                    &verified,
                    admitted.baseline(),
                    admitted.local_setup(),
                    &foreign_source_device,
                )
                .await
                .is_err());
            let mut changed_frame = frame.clone();
            changed_frame.payload[0] ^= 1;
            assert!(
                signed
                    .clone()
                    .verify_for_admitted_original(
                        invitee.runtime().effects().as_ref(),
                        &verified,
                        admitted.baseline(),
                        admitted.local_setup(),
                        &changed_frame,
                    )
                    .await
                    .is_err(),
                "setup proof cannot be reused for another payload"
            );
            let mut foreign_receiver = frame.clone();
            foreign_receiver.destination = manifest.invitee_authority;
            assert!(signed
                .clone()
                .verify_for_admitted_original(
                    invitee.runtime().effects().as_ref(),
                    &verified,
                    admitted.baseline(),
                    admitted.local_setup(),
                    &foreign_receiver,
                )
                .await
                .is_err());
            let mut foreign_digest = serde_json::to_value(&signed).unwrap();
            foreign_digest["statement"]["manifest_digest"] = serde_json::json!(vec![0_u8; 32]);
            let foreign_digest: aura_invitation::enrollment_mailbox::EnrollmentReturnEnvelopeAuthorization =
                serde_json::from_value(foreign_digest).unwrap();
            assert!(foreign_digest
                .verify_for_admitted_original(
                    invitee.runtime().effects().as_ref(),
                    &verified,
                    admitted.baseline(),
                    admitted.local_setup(),
                    &frame,
                )
                .await
                .is_err());
            let forged_location = SecureStorageLocation::new(
                "issuer_return_mailbox_registration_v1",
                format!("{}:foreign:{}", manifest.subject, hex::encode(digest)),
            );
            issuer
                .runtime()
                .effects()
                .secure_store_immutable(&forged_location, &bytes, &[SecureStorageCapability::Write])
                .await
                .expect("test retains a valid proof under a foreign record key");
            let error = issuer
                .drain_issuer_return_vm_mailbox()
                .await
                .expect_err("a copied registration at a foreign key cannot authorize intake");
            assert!(
                format!("{error:?}").contains("registration storage key changed"),
                "foreign storage key refusal retains its concrete cause: {error:?}"
            );
            clock.set_time(manifest.expires_at_ms);
            assert!(matches!(signed.verify_for_admitted_original(
                invitee.runtime().effects().as_ref(), &verified,
                admitted.baseline(), admitted.local_setup(), &frame,
            ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::OutsideValidity)));
        });
    }
}
pub(super) fn terminal_failure(
    reason: aura_app::runtime_bridge::CeremonyFailureReason,
) -> AgentError {
    failure(EnrollmentVmAdmissionError::TerminalFailed(reason))
}
pub(super) fn failure(error: EnrollmentVmAdmissionError) -> AgentError {
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
    parent_inventory: Vec<aura_invitation::enrollment_manifest::EnrollmentParentVerifier>,
    ops: Vec<aura_core::AttestedOp>,
    state: Box<aura_journal::commitment_tree::state::TreeState>,
}
impl VerifiedEnrollmentCommittedTransition {
    pub(crate) fn parent_inventory(
        &self,
    ) -> &[aura_invitation::enrollment_manifest::EnrollmentParentVerifier] {
        &self.parent_inventory
    }

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
    manifest_digest: [u8; 32],
    parent_inventory: Vec<aura_invitation::enrollment_manifest::EnrollmentParentVerifier>,
    state: Box<aura_journal::commitment_tree::state::TreeState>,
}
impl VerifiedEnrollmentTreeExtension {
    pub(crate) fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }

    pub(crate) fn parent_inventory(
        &self,
    ) -> &[aura_invitation::enrollment_manifest::EnrollmentParentVerifier] {
        &self.parent_inventory
    }

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
        if current.len() < original.len() || current.len() > 2 * aura_invitation::enrollment_manifest::EnrollmentTrustManifest::MAX_BASELINE_OPS {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        for (left, right) in current.iter().zip(original) {
            if aura_core::util::serialization::to_vec(left).map_err(stage)?
                != aura_core::util::serialization::to_vec(right).map_err(stage)?
            {
                return Err(failure(EnrollmentVmAdmissionError::Binding));
            }
        }
        let mut parent_inventory = self.committed_transition.parent_inventory().to_vec();
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
            let parent = check_admitted_node_operation(
                &self.manifest,
                &self.canonical_invitation,
                &state,
                op,
                target,
            )?;
            retain_verified_parent(&mut parent_inventory, parent)?;
            ops.push(op.clone());
            state = aura_journal::commitment_tree::reduce(&ops).map_err(stage)?;
            retain_admitted_node_keys(&self.manifest, &self.canonical_invitation, &mut state)?;
        }
        Ok(VerifiedEnrollmentTreeExtension {
            manifest_digest: self.manifest_digest,
            parent_inventory,
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
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "VerifiedEnrollmentFailureCapability",
        family = "proof_issuer"
    )]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(super) async fn verify_terminal_notice(
        &self,
        effects: &AuraEffectSystem,
        admitted: &AdmittedEnrollmentManifest,
    ) -> AgentResult<VerifiedEnrollmentFailureCapability> {
        let now = effects.physical_time().await.map_err(stage)?;
        self.verify_failure_at(effects, admitted, now.ts_ms).await
    }

    pub(super) async fn verify_failure_at(
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
/// Required producer check while the caller retains generation and tree custody.
/// Establishes current local membership at signing, without proving absence of
/// later remote revocation to the receiver.
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
    for parent in manifest
        .parents
        .iter()
        .chain(manifest.final_inventory().map_err(stage)?.iter())
    {
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
/// Called only after this exact operation passed admitted-node crypto/policy
/// verification. The authenticated intermediate head supplies the commitment;
/// signer policy and public material remain the original admitted inventory.
fn captured_admitted_parent(
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    canonical: &super::Invitation,
    state: &aura_journal::commitment_tree::state::TreeState,
    target: aura_core::tree::NodeIndex,
) -> AgentResult<aura_invitation::enrollment_manifest::EnrollmentParentVerifier> {
    use aura_invitation::enrollment_manifest::{EnrollmentParentVerifier, EnrollmentTrustManifest};
    if state.epoch.value() == manifest.pending_epoch {
        if target != aura_core::tree::NodeIndex(0) {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
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
            EnrollmentTrustManifest::decode_pending_policy(threshold_config).map_err(stage)?;
        return Ok(EnrollmentParentVerifier {
            epoch: state.epoch.value(),
            commitment: state.root_commitment,
            signing_node: target,
            mode: policy.signing_mode(),
            threshold: policy.threshold(),
            participants: policy.participants().to_vec(),
            public_key_package: public_key_package.clone(),
            agreement: policy.agreement(),
        });
    }
    let mut selected: Option<EnrollmentParentVerifier> = None;
    for origin in manifest
        .parents
        .iter()
        .chain(manifest.final_inventory().map_err(stage)?.iter())
        .filter(|origin| origin.epoch == state.epoch.value() && origin.signing_node == target)
    {
        let mut captured = origin.clone();
        captured.commitment = state.root_commitment;
        if let Some(previous) = &selected {
            if aura_core::util::serialization::to_vec(previous).map_err(stage)?
                != aura_core::util::serialization::to_vec(&captured).map_err(stage)?
            {
                return Err(failure(EnrollmentVmAdmissionError::Binding));
            }
        } else {
            selected = Some(captured);
        }
    }
    selected.ok_or_else(|| failure(EnrollmentVmAdmissionError::Binding))
}
fn retain_verified_parent(
    inventory: &mut Vec<aura_invitation::enrollment_manifest::EnrollmentParentVerifier>,
    parent: aura_invitation::enrollment_manifest::EnrollmentParentVerifier,
) -> AgentResult<()> {
    if let Some(existing) = inventory.iter().find(|existing| {
        existing.epoch == parent.epoch
            && existing.commitment == parent.commitment
            && existing.signing_node == parent.signing_node
    }) {
        if aura_core::util::serialization::to_vec(existing).map_err(stage)?
            != aura_core::util::serialization::to_vec(&parent).map_err(stage)?
        {
            return Err(failure(EnrollmentVmAdmissionError::Binding));
        }
        return Ok(());
    }
    if inventory.len()
        >= 2 * aura_invitation::enrollment_manifest::EnrollmentTrustManifest::MAX_PARENTS
    {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    inventory.push(parent);
    Ok(())
}

fn check_admitted_node_operation(
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    canonical: &super::Invitation,
    state: &aura_journal::commitment_tree::state::TreeState,
    op: &aura_core::AttestedOp,
    target: aura_core::tree::NodeIndex,
) -> AgentResult<aura_invitation::enrollment_manifest::EnrollmentParentVerifier> {
    let mut policies = std::collections::BTreeMap::new();
    let mut signing_rosters = std::collections::BTreeMap::new();
    for parent in manifest
        .parents
        .iter()
        .chain(manifest.final_inventory().map_err(stage)?.iter())
    {
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
    })?;
    captured_admitted_parent(manifest, canonical, state, target)
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
    let mut parent_inventory = admitted.manifest().parents.clone();
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
        let parent = check_admitted_node_operation(
            admitted.manifest(),
            admitted.canonical_invitation(),
            &state,
            op,
            target,
        )?;
        retain_verified_parent(&mut parent_inventory, parent)?;
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
        parent_inventory,
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
    #[cfg(test)]
    eprintln!("enrollment control signing: awaiting original generation guard");
    let _generation_owner = effects.enrollment_retirement_generation_guard().await;
    #[cfg(test)]
    eprintln!("enrollment control signing: generation acquired; awaiting tree guard");
    let _decision_owner = effects.lock_tree_decision().await;
    #[cfg(test)]
    eprintln!("enrollment control signing: tree acquired");
    if matches!(&decision, EnrollmentControlDecision::Committed(_)) {
        require_current_confirmation_membership(effects, manifest).await?;
    }
    let identity_context =
        crate::handlers::rendezvous_identity::require_retained_identity_signing_context(
            effects, retained,
        )
        .await
        .map_err(stage)?;
    let (private, public) =
        crate::handlers::rendezvous_identity::require_identity_keys(&identity_context)
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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredQuorumInitialRequest {
    version: u16,
    transcript_digest: [u8; 32],
    signature: Vec<u8>,
}

/// Native evidence has no public construction, clone, or deserialization path.
/// ```compile_fail
/// use aura_agent::handlers::invitation::enrollment_vm_admission::VerifiedQuorumInitialRequest;
/// let forged: VerifiedQuorumInitialRequest = serde_json::from_str("{}").unwrap();
/// ```
struct VerifiedQuorumInitialRequest {
    frame: EnrollmentControlFrame,
}

fn initial_request_location(
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
) -> AgentResult<SecureStorageLocation> {
    let transcript = aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript::from_manifest(manifest).map_err(transcript_stage)?;
    let digest = aura_core::hash::hash(&transcript.transcript_bytes().map_err(transcript_stage)?);
    Ok(SecureStorageLocation::with_sub_key(
        "enrollment_quorum_initial_request_v1",
        manifest.subject.to_string(),
        // The canonical transcript already binds the complete original manifest
        // and Request. Compact ASCII addressing avoids escaped display values
        // exceeding the native filesystem component bound; record verification
        // retains the original signature, transcript and runtime/window checks.
        hex::encode(digest),
    ))
}

// Borrows actual native issuance or retained control custody. Neither remote
// manifests nor independently supplied byte slices can construct this owner.
enum OriginalInitialRequestVerifier<'owner> {
    Issued(&'owner crate::handlers::invitation_service::IssuedEnrollmentManifestBinding),
    Retained(&'owner super::enrollment_trust::RetainedEnrollmentVmControl),
}
impl OriginalInitialRequestVerifier<'_> {
    fn original_request_key<'owner>(
        &'owner self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<&'owner [u8]> {
        match self {
            Self::Issued(issued) => {
                issued.require_effects(effects)?;
                Ok(issued.confirmation_verifier())
            }
            Self::Retained(retained) => {
                retained.require_runtime_owner(effects)?;
                Ok(retained.expected_request_verifier())
            }
        }
    }
    fn manifest(&self) -> &aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
        match self {
            Self::Issued(issued) => issued.manifest(),
            Self::Retained(retained) => retained.manifest(),
        }
    }
}
async fn verify_initial_request(
    effects: &AuraEffectSystem,
    original: &OriginalInitialRequestVerifier<'_>,
    signature: Vec<u8>,
) -> AgentResult<VerifiedQuorumInitialRequest> {
    use aura_core::effects::CryptoExtendedEffects;
    let manifest = original.manifest();
    let transcript = aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript::from_manifest(manifest).map_err(transcript_stage)?;
    let bytes = transcript.required_transcript_bytes().map_err(stage)?;
    if original.original_request_key(effects)? != manifest.initiator_confirmation_verifier
        || !effects
            .frost_verify(&bytes, &signature, original.original_request_key(effects)?)
            .await?
    {
        return Err(failure(EnrollmentVmAdmissionError::Signature));
    }
    Ok(VerifiedQuorumInitialRequest {
        frame: EnrollmentControlFrame {
            version: 2,
            manifest_digest: aura_core::hash::hash(
                &manifest.transcript_bytes().map_err(transcript_stage)?,
            ),
            decision: EnrollmentControlDecision::Request(DeviceEnrollmentRequest {
                invitation_id: manifest.invitation.clone(),
                subject_authority: manifest.subject,
                ceremony_id: manifest.ceremony.clone(),
                pending_epoch: manifest.pending_epoch,
                device_id: manifest.invitee_device,
            }),
            committed_ops: None,
            signature,
        },
    })
}

#[aura_macros::capability_boundary(category = "capability_gated", capability = "issued", capability_type = crate::handlers::invitation_service::IssuedEnrollmentManifestBinding, family = "runtime_helper")]
pub(crate) async fn retain_quorum_initial_request(
    effects: &AuraEffectSystem,
    issued: &crate::handlers::invitation_service::IssuedEnrollmentManifestBinding,
    final_inventory: &crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<'_, '_>,
    approved: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    signature: Vec<u8>,
) -> AgentResult<()> {
    issued.require_effects(effects)?;
    final_inventory.require_manifest(effects, issued.manifest())?;
    if !std::ptr::eq(approved.effects().as_ref(), effects)
        || approved
            .manifest()
            .transcript_bytes()
            .map_err(transcript_stage)?
            != issued
                .manifest()
                .transcript_bytes()
                .map_err(transcript_stage)?
    {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let root = final_inventory
        .inventory()
        .iter()
        .find(|entry| entry.signing_node == aura_core::tree::NodeIndex(0))
        .ok_or_else(|| failure(EnrollmentVmAdmissionError::Binding))?;
    if root.mode != aura_core::effects::crypto::SigningMode::Threshold || root.threshold < 2 {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let package = frost_ed25519::keys::PublicKeyPackage::deserialize(&root.public_key_package)
        .map_err(stage)?;
    let expected = package.verifying_key().serialize();
    if expected.as_slice() != issued.confirmation_verifier() {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let verified = verify_initial_request(
        effects,
        &OriginalInitialRequestVerifier::Issued(issued),
        signature,
    )
    .await?;
    let bytes = verified
        .frame
        .transcript()
        .transcript_bytes()
        .map_err(transcript_stage)?;
    let record = StoredQuorumInitialRequest {
        version: 1,
        transcript_digest: aura_core::hash::hash(&bytes),
        signature: verified.frame.signature,
    };
    let encoded = aura_core::util::serialization::to_vec(&record).map_err(stage)?;
    let location = initial_request_location(issued.manifest())?;
    match effects
        .secure_store_immutable(
            &location,
            &encoded,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?
    {
        aura_core::effects::secure::ImmutableSecureStoreOutcome::Created => Ok(()),
        aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists => {
            if effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await?
                == encoded
            {
                Ok(())
            } else {
                Err(failure(EnrollmentVmAdmissionError::Binding))
            }
        }
    }
}

async fn load_quorum_initial_request(
    effects: &AuraEffectSystem,
    retained: &RetainedEnrollmentVmControl,
) -> AgentResult<Option<VerifiedQuorumInitialRequest>> {
    retained
        .require_runtime_owner(effects)
        .map_err(AgentError::from)?;
    let location = initial_request_location(retained.manifest())?;
    if !effects.secure_exists(&location).await? {
        return Ok(None);
    }
    let bytes = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await?;
    if bytes.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    let record: StoredQuorumInitialRequest =
        aura_core::util::serialization::from_slice(&bytes).map_err(stage)?;
    let verified = verify_initial_request(
        effects,
        &OriginalInitialRequestVerifier::Retained(retained),
        record.signature,
    )
    .await?;
    if record.version != 1
        || retained.digest() != verified.frame.manifest_digest
        || record.transcript_digest
            != aura_core::hash::hash(
                &verified
                    .frame
                    .transcript()
                    .transcript_bytes()
                    .map_err(transcript_stage)?,
            )
    {
        return Err(failure(EnrollmentVmAdmissionError::Binding));
    }
    Ok(Some(verified))
}

pub(super) async fn sign_request(
    effects: &AuraEffectSystem,
    retained: &RetainedEnrollmentVmControl,
) -> AgentResult<EnrollmentControlFrame> {
    if let Some(verified) = load_quorum_initial_request(effects, retained).await? {
        return Ok(verified.frame);
    }
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

/// Test-only cryptographic assertion helper; production uses the original owned terminal receipt.
#[cfg(test)]
async fn sign_committed_confirmation(
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
            sign_committed_confirmation_frame(effects, retained).await
        }
        aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(reason) => {
            Err(failure(EnrollmentVmAdmissionError::TerminalFailed(reason)))
        }
    }
}
async fn sign_committed_confirmation_frame(
    effects: &AuraEffectSystem,
    retained: &RetainedEnrollmentVmControl,
) -> AgentResult<EnrollmentControlFrame> {
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
/// Native terminal signing custody. Wire frames and stored terminal states
/// cannot reconstruct this receipt; VM completion alone does not create it.
pub(crate) struct IssuerEnrollmentTerminalReceipt {
    runtime_owner: Arc<AuraEffectSystem>,
    retained: Arc<RetainedEnrollmentVmControl>,
    frame: EnrollmentControlFrame,
}

impl IssuerEnrollmentTerminalReceipt {
    /// Read-only projection of the actual native terminal receipt.
    pub(crate) fn positive(&self) -> bool {
        matches!(self.frame.decision, EnrollmentControlDecision::Committed(_))
    }
    pub(crate) fn require_runtime_owner(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<(), aura_core::AuraError> {
        if !std::ptr::eq(self.runtime_owner.as_ref(), effects) {
            return Err(aura_core::AuraError::PermissionDenied {
                message: "issuer terminal receipt belongs to another physical provider".into(),
                source: Some(Arc::new(
                    super::enrollment_trust::EnrollmentVerifierError::RuntimeOwner,
                )),
            });
        }
        self.retained.require_runtime_owner(effects)
    }

    pub(crate) fn retained(&self) -> &RetainedEnrollmentVmControl {
        &self.retained
    }

    pub(super) fn frame(&self) -> &EnrollmentControlFrame {
        &self.frame
    }
}

pub(super) async fn sign_terminal_receipt(
    effects: Arc<AuraEffectSystem>,
    retained: Arc<RetainedEnrollmentVmControl>,
    original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
) -> AgentResult<IssuerEnrollmentTerminalReceipt> {
    let observed = original
        .await_original_terminal(effects.as_ref(), &retained)
        .await?;
    observed.validation.map_err(|source| {
        AgentError::from(aura_core::AuraError::crypto_with_source(
            "original terminal observation validation",
            Arc::new(source),
        ))
    })?;
    let outcome = observed.operation.ok_or_else(|| {
        aura_core::AuraError::invalid("original terminal observation remains unresolved")
    })??;
    if retained
        .manifest()
        .final_inventory
        .iter()
        .any(|parent| parent.mode == aura_core::crypto::single_signer::SigningMode::Threshold)
    {
        let observed = original
            .acquire_original_conditional_signing(effects.as_ref(), &retained)
            .await?;
        retained.retain_original_conditional_acquisition(&observed)?;
        observed.validation.map_err(|source| {
            AgentError::from(aura_core::AuraError::crypto_with_source(
                "original conditional state acquisition validation",
                Arc::new(source),
            ))
        })?;
        let mut state = observed.operation.ok_or_else(|| {
            aura_core::AuraError::invalid("original conditional state acquisition interrupted")
        })??;
        if let aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(reason) = outcome {
            let decision = original.original_negative_decision(&retained)?;
            let signature = state
                .sign_original_negative_terminal(Arc::clone(&retained), original, decision)
                .await?;
            let manifest = retained.manifest();
            let frame = EnrollmentControlFrame {
                version: 2,
                manifest_digest: retained.digest(),
                decision: EnrollmentControlDecision::Failed {
                    invitation: manifest.invitation.clone(),
                    ceremony: manifest.ceremony.clone(),
                    reason,
                },
                committed_ops: None,
                signature,
            };
            state.require_original_terminal_message(
                &EnrollmentControlTranscript {
                    version: frame.version,
                    manifest_digest: frame.manifest_digest,
                    decision: frame.decision.clone(),
                    committed_ops: None,
                }
                .transcript_bytes()
                .map_err(transcript_stage)?,
            )?;
            return Ok(IssuerEnrollmentTerminalReceipt {
                runtime_owner: effects,
                retained,
                frame,
            });
        }
        let decision = original.original_positive_decision(&retained)?;
        let signature = state
            .sign_original_positive_terminal(Arc::clone(&retained), original, decision)
            .await?;
        let manifest = retained.manifest();
        let frame = EnrollmentControlFrame {
            version: 2,
            manifest_digest: retained.digest(),
            decision: EnrollmentControlDecision::Committed(DeviceEnrollmentConfirm {
                invitation_id: manifest.invitation.clone(),
                ceremony_id: manifest.ceremony.clone(),
                established: true,
                new_epoch: Some(manifest.pending_epoch),
            }),
            committed_ops: Some(decision.committed_ops().to_vec()),
            signature,
        };
        state.require_original_positive_terminal_message(
            &frame
                .transcript()
                .transcript_bytes()
                .map_err(transcript_stage)?,
        )?;
        return Ok(IssuerEnrollmentTerminalReceipt {
            runtime_owner: effects,
            retained,
            frame,
        });
    }
    let frame = original
        .execute(effects.as_ref(), || {
            sign_terminal_confirmation_for_outcome(effects.as_ref(), &retained, outcome)
        })
        .await
        .map_err(|source| original.map_run_error("original issuer terminal signing", source))?;
    Ok(IssuerEnrollmentTerminalReceipt {
        runtime_owner: effects,
        retained,
        frame,
    })
}

pub(super) async fn sign_terminal_confirmation(
    effects: &AuraEffectSystem,
    retained: &RetainedEnrollmentVmControl,
    runner: &crate::runtime::services::ceremony_runner::CeremonyRunner,
) -> AgentResult<EnrollmentControlFrame> {
    #[cfg(test)]
    eprintln!("enrollment terminal signing: awaiting actual terminal outcome");
    let outcome = runner
        .await_enrollment_terminal_outcome(&retained.manifest().ceremony)
        .await
        .map_err(AgentError::from)?;
    sign_terminal_confirmation_for_outcome(effects, retained, outcome).await
}

async fn sign_terminal_confirmation_for_outcome(
    effects: &AuraEffectSystem,
    retained: &RetainedEnrollmentVmControl,
    outcome: aura_app::runtime_bridge::CeremonyTerminalOutcome,
) -> AgentResult<EnrollmentControlFrame> {
    match outcome {
        aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed => {
            sign_committed_confirmation_frame(effects, retained).await
        }
        aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(reason) => {
            #[cfg(test)]
            eprintln!("enrollment terminal signing: actual negative outcome {reason:?}");
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
        manifest_digest: admitted.manifest_digest(),
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
            manifest_digest: admitted.manifest_digest(),
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
            // The issuer will not even sign a Committed decision for an epoch it
            // has not committed, so no authenticated negative confirmation exists
            // for the invitee to adopt.
            sign_control(
                issuer_effects.as_ref(),
                &retained,
                EnrollmentControlDecision::Committed(negative),
            )
            .await
            .expect_err("issuer cannot sign a negative uncommitted confirmation")
        }
        _ => panic!("unsupported actual negative fixture"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_first_enrollment_vm_frame_reaches_verified_invitee_response_boundary() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(5_000));
            let selected: Arc<dyn aura_core::effects::PhysicalTimeEffects> = clock.clone();
            let ((issuer, invitee, invitation, _start, _accept, _verified), _original, issuer_entered, issuer_release) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_for_pending_mailbox(
                    "genuine-first-mailbox-vm", selected,
                ),
            ).await;
            let issuer_effects = issuer.runtime().effects();
            let invitee_effects = invitee.runtime().effects();
            let admitted = super::super::enrollment_manifest_admission::load_admitted_baseline(
                invitee_effects.as_ref(),
                invitee.authority_id(),
                &invitation,
            )
            .await
            .expect("actual transferred manifest remains admitted");
            let broker = crate::runtime::services::bootstrap_broker::LocalBootstrapBrokerService::bind(
                "127.0.0.1:0", std::time::Duration::from_secs(60), "genuine-first-vm-broker-auth",
                crate::runtime::services::bootstrap_broker::BootstrapBrokerLanBindPolicy::LoopbackOnly,
            ).await.expect("actual loopback broker binds")
                .with_mailbox_time_provider(Arc::new(invitee_effects.time_effects().clone()));
            let broker_tasks = std::sync::Arc::new(crate::runtime::TaskSupervisor::new());
            broker.start(&broker_tasks.group("genuine-first-vm-broker"));
            let broker_config =
                crate::runtime::services::bootstrap_broker::BootstrapBrokerConfig::default()
                    .enabled(true)
                    .with_base_url(broker.public_url())
                    .with_auth_token("genuine-first-vm-broker-auth");
            let issuer_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
                issuer.authority_id(),
                crate::runtime::services::RendezvousManagerConfig::default()
                    .with_bootstrap_broker(broker_config.clone()),
                Arc::new(issuer_effects.time_effects().clone()),
            );
            let invitee_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
                invitee.authority_id(),
                crate::runtime::services::RendezvousManagerConfig::default()
                    .with_bootstrap_broker(broker_config.clone()),
                Arc::new(invitee_effects.time_effects().clone()),
            );
            issuer_effects.attach_rendezvous_manager(issuer_manager);
            invitee_effects.attach_rendezvous_manager(invitee_manager);
            let invitee_bridge: Arc<dyn aura_app::runtime_bridge::RuntimeBridge> = Arc::new(
                crate::runtime_bridge::AgentRuntimeBridge::new(invitee.clone()),
            );
            let invitee_app = Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(
                    aura_app::AppConfig::default(),
                    invitee_bridge.clone(),
                )
                .expect("actual invitee app owns its runtime"),
            ));
            let session =
                aura_invitation::enrollment_mailbox_session::original_enrollment_session_id(
                    &invitation.invitation_id,
                );
            assert!(
                issuer_effects.original_mailbox_issuer(session).is_some(),
                "actual issuer still owns the original VM mailbox session"
            );
            tokio::time::timeout(std::time::Duration::from_secs(20), issuer_entered)
                .await
                .expect("original issuer reached its actual first VM execution")
                .expect("original issuer first-VM gate remained owned");
            let first_lookup_miss = broker
                .observe_next_pending_registration_miss_for_test()
                .await;
            let retry_sleep = clock.observe_next_relative_sleep_of(100).await.unwrap();
            issuer_release.release();
            tokio::time::timeout(std::time::Duration::from_secs(20), first_lookup_miss)
                .await
                .expect("Alice looked up Bob's original mailbox before import registered it")
                .expect("first missing registration was observed");
            tokio::time::timeout(std::time::Duration::from_secs(20), retry_sleep)
                .await
                .expect("Alice waits on the original signed enrollment lifetime")
                .expect("original registration retry remained owned");
            let manifest = admitted.manifest();
            let missing_progress = crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                    broker.public_url(),
                    "genuine-first-vm-broker-auth",
                    manifest.subject,
                    manifest.invitee_device,
                    admitted.manifest_digest(),
                )
                .await.unwrap();
            assert_eq!(missing_progress["registration_present"], false);
            assert!(missing_progress["first_message_id"].is_null());
            assert_eq!(missing_progress["messages"].as_array().unwrap().len(), 0);
            let (registration_entered, release_registration) =
                broker.pause_next_pending_registration_for_test().await;
            let pending_app = invitee_app.clone();
            let pending_bridge = invitee_bridge.clone();
            let blocked_pass = tokio::spawn(async move {
                aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                    &pending_app,
                    &pending_bridge,
                )
                .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(10), registration_entered)
                .await
                .expect("receiver reached actual broker registration")
                .expect("registration pause remained owned");
            // The fixture uses the selected manual PhysicalTimeEffects provider;
            // elapsed wall time must not be substituted for its deadline.
            clock.advance(1_100);
            let blocked_result =
                tokio::time::timeout(std::time::Duration::from_secs(10), blocked_pass)
                    .await
                    .expect("bounded maintenance terminates while registration is held")
                    .expect("maintenance task did not panic");
            assert!(
                blocked_result.is_err(),
                "registration wait must fail bounded maintenance"
            );
            assert!(
                broker
                    .pending_enrollment_mailbox_registration(
                        manifest.subject,
                        manifest.invitee_device,
                        admitted.manifest_digest(),
                    )
                    .await
                    .unwrap()
                    .is_none(),
                "held registration has not been published or exposed to Alice"
            );
            release_registration
                .send(())
                .expect("release broker registration response");
            let first_progress = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                let mut observation_tick = tokio::time::interval(std::time::Duration::from_millis(20));
                let mut last_stage = None;
                loop {
                    let progress = crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                        broker.public_url(),
                        "genuine-first-vm-broker-auth",
                        manifest.subject,
                        manifest.invitee_device,
                        admitted.manifest_digest(),
                    ).await.expect("authenticated exact broker progress");
                    let stage = (
                        progress["subject"] == manifest.subject.to_string(),
                        progress["device"] == manifest.invitee_device.to_string(),
                        progress["manifest_digest"] == hex::encode(admitted.manifest_digest()),
                        progress["registration_present"] == true,
                        progress["messages"].as_array().map(Vec::len),
                        progress["messages"][0]["stage"].as_str().map(str::to_owned),
                    );
                    if last_stage.as_ref() != Some(&stage) {
                        eprintln!("broker fixture progress flags: {stage:?}");
                        last_stage = Some(stage);
                    }
                    if progress["subject"] == manifest.subject.to_string()
                        && progress["device"] == manifest.invitee_device.to_string()
                        && progress["manifest_digest"] == hex::encode(admitted.manifest_digest())
                        && progress["registration_present"] == true
                        && progress["messages"].as_array().is_some_and(|messages| messages.len()==1)
                        && matches!(progress["messages"][0]["stage"].as_str(), Some("queued" | "retrieval_selected_for_receiver_credential" | "receiver_acknowledged_retained_frame")) {
                        break progress;
                    }
                    if let Some(target) = clock.next_registered_deadline().await {
                        assert!(target.value() > clock.now_ms(), "observed selected-clock wait must be future");
                        assert!(target.value() < manifest.expires_at_ms, "retry must stay inside original signed lifetime");
                        clock.set_time(target.value());
                    } else {
                        observation_tick.tick().await;
                    }
                }
            })
            .await
            .expect("original issuer submitted the first exact authenticated envelope");
            let first_message_id = first_progress["messages"][0]["message_id"]
                .as_str()
                .expect("broker message has exact ID")
                .to_owned();
            uuid::Uuid::parse_str(&first_message_id).expect("broker message ID is typed UUID");
            let first_digest = first_progress["messages"][0]["envelope_digest"]
                .as_str()
                .expect("broker message has exact digest");
            assert_eq!(hex::decode(first_digest).unwrap().len(), 32);
            assert_eq!(first_progress["registration_present"], true);
            assert_eq!(first_progress["first_message_id"], first_message_id);
            assert_eq!(
                first_progress["messages"][0]["message_id"],
                first_message_id
            );
            assert!(
                crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                    broker.public_url(),
                    "wrong-bearer",
                    manifest.subject,
                    manifest.invitee_device,
                    admitted.manifest_digest(),
                )
                .await
                .is_err(),
                "progress witness refuses the wrong bearer even for an existing selector"
            );
            for (subject, device, digest) in [
                (
                    aura_core::AuthorityId::new_from_entropy([9; 32]),
                    manifest.invitee_device,
                    admitted.manifest_digest(),
                ),
                (
                    manifest.subject,
                    aura_core::DeviceId::from_bytes([9; 32]),
                    admitted.manifest_digest(),
                ),
                (manifest.subject, manifest.invitee_device, [9; 32]),
            ] {
                let mismatched = crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                    broker.public_url(),
                    "genuine-first-vm-broker-auth",
                    subject,
                    device,
                    digest,
                )
                .await
                .expect("authorized nonmatching selector has no broker custody");
                assert_eq!(mismatched["registration_present"], false);
                assert!(mismatched["first_message_id"].is_null());
                assert_eq!(mismatched["messages"].as_array().unwrap().len(), 0);
            }
            use aura_core::effects::{SecureStorageCapability, SecureStorageEffects};
            let credential_location =
                crate::core::agent::pending_enrollment_mailbox::retained_location(&admitted);
            let credential_bytes = invitee_effects
                .secure_retrieve(&credential_location, &[SecureStorageCapability::Read])
                .await
                .expect("recipient owns exact mailbox credential");
            let retained: crate::core::agent::pending_enrollment_mailbox::RetainedMailboxCredential =
                aura_core::util::serialization::from_slice(&credential_bytes).unwrap();
            let encoded_progress = serde_json::to_string(&first_progress).unwrap();
            assert!(!encoded_progress.contains(&retained.credential));
            assert_eq!(first_progress.as_object().unwrap().len(), 6);
            assert_eq!(first_progress["messages"][0].as_object().unwrap().len(), 3);
            let retry_result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                    &invitee_app,
                    &invitee_bridge,
                ),
            )
            .await
            .expect("receiver retries bounded maintenance");
            // Mailbox upkeep precedes sibling sync; this fixture has no running
            // rendezvous service, so the independent sync error stays typed.
            assert!(
                matches!(retry_result, Err(aura_core::AuraError::Internal { .. })),
                "retry must preserve the independent typed sync failure: {retry_result:?}"
            );
            issuer
                .drain_issuer_return_vm_mailbox()
                .await
                .expect("Alice publishes her signed return mailbox before Bob responds");
            broker.interrupt_next_issuer_return_ack_for_test().await;
            let accepted = async {
                invitee
                    .invitations()
                    .unwrap()
                    .accept(&invitation.invitation_id)
                    .await
            };
            let mut accepted = Box::pin(accepted);
            // Preserve Alice's in-flight broker take/ACK across select turns.
            // Recreating it after every ready Bob poll cancels the network request.
            let mut issuer_drain = Box::pin(issuer.drain_issuer_return_vm_mailbox());
            let mut maintenance_rounds = 0;
            let mut accepted_completed = false;
            let mut receiver_maintenance = Box::pin(
                aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                    &invitee_app,
                    &invitee_bridge,
                ),
            );
            let ack_failure = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                loop {
                    tokio::select! {
                        result = &mut accepted, if !accepted_completed => {
                            let result = result.expect("Bob's actual signed response was accepted");
                            assert_eq!(result.new_status, super::super::InvitationStatus::Accepted);
                            // Bob may finish once the broker has durably enqueued his
                            // response. Alice's separate take/ACK must still be driven
                            // and tested; never poll the completed future again.
                            accepted_completed = true;
                        },
                        _result = &mut receiver_maintenance => {
                            maintenance_rounds += 1;
                            receiver_maintenance = Box::pin(
                                aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                                    &invitee_app, &invitee_bridge,
                                ),
                            );
                            tokio::task::yield_now().await;
                        },
                        result = &mut issuer_drain => {
                            match result {
                                Ok(_) => {
                                    issuer_drain = Box::pin(issuer.drain_issuer_return_vm_mailbox());
                                    tokio::task::yield_now().await;
                                }
                                Err(error) => break error,
                            }
                        }
                    }
                }
            })
            .await
            .expect("Bob's first response reaches Alice's interrupted broker ACK");
            assert!(
                format!("{ack_failure:?}").contains("status: 401"),
                "first return frame must fail only at injected broker ACK status: {ack_failure:?}"
            );
            let pending_acks = broker
                .pending_ack_witnesses_for_test(
                    manifest.subject,
                    manifest.invitee_device,
                    admitted.manifest_digest(),
                )
                .await
                .unwrap();
            assert!(
                pending_acks
                    .iter()
                    .any(
                        |(message_id, subject, device, digest)| message_id == &first_message_id
                            && subject == &manifest.subject.to_string()
                            && device == &manifest.invitee_device.to_string()
                            && digest == &hex::encode(admitted.manifest_digest())
                    ),
                "runtime maintenance must retain and ACK the exact original first envelope"
            );
            let acknowledged_progress = crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                    broker.public_url(),
                    "genuine-first-vm-broker-auth",
                    manifest.subject,
                    manifest.invitee_device,
                    admitted.manifest_digest(),
                )
                .await.unwrap();
            assert_eq!(acknowledged_progress["first_message_id"], first_message_id);
            assert_eq!(
                acknowledged_progress["messages"][0]["message_id"],
                first_message_id
            );
            assert_eq!(
                acknowledged_progress["messages"][0]["stage"],
                "receiver_acknowledged_retained_frame"
            );
            let pending_location =
                crate::core::agent::pending_enrollment_mailbox::retained_location(&admitted);
            let pending_credential = invitee_effects
                .secure_retrieve(&pending_location, &[SecureStorageCapability::Read])
                .await
                .expect("Bob retained his exact signed mailbox credential");
            let pending_root_bytes = invitee_effects
                .secure_retrieve(
                    &crate::core::agent::pending_enrollment_mailbox::retirement_root_location(
                        admitted.manifest_digest(),
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("Bob sealed the original credential before broker registration");
            let pending_root: crate::core::agent::pending_enrollment_mailbox::PendingMailboxRetirementRoot =
                aura_core::util::serialization::from_slice(&pending_root_bytes).unwrap();
            assert!(pending_root.verifies(admitted.manifest_digest(), &pending_credential));
            let pending_root_digest = aura_core::hash::hash(&pending_root_bytes);
            let pending_frame_keys = invitee_effects
                .secure_list_keys(
                    "pending_enrollment_mailbox_frame_v1",
                    &[SecureStorageCapability::List],
                )
                .await
                .expect("Bob's protected first-frame inventory");
            assert!(
                !pending_frame_keys.is_empty(),
                "Bob retained Alice's first VM frame"
            );
            for key in pending_frame_keys {
                let frame = invitee_effects
                    .secure_retrieve(
                        &aura_core::effects::SecureStorageLocation::new(
                            "pending_enrollment_mailbox_frame_v1",
                            &key,
                        ),
                        &[SecureStorageCapability::Read],
                    )
                    .await
                    .expect("Bob's original protected frame");
                let leaf_bytes = invitee_effects
                    .secure_retrieve(
                        &crate::core::agent::pending_enrollment_mailbox::retirement_leaf_location(
                            &key,
                        ),
                        &[SecureStorageCapability::Read],
                    )
                    .await
                    .expect("Bob sealed original frame before broker ACK");
                let leaf: crate::core::agent::pending_enrollment_mailbox::PendingMailboxRetirementLeaf =
                    aura_core::util::serialization::from_slice(&leaf_bytes).unwrap();
                assert!(leaf.verifies(pending_root_digest, &key, &frame));
                let mut substituted = frame.clone();
                substituted[0] ^= 1;
                assert!(!leaf.verifies(pending_root_digest, &key, &substituted));
            }
            let frame_keys = issuer_effects
                .secure_list_keys(
                    "issuer_return_mailbox_frame_v1",
                    &[SecureStorageCapability::List],
                )
                .await
                .expect("Alice's protected return frame inventory");
            assert_eq!(
                frame_keys.len(),
                1,
                "Alice retained the exact encrypted frame before the interrupted ACK; maintenance rounds={maintenance_rounds}"
            );
            let frame_key = &frame_keys[0];
            let frame_bytes = issuer_effects
                .secure_retrieve(
                    &aura_core::effects::SecureStorageLocation::new(
                        "issuer_return_mailbox_frame_v1",
                        frame_key,
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("return frame retained before ACK");
            let leaf_bytes = issuer_effects
                .secure_retrieve(
                    &crate::core::agent::issuer_return_mailbox::retirement_leaf_location(frame_key),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("exact frame retirement leaf sealed before ACK");
            let leaf: crate::core::agent::issuer_return_mailbox::IssuerReturnRetirementLeaf =
                aura_core::util::serialization::from_slice(&leaf_bytes).unwrap();
            let manifest = admitted.manifest();
            let registration_bytes = issuer_effects
                .secure_retrieve(
                    &aura_core::effects::SecureStorageLocation::new(
                        "issuer_return_mailbox_registration_v1",
                        format!(
                            "{}:{}:{}",
                            manifest.subject,
                            manifest.invitation,
                            hex::encode(admitted.manifest_digest())
                        ),
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("original return registration retained");
            let registration: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration =
                aura_core::util::serialization::from_slice(&registration_bytes).unwrap();
            let root_bytes = issuer_effects
                .secure_retrieve(
                    &crate::core::agent::issuer_return_mailbox::retirement_root_location(
                        &registration.statement,
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("root sealed before broker publication");
            let root_digest = aura_core::hash::hash(&root_bytes);
            assert!(leaf.verifies(root_digest, frame_key, &frame_bytes));
            let mut substituted_frame = frame_bytes.clone();
            substituted_frame[0] ^= 1;
            assert!(!leaf.verifies(root_digest, frame_key, &substituted_frame));
            let credential_location = aura_core::effects::SecureStorageLocation::with_sub_key(
                "issuer_return_mailbox_credential_v1",
                manifest.subject.to_string(),
                format!(
                    "{}:{}",
                    manifest.invitation,
                    hex::encode(admitted.manifest_digest())
                ),
            );
            let credential_bytes = issuer_effects
                .secure_retrieve(&credential_location, &[SecureStorageCapability::Read])
                .await
                .expect("same original issuer retained the return retrieval secret");
            let credential = std::str::from_utf8(&credential_bytes).unwrap();
            let held = broker
                .take_issuer_return_vm_envelopes(
                    manifest.subject,
                    manifest.initiator_device,
                    admitted.manifest_digest(),
                    credential,
                )
                .await
                .unwrap();
            assert_eq!(
                held.len(),
                1,
                "interrupted broker ACK retains the exact authenticated response"
            );
            let delivery = held.into_iter().next().unwrap();
            drop(accepted);
            drop(issuer_drain);
            let issuer_config = issuer_effects.config().clone();
            let issuer_authority = issuer.authority_id();
            let shutdown = issuer
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await;
            assert!(
                shutdown.is_err(),
                "interrupted original VM retains its supervised cancellation cause"
            );
            drop(_original);
            drop(issuer_effects);
            drop(issuer);
            let profile =
                crate::runtime::builder::TestingOwnedProfileCapability::acquire(&issuer_config)
                    .expect("interrupted original issuer released its physical profile");
            let context = aura_core::context::EffectContext::new(
                issuer_authority,
                aura_core::ContextId::new_from_entropy([153; 32]),
                aura_core::effects::ExecutionMode::Testing,
            );
            let build = crate::runtime::EffectSystemBuilder::testing_with_owned_profile(profile)
                .with_authority(issuer_authority)
                .with_config(issuer_config)
                .with_physical_time_provider(clock.clone())
                .build(&context);
            let advance = async {
                let startup = clock
                    .observe_next_relative_sleep_of(1_000)
                    .await
                    .expect("observe selected physical startup replay");
                let target = startup.await.expect("selected startup replay admitted");
                clock.set_time(target.value());
            };
            let (reopened_runtime, ()) = tokio::join!(build, advance);
            let reopened = std::sync::Arc::new(crate::AuraAgent::new(
                reopened_runtime.expect("same original issuer profile reopened"),
                issuer_authority,
            ));
            let manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
                issuer_authority,
                crate::runtime::services::RendezvousManagerConfig::default()
                    .with_bootstrap_broker(broker_config),
                Arc::new(reopened.runtime().effects().time_effects().clone()),
            );
            reopened
                .runtime()
                .effects()
                .attach_rendezvous_manager(manager);
            assert_eq!(
                reopened.drain_issuer_return_vm_mailbox().await.unwrap(),
                0,
                "same-profile reopen verifies committed encrypted custody without replaying the VM frame"
            );
            assert!(
                broker
                    .take_issuer_return_vm_envelopes(
                        manifest.subject,
                        manifest.initiator_device,
                        admitted.manifest_digest(),
                        credential,
                    )
                    .await
                    .unwrap()
                    .is_empty(),
                "reopen ACK removes the broker copy only after encrypted runtime custody"
            );
            let exact =
                crate::runtime::services::bootstrap_broker::BootstrapIssuerReturnMailboxEnvelope {
                    manifest_digest: admitted.manifest_digest(),
                    setup_request: delivery.setup_request,
                    authorization: delivery.authorization,
                    envelope: delivery.envelope,
                };
            broker
                .queue_issuer_return_vm_envelope(exact.clone())
                .await
                .expect("exact acknowledged response replay is idempotent");
            assert!(
                broker
                    .take_issuer_return_vm_envelopes(
                        manifest.subject,
                        manifest.initiator_device,
                        admitted.manifest_digest(),
                        credential,
                    )
                    .await
                    .unwrap()
                    .is_empty(),
                "acknowledged message ID cannot reappear"
            );
            let mut wrong_direction = exact.clone();
            wrong_direction.envelope.source = manifest.subject;
            assert!(
                broker
                    .queue_issuer_return_vm_envelope(wrong_direction)
                    .await
                    .is_err(),
                "setup proof cannot authorize the opposite authority direction"
            );
            let mut foreign_device = exact.clone();
            foreign_device.envelope.metadata.insert(
                "aura-destination-device-id".into(),
                manifest.invitee_device.to_string(),
            );
            assert!(
                broker
                    .queue_issuer_return_vm_envelope(foreign_device)
                    .await
                    .is_err(),
                "setup proof cannot retarget a foreign device"
            );
            let mut foreign_setup = exact.clone();
            foreign_setup.setup_request.proof.signature[0] ^= 1;
            assert!(
                broker
                    .queue_issuer_return_vm_envelope(foreign_setup)
                    .await
                    .is_err(),
                "broker independently rejects altered setup possession"
            );
            clock.set_time(manifest.expires_at_ms);
            assert!(
                broker.queue_issuer_return_vm_envelope(exact).await.is_err(),
                "selected physical expiry ends this exact return route"
            );
            broker_tasks
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
                .expect("standalone broker owner releases after exact return proof");
            assert_eq!(admitted.manifest().invitation, invitation.invitation_id);
        });
    }
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
            let public_request = aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript::from_manifest(retained.manifest())
                .expect("derive narrowly scoped public request");
            assert_eq!(
                public_request.transcript_bytes().unwrap(),
                frame.transcript().transcript_bytes().unwrap(),
                "reviewed public request must match actual native control bytes exactly"
            );
            for (kind, reason) in [
                (aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind::Rejected, aura_app::runtime_bridge::CeremonyFailureReason::Rejected),
                (aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind::Cancelled, aura_app::runtime_bridge::CeremonyFailureReason::Cancelled),
                (aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind::TimedOut, aura_app::runtime_bridge::CeremonyFailureReason::TimedOut),
                (aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind::ChoreographyFailed, aura_app::runtime_bridge::CeremonyFailureReason::ChoreographyFailed),
                (aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind::RuntimeFailed, aura_app::runtime_bridge::CeremonyFailureReason::RuntimeFailed),
                (aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind::Superseded, aura_app::runtime_bridge::CeremonyFailureReason::Superseded),
            ] {
                let reviewed = aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalTranscript::from_manifest(retained.manifest(), kind)
                    .expect("derive reviewed conditional negative transcript");
                let native = EnrollmentControlTranscript {
                    version: 2,
                    manifest_digest: retained.digest(),
                    decision: EnrollmentControlDecision::Failed {
                        invitation: retained.manifest().invitation.clone(),
                        ceremony: retained.manifest().ceremony.clone(),
                        reason,
                    },
                    committed_ops: None,
                };
                assert_eq!(
                    reviewed.required_transcript_bytes().unwrap(),
                    native.required_transcript_bytes().unwrap(),
                    "reviewed negative {kind:?} must match actual native control bytes exactly"
                );
            }
            verify_initial_request(
                issuer_effects.as_ref(),
                &OriginalInitialRequestVerifier::Retained(&retained),
                frame.signature.clone(),
            )
            .await
            .expect("genuine native initial request signature");
            let mut substituted_manifest = retained.manifest().clone();
            substituted_manifest.invitee_device = issuer_effects.device_id();
            let substituted_request = aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript::from_manifest(&substituted_manifest)
                .expect("actual substituted physical request");
            assert!(!aura_core::effects::CryptoExtendedEffects::frost_verify(
                issuer_effects.as_ref(),
                &substituted_request
                    .required_transcript_bytes()
                    .expect("encode actual substituted request"),
                &frame.signature,
                retained.expected_request_verifier(),
            )
            .await
            .expect("native verification of substituted physical target"));
            let mut substituted_signature = frame.signature.clone();
            substituted_signature[0] ^= 1;
            verify_initial_request(
                issuer_effects.as_ref(),
                &OriginalInitialRequestVerifier::Retained(&retained),
                substituted_signature,
            )
            .await
            .err()
            .expect("corrupt real native signature rejected");
            let request = frame
                .verify_request(invitee_effects.as_ref(), &admitted)
                .await
                .expect("admitted actual request");
            let acceptance =
                sign_acceptance_for_request(invitee_effects.as_ref(), &admitted, &request)
                    .await
                    .expect("actual retained setup signer");
            assert_eq!(acceptance.manifest_digest, admitted.manifest_digest());
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
    fn original_signing_receive_waits_for_its_session_and_original_expiry() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
            let transport = crate::SharedTransport::new();
            let (_issuer, invitee, invitation, _, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_clock_and_transport(
                    "original-signing-receive-readiness", Arc::new(clock.clone()), transport.clone(),
                ),
            ).await;
            let effects = invitee.runtime().effects();
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .unwrap();
            let session = [41; 32];
            let other_session = [42; 32];
            let candidate = |session: [u8; 32], device: aura_core::DeviceId| {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert(
                    "content-type".into(),
                    "application/aura-approved-enrollment-round-v1".into(),
                );
                metadata.insert("aura-enrollment-round-session".into(), hex::encode(session));
                metadata.insert("aura-destination-device-id".into(), device.to_string());
                aura_core::effects::TransportEnvelope {
                    source: invitation.sender_id,
                    destination: invitee.authority_id(),
                    context: invitation.context_id,
                    payload: vec![0xff],
                    metadata,
                    receipt: None,
                }
            };
            let wrong_device = aura_core::DeviceId::new_from_entropy([43; 32]);
            transport.route_envelope(candidate(other_session, effects.device_id()));
            transport.route_envelope(candidate(session, wrong_device));
            let receive = effects.receive_owned_enrollment_round(root.child(), session);
            futures::pin_mut!(receive);
            assert!(
                futures::poll!(receive.as_mut()).is_pending(),
                "foreign session and physical device cannot satisfy the original waiter"
            );
            assert_eq!(clock.now_ms(), 5_000);
            transport.route_envelope(candidate(session, effects.device_id()));
            let error = receive.await.expect_err(
                "an arrival is still independently decoded, never authenticated by readiness",
            );
            let source = std::error::Error::source(&error).expect("native sealed receive source");
            assert!(
                matches!(
                    source.downcast_ref::<aura_core::TimeoutRunError<aura_core::AuraError>>(),
                    Some(aura_core::TimeoutRunError::Operation(aura_core::AuraError::Serialization { source: Some(native), .. }))
                        if native.is::<aura_core::util::serialization::SerializationError>()
                ),
                "matching invalid bytes reach the actual decoder rather than a timeout"
            );
            assert_eq!(
                clock.now_ms(),
                5_000,
                "actual matching arrival wakes without physical time advancement"
            );
            let mut cancelled =
                Box::pin(effects.receive_owned_enrollment_round(root.child(), session));
            assert!(futures::poll!(cancelled.as_mut()).is_pending());
            drop(cancelled);
            let mut replacement =
                Box::pin(effects.receive_owned_enrollment_round(root.child(), session));
            assert!(futures::poll!(replacement.as_mut()).is_pending());
            transport.route_envelope(candidate(session, effects.device_id()));
            let error = replacement
                .await
                .expect_err("same original child can receive after losing waiter disposal");
            let source =
                std::error::Error::source(&error).expect("actual replacement receive source");
            assert!(
                matches!(source.downcast_ref::<aura_core::TimeoutRunError<aura_core::AuraError>>(), Some(aura_core::TimeoutRunError::Operation(aura_core::AuraError::Serialization { source: Some(native), .. })) if native.is::<aura_core::util::serialization::SerializationError>())
            );
            let receive = effects.receive_owned_enrollment_round(root.child(), session);
            futures::pin_mut!(receive);
            assert!(futures::poll!(receive.as_mut()).is_pending());
            clock.set_time(root.origin().manifest().expires_at_ms);
            let error = receive
                .await
                .expect_err("the original endpoint still bounds an empty event wait");
            let source = std::error::Error::source(&error).expect("native original timeout source");
            assert!(matches!(
                source.downcast_ref::<aura_core::TimeoutRunError<aura_core::AuraError>>(),
                Some(aura_core::TimeoutRunError::Timeout(
                    aura_core::TimeoutBudgetError::DeadlineExceeded { .. }
                ))
            ));
            let preserved = transport.inbox_for(invitee.authority_id());
            let preserved = preserved.read();
            let original_frames: Vec<_> = preserved
                .iter()
                .filter_map(|envelope| {
                    let session = envelope.metadata.get("aura-enrollment-round-session")?;
                    let device = envelope.metadata.get("aura-destination-device-id")?;
                    Some((session.clone(), device.clone()))
                })
                .collect();
            assert_eq!(original_frames, vec![
                (hex::encode(other_session), effects.device_id().to_string()),
                (hex::encode(session), wrong_device.to_string()),
            ], "drop and original expiry preserve foreign session and wrong-device frames in their original order");
        });
    }
    #[test]
    fn real_committed_confirmation_is_durable_and_reverified_before_activation_capability() {
        actual_confirmed_parent_scenario(false, false, None, false);
    }

    #[test]
    fn confirmed_profile_negative_recovery_refuses_foreign_adopted_subject() {
        actual_confirmed_parent_scenario_with_foreign_subject(
            false, false, None, false, false, false, None, true,
        );
    }

    #[test]
    fn original_native_acquisition_failure_survives_disposal_and_negative_transition() {
        actual_confirmed_parent_scenario(true, false, None, false);
    }

    #[test]
    fn original_registration_commit_drop_retains_actual_cleanup_owner() {
        actual_confirmed_parent_scenario(false, true, None, false);
    }

    #[test]
    fn original_allocation_commit_before_ack_drop_retains_actual_cleanup_owner() {
        actual_confirmed_parent_scenario(
            false,
            false,
            Some(AllocationDropBoundary::ImmutableCommit),
            false,
        );
    }

    #[test]
    fn original_allocation_before_write_drop_retains_unwritten_disposition() {
        actual_confirmed_parent_scenario(
            false,
            false,
            Some(AllocationDropBoundary::BeforeWrite),
            false,
        );
    }

    #[derive(Clone, Copy)]
    enum AllocationDropBoundary {
        BeforeWrite,
        ImmutableCommit,
        WrapBirth,
    }

    #[test]
    fn original_allocation_wrap_birth_drop_retires_actual_original_lifetime() {
        actual_confirmed_parent_scenario(
            false,
            false,
            Some(AllocationDropBoundary::WrapBirth),
            false,
        );
    }

    #[test]
    fn real_single_signer_committed_cancellation_closes_original_resources_without_confirmation() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
            let ((issuer, invitee, invitation, _start, _, _), original) = crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
            "single-signer-committed-cancellation", Some(Arc::new(clock.clone())), None,
        ).await;
            let invitee_service = invitee.invitations().unwrap();
            let actual_root = invitee_service
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .unwrap();
            let actual_scope = actual_root.origin_observation();
            let original_subject = actual_scope.manifest().subject;
            let original_epoch = actual_scope.manifest().final_epoch;
            invitee_service
                .return_original_enrollment_execution_for_test(actual_root)
                .await
                .unwrap();
            let key_location = aura_core::effects::SecureStorageLocation::with_sub_key(
                "signing_keys",
                format!("{original_subject}:{original_epoch}"),
                "1",
            );
            let original_key = zeroize::Zeroizing::new(
                issuer
                    .runtime()
                    .effects()
                    .secure_retrieve(
                        &key_location,
                        &[aura_core::effects::secure::SecureStorageCapability::Read],
                    )
                    .await
                    .expect("actual original account signing key"),
            );
            let original_key_digest = aura_core::hash::hash(&original_key);
            drop(original_key);
            let actual_disposal = original
                .original_disposal_for_test()
                .expect("same originally issued task observer");
            let (committed, release) = original
                .install_committed_confirmation_gate_for_test()
                .unwrap();
            let invitations = invitee.invitations().unwrap();
            let mut acceptance = Box::pin(invitations.accept(&invitation.invitation_id));
            tokio::select! {
                actual = committed => actual.expect("actual protected Committed receipt retained before visible notification"),
                result = &mut acceptance => panic!("actual acceptance ended before committed confirmation gate: {result:?}"),
            }
            assert!(original.has_original_positive_receipt_for_test());

            assert!(!original
                .observes_retained_retirement_acknowledgment()
                .unwrap());
            original
                .request_original_issuer_cancel_for_test()
                .expect("cancel SAME actual SingleSigner issuer task");
            let disposed = actual_disposal.await_disposal().await;
            original
                .require_original_issuer_disposal_for_test(&disposed)
                .unwrap();
            assert!(
                matches!(
                    disposed.outcome(),
                    aura_core::effects::task::RequiredTaskExit::Failed(_)
                ),
                "actual cancelled issuer source retained: {:?}",
                disposed.outcome()
            );
            assert!(
                release.send(()).is_err(),
                "actual original finalizer was disposed while gated before confirmation"
            );
            drop(acceptance);
            assert!(!original
                .observes_retained_retirement_acknowledgment()
                .unwrap());
            assert!(original.has_original_positive_receipt_for_test());

            assert_eq!(clock.now_ms(), 5_000);
            let issuer_shutdown = issuer
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
                .expect_err("cancelled SingleSigner issuer retains actual domain failure");
            crate::handlers::invitation_service::assert_original_single_signer_cancelled_shutdown_source(&disposed.outcome(), &issuer_shutdown);
            let preserved_key = zeroize::Zeroizing::new(
                issuer
                    .runtime()
                    .effects()
                    .secure_retrieve(
                        &key_location,
                        &[aura_core::effects::secure::SecureStorageCapability::Read],
                    )
                    .await
                    .expect("original account key survives resource cleanup"),
            );
            assert_eq!(
                aura_core::hash::hash(&preserved_key),
                original_key_digest,
                "resource cleanup preserves actual original account signing key bytes"
            );
            drop(preserved_key);
            match invitee
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
            {
                Ok(()) => {}
                Err(crate::task_registry::TaskSupervisionError::TaskFailed { source, .. }) => {
                    eprintln!("actual invitee SingleSigner cancellation domain source: {source:?}");
                }
                Err(source) => {
                    panic!("SingleSigner original invitee resource cleanup failed: {source:?}")
                }
            }
        });
    }
    #[test]
    fn real_quorum_positive_retirement_waits_for_canonical_existing_device_response() {
        actual_confirmed_parent_scenario(false, false, None, true);
    }

    #[test]
    fn real_quorum_peer_commit_postread_drop_reenters_same_admission_without_early_ack() {
        actual_confirmed_parent_scenario_with_postread_drop(false, false, None, true, true, false);
    }

    #[test]
    fn real_quorum_positive_material_cleanup_preserves_refusal_without_positive_completion() {
        actual_confirmed_parent_scenario_with_postread_drop(false, false, None, true, true, true);
    }

    #[test]
    fn real_quorum_positive_actor_consumed_nonce_drop_retains_original_interruption() {
        actual_confirmed_parent_scenario_with_actor_fault(false, false, None, true, false, true,
        Some(crate::runtime::services::threshold_signing::OriginalPositiveActorTestMode::ConsumedNonce));
    }
    #[test]
    fn real_quorum_positive_actor_native_failure_survives_postread_drop() {
        actual_confirmed_parent_scenario_with_actor_fault(false, false, None, true, false, true,
        Some(crate::runtime::services::threshold_signing::OriginalPositiveActorTestMode::NativeFailure));
    }
    fn actual_confirmed_parent_scenario(
        native_failure: bool,
        registration_drop: bool,
        allocation_drop: Option<AllocationDropBoundary>,
        second_positive: bool,
    ) {
        actual_confirmed_parent_scenario_with_postread_drop(
            native_failure,
            registration_drop,
            allocation_drop,
            second_positive,
            false,
            false,
        );
    }

    fn actual_confirmed_parent_scenario_with_postread_drop(
        native_failure: bool,
        registration_drop: bool,
        allocation_drop: Option<AllocationDropBoundary>,
        second_positive: bool,
        postread_drop: bool,
        cleanup_positive: bool,
    ) {
        actual_confirmed_parent_scenario_with_actor_fault(
            native_failure,
            registration_drop,
            allocation_drop,
            second_positive,
            postread_drop,
            cleanup_positive,
            None,
        );
    }
    fn actual_confirmed_parent_scenario_with_actor_fault(
        native_failure: bool,
        registration_drop: bool,
        allocation_drop: Option<AllocationDropBoundary>,
        second_positive: bool,
        postread_drop: bool,
        cleanup_positive: bool,
        actor_fault: Option<
            crate::runtime::services::threshold_signing::OriginalPositiveActorTestMode,
        >,
    ) {
        actual_confirmed_parent_scenario_with_foreign_subject(
            native_failure,
            registration_drop,
            allocation_drop,
            second_positive,
            postread_drop,
            cleanup_positive,
            actor_fault,
            false,
        );
    }
    fn actual_confirmed_parent_scenario_with_foreign_subject(
        native_failure: bool,
        registration_drop: bool,
        allocation_drop: Option<AllocationDropBoundary>,
        second_positive: bool,
        postread_drop: bool,
        cleanup_positive: bool,
        actor_fault: Option<
            crate::runtime::services::threshold_signing::OriginalPositiveActorTestMode,
        >,
        foreign_subject: bool,
    ) {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async move {
            let original_transport = crate::SharedTransport::new();
            let original_clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
            let postread_test = Arc::new(
                crate::runtime::services::threshold_signing::OriginalPeerCommitPostreadTest::new(
                    original_clock.clone(),
                    cleanup_positive,
                ),
            );
            let actor_test = actor_fault.map(|mode| {
                Arc::new(
                    crate::runtime::services::threshold_signing::OriginalPositiveActorTest::new(
                        original_clock.clone(),
                        mode,
                    ),
                )
            });
            let selected_clock: Arc<dyn aura_core::effects::PhysicalTimeEffects> =
                if let Some(test) = &actor_test {
                    test.clone()
                } else if postread_drop {
                    postread_test.clone()
                } else {
                    Arc::new(original_clock.clone())
                };

            let ((issuer, invitee, invitation, _start, _accept, _verified), original) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                    "confirmed-import-receipt",
                    Some(selected_clock.clone()),
                    Some(original_transport.clone()),
                ),
            )
            .await;
            let issuer_effects = issuer.runtime().effects();
            let invitee_effects = invitee.runtime().effects();
            let original_invitee_config = invitee_effects.config().clone();
            let original_root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .unwrap();
            let manager = crate::runtime::services::InvitationManager::with_capacity(1);
            manager
                .cache_admitted_enrollment(original_root)
                .await
                .unwrap();
            manager
                .update_invitation(&invitation.invitation_id, |observed| {
                    observed.status = super::super::InvitationStatus::Accepted;
                    observed.receiver_nickname = Some("local confirmed device".into());
                })
                .await
                .unwrap();
            let original_root = manager
                .take_admitted_enrollment(&invitation.invitation_id)
                .await
                .unwrap();
            let admitted = original_root.origin_observation();
            assert_eq!(admitted.canonical_invitation().status, invitation.status);
            assert_eq!(
                admitted.canonical_invitation().receiver_nickname,
                invitation.receiver_nickname,
            );
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
            let captured = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let observed_capture = captured.clone();
            let selected_effects = issuer_effects.clone();
            original.install_original_receipt_test_hook(Box::new(move |receipt| {
                let original_bytes = aura_core::util::serialization::to_vec(receipt.frame()).unwrap();
                let receipt = crate::handlers::invitation_service::assert_original_terminal_capture_after_primary_failure_for_test(
                    receipt, selected_effects.as_ref(),
                );
                assert_eq!(aura_core::util::serialization::to_vec(receipt.frame()).unwrap(), original_bytes);
                observed_capture.store(true, std::sync::atomic::Ordering::Release);
                receipt
            })).unwrap();
            invitee
                .invitations()
                .unwrap()
                .return_original_enrollment_execution_for_test(original_root)
                .await
                .unwrap();
            invitee
                .invitations()
                .unwrap()
                .accept(&invitation.invitation_id)
                .await
                .expect("same original invitee executes canonical acceptance and VM exchange");
            let delivery = original.original_disposal_for_test().unwrap();
            let disposed = delivery.await_disposal().await;
            delivery.require_positive(&disposed).unwrap();
            assert!(
                captured.load(std::sync::atomic::Ordering::Acquire),
                "actual canonical issuer receipt passed the custody regression"
            );
            original
                .observe_original_finalizer()
                .await
                .expect("actual original issuer finalization after canonical acceptance");
            let durable = super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                invitee_effects.as_ref(),
                invitation.receiver_id,
                &invitation.invitation_id,
            )
            .await
            .expect("canonical durable confirmation from actual VM exchange");
            let verified = durable.confirmation();
            let frame = verified.frame().clone();
            let committed_ops = frame
                .committed_ops
                .as_ref()
                .expect("actual committed suffix");
            let public_positive = aura_invitation::enrollment_signing_intent::EnrollmentPositiveTerminalTranscript::from_manifest(
                admitted.manifest(), committed_ops,
            ).expect("exact public positive control from actual signed transition");
            assert_eq!(
                public_positive.required_transcript_bytes().unwrap(),
                frame.transcript().required_transcript_bytes().unwrap(),
                "finite positive quorum transcript must equal actual native control bytes"
            );
            assert!(aura_invitation::enrollment_signing_intent::EnrollmentPositiveTerminalTranscript::from_manifest(
                admitted.manifest(), &committed_ops[..1],
            ).is_err(), "a leaf without the epoch fence cannot identify the positive control");
            let mut reversed = committed_ops.clone();
            reversed.reverse();
            assert!(aura_invitation::enrollment_signing_intent::EnrollmentPositiveTerminalTranscript::from_manifest(
                admitted.manifest(), &reversed,
            ).is_err(), "the positive control requires the leaf then its fence");
            let request = expected_request(&admitted);
            // The genuine original signer roster stays one while AddLeaf creates
            // two authenticated root children before the original-key epoch fence.
            let history = verified.committed_transition().ops();
            let fence = history.last().expect("real signed epoch fence").clone();
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
            let captured = check_admitted_node_operation(
                admitted.manifest(),
                admitted.canonical_invitation(),
                &before_fence,
                &fence,
                aura_core::tree::NodeIndex(0),
            )
            .unwrap();
            assert_eq!(captured.epoch, before_fence.epoch.value());
            assert_eq!(captured.commitment, fence.op.parent_commitment);
            assert_eq!(captured.public_key_package, parent.public_key_package);
            assert_eq!(captured.participants, parent.participants);
            assert!(verified
                .committed_transition()
                .parent_inventory()
                .iter()
                .any(|tuple| tuple.epoch == fence.op.parent_epoch.value()
                    && tuple.commitment == fence.op.parent_commitment
                    && tuple.signing_node == aura_core::tree::NodeIndex(0)));
            assert!(admitted
                .manifest()
                .parents
                .iter()
                .chain(admitted.manifest().final_inventory().unwrap().iter())
                .all(|tuple| tuple.epoch != captured.epoch
                    || tuple.commitment != captured.commitment));
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
            // Reload the public archive through the original actual confirmed
            // receipt; cached tuples and a foreign physical runtime are insufficient.
            // Real pre-change archive encoding from this actual independently
            // pinned/confirmed fixture. Legacy bytes are audit evidence, never
            // automatically promoted into the new pending inventory source.
            #[derive(serde::Serialize)]
            struct HistoricalArchiveV1 {
                version: u16,
                physical_device: aura_core::DeviceId,
                provisional: aura_core::AuthorityId,
                subject: aura_core::AuthorityId,
                invitation: aura_core::InvitationId,
                manifest_digest: [u8; 32],
                confirmed_history_digest: [u8; 32],
                tuples: Vec<u8>,
            }
            let mut old_tuples = admitted.manifest().parents.clone();
            old_tuples.extend_from_slice(admitted.manifest().final_inventory().unwrap());
            let historical_bytes = aura_core::util::serialization::to_vec(&HistoricalArchiveV1 {
                version: 1,
                physical_device: invitee_effects.device_id(),
                provisional: invitation.receiver_id,
                subject: admitted.manifest().subject,
                invitation: invitation.invitation_id.clone(),
                manifest_digest: durable.confirmation().manifest_digest(),
                confirmed_history_digest: aura_core::hash::hash(
                    &aura_core::util::serialization::to_vec(
                        &durable.confirmation().committed_transition().ops().to_vec(),
                    )
                    .unwrap(),
                ),
                tuples: aura_core::util::serialization::to_vec(&old_tuples).unwrap(),
            })
            .unwrap();
            let historical_key = SecureStorageLocation::with_sub_key(
                "confirmed_enrollment_parent_inventory_v1",
                invitee_effects.device_id().to_string(),
                format!(
                    "{}:{}",
                    admitted.manifest().subject,
                    invitation.invitation_id
                ),
            );
            invitee_effects
                .secure_store_immutable(
                    &historical_key,
                    &historical_bytes,
                    &[SecureStorageCapability::Write],
                )
                .await
                .unwrap();
            assert!(
                super::super::enrollment_parent_archive::load_confirmed_parent_archive(
                    invitee_effects.as_ref(),
                    invitation.receiver_id,
                    &invitation.invitation_id
                )
                .await
                .is_err()
            );
            super::super::enrollment_parent_archive::retain_confirmed_parent_archive(
                invitee_effects.as_ref(),
                &durable,
            )
            .await
            .unwrap();
            let archive = super::super::enrollment_parent_archive::load_confirmed_parent_archive(
                invitee_effects.as_ref(),
                invitation.receiver_id,
                &invitation.invitation_id,
            )
            .await
            .unwrap();
            assert_eq!(
                invitee_effects
                    .secure_retrieve(&historical_key, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                historical_bytes
            );
            let history = durable.confirmation().committed_transition().ops();
            let inventory = invitee_effects
                .collect_imported_enrollment_parent_inventory(&archive, history)
                .await
                .unwrap();
            assert!(!inventory.is_empty());
            let mut replayed_extension = history.to_vec();
            replayed_extension.push(fence.clone());
            assert!(
                invitee_effects
                    .collect_imported_enrollment_parent_inventory(&archive, &replayed_extension)
                    .await
                    .is_err(),
                "original-epoch fence cannot authorize a pending-epoch successor"
            );

            assert!(archive
                .verify_imported_history(
                    issuer_effects.as_ref(),
                    admitted.manifest().subject,
                    history
                )
                .is_err());
            let mut substituted_history = history.to_vec();
            substituted_history.remove(0);
            assert!(invitee_effects
                .collect_imported_enrollment_parent_inventory(&archive, &substituted_history)
                .await
                .is_err());
            let archive_reloaded =
                super::super::enrollment_parent_archive::load_confirmed_parent_archive(
                    invitee_effects.as_ref(),
                    invitation.receiver_id,
                    &invitation.invitation_id,
                )
                .await
                .unwrap();
            assert_eq!(
                aura_core::util::serialization::to_vec(&archive.inventory().to_vec()).unwrap(),
                aura_core::util::serialization::to_vec(&archive_reloaded.inventory().to_vec())
                    .unwrap()
            );
            #[cfg(unix)]
            {
                let archive_location = SecureStorageLocation::with_sub_key(
                    "confirmed_enrollment_parent_inventory_v2",
                    invitee_effects.device_id().to_string(),
                    format!(
                        "{}:{}",
                        admitted.manifest().subject,
                        invitation.invitation_id
                    ),
                );
                assert!(invitee_effects
                    .fault_remove_secure_record_for_test(&archive_location)
                    .await
                    .unwrap());
                assert!(
                    super::super::enrollment_parent_archive::load_confirmed_parent_archive(
                        invitee_effects.as_ref(),
                        invitation.receiver_id,
                        &invitation.invitation_id,
                    )
                    .await
                    .is_err()
                );
                // Explicit original proof publication can recover only the exact
                // same public archive; a loader never repairs missing evidence.
                super::super::enrollment_parent_archive::retain_confirmed_parent_archive(
                    invitee_effects.as_ref(),
                    &durable,
                )
                .await
                .unwrap();
            }
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
            let immutable_denial = invitee_effects
                .secure_store(
                    &package_location,
                    &substituted,
                    &[SecureStorageCapability::Write],
                )
                .await
                .expect_err("ordinary storage cannot substitute an immutable public package");
            assert!(matches!(
                immutable_denial,
                aura_core::AuraError::PermissionDenied { .. }
            ));
            let mut cause = std::error::Error::source(&immutable_denial);
            let mut found_mutation = false;
            while let Some(error) = cause {
                if let Some(mutation) = error
                    .downcast_ref::<aura_core::effects::secure::ImmutableSecureRecordMutation>(
                ) {
                    assert_eq!(mutation.operation, "secure_store");
                    found_mutation = true;
                    break;
                }
                cause = error.source();
            }
            assert!(
                found_mutation,
                "denial retains the actual provider lifetime cause"
            );
            assert_eq!(
                invitee_effects
                    .secure_retrieve(&package_location, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                original_package
            );
            #[cfg(unix)]
            {
                // Corruption uses selected-provider backing loss, never a generic
                // overwrite authority that production deliberately forbids.
                assert!(invitee_effects
                    .fault_remove_secure_record_for_test(&package_location)
                    .await
                    .unwrap());
                invitee_effects
                    .secure_store_immutable(
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
                assert!(invitee_effects
                    .fault_remove_secure_record_for_test(&package_location)
                    .await
                    .unwrap());
                invitee_effects
                    .secure_store_immutable(
                        &package_location,
                        &original_package,
                        &[SecureStorageCapability::Write],
                    )
                    .await
                    .unwrap();
            }
            #[cfg(not(unix))]
            drop(recovered);
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
            let original_import_config_location = SecureStorageLocation::with_sub_key(
                "threshold_config",
                admitted.manifest().subject.to_string(),
                admitted.manifest().pending_epoch.to_string(),
            );
            let original_import_config = invitee_effects
                .secure_retrieve(
                    &original_import_config_location,
                    &[SecureStorageCapability::Read],
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
            assert_eq!(
                invitee_effects
                    .secure_retrieve(&package_location, &[SecureStorageCapability::Read],)
                    .await
                    .unwrap(),
                original_package,
                "activation preserves original signed immutable raw share"
            );
            assert_eq!(
                invitee_effects
                    .secure_retrieve(
                        &original_import_config_location,
                        &[SecureStorageCapability::Read],
                    )
                    .await
                    .unwrap(),
                original_import_config,
                "activation preserves original signed immutable config"
            );
            #[cfg(unix)]
            let original_activation = {
                let proof = super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                    invitee_effects.as_ref(),
                    invitation.receiver_id,
                    &invitation.invitation_id,
                )
                .await
                .unwrap();
                let owner = invitee_effects
                    .load_confirmed_activation_envelope(&proof)
                    .await
                    .unwrap();
                invitee_effects
                    .assert_confirmed_managed_allocation_boundaries_for_test(&owner)
                    .await
                    .expect("genuine confirmed owner enforces native allocation scope, reference and one-time birth");
                let target = invitee_effects.confirmed_activation_record_location_for_test(&owner);
                let bytes = invitee_effects
                    .secure_retrieve(&target, &[SecureStorageCapability::Read])
                    .await
                    .unwrap();
                (target, bytes)
            };
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

            #[cfg(unix)]
            {
                let (target, original) = &original_activation;
                assert_eq!(
                    invitee_effects
                        .secure_retrieve(target, &[SecureStorageCapability::Read])
                        .await
                        .unwrap(),
                    *original,
                    "repeated actual handoff acknowledges original envelope without reencryption"
                );
                assert!(invitee_effects
                    .fault_remove_secure_record_for_test(target)
                    .await
                    .unwrap());
                let proof = super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                    invitee_effects.as_ref(),
                    invitation.receiver_id,
                    &invitation.invitation_id,
                )
                .await
                .unwrap();
                assert!(
                    crate::runtime::services::enrollment_profile::complete_confirmed_handoff(
                        invitee_effects.as_ref(),
                        &invitee.runtime().threshold_signing(),
                        proof,
                    )
                    .await
                    .is_err(),
                    "missing original activation envelope cannot renew its birth"
                );
                assert!(
                    !invitee_effects.secure_exists(target).await.unwrap(),
                    "failed loader must not repair missing envelope"
                );
                invitee_effects
                    .secure_store_immutable(target, &[0xff], &[SecureStorageCapability::Write])
                    .await
                    .unwrap();
                let proof = super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                    invitee_effects.as_ref(),
                    invitation.receiver_id,
                    &invitation.invitation_id,
                )
                .await
                .unwrap();
                let failure =
                    crate::runtime::services::enrollment_profile::complete_confirmed_handoff(
                        invitee_effects.as_ref(),
                        &invitee.runtime().threshold_signing(),
                        proof,
                    )
                    .await
                    .expect_err("corrupt original activation envelope fails closed");
                let mut cause: &(dyn std::error::Error + 'static) = &failure;
                let mut native_codec = false;
                loop {
                    native_codec |= cause.is::<serde_json::Error>();
                    match cause.source() {
                        Some(source) => cause = source,
                        None => break,
                    }
                }
                assert!(
                    native_codec,
                    "original provider codec failure reaches handoff owner"
                );
                assert_eq!(
                    invitee_effects
                        .secure_retrieve(target, &[SecureStorageCapability::Read])
                        .await
                        .unwrap(),
                    [0xff]
                );
                assert!(invitee_effects
                    .fault_remove_secure_record_for_test(target)
                    .await
                    .unwrap());
                invitee_effects
                    .secure_store_immutable(target, original, &[SecureStorageCapability::Write])
                    .await
                    .unwrap();
            }

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
            let mutation = invitee_effects
                .secure_store(&key, b"corrupt", &[SecureStorageCapability::Write])
                .await
                .expect_err("production write cannot replace immutable original confirmation");
            let deletion = invitee_effects
                .secure_delete(&key, &[SecureStorageCapability::Delete])
                .await
                .expect_err("production delete cannot retire immutable original confirmation");
            for failure in [&mutation, &deletion] {
                let mut cause: &(dyn std::error::Error + 'static) = failure;
                let mut native_immutable = false;
                loop {
                    native_immutable |=
                        cause.is::<aura_core::effects::secure::ImmutableSecureRecordMutation>();
                    match cause.source() {
                        Some(source) => cause = source,
                        None => break,
                    }
                }
                assert!(
                    native_immutable,
                    "production immutable denial retains native cause: {failure:?}"
                );
            }
            assert_eq!(
                invitee_effects
                    .secure_retrieve(&key, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                bytes
            );
            #[cfg(unix)]
            {
                assert!(invitee_effects
                    .fault_remove_secure_record_for_test(&key)
                    .await
                    .unwrap());
                invitee_effects
                    .secure_store_immutable(&key, b"corrupt", &[SecureStorageCapability::Write])
                    .await
                    .unwrap();
                assert!(
                    super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                        invitee_effects.as_ref(),
                        invitation.receiver_id,
                        &invitation.invitation_id,
                    )
                    .await
                    .is_err(),
                    "actual selected-provider confirmation corruption is refused"
                );
                assert!(invitee_effects
                    .fault_remove_secure_record_for_test(&key)
                    .await
                    .unwrap());
                invitee_effects
                    .secure_store_immutable(&key, &bytes, &[SecureStorageCapability::Write])
                    .await
                    .unwrap();
                assert!(invitee_effects
                    .fault_remove_secure_record_for_test(&key)
                    .await
                    .unwrap());
                assert!(
                    super::super::enrollment_manifest_admission::load_confirmed_enrollment(
                        invitee_effects.as_ref(),
                        invitation.receiver_id,
                        &invitation.invitation_id,
                    )
                    .await
                    .is_err(),
                    "actual selected-provider confirmation loss is refused"
                );
                // Restore only the captured original after observing refusal.
                invitee_effects
                    .secure_store_immutable(&key, &bytes, &[SecureStorageCapability::Write])
                    .await
                    .expect("restore exact original receipt after intentional deletion fault");
            }
            let first_manifest = admitted.manifest().clone();
            let original_memo_location = initial_request_location(&first_manifest).unwrap();
            assert_eq!(original_memo_location.sub_key.as_deref().unwrap().len(), 64);
            let mut long_bound_manifest = first_manifest.clone();
            long_bound_manifest.ceremony = aura_core::CeremonyId::new("c".repeat(128));
            long_bound_manifest.invitation = aura_core::InvitationId::new("i".repeat(128));
            let long_bound_location = initial_request_location(&long_bound_manifest).unwrap();
            assert_ne!(
                original_memo_location, long_bound_location,
                "compact addressing retains complete ceremony/invitation transcript binding"
            );
            for component in [
                long_bound_location.namespace.as_str(),
                long_bound_location.key.as_str(),
                long_bound_location.sub_key.as_deref().unwrap(),
            ] {
                assert!(
                    component
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric()
                            || matches!(byte, b'-' | b'_' | b'.')),
                    "native component encoding must not expand compact addresses"
                );
                assert!(
                    component.len() + 32 < 255,
                    "compact memo component leaves room for native immutable publication suffix"
                );
            }

            let adopted_context: aura_core::ContextId =
                serde_json::from_value(projected["context_id"].clone())
                    .expect("actual committed profile context projection");

            // Required task completion and every actual old provider reference
            // precede reopening the same native physical profile. Success of
            // this exclusive acquisition proves no old profile lease survived.
            invitee
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
                .expect("actual original invitee task completion before same-profile reopen");
            drop(archive_reloaded);
            drop(archive);
            drop(durable);
            drop(admitted);
            drop(invitee_effects);
            drop(invitee);
            if foreign_subject {
                let foreign = aura_core::AuthorityId::new_from_entropy([219; 32]);
                assert_ne!(foreign, first_manifest.subject);
                assert_ne!(foreign, invitation.receiver_id);
                let profile = crate::runtime::builder::TestingOwnedProfileCapability::acquire(
                    &original_invitee_config,
                )
                .expect("same exclusive confirmed profile for foreign runtime refusal");
                let context = aura_core::context::EffectContext::new(
                    foreign,
                    adopted_context,
                    aura_core::effects::ExecutionMode::Testing,
                );
                let startup_timer_admitted = original_clock
                    .observe_next_relative_sleep_of(1_000)
                    .await
                    .expect("observe actual foreign-profile startup timer admission");
                let build =
                    crate::runtime::EffectSystemBuilder::testing_with_owned_profile(profile)
                        .with_authority(foreign)
                        .with_config(original_invitee_config.clone())
                        .with_shared_transport(original_transport.clone())
                        .with_physical_time_provider(selected_clock.clone())
                        .build(&context);
                let drive_startup_timer = async {
                    let target = startup_timer_admitted
                        .await
                        .expect("actual foreign startup timer admitted");
                    assert_eq!(target.value(), original_clock.now_ms() + 1_000);
                    original_clock.set_time(target.value());
                };
                let (runtime, ()) = tokio::join!(build, drive_startup_timer);
                let runtime =
                    runtime.expect("actual foreign runtime on same confirmed physical profile");
                let effects = runtime.effects();
                let admission_key = SecureStorageLocation::new(
                    "enrollment_manifest_admission_v2",
                    format!("{}:{}", invitation.receiver_id, invitation.invitation_id),
                );
                let admission_bytes = effects
                    .secure_retrieve(&admission_key, &[SecureStorageCapability::Read])
                    .await
                    .expect("actual original protected admission");
                let confirmed_bytes = effects
                    .secure_retrieve(&key, &[SecureStorageCapability::Read])
                    .await
                    .expect("actual original protected confirmation");
                let notice_tasks = || {
                    runtime
                        .tasks()
                        .active_tasks()
                        .into_iter()
                        .filter(|name| {
                            name.contains("recovered-negative-notice")
                                || name.contains("recovered_negative_notice")
                        })
                        .collect::<Vec<_>>()
                };
                let tasks_before = notice_tasks();
                let refused = super::super::enrollment_manifest_admission::load_recoverable_negative_admissions(effects.as_ref()).await;
                assert!(matches!(
                    refused,
                    Err(aura_invitation::enrollment_manifest::EnrollmentManifestError::Pin)
                ));
                assert_eq!(
                    effects
                        .secure_retrieve(&admission_key, &[SecureStorageCapability::Read])
                        .await
                        .expect("unchanged original admission"),
                    admission_bytes
                );
                assert_eq!(
                    effects
                        .secure_retrieve(&key, &[SecureStorageCapability::Read])
                        .await
                        .expect("unchanged original confirmation"),
                    confirmed_bytes
                );
                assert_eq!(notice_tasks(), tasks_before);
                runtime
                    .tasks()
                    .shutdown_gracefully(std::time::Duration::from_secs(5))
                    .await
                    .expect("foreign observation runtime strict cleanup");
                drop(effects);
                drop(runtime);
            }
            let selected_profile = crate::runtime::builder::TestingOwnedProfileCapability::acquire(
                &original_invitee_config,
            )
            .expect("exclusive original physical profile after acknowledged old runtime teardown");
            let adopted_context = aura_core::context::EffectContext::new(
                first_manifest.subject,
                adopted_context,
                aura_core::effects::ExecutionMode::Testing,
            );
            let startup_timer_admitted = original_clock
                .observe_next_relative_sleep_of(1_000)
                .await
                .expect("observe actual confirmed-profile startup timer admission");
            let build =
                crate::runtime::EffectSystemBuilder::testing_with_owned_profile(selected_profile)
                    .with_authority(first_manifest.subject)
                    .with_config(original_invitee_config)
                    .with_shared_transport(original_transport.clone())
                    .with_physical_time_provider(selected_clock.clone())
                    .build(&adopted_context);
            let drive_startup_timer = async {
                let target = startup_timer_admitted
                    .await
                    .expect("actual confirmed startup timer admitted");
                assert_eq!(target.value(), original_clock.now_ms() + 1_000);
                original_clock.set_time(target.value());
            };
            let (reopened_runtime, ()) = tokio::join!(build, drive_startup_timer);
            let reopened_runtime = reopened_runtime
                .expect("actual confirmed original physical profile reopen under adopted subject");
            let reopened_invitee = std::sync::Arc::new(crate::AuraAgent::new(
                reopened_runtime,
                first_manifest.subject,
            ));
            use aura_app::runtime_bridge::RuntimeBridge;
            crate::runtime_bridge::AgentRuntimeBridge::new(reopened_invitee.clone())
                .bootstrap_signing_keys(
                    crate::runtime::services::RuntimeServiceContext::test_original(
                        reopened_invitee.runtime().tasks(),
                        reopened_invitee.runtime().effects(),
                    )
                    .await
                    .startup_window(),
                )
                .await
                .expect(
                    "restore original confirmed active native share without reminting authority",
                );
            assert_eq!(
                reopened_invitee
                    .runtime()
                    .threshold_signing()
                    .public_key_package(&first_manifest.subject)
                    .await
                    .expect("actual active native threshold package after original profile reload"),
                original_package,
            );
            let quorum_required = reopened_invitee
                .runtime()
                .effects()
                .require_local_physical_solo_identity_policy(
                    &first_manifest.subject,
                    first_manifest.pending_epoch,
                )
                .await
                .expect_err(
                    "actual reloaded 2/2 key remains quorum-owned after valid native share read",
                );
            let mut cause: &(dyn std::error::Error + 'static) = &quorum_required;
            let mut actual_quorum = false;
            loop {
                actual_quorum |= matches!(cause.downcast_ref::<crate::runtime::effects::RequiredSigningParticipantError>(),
                    Some(crate::runtime::effects::RequiredSigningParticipantError::QuorumOwnerRequired { threshold: 2 }));
                match cause.source() {
                    Some(source) => cause = source,
                    None => break,
                }
            }
            assert!(actual_quorum, "native current-key effect must preserve concrete 2/2 quorum source, not Storage or solo authority");
            if foreign_subject {
                reopened_invitee
                    .runtime()
                    .tasks()
                    .shutdown_gracefully(std::time::Duration::from_secs(5))
                    .await
                    .expect(
                        "actual confirmed subject runtime strict cleanup after foreign refusal",
                    );
                issuer
                    .runtime()
                    .tasks()
                    .shutdown_gracefully(std::time::Duration::from_secs(5))
                    .await
                    .expect("actual original issuer strict cleanup without second ceremony");
                return;
            }
            // A second real issuance after the first attested epoch fence must
            // export the actual active threshold package, although its history
            // contains only old-epoch signature parents.
            {
                use aura_app::runtime_bridge::RuntimeBridge;
                let third_authority = aura_core::AuthorityId::new_from_entropy(aura_core::hash::hash(b"aura-agent.enrollment.real-committed-confirmation.second-quorum.third-authority"));
                let third_config = crate::core::AgentConfig {
                    device_id: aura_core::DeviceId::new_from_entropy(aura_core::hash::hash(b"aura-agent.enrollment.real-committed-confirmation.second-quorum.third-device")),
                    storage: crate::core::config::StorageConfig {
                        base_path: tempfile::Builder::new()
                            .prefix("aura-second-enrollment-")
                            .tempdir()
                            .unwrap()
                            .keep(),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let third_context = aura_core::context::EffectContext::new(
                    third_authority,
                    aura_core::ContextId::new_from_entropy(aura_core::hash::hash(b"aura-agent.enrollment.real-committed-confirmation.second-quorum.third-context")),
                    aura_core::effects::ExecutionMode::Testing,
                );
                let third_profile =
                    crate::runtime::builder::TestingOwnedProfileCapability::acquire(&third_config)
                        .expect("actual independent third-device physical profile custody");
                let third_runtime =
                    crate::runtime::EffectSystemBuilder::testing_with_owned_profile(third_profile)
                        .with_authority(third_authority)
                        .with_config(third_config)
                        .with_shared_transport(original_transport.clone())
                        .with_physical_time_provider(selected_clock.clone())
                        .build(&third_context)
                        .await
                        .unwrap();
                let third =
                    std::sync::Arc::new(crate::AuraAgent::new(third_runtime, third_authority));
                let third_bridge = crate::runtime_bridge::AgentRuntimeBridge::new(third.clone());
                third_bridge
                    .bootstrap_signing_keys(
                        crate::runtime::services::RuntimeServiceContext::test_original(
                            third.runtime().tasks(),
                            third.runtime().effects(),
                        )
                        .await
                        .startup_window(),
                    )
                    .await
                    .unwrap();
                let setup = third_bridge
                    .export_device_enrollment_setup_request()
                    .await
                    .unwrap();
                let issuer_bridge = std::sync::Arc::new(
                    crate::runtime_bridge::AgentRuntimeBridge::new(issuer.clone()),
                );
                let issuer_app = std::sync::Arc::new(async_lock::RwLock::new(
                    aura_app::AppCore::with_runtime(
                        aura_app::AppConfig::default(),
                        issuer_bridge.clone(),
                    )
                    .unwrap(),
                ));
                let setup=aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(&issuer_app,setup).await.unwrap();
                if let Some(boundary) = allocation_drop {
                    let write_started = !matches!(boundary, AllocationDropBoundary::BeforeWrite);
                    let allocation_effects = issuer.runtime().effects();
                    let namespaces = [
                        "device_enrollment_generation_allocation_v1",
                        "device_enrollment_generation_live_slot_v2",
                        "device_enrollment_orphan_retirement_v1",
                        "device_enrollment_orphan_cleaned_v1",
                    ];
                    let mut before = Vec::new();
                    if !write_started {
                        for namespace in namespaces {
                            let mut keys = allocation_effects
                                .secure_list_keys(namespace, &[SecureStorageCapability::List])
                                .await
                                .unwrap();
                            keys.sort();
                            before.push(keys);
                        }
                    }
                    type BirthReference =
                        aura_core::effects::secret_lifetime::SecretAllocationReference;
                    type Gate = std::pin::Pin<
                        Box<
                            dyn std::future::Future<
                                    Output = Result<
                                        Option<BirthReference>,
                                        tokio::sync::oneshot::error::RecvError,
                                    >,
                                > + Send,
                        >,
                    >;
                    let (committed, retain_ack): (Gate, _) = match boundary {
                        AllocationDropBoundary::WrapBirth => {
                            let (reached, resume) = allocation_effects
                                .hold_original_allocation_birth_for_test()
                                .unwrap();
                            (Box::pin(async move { reached.await.map(Some) }), resume)
                        }
                        _ => {
                            let (reached, resume) = if write_started {
                                allocation_effects
                                    .hold_original_allocation_commit_for_test()
                                    .unwrap()
                            } else {
                                allocation_effects
                                    .hold_original_allocation_prewrite_for_test()
                                    .unwrap()
                            };
                            (
                                Box::pin(async move { reached.await.map(|()| None) }),
                                resume,
                            )
                        }
                    };
                    let probe = issuer
                        .threshold_signing()
                        .launch_original_prepared_probe_for_test(issuer.clone(), setup)
                        .await
                        .unwrap();
                    let disposal = probe.disposal_observer().unwrap();
                    tokio::pin!(disposal);
                    let born = tokio::select! {
                        result = committed => result.expect("actual original allocation boundary"),
                        outcome = disposal.await_disposal() => panic!("original task ended before actual allocation boundary: {:?}", outcome.outcome()),
                    };
                    if let Some(reference) = born.as_ref() {
                        let (state, readable) = allocation_effects
                            .observe_original_lifetime_for_test(reference)
                            .await
                            .unwrap();
                        assert!(matches!(
                            state,
                            aura_core::effects::secret_lifetime::SecretLifetimeState::Live
                        ));
                        assert!(readable);
                    }
                    let generation = allocation_effects.enrollment_retirement_generation_guard();
                    tokio::pin!(generation);
                    assert!(
                        futures::poll!(generation.as_mut()).is_pending(),
                        "actual original generation lease retained at cancellation boundary"
                    );
                    probe.request_cancel().unwrap();
                    let disposed = disposal.await_disposal().await;
                    probe.require_original_disposal_for_test(&disposed).unwrap();
                    drop(generation.await);
                    let wrap_original = probe
                        .with_retained_resources_for_test(|resources| {
                            assert!(resources.reservation.is_none());
                            assert!(resources.root.is_none());
                            assert!(resources.registered.is_none());
                            let producer = resources
                                .allocation
                                .as_ref()
                                .expect("same original producer retained");
                            let wrap = if born.is_some() {
                                Some(producer.acknowledged_wrap_original_for_test())
                            } else {
                                if write_started {
                                    producer.assert_pending_allocation_retired_for_test();
                                } else {
                                    producer.assert_unwritten_allocation_disposed_for_test();
                                }
                                None
                            };
                            assert!(resources.first_failure.is_some());
                            wrap
                        })
                        .unwrap();
                    if let (Some(reference), Some((ceremony, authority, epoch, decision))) =
                        (born.as_ref(), wrap_original)
                    {
                        let (state, readable) = allocation_effects
                            .observe_original_lifetime_for_test(reference)
                            .await
                            .unwrap();
                        match state {
                            aura_core::effects::secret_lifetime::SecretLifetimeState::Retired {
                                decision: actual,
                            } => assert_eq!(actual, decision),
                            _ => panic!("actual original birth remains unretired"),
                        }
                        assert!(!readable);
                        for namespace in [
                            "device_enrollment_generation_allocation_v1",
                            "device_enrollment_orphan_retirement_v1",
                            "device_enrollment_orphan_cleaned_v1",
                        ] {
                            assert_eq!(
                                allocation_effects
                                    .secure_retrieve(
                                        &aura_core::effects::SecureStorageLocation::new(
                                            namespace,
                                            ceremony.to_string()
                                        ),
                                        &[SecureStorageCapability::Read]
                                    )
                                    .await
                                    .unwrap(),
                                decision
                            );
                        }
                        assert!(!allocation_effects
                            .secure_exists(
                                &crate::runtime::effects::enrollment_generation_profile_location(
                                    &authority, epoch
                                )
                            )
                            .await
                            .unwrap());
                    }
                    if !write_started {
                        let mut after = Vec::new();
                        for namespace in namespaces {
                            let mut keys = allocation_effects
                                .secure_list_keys(namespace, &[SecureStorageCapability::List])
                                .await
                                .unwrap();
                            keys.sort();
                            after.push(keys);
                        }
                        assert_eq!(before, after, "unwritten cleanup publishes no allocation/live/retirement/cleaned record");
                    }
                    drop(retain_ack);
                    return;
                }
                let prepared = issuer_bridge
                    .prepare_device_enrollment_ceremony(
                        "Second actual device".into(),
                        setup.clone(),
                    )
                    .await
                    .expect("one original held issuer prepares exact public signing intent");
                eprintln!("quorum fixture: second prepared intent observed");
                let sibling_bridge = std::sync::Arc::new(
                    crate::runtime_bridge::AgentRuntimeBridge::new(reopened_invitee.clone()),
                );
                assert!(prepared
                    .signing_intent_code
                    .starts_with("aura-enrollment-signing-intent:v4:"));
                assert!(aura_app::ui::workflows::ceremonies::select_user_transferred_enrollment_signing_intent(
                    prepared.signing_intent_code.replacen("aura-enrollment-signing-intent:v4:", "aura-enrollment-signing-intent:v1:", 1),
                ).is_err(), "historical consent cannot authorize current terminal policy");
                assert!(aura_app::ui::workflows::ceremonies::select_user_transferred_enrollment_signing_intent(
                    prepared.signing_intent_code.replacen("aura-enrollment-signing-intent:v4:", "aura-enrollment-signing-intent:v3:", 1),
                ).is_err(), "v3 wrapper cannot downgrade the approved fourth domain");
                {
                    use base64::Engine;
                    let encoded = prepared
                        .signing_intent_code
                        .strip_prefix("aura-enrollment-signing-intent:v4:")
                        .expect("current original approval code");
                    let canonical = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(encoded)
                        .expect("canonical current approval bytes");
                    let current: aura_invitation::enrollment_signing_intent::EnrollmentSigningIntent =
                        aura_core::util::serialization::from_slice(&canonical)
                            .expect("canonical typed approval shape");
                    let mut value = serde_json::to_value(&current)
                        .expect("typed approval has a JSON-compatible test mutation shape");
                    value["version"] = serde_json::json!(3);
                    let historical: aura_invitation::enrollment_signing_intent::EnrollmentSigningIntent =
                        serde_json::from_value(value).expect("historical version field remains parseable");
                    assert!(
                        historical.validate().is_err(),
                        "v3 body cannot authorize the current five-domain approval"
                    );
                    let mut reordered = serde_json::to_value(&current)
                        .expect("current intent has a JSON-compatible mutation shape");
                    reordered["domains"]
                        .as_array_mut()
                        .expect("current intent has ordered domains")
                        .swap(0, 1);
                    let reordered: aura_invitation::enrollment_signing_intent::EnrollmentSigningIntent =
                        serde_json::from_value(reordered)
                            .expect("reordered domains remain parseable");
                    assert!(
                        reordered.validate().is_err(),
                        "reordered domains cannot authorize the current approval"
                    );
                    let mut substituted = serde_json::to_value(&current)
                        .expect("current intent has a JSON-compatible mutation shape");
                    let domains = substituted["domains"]
                        .as_array_mut()
                        .expect("current intent has ordered domains");
                    let replacement = domains[2].clone();
                    domains[3] = replacement;
                    let substituted: aura_invitation::enrollment_signing_intent::EnrollmentSigningIntent =
                        serde_json::from_value(substituted)
                            .expect("substituted domain remains parseable");
                    assert!(
                        substituted.validate().is_err(),
                        "a substituted issuer return domain cannot authorize current approval"
                    );
                }
                let sibling_app = std::sync::Arc::new(async_lock::RwLock::new(
                    aura_app::AppCore::with_runtime(
                        aura_app::AppConfig::default(),
                        sibling_bridge.clone(),
                    )
                    .expect("actual adopted sibling app runtime owner"),
                ));
                let sibling_setup = aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                    &sibling_app, prepared.setup_transfer_code.clone(),
                ).await.expect("explicit independent sibling setup verifier pin");
                let sibling_intent = aura_app::ui::workflows::ceremonies::select_user_transferred_enrollment_signing_intent(
                    prepared.signing_intent_code.clone(),
                ).expect("explicit sibling selection of exact public intent");
                assert_eq!(sibling_intent.domains(), [
                    aura_invitation::enrollment_signing_intent::EnrollmentSigningDomain::Manifest,
                    aura_invitation::enrollment_signing_intent::EnrollmentSigningDomain::PublicTransport,
                    aura_invitation::enrollment_signing_intent::EnrollmentSigningDomain::InitialRequest,
                    aura_invitation::enrollment_signing_intent::EnrollmentSigningDomain::IssuerReturnMailbox,
                    aura_invitation::enrollment_signing_intent::EnrollmentSigningDomain::NegativeTerminalPolicy,
                ]);
                let sibling_approval = aura_app::ui::workflows::ceremonies::approve_user_selected_enrollment_signing_intent(
                    &sibling_app, sibling_intent, sibling_setup,
                ).await.expect("explicit original active sibling user approval");
                eprintln!("quorum fixture: sibling independent approval observed");
                let original_dynamic_owner = sibling_app
                    .read()
                    .await
                    .runtime()
                    .cloned()
                    .expect("app retains original sibling bridge allocation directly");
                sibling_approval
                    .require_runtime_owner(original_dynamic_owner.as_ref())
                    .expect("original retained dynamic bridge is the same approval owner");
                sibling_approval
                    .require_runtime_owner(sibling_bridge.as_ref())
                    .expect("concrete coercion preserves the same original bridge allocation");
                let different_bridge_same_agent =
                    crate::runtime_bridge::AgentRuntimeBridge::new(reopened_invitee.clone());
                assert!(matches!(
    sibling_approval.require_runtime_owner(&different_bridge_same_agent),
    Err(aura_core::AuraError::PermissionDenied { .. }),
), "a different bridge allocation over the same agent cannot consume original approval");
                drop(original_dynamic_owner);
                if native_failure {
                    let sibling_approval = crate::runtime_bridge::enrollment_quorum::admit_original_runtime_approval_for_test(
                        sibling_bridge.as_ref(), sibling_approval,
                    ).expect("same actual independently adopted sibling approval");
                    crate::runtime::services::enrollment_window::assert_original_native_failure_for_test(
                        reopened_invitee.clone(), sibling_approval,
                    ).await;
                    let own_intent = aura_app::ui::workflows::ceremonies::select_user_transferred_enrollment_signing_intent(
                        prepared.signing_intent_code,
                    ).unwrap();
                    let own_approval = aura_app::ui::workflows::ceremonies::approve_user_selected_enrollment_signing_intent(
                        &issuer_app, own_intent, setup,
                    ).await.unwrap();
                    let own_approval = crate::runtime_bridge::enrollment_quorum::admit_original_runtime_approval_for_test(
                        issuer_bridge.as_ref(), own_approval,
                    ).unwrap();
                    // This exact prepared entry acknowledges supervised disposal only.
                    // It does not claim a durable enrollment negative acknowledgment.
                    let teardown = issuer
                        .threshold_signing()
                        .cancel_original_prepared_for_test(&own_approval)
                        .await;
                    if let Err(source) = teardown {
                        assert!(
                            std::error::Error::source(&source).is_some(),
                            "original supervised failure retains source"
                        );
                    }
                    return;
                }
                sibling_bridge
                    .approve_device_enrollment_signing(sibling_approval)
                    .await
                    .expect("owned original sibling accepts bounded participant ingress");
                eprintln!("quorum fixture: sibling participant ingress installed");
                // The second issuer export itself needs this adopted sibling's
                // genuine quorum ingress. Start its owned driver before awaiting
                // export, and keep it alive through the later rotation/acceptance.
                let (quorum_finished, mut finished_rx) = tokio::sync::watch::channel(false);
                let quorum_notify = original_transport.inbox_notify(first_manifest.subject);
                let signing = reopened_invitee.threshold_signing();
                let issuer_device = issuer.runtime().effects().device_id();
                let mut quorum_driver = tokio::task::JoinSet::new();
                quorum_driver.spawn(async move {
                        loop {
                            let arrival = quorum_notify.notified();
                            tokio::pin!(arrival);
                            arrival.as_mut().enable();
                            signing.process_device_quorum_inbound().await.unwrap();
                            for request in signing.pending_signing_requests().await {
                                assert_eq!(request.requesting_device, issuer_device);
                                assert!(!second_positive || [
                                    "signed message (aura.sync.device_epoch_rotation.proposal.v2)",
                                    "signed message (aura.sync.device_epoch_rotation.enrollment_commit.v2)",
                                    "account device tree update",
                                ].contains(&request.operation.as_str()),
                                    "explicit original enrollment rotation signing consent: {}", request.operation);
                                signing
                                    .decide_pending_signing_request(&request.id, true)
                                    .await
                                    .unwrap();
                            }
                            if *finished_rx.borrow() {
                                break;
                            }
                            tokio::select! {
                                _ = arrival => {},
                                changed = finished_rx.changed() => { changed.unwrap(); },
                            }
                        }
                    });
                let own_intent = aura_app::ui::workflows::ceremonies::select_user_transferred_enrollment_signing_intent(
                    prepared.signing_intent_code,
                ).expect("original issuer selects exact prepared public intent");
                let own_approval = aura_app::ui::workflows::ceremonies::approve_user_selected_enrollment_signing_intent(
                    &issuer_app, own_intent, setup,
                ).await.expect("original issuer explicit user approval");
                eprintln!("quorum fixture: issuer original approval observed");
                if registration_drop {
                    let actual = crate::runtime_bridge::enrollment_quorum::admit_original_runtime_approval_for_test(
                        issuer_bridge.as_ref(), own_approval,
                    ).expect("genuine original issuer approval");
                    let signing = issuer.threshold_signing();
                    let probe = signing
                        .original_prepared_issuer_probe_for_test(&actual)
                        .await
                        .expect("same exact approval-bound required entry");
                    let disposed = probe.disposal_observer().unwrap();
                    let (committed, continue_readback) = issuer
                        .runtime()
                        .effects()
                        .hold_original_registration_readback_for_test()
                        .unwrap();
                    let resume = signing.resume_original_quorum_issuer(actual);
                    tokio::pin!(resume);
                    tokio::select! {
                        commit = committed => commit.expect("actual secure registration write completed"),
                        result = &mut resume => panic!("issuance ended before actual registration commit gate: {result:?}"),
                    }
                    println!(
                        "original-registration-drop: committed actual live profile before readback"
                    );
                    probe.request_cancel().unwrap();
                    let disposal = disposed.await_disposal().await;
                    println!(
                        "original-registration-drop: original required task actually disposed"
                    );
                    probe.require_original_disposal_for_test(&disposal).unwrap();
                    probe
                        .with_retained_resources_for_test(|resources| {
                            assert!(
                                resources.registered_custody.is_none(),
                                "no signed-control custodian fabricated before readback"
                            );
                            assert!(
                                resources.registered.is_none(),
                                "no positive registered authority survived negative handoff"
                            );
                            resources
                                .negative
                                .as_ref()
                                .expect("same original held negative owner retained")
                                .assert_retained_cleanup_for_test(
                                    resources
                                        .first_failure
                                        .as_ref()
                                        .expect("actual first cancellation source retained"),
                                );
                        })
                        .unwrap();
                    drop(continue_readback);
                    return;
                }
                for (domain, diagnostic, required) in [
                    (
                        "manifest",
                        own_approval.manifest().transcript_bytes().unwrap(),
                        own_approval.manifest().required_transcript_bytes().unwrap(),
                    ),
                    (
                        "public transport",
                        own_approval.transport().transcript_bytes().unwrap(),
                        own_approval
                            .transport()
                            .required_transcript_bytes()
                            .unwrap(),
                    ),
                    (
                        "initial Request",
                        own_approval.initial_request().transcript_bytes().unwrap(),
                        own_approval
                            .initial_request()
                            .required_transcript_bytes()
                            .unwrap(),
                    ),
                ] {
                    assert_eq!(
                        diagnostic, required,
                        "required encoding preserves exact original approved {domain} domain bytes"
                    );
                }
                let (second, second_disposal, second_probe) = if second_positive {
                    let actual = crate::runtime_bridge::enrollment_quorum::admit_original_runtime_approval_for_test(
                        issuer_bridge.as_ref(), own_approval,
                    ).expect("same actual issuer user approval for positive quorum scenario");
                    let signing = issuer.threshold_signing();
                    let probe = signing
                        .original_prepared_issuer_probe_for_test(&actual)
                        .await
                        .unwrap();
                    let disposal = probe.disposal_observer().unwrap();
                    eprintln!("quorum fixture: positive issuer resume entered");
                    let start = signing.resume_original_quorum_issuer(actual).await.unwrap();
                    eprintln!("quorum fixture: positive issuer resume returned");
                    (start, Some(disposal), Some(probe))
                } else {
                    (
                        {
                            eprintln!("quorum fixture: committed issuer resume entered");
                            let result = issuer_bridge
                                .resume_device_enrollment_signing(own_approval)
                                .await
                                .expect("genuine approved active 2-of-2 runtime quorum issuance");
                            eprintln!("quorum fixture: committed issuer resume returned");
                            result
                        },
                        None,
                        None,
                    )
                };
                assert_eq!(
                    second.ceremony_id, prepared.ceremony_id,
                    "resume retains the original prepared ceremony allocation"
                );
                let transfer = second.manifest_transfer.as_ref().unwrap();
                let third_app = std::sync::Arc::new(async_lock::RwLock::new(
                    aura_app::AppCore::with_runtime(
                        aura_app::AppConfig::default(),
                        std::sync::Arc::new(third_bridge),
                    )
                    .unwrap(),
                ));
                let selected =
                    aura_app::ui::workflows::ceremonies::pin_user_transferred_enrollment_manifest(
                        &third_app,
                        transfer.manifest_code.clone(),
                        transfer.initiator_verifier_code.clone(),
                    )
                    .await
                    .unwrap();
                let manifest = selected.manifest();
                assert_eq!(
                    manifest.version,
                    aura_invitation::enrollment_manifest::EnrollmentTrustManifest::CURRENT_VERSION
                );
                assert_eq!(
                    aura_invitation::shareable::ShareableInvitation::from_code(
                        &second.enrollment_code
                    )
                    .unwrap()
                    .version,
                    aura_invitation::shareable::ShareableInvitation::ENROLLMENT_QUORUM_VERSION,
                );
                assert_eq!(manifest.final_epoch, first_manifest.pending_epoch);
                assert!(manifest
                    .parents
                    .iter()
                    .all(|parent| parent.epoch < manifest.final_epoch));
                let active = manifest.final_inventory().unwrap();
                assert_eq!(active.len(), 1);
                assert_eq!(active[0].epoch, manifest.final_epoch);
                assert_eq!(active[0].commitment, manifest.final_commitment);
                assert_eq!(
                    active[0].mode,
                    aura_core::crypto::single_signer::SigningMode::Threshold
                );
                assert_eq!(active[0].threshold, 2);
                assert_eq!(active[0].participants.len(), 2);
                assert_eq!(active[0].public_key_package, original_package);
                assert_ne!(
                    active[0].public_key_package,
                    manifest.parents[0].public_key_package
                );
                let second_invitation = manifest.invitation.clone();
                let peer_subject = manifest.subject;
                let peer_ceremony = manifest.ceremony.clone();
                crate::runtime_bridge::AgentRuntimeBridge::new(third.clone())
                    .import_enrollment_invitation(&second.enrollment_code, selected)
                    .await
                    .unwrap();
                let return_admissions = super::super::enrollment_manifest_admission::load_recoverable_negative_admissions(
                    third.runtime().effects().as_ref(),
                ).await.expect("third device retained independently pinned quorum admission");
                let return_original = return_admissions
                    .iter()
                    .find(|admission| admission.manifest().invitation == second_invitation)
                    .expect("exact second original quorum admission");
                use aura_signature::SecurityTranscript;
                let return_manifest = return_original.manifest();
                let return_digest = aura_core::hash::hash(
                    &return_manifest
                        .transcript_bytes()
                        .expect("canonical second manifest"),
                );
                let return_location = aura_core::effects::SecureStorageLocation::new(
                    "issuer_return_mailbox_registration_v1",
                    format!(
                        "{}:{}:{}",
                        return_manifest.subject,
                        return_manifest.invitation,
                        hex::encode(return_digest)
                    ),
                );
                let return_bytes = issuer_effects
                    .secure_retrieve(&return_location, &[SecureStorageCapability::Read])
                    .await
                    .expect("original approved quorum retained fourth-domain registration");
                let return_registration: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration =
                    aura_core::util::serialization::from_slice(&return_bytes)
                        .expect("canonical original quorum return registration");
                assert_eq!(
                    return_registration.statement.manifest_digest(),
                    return_digest
                );
                assert_eq!(return_registration.proof.signer_count, 2);
                assert_eq!(return_registration.proof.signers, [1, 2]);
                return_registration
                    .clone()
                    .verify_for_admitted_original(
                        third.runtime().effects().as_ref(),
                        return_original.baseline(),
                        return_original.local_setup(),
                    )
                    .await
                    .expect(
                        "genuine four-domain aggregate verifies against third device admission",
                    );
                for (field, replacement) in [
                    ("invitation", serde_json::json!("foreign-return-invitation")),
                    ("manifest_digest", serde_json::json!(vec![0_u8; 32])),
                    (
                        "issuer_device",
                        serde_json::json!(aura_core::DeviceId::from_bytes([49; 32])),
                    ),
                    (
                        "invitee_device",
                        serde_json::json!(aura_core::DeviceId::from_bytes([50; 32])),
                    ),
                    ("issuer_policy_index", serde_json::json!(u16::MAX)),
                    (
                        "issuer_public_key_package_digest",
                        serde_json::json!(vec![0_u8; 32]),
                    ),
                    (
                        "expires_at_ms",
                        serde_json::json!(return_manifest.expires_at_ms - 1),
                    ),
                ] {
                    let mut altered = serde_json::to_value(&return_registration).unwrap();
                    altered["statement"][field] = replacement;
                    let altered: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration =
                        serde_json::from_value(altered).unwrap();
                    assert!(matches!(altered.verify_for_admitted_original(
                        third.runtime().effects().as_ref(), return_original.baseline(), return_original.local_setup(),
                    ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::Binding)),
                        "foreign {field} cannot select original quorum return route"
                    );
                }
                for (field, replacement, expected_binding) in [
                    (
                        "session",
                        serde_json::json!(uuid::Uuid::from_bytes([91; 16])),
                        true,
                    ),
                    (
                        "retrieval_credential_commitment",
                        serde_json::json!(vec![91_u8; 32]),
                        false,
                    ),
                ] {
                    let mut altered = serde_json::to_value(&return_registration).unwrap();
                    altered["statement"][field] = replacement;
                    let altered: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration =
                        serde_json::from_value(altered).unwrap();
                    let result = altered
                        .verify_for_admitted_original(
                            third.runtime().effects().as_ref(),
                            return_original.baseline(),
                            return_original.local_setup(),
                        )
                        .await;
                    assert!(
                        if expected_binding {
                            matches!(result, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::Binding))
                        } else {
                            matches!(result, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::InvalidSignature))
                        },
                        "foreign {field} must fail with its typed boundary"
                    );
                }
                let mut false_key = return_registration.clone();
                false_key.proof.public_key_package[0] ^= 1;
                assert!(
                    false_key
                        .verify_for_admitted_original(
                            third.runtime().effects().as_ref(),
                            return_original.baseline(),
                            return_original.local_setup(),
                        )
                        .await
                        .is_err(),
                    "foreign group package cannot authorize return route"
                );
                let mut stale_package = return_registration.clone();
                stale_package.proof.public_key_package =
                    return_manifest.parents[0].public_key_package.clone();
                assert!(matches!(stale_package.verify_for_admitted_original(
                    third.runtime().effects().as_ref(), return_original.baseline(), return_original.local_setup(),
                ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::SigningPolicy)),
                    "previous epoch package cannot authorize the current return route");
                let mut stale_epoch = return_registration.clone();
                stale_epoch.proof.epoch = stale_epoch.proof.epoch.wrapping_add(1);
                assert!(matches!(stale_epoch.verify_for_admitted_original(
                    third.runtime().effects().as_ref(), return_original.baseline(), return_original.local_setup(),
                ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::SigningPolicy)),
                    "stale group epoch cannot authorize return route");
                let mut reordered_roster = return_registration.clone();
                reordered_roster.proof.signers.reverse();
                assert!(matches!(reordered_roster.verify_for_admitted_original(
                    third.runtime().effects().as_ref(), return_original.baseline(), return_original.local_setup(),
                ).await, Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::SigningPolicy)),
                    "reordered full signer metadata cannot authorize return route");
                let mut false_roster = return_registration.clone();
                false_roster.proof.signer_count = 1;
                false_roster.proof.signers = vec![1];
                assert!(
                    false_roster
                        .verify_for_admitted_original(
                            third.runtime().effects().as_ref(),
                            return_original.baseline(),
                            return_original.local_setup(),
                        )
                        .await
                        .is_err(),
                    "partial claimed roster cannot authorize full-roster registration"
                );
                let mut false_signature = return_registration.clone();
                false_signature.proof.signature[0] ^= 1;
                assert!(
                    false_signature
                        .verify_for_admitted_original(
                            third.runtime().effects().as_ref(),
                            return_original.baseline(),
                            return_original.local_setup(),
                        )
                        .await
                        .is_err(),
                    "tampered group signature cannot authorize return route"
                );
                if second_positive {
                    let rotation_notify = original_transport.inbox_notify(first_manifest.subject);
                    let signing = reopened_invitee.threshold_signing();
                    if postread_drop {
                        signing
                            .install_original_peer_commit_postread_test(
                                peer_subject,
                                &peer_ceremony,
                                postread_test.clone(),
                            )
                            .await
                            .unwrap();
                    }
                    let actor_disposal = if let Some(test) = &actor_test {
                        Some(
                            signing
                                .install_original_positive_actor_test(
                                    peer_subject,
                                    &peer_ceremony,
                                    test.clone(),
                                )
                                .await
                                .unwrap(),
                        )
                    } else {
                        None
                    };
                    let rotation =
                        crate::runtime_bridge::AgentRuntimeBridge::new(reopened_invitee.clone())
                            .original_enrollment_finalizer_service();
                    let rotation_driver = async {
                        loop {
                            let arrival = rotation_notify.notified();
                            tokio::pin!(arrival);
                            arrival.as_mut().enable();
                            let result = rotation.process_pending_participant_sessions().await;
                            if cleanup_positive && result.is_err() {
                                break;
                            }
                            let (_, completed) = result.unwrap();
                            if completed > 0 {
                                break;
                            }
                            arrival.await;
                        }
                    };
                    let accept = async {
                        if cleanup_positive {
                            let actual = third.invitations().unwrap();
                            let mut acceptance = Box::pin(actual.accept(&second_invitation));
                            tokio::select! {
                                _ = async { if let Some(test) = &actor_test { test.await_completed().await; } else { postread_test.await_cleanup_completed().await; } } => {},
                                result = &mut acceptance => panic!("acceptance ended before actual original peer cleanup: {result:?}"),
                            }
                            drop(acceptance);
                            if let Some(test) = &actor_test {
                                test.assert_completed();
                            } else {
                                postread_test.assert_completed();
                            }
                            return;
                        }
                        third.invitations().unwrap().accept(&second_invitation).await
                            .expect("real third-device acceptance and full existing-device quorum commit");
                        let disposed = second_disposal
                            .as_ref()
                            .expect("same original issuer disposal observer")
                            .await_disposal()
                            .await;
                        assert!(
                            matches!(
                                disposed.outcome(),
                                aura_core::effects::task::RequiredTaskExit::Completed
                            ),
                            "same original issuer acknowledges actual positive cleanup"
                        );
                    };
                    let mut work = Box::pin(async { futures::join!(accept, rotation_driver) });
                    tokio::select! {
                        _ = &mut work => {},
                        result = quorum_driver.join_next() => panic!(
                            "original sibling quorum ingress ended before acceptance and rotation: {result:?}"
                        ),
                    }
                    quorum_finished.send_replace(true);
                    quorum_driver
                        .join_next()
                        .await
                        .expect("original sibling quorum ingress task completed")
                        .expect("original sibling quorum ingress retained its result");
                    if postread_drop {
                        postread_test.assert_completed();
                    }
                    if let Some(disposal) = &actor_disposal {
                        let actual = disposal.await_disposal().await;
                        assert!(
                            matches!(
                                actual.outcome(),
                                aura_core::effects::task::RequiredTaskExit::Failed(_)
                            ),
                            "actual original peer failure retained: {:?}",
                            actual.outcome()
                        );
                        actor_test.as_ref().unwrap().assert_completed();
                        signing
                            .assert_original_positive_actor_cleanup(peer_subject, &peer_ceremony)
                            .await;
                    }
                    assert_eq!(
                        original_clock.now_ms(),
                        6_000,
                        "only the admitted confirmed-profile startup timer advances the selected clock"
                    );
                    if cleanup_positive {
                        assert_eq!(
                            issuer
                                .runtime()
                                .ceremony_runner()
                                .terminal_outcome(&peer_ceremony)
                                .await
                                .unwrap(),
                            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed),
                            "destructive local cleanup never rewrites original first decision"
                        );
                        let original_issuer = second_probe
                            .as_ref()
                            .expect("same actual issuer probe")
                            .original_registered_owner_for_test()
                            .expect("project actual original registered owner");
                        original_issuer
                            .request_original_issuer_cancel_for_test()
                            .expect("cancel SAME retained running issuer task");
                        let original_disposal = second_disposal
                            .as_ref()
                            .expect("same original issuer disposal observer")
                            .await_disposal()
                            .await;
                        original_issuer
                            .require_original_issuer_disposal_for_test(&original_disposal)
                            .expect("actual SAME issuer disposal");
                        let aura_core::effects::task::RequiredTaskExit::Failed(source) =
                            original_disposal.outcome()
                        else {
                            panic!("cancelled original issuer must retain its actual domain failure: {:?}", original_disposal.outcome());
                        };
                        eprintln!("actual original issuer failure before shutdown: {source:?}");
                        let issuer_settlement = issuer
                            .runtime()
                            .tasks()
                            .shutdown_gracefully(std::time::Duration::from_secs(5))
                            .await;
                        let Err(crate::task_registry::TaskSupervisionError::TaskFailed {
                            source,
                            ..
                        }) = issuer_settlement
                        else {
                            panic!("expected retained unresolved issuer terminal, got {issuer_settlement:?}");
                        };
                        eprintln!(
                            "actual unresolved issuer terminal after peer cleanup: {source:?}"
                        );
                        second_probe
                            .as_ref()
                            .expect("same actual issuer probe")
                            .with_retained_resources_for_test(|resources| {
                                assert!(
                                    resources.registered_custody.is_some(),
                                    "same registered issuer custody remains retained"
                                );
                            })
                            .unwrap();
                        for (label, settlement) in [
                            (
                                "peer",
                                reopened_invitee
                                    .runtime()
                                    .tasks()
                                    .shutdown_gracefully(std::time::Duration::from_secs(5))
                                    .await,
                            ),
                            (
                                "invitee",
                                third
                                    .runtime()
                                    .tasks()
                                    .shutdown_gracefully(std::time::Duration::from_secs(5))
                                    .await,
                            ),
                        ] {
                            match settlement {
                                Ok(()) => {},
                                Err(crate::task_registry::TaskSupervisionError::TaskFailed { source, .. }) => eprintln!("actual {label} retained domain failure after cleanup: {source:?}"),
                                Err(source) => panic!("unexpected {label} resource shutdown failure: {source:?}"),
                            }
                        }
                        return;
                    }
                } else {
                    // The same enrolled sibling that signed the second export
                    // remains the real quorum ingress for its negative terminal.
                    let decline = async {
                        third.invitations().unwrap().decline(&second_invitation).await.expect(
                            "actual second invitation decline settles its original negative exchange");
                    };
                    let mut decline = Box::pin(decline);
                    tokio::select! {
                        _ = &mut decline => {},
                        result = quorum_driver.join_next() => panic!(
                            "original sibling quorum ingress ended before decline: {result:?}"
                        ),
                    }
                    quorum_finished.send_replace(true);
                    quorum_driver
                        .join_next()
                        .await
                        .expect("original sibling quorum ingress task completed")
                        .expect("original sibling quorum ingress retained its result");
                }
                third
                    .runtime()
                    .tasks()
                    .shutdown_gracefully(std::time::Duration::from_secs(5))
                    .await
                    .unwrap();
                if second_positive && !postread_drop && !cleanup_positive {
                    original_clock.set_time(return_manifest.expires_at_ms);
                    assert!(matches!(
                        return_registration.verify_for_admitted_original(
                            third.runtime().effects().as_ref(),
                            return_original.baseline(), return_original.local_setup(),
                        ).await,
                        Err(aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxError::OutsideValidity)
                    ), "selected physical expiry closes the original quorum return route");
                }
                drop(sibling_app);
                drop(sibling_bridge);
            }
            reopened_invitee
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
                .expect("actual reopened sibling signing actor completion");
            issuer
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
                .expect("actual issuer task completion after second issuance evidence");
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
            let ((issuer, invitee, invitation, start, _acceptance, witness), original) = Box::pin(
                super::super::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                    "actual-committed-checkpoint", None, None,
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
            original
                .observe_original_finalizer()
                .await
                .expect("actual original owned finalizer commits");
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
