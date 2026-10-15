//! Runtime-owned local signer for explicitly approved enrollment transcripts.
//!
//! Participants consume explicit local approval and publish public outputs only.
//! The coordinator has no API accepting a private share or nonce.
use std::sync::Arc;

use aura_core::effects::crypto::{FrostPublicCommitment, FrostSigningPackage};
use aura_core::effects::secure::{ImmutableSecureStoreOutcome, SecureStorageCapability};
use aura_core::effects::{CryptoExtendedEffects, SecureStorageEffects};
use aura_core::{AuraError, DeviceId};
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentTranscriptSigningError {
    #[error("original signing participant actor stopped")]
    ActorStopped,
    #[error("signing participant response was cancelled")]
    ResponseCancelled,
    #[error("original transcript approval was already consumed")]
    ApprovalConsumed,
    #[error("original positive signing interrupted during {stage}")]
    PositiveInterrupted { stage: &'static str },
    #[error("participant signing package differs from original local approval")]
    PackageBinding,
    #[error("public coordinator participant inventory is invalid")]
    Inventory,
}

fn rejected(source: EnrollmentTranscriptSigningError) -> AuraError {
    AuraError::Crypto {
        message: "owned enrollment transcript signing rejected".into(),
        source: Some(Arc::new(source)),
    }
}

pub(super) fn native_commitment_matches(
    package: &FrostSigningPackage,
    index: u16,
    original: Option<&Vec<u8>>,
) -> Result<bool, AuraError> {
    let native =
        frost_ed25519::SigningPackage::deserialize(&package.package).map_err(|source| {
            AuraError::crypto_with_source(
                "decode original participant signing package",
                Arc::new(source),
            )
        })?;
    let identifier = frost_ed25519::Identifier::try_from(index).map_err(|source| {
        AuraError::crypto_with_source(
            "decode original participant signing identifier",
            Arc::new(source),
        )
    })?;
    let commitment = native
        .signing_commitments()
        .get(&identifier)
        .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::PackageBinding))?;
    let original =
        original.ok_or_else(|| rejected(EnrollmentTranscriptSigningError::PackageBinding))?;
    let actual = commitment.serialize().map_err(|source| {
        AuraError::crypto_with_source(
            "encode original participant signing commitment",
            Arc::new(source),
        )
    })?;
    Ok(original == &actual)
}

#[cfg(test)]
mod native_commitment_tests {
    use super::*;

    #[test]
    fn malformed_native_package_retains_original_codec_cause() {
        let package = FrostSigningPackage {
            message: vec![],
            package: vec![0xff],
            participants: vec![1],
            public_key_package: vec![],
        };
        let original = vec![];
        let error = native_commitment_matches(&package, 1, Some(&original))
            .expect_err("malformed native package must fail before signing");
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        let mut native_codec = false;
        loop {
            native_codec |= cause.is::<frost_ed25519::Error>();
            match cause.source() {
                Some(source) => cause = source,
                None => break,
            }
        }
        assert!(
            native_codec,
            "native codec source must reach actor supervision"
        );
    }
}

use super::{ApprovedEnrollmentTranscriptRound, ApprovedLocalEnrollmentTranscript};

pub(super) enum ParticipantRequest {
    BeginPositiveTerminal(
        Arc<super::positive_terminal::AdmittedPositiveTerminalRequest>,
        oneshot::Sender<Result<(), AuraError>>,
    ),
    BeginNegativeTerminal(
        Arc<super::enrollment_transcript_wire::AdmittedNegativeTerminalRequest>,
        oneshot::Sender<Result<(), AuraError>>,
    ),
    RetireConditional(oneshot::Sender<Result<(), AuraError>>),
    Commitment(oneshot::Sender<Result<FrostPublicCommitment, AuraError>>),
    Share(
        FrostSigningPackage,
        oneshot::Sender<Result<Vec<u8>, AuraError>>,
    ),
}

/// Bounded public ingress. It exposes neither local effects nor private keys.
#[derive(Clone)]
pub(crate) struct EnrollmentTranscriptParticipantIngress {
    device: DeviceId,
    index: u16,
    requests: mpsc::Sender<ParticipantRequest>,
    terminal_failure: watch::Receiver<Option<AuraError>>,
}

impl EnrollmentTranscriptParticipantIngress {
    pub(super) fn matches_original_participant(&self, device: DeviceId, index: u16) -> bool {
        self.device == device && self.index == index
    }

    pub(super) async fn begin_negative_terminal(
        &self,
        request: Arc<super::OriginalNegativeTerminalRequest>,
    ) -> Result<(), AuraError> {
        let (sender, receiver) = oneshot::channel();
        self.requests
            .send(ParticipantRequest::BeginNegativeTerminal(
                Arc::new(
                    super::enrollment_transcript_wire::AdmittedNegativeTerminalRequest::Local(
                        request,
                    ),
                ),
                sender,
            ))
            .await
            .map_err(|_| rejected(EnrollmentTranscriptSigningError::ActorStopped))?;
        receiver
            .await
            .map_err(|_| rejected(EnrollmentTranscriptSigningError::ResponseCancelled))?
    }

    async fn stopped(&self, source: EnrollmentTranscriptSigningError) -> AuraError {
        let mut terminal = self.terminal_failure.clone();
        loop {
            if let Some(original) = terminal.borrow().as_ref() {
                return original.clone();
            }
            if terminal.changed().await.is_err() {
                return rejected(source);
            }
        }
    }
    pub(super) async fn commitment(&self) -> Result<FrostPublicCommitment, AuraError> {
        let (sender, receiver) = oneshot::channel();
        if self
            .requests
            .send(ParticipantRequest::Commitment(sender))
            .await
            .is_err()
        {
            return Err(self
                .stopped(EnrollmentTranscriptSigningError::ActorStopped)
                .await);
        }
        match receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(self
                .stopped(EnrollmentTranscriptSigningError::ResponseCancelled)
                .await),
        }
    }
    pub(super) async fn share(&self, package: FrostSigningPackage) -> Result<Vec<u8>, AuraError> {
        let (sender, receiver) = oneshot::channel();
        if self
            .requests
            .send(ParticipantRequest::Share(package, sender))
            .await
            .is_err()
        {
            return Err(self
                .stopped(EnrollmentTranscriptSigningError::ActorStopped)
                .await);
        }
        match receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(self
                .stopped(EnrollmentTranscriptSigningError::ResponseCancelled)
                .await),
        }
    }
}

pub(super) fn remote_public_ingress(
    device: DeviceId,
    index: u16,
) -> (
    EnrollmentTranscriptParticipantIngress,
    mpsc::Receiver<ParticipantRequest>,
    watch::Sender<Option<AuraError>>,
) {
    let (requests, receiver) = mpsc::channel(2);
    let (terminal_sender, terminal_failure) = watch::channel(None);
    (
        EnrollmentTranscriptParticipantIngress {
            device,
            index,
            requests,
            terminal_failure,
        },
        receiver,
        terminal_sender,
    )
}

// Private, move-only proof from the actual native participant. Its bytes are
// observations; only this producer's Created/readback and all-four-share path
// can issue the owner. It cannot be decoded, cloned, or built by a consumer.
pub(in crate::runtime::services) struct OriginalParticipantCompletion {
    effects: Arc<crate::runtime::AuraEffectSystem>,
    device: DeviceId,
    approved_intent_digest: [u8; 32],
    retirement_readbacks: [Vec<u8>; 4],
}
impl OriginalParticipantCompletion {
    pub(in crate::runtime::services) fn require_original(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.device != approval.effects().device_id()
            || self.approved_intent_digest != approval.canonical_intent_digest()
            || self
                .retirement_readbacks
                .iter()
                .any(|bytes| bytes.len() != 32)
        {
            return Err(rejected(EnrollmentTranscriptSigningError::PackageBinding));
        }
        Ok(())
    }
}

/// One producer-private slot belongs to the original registered participant.
/// Sharing the slot never clones, extracts, or reconstructs its actual receipt.
pub(in crate::runtime::services) struct OriginalParticipantCompletionCustody {
    actual: std::sync::OnceLock<OriginalParticipantCompletion>,
    first_failure: std::sync::OnceLock<AuraError>,
}
impl OriginalParticipantCompletionCustody {
    pub(in crate::runtime::services) fn fresh() -> Self {
        Self {
            actual: std::sync::OnceLock::new(),
            first_failure: std::sync::OnceLock::new(),
        }
    }
    pub(in crate::runtime::services) fn original(
        &self,
    ) -> Result<&OriginalParticipantCompletion, AuraError> {
        self.actual
            .get()
            .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::ResponseCancelled))
    }
    pub(in crate::runtime::services) fn original_failure(&self) -> Option<&AuraError> {
        self.first_failure.get()
    }
    fn retain_failure(&self, source: &AuraError) {
        let _ = self.first_failure.set(source.clone());
    }
    fn retain(&self, actual: OriginalParticipantCompletion) -> Result<(), AuraError> {
        self.actual
            .set(actual)
            .map_err(|actual| AuraError::Internal {
                message: "original participant completion already retained".into(),
                source: Some(Arc::new(UnacceptedParticipantCompletion { actual })),
            })
    }
}
#[derive(thiserror::Error)]
#[error("duplicate original participant completion remains retained")]
struct UnacceptedParticipantCompletion {
    actual: OriginalParticipantCompletion,
}
impl std::fmt::Debug for UnacceptedParticipantCompletion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("UnacceptedParticipantCompletion")
    }
}

/// Returns an owned participant future for the existing runtime TaskGroup.
/// The caller must retain the actual returned owned task handle, observe its
/// terminal failure and drain it during service shutdown. No raw spawn occurs.
/// Same move-owned participant actor across initiation and terminal phases.
/// The caller deposits this actor in original task custody before any await.
#[aura_macros::actor_owned(
    owner = "original_enrollment_transcript_actor",
    domain = "threshold_signing",
    gate = "enrollment_transcript_participant_ingress",
    command = ParticipantRequest,
    capacity = 2,
    category = "actor_owned"
)]
pub(in crate::runtime::services) struct OriginalEnrollmentTranscriptActor {
    device: DeviceId,
    index: u16,
    receiver: mpsc::Receiver<ParticipantRequest>,
    terminal_sender: watch::Sender<Option<AuraError>>,
    initial_started: bool,
    initial_completed: bool,
    conditional: OriginalConditionalRound,
    positive: OriginalPositiveRound,
    #[cfg(test)]
    positive_test: Option<Arc<OriginalPositiveActorTest>>,
}

pub(in crate::runtime::services) fn original_participant_actor(
    device: DeviceId,
    index: u16,
) -> (
    EnrollmentTranscriptParticipantIngress,
    OriginalEnrollmentTranscriptActor,
) {
    let (ingress, receiver, terminal_sender) = remote_public_ingress(device, index);
    (
        ingress,
        OriginalEnrollmentTranscriptActor {
            device,
            index,
            receiver,
            terminal_sender,
            initial_started: false,
            initial_completed: false,
            conditional: OriginalConditionalRound::default(),
            positive: OriginalPositiveRound::default(),
            #[cfg(test)]
            positive_test: None,
        },
    )
}

impl OriginalEnrollmentTranscriptActor {
    pub(in crate::runtime::services) async fn run_initial<'tree>(
        &mut self,
        approval: ApprovedLocalEnrollmentTranscript<'tree>,
        retained_completion: Arc<OriginalParticipantCompletionCustody>,
    ) -> Result<(), AuraError> {
        if self.initial_started || self.device != approval.device || self.index != approval.index {
            return Err(rejected(EnrollmentTranscriptSigningError::PackageBinding));
        }
        // Cancellation cannot replay initial domains or allocate another nonce.
        self.initial_started = true;
        let outcome = async {
            let effects = approval.effects.clone();
            approval
                .custody
                .generation()
                .require_effects(effects.as_ref())?;
            approval
                .original_window
                .execute(effects.as_ref(), || async {
                    let native_outcome = async {
                        let mut retirement_readbacks = Vec::with_capacity(4);
                        for domain in &approval.domains {
                            // A consumed approval is durably recorded before nonce creation. A
                            // restart cannot generate another nonce under the same approval.
                            let retired = effects
                                .secure_store_immutable(
                                    &domain.retirement_location,
                                    &domain.approval_digest,
                                    &[
                                        SecureStorageCapability::Read,
                                        SecureStorageCapability::Write,
                                    ],
                                )
                                .await?;
                            if retired != ImmutableSecureStoreOutcome::Created {
                                return Err(rejected(
                                    EnrollmentTranscriptSigningError::ApprovalConsumed,
                                ));
                            }
                            let actual = effects
                                .secure_retrieve(
                                    &domain.retirement_location,
                                    &[SecureStorageCapability::Read],
                                )
                                .await?;
                            if actual != domain.approval_digest {
                                return Err(rejected(
                                    EnrollmentTranscriptSigningError::PackageBinding,
                                ));
                            }
                            retirement_readbacks.push(actual);
                            // The nonces stay in this task's memory, so the durable
                            // approval record above plus an in-process retirement
                            // log bound them to one share.
                            let nonce_retirement = effects.enrollment_nonce_retirement();
                            let mut nonces =
                                Some(effects.frost_generate_nonces(approval.local_share).await?);
                            let mut published_commitment = false;
                            let mut original_commitment = None;
                            while let Some(request) = self.receiver.recv().await {
                                match request {
                                    ParticipantRequest::BeginPositiveTerminal(_, response)
                                    | ParticipantRequest::BeginNegativeTerminal(_, response)
                                    | ParticipantRequest::RetireConditional(response) => {
                                        let source = rejected(
                                            EnrollmentTranscriptSigningError::PackageBinding,
                                        );
                                        let _ = response.send(Err(source.clone()));
                                        return Err(source);
                                    }

                                    ParticipantRequest::Commitment(response) => {
                                        if published_commitment {
                                            let source = rejected(
                                                EnrollmentTranscriptSigningError::ApprovalConsumed,
                                            );
                                            let _ = response.send(Err(source.clone()));
                                            return Err(source);
                                        }
                                        let Some(local) = nonces.as_ref() else {
                                            let source = rejected(
                                                EnrollmentTranscriptSigningError::ApprovalConsumed,
                                            );
                                            let _ = response.send(Err(source.clone()));
                                            return Err(source);
                                        };
                                        let commitment = local.public_commitment();
                                        if commitment.participant_index != approval.index {
                                            let source = rejected(
                                                EnrollmentTranscriptSigningError::PackageBinding,
                                            );
                                            let _ = response.send(Err(source.clone()));
                                            return Err(source);
                                        }
                                        original_commitment =
                                            Some(commitment.commitment_bytes.clone());
                                        published_commitment = true;
                                        if response.send(Ok(commitment)).is_err() {
                                            return Err(rejected(
                                                EnrollmentTranscriptSigningError::ResponseCancelled,
                                            ));
                                        }
                                    }
                                    ParticipantRequest::Share(package, response) => {
                                        let native_commitment_matches =
                                            match native_commitment_matches(
                                                &package,
                                                approval.index,
                                                original_commitment.as_ref(),
                                            ) {
                                                Ok(matches) => matches,
                                                Err(source) => {
                                                    let _ = response.send(Err(source.clone()));
                                                    return Err(source);
                                                }
                                            };
                                        if !published_commitment
                                            || package.message != domain.message
                                            || package.public_key_package != approval.public_package
                                            || !package.participants.contains(&approval.index)
                                            || !native_commitment_matches
                                        {
                                            let source = rejected(
                                                EnrollmentTranscriptSigningError::PackageBinding,
                                            );
                                            let _ = response.send(Err(source.clone()));
                                            return Err(source);
                                        }
                                        // Move the nonce out before any awaited primitive. Errors
                                        // and cancellation drop it; no retry can borrow it again.
                                        let Some(local) = nonces.take() else {
                                            let source = rejected(
                                                EnrollmentTranscriptSigningError::ApprovalConsumed,
                                            );
                                            let _ = response.send(Err(source.clone()));
                                            return Err(source);
                                        };
                                        let local =
                                            match local.retire(nonce_retirement.as_ref()).await {
                                                Ok(local) => local,
                                                Err(source) => {
                                                    let _ = response.send(Err(source.clone()));
                                                    return Err(source);
                                                }
                                            };
                                        let share = match effects
                                            .frost_sign_share_for_message(
                                                &package,
                                                approval.local_share,
                                                local,
                                                &domain.message,
                                                &approval.public_package,
                                                approval.threshold,
                                            )
                                            .await
                                        {
                                            Ok(share) => share,
                                            Err(source) => {
                                                let _ = response.send(Err(source.clone()));
                                                return Err(source);
                                            }
                                        };
                                        response.send(Ok(share)).map_err(|_| {
                                            rejected(
                                                EnrollmentTranscriptSigningError::ResponseCancelled,
                                            )
                                        })?;
                                        break;
                                    }
                                }
                            }
                            if nonces.is_some() {
                                return Err(rejected(
                                    EnrollmentTranscriptSigningError::ResponseCancelled,
                                ));
                            }
                        }
                        let retirement_readbacks =
                            retirement_readbacks.try_into().map_err(|_| {
                                rejected(EnrollmentTranscriptSigningError::PackageBinding)
                            })?;
                        retained_completion.retain(OriginalParticipantCompletion {
                            effects: effects.clone(),
                            device: approval.device,
                            approved_intent_digest: approval.approved_intent_digest,
                            retirement_readbacks,
                        })
                    }
                    .await;
                    if let Err(source) = &native_outcome {
                        retained_completion.retain_failure(source);
                    }
                    native_outcome
                })
                .await
                .map_err(|source| AuraError::Crypto {
                    message: "original participant signing window failed".into(),
                    source: Some(Arc::new(source)),
                })
        }
        .await;
        if let Err(original) = &outcome {
            // Publish before dropping ingress: early protected-store/nonce/time
            // failures remain the concrete request error and owned task cause.
            let _ = self.terminal_sender.send(Some(original.clone()));
        }
        self.initial_completed = outcome.is_ok();
        outcome
    }
}

/// Public-only coordinator inputs. The constructor must be restricted to the
/// original local approved intent owner and retain its actual public policy.
impl ApprovedEnrollmentTranscriptRound<'_> {
    pub(super) async fn sign<T: aura_signature::SecurityTranscript>(
        self,
        transcript: &T,
        ingress: &[EnrollmentTranscriptParticipantIngress],
    ) -> Result<Vec<u8>, AuraError> {
        let original_bytes = transcript.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "encode original approved typed signing domain",
                Arc::new(source),
            )
        })?;
        if original_bytes != self.message {
            return Err(rejected(EnrollmentTranscriptSigningError::PackageBinding));
        }
        let effects = self.effects.clone();
        let admitted = ingress;
        let policy =
            aura_protocol::public_transcript_signing::PublicTranscriptSigningPolicy::checked(
                transcript,
                &self.public_package,
                &self.verifying_key,
                self.threshold,
                &self.participants,
            )?;
        if ingress.len() != self.participants.len()
            || self.participants.iter().any(|(device, index)| {
                ingress
                    .iter()
                    .filter(|entry| entry.device == *device && entry.index == *index)
                    .count()
                    != 1
            })
        {
            return Err(rejected(EnrollmentTranscriptSigningError::Inventory));
        }
        self.original_window
            .execute(effects.as_ref(), || async {
                aura_protocol::public_transcript_signing::sign_public_transcript(
                    effects.as_ref(),
                    policy,
                    |device, index| async move {
                        let participant = admitted
                            .iter()
                            .find(|entry| entry.device == device && entry.index == index)
                            .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::Inventory))?;
                        participant.commitment().await
                    },
                    |device, index, package| async move {
                        let participant = admitted
                            .iter()
                            .find(|entry| entry.device == device && entry.index == index)
                            .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::Inventory))?;
                        participant.share(package).await
                    },
                )
                .await
            })
            .await
            .map_err(|source| AuraError::Crypto {
                message: "original coordinator signing window failed".into(),
                source: Some(Arc::new(source)),
            })
    }
}

// Outside-only conditional phase; shared request/owner APIs must compile first.
use super::OriginalNegativeTerminalSigningMaterial;
use crate::runtime::services::enrollment_window::EnrollmentExecutionChild;

pub(in crate::runtime::services) enum OriginalConditionalActorExit {
    PositiveRetirement,
    NegativeSigned,
}

// These fields belong in OriginalEnrollmentTranscriptActor, not a future frame.
#[derive(Default)]
struct OriginalConditionalRound {
    request: Option<Arc<super::enrollment_transcript_wire::AdmittedNegativeTerminalRequest>>,
    retirement_started: bool,
    retirement_readback: Option<Vec<u8>>,
    nonces: Option<aura_core::crypto::tree_signing::FrostNonces>,
    nonce_consumed: bool,
    commitment: Option<FrostPublicCommitment>,
    commitment_delivered: bool,
    share: Option<Vec<u8>>,
    positive_retirement: Option<oneshot::Sender<Result<(), AuraError>>>,
    operation: Option<Result<(), AuraError>>,
    first_operation_failure: Option<AuraError>,
    first_validation_failure: Option<aura_core::TimeoutBudgetError>,
    validation: Option<Result<aura_core::time::PhysicalTime, aura_core::TimeoutBudgetError>>,
}

impl OriginalEnrollmentTranscriptActor {
    pub(in crate::runtime::services) async fn run_negative(
        &mut self,
        material: &OriginalNegativeTerminalSigningMaterial,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        window: &EnrollmentExecutionChild,
    ) -> Result<OriginalConditionalActorExit, AuraError> {
        if !self.initial_completed || self.device != material.device || self.index != material.index
        {
            return Err(rejected(EnrollmentTranscriptSigningError::PackageBinding));
        }
        let effects = material.effects.clone();
        let outcome = window
            .execute_original_negative_terminal(material, approval, || async {
                if self.conditional.positive_retirement.is_some()
                    || self.conditional.share.is_some()
                {
                    return Ok(());
                }
                if self.conditional.request.is_none() {
                    match self
                        .receiver
                        .recv()
                        .await
                        .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::ActorStopped))?
                    {
                        ParticipantRequest::RetireConditional(response) => {
                            self.conditional.positive_retirement = Some(response);
                            return Ok(());
                        }
                        ParticipantRequest::BeginNegativeTerminal(request, response) => {
                            let validation = request.require_material(material);
                            if let Err(source) = validation {
                                let _ = response.send(Err(source.clone()));
                                return Err(source);
                            }
                            self.conditional.request = Some(request);
                            response.send(Ok(())).map_err(|_| {
                                rejected(EnrollmentTranscriptSigningError::ResponseCancelled)
                            })?;
                        }
                        ParticipantRequest::BeginPositiveTerminal(_, response) => {
                            let source = rejected(EnrollmentTranscriptSigningError::PackageBinding);
                            let _ = response.send(Err(source.clone()));
                            return Err(source);
                        }
                        ParticipantRequest::Commitment(response) => {
                            let source = rejected(EnrollmentTranscriptSigningError::PackageBinding);
                            let _ = response.send(Err(source.clone()));
                            return Err(source);
                        }
                        ParticipantRequest::Share(_, response) => {
                            let source = rejected(EnrollmentTranscriptSigningError::PackageBinding);
                            let _ = response.send(Err(source.clone()));
                            return Err(source);
                        }
                    }
                }
                let request = self
                    .conditional
                    .request
                    .as_ref()
                    .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::PackageBinding))?
                    .clone();
                request.require_material(material)?;
                if self.conditional.retirement_started {
                    return Err(rejected(EnrollmentTranscriptSigningError::ApprovalConsumed));
                }
                self.conditional.retirement_started = true;
                let digest = request.approval_digest();
                let created = effects
                    .secure_store_immutable(
                        request.retirement_location(),
                        &digest,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await?;
                if created != ImmutableSecureStoreOutcome::Created {
                    return Err(rejected(EnrollmentTranscriptSigningError::ApprovalConsumed));
                }
                let readback = effects
                    .secure_retrieve(
                        request.retirement_location(),
                        &[SecureStorageCapability::Read],
                    )
                    .await?;
                if readback != digest {
                    return Err(rejected(EnrollmentTranscriptSigningError::PackageBinding));
                }
                self.conditional.retirement_readback = Some(readback);
                self.conditional.nonces =
                    Some(effects.frost_generate_nonces(&material.local_share).await?);
                let nonce_retirement = effects.enrollment_nonce_retirement();
                loop {
                    request.require_material(material)?;
                    match self
                        .receiver
                        .recv()
                        .await
                        .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::ActorStopped))?
                    {
                        ParticipantRequest::Commitment(response) => {
                            if self.conditional.commitment_delivered {
                                let source =
                                    rejected(EnrollmentTranscriptSigningError::ApprovalConsumed);
                                let _ = response.send(Err(source.clone()));
                                return Err(source);
                            }
                            let commitment = self
                                .conditional
                                .nonces
                                .as_ref()
                                .ok_or_else(|| {
                                    rejected(EnrollmentTranscriptSigningError::ApprovalConsumed)
                                })?
                                .public_commitment();
                            if commitment.participant_index != material.index {
                                return Err(rejected(
                                    EnrollmentTranscriptSigningError::PackageBinding,
                                ));
                            }
                            self.conditional.commitment = Some(commitment.clone());
                            self.conditional.commitment_delivered = true;
                            response.send(Ok(commitment)).map_err(|_| {
                                rejected(EnrollmentTranscriptSigningError::ResponseCancelled)
                            })?;
                        }
                        ParticipantRequest::Share(package, response) => {
                            let message = request.message()?;
                            let original = self
                                .conditional
                                .commitment
                                .as_ref()
                                .map(|value| &value.commitment_bytes);
                            if !self.conditional.commitment_delivered
                                || package.message != message
                                || package.public_key_package != material.public_package
                                || !package.participants.contains(&material.index)
                                || !native_commitment_matches(&package, material.index, original)?
                            {
                                let source =
                                    rejected(EnrollmentTranscriptSigningError::PackageBinding);
                                let _ = response.send(Err(source.clone()));
                                return Err(source);
                            }
                            let nonce = self.conditional.nonces.take().ok_or_else(|| {
                                rejected(EnrollmentTranscriptSigningError::ApprovalConsumed)
                            })?;
                            self.conditional.nonce_consumed = true;
                            let retired = nonce.retire(nonce_retirement.as_ref()).await?;
                            let share = effects
                                .frost_sign_share_for_message(
                                    &package,
                                    &material.local_share,
                                    retired,
                                    message,
                                    &material.public_package,
                                    material.threshold,
                                )
                                .await?;
                            self.conditional.share = Some(share.clone());
                            response.send(Ok(share)).map_err(|_| {
                                rejected(EnrollmentTranscriptSigningError::ResponseCancelled)
                            })?;
                            return Ok(());
                        }
                        ParticipantRequest::BeginPositiveTerminal(_, response)
                        | ParticipantRequest::BeginNegativeTerminal(_, response)
                        | ParticipantRequest::RetireConditional(response) => {
                            let source =
                                rejected(EnrollmentTranscriptSigningError::ApprovalConsumed);
                            let _ = response.send(Err(source.clone()));
                            return Err(source);
                        }
                    }
                }
            })
            .await?;
        // The primary is still the original material in its holder. Retain only the
        // independent result fields, avoiding a self-reference into that holder.
        if let Some(Err(source)) = &outcome.operation {
            self.conditional
                .first_operation_failure
                .get_or_insert_with(|| source.clone());
        }
        if let Err(source) = &outcome.validation {
            self.conditional
                .first_validation_failure
                .get_or_insert_with(|| source.clone());
        }
        self.conditional.operation = outcome.operation;

        self.conditional.validation = Some(outcome.validation);
        let interpreted = (|| {
            if let Some(source) = &self.conditional.first_operation_failure {
                return Err(source.clone());
            }
            if let Some(source) = &self.conditional.first_validation_failure {
                return Err(AuraError::from(source.clone()));
            }

            self.conditional
                .validation
                .as_ref()
                .ok_or_else(|| AuraError::invalid("conditional validation absent"))?
                .clone()
                .map_err(AuraError::from)?;
            self.conditional
                .operation
                .as_ref()
                .ok_or_else(|| AuraError::invalid("conditional operation interrupted"))?
                .clone()?;
            if self.conditional.positive_retirement.is_some() {
                return Ok(OriginalConditionalActorExit::PositiveRetirement);
            }
            if self.conditional.share.is_none() {
                return Err(AuraError::invalid("conditional share completion absent"));
            }
            Ok(OriginalConditionalActorExit::NegativeSigned)
        })();
        if let Err(source) = &interpreted {
            self.terminal_sender.send_replace(Some(source.clone()));
        }
        interpreted
    }
}
impl OriginalEnrollmentTranscriptActor {
    /// Acknowledges only actual local material zeroization. Physical task/domain
    /// disposal and activation authorization remain separately owned by caller.
    pub(in crate::runtime::services) fn acknowledge_conditional_material_retirement(
        &mut self,
        retired: &super::RetiredOriginalSigningMaterial,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        retired.require_original(approval)?;
        if self.device != retired.device
            || self.conditional.request.is_some()
            || !self.initial_completed
            || self.index == 0
        {
            return Err(rejected(EnrollmentTranscriptSigningError::PackageBinding));
        }
        self.conditional
            .positive_retirement
            .take()
            .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::ResponseCancelled))?
            .send(Ok(()))
            .map_err(|_| rejected(EnrollmentTranscriptSigningError::ResponseCancelled))
    }
}
/// One actual bounded ingress request, including its reply receiver, retained
/// by the original proxy/dispatcher rather than a disposable await frame.
pub(super) struct RetainedParticipantCall<T> {
    pending: Option<ParticipantRequest>,
    receiver: oneshot::Receiver<Result<T, AuraError>>,
    reply_cancelled: bool,
    result: Option<Result<T, AuraError>>,
}
impl<T: Clone> RetainedParticipantCall<T> {
    pub(super) fn retained_result(&self) -> Result<T, AuraError> {
        self.result
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original conditional ingress reply absent"))?
            .clone()
    }

    /// Deposit the actual request in the original actor ingress without
    /// awaiting its reply; the receiver remains in this retained call.
    pub(super) async fn admit(
        &mut self,
        ingress: &EnrollmentTranscriptParticipantIngress,
    ) -> Result<(), AuraError> {
        if let Some(Err(source)) = &self.result {
            return Err(source.clone());
        }
        if self.pending.is_some() {
            let permit = match ingress.requests.reserve().await {
                Ok(permit) => permit,
                Err(_) => {
                    let source = ingress
                        .stopped(EnrollmentTranscriptSigningError::ActorStopped)
                        .await;
                    self.result = Some(Err(source.clone()));
                    return Err(source);
                }
            };
            let request = self
                .pending
                .take()
                .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::ApprovalConsumed))?;
            permit.send(request);
        }
        Ok(())
    }

    pub(super) async fn run(
        &mut self,
        ingress: &EnrollmentTranscriptParticipantIngress,
    ) -> Result<T, AuraError> {
        if let Some(result) = &self.result {
            return result.clone();
        }
        self.admit(ingress).await?;
        if !self.reply_cancelled {
            match (&mut self.receiver).await {
                Ok(result) => {
                    self.result = Some(result.clone());
                    return result;
                }
                Err(_) => {
                    // The receiver is completed. Deposit this disposition before
                    // awaiting the original actor's concrete terminal source.
                    self.reply_cancelled = true;
                }
            }
        }
        let result = Err(ingress
            .stopped(EnrollmentTranscriptSigningError::ResponseCancelled)
            .await);
        self.result = Some(result.clone());
        result
    }
}
impl RetainedParticipantCall<()> {
    /// Observe only an already delivered original reply, without a new await
    /// after the holder has consumed its original signing material.
    pub(super) fn observe_ready_reply(&mut self) -> Result<(), AuraError> {
        if let Some(result) = &self.result {
            return result.clone();
        }
        if self.pending.is_some() || self.reply_cancelled {
            return Err(rejected(
                EnrollmentTranscriptSigningError::ResponseCancelled,
            ));
        }
        match self.receiver.try_recv() {
            Ok(result) => {
                self.result = Some(result.clone());
                result
            }
            Err(oneshot::error::TryRecvError::Empty) => Err(AuraError::invalid(
                "original retirement reply remains unresolved",
            )),
            Err(oneshot::error::TryRecvError::Closed) => {
                self.reply_cancelled = true;
                Err(rejected(
                    EnrollmentTranscriptSigningError::ResponseCancelled,
                ))
            }
        }
    }

    pub(super) fn retire() -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            pending: Some(ParticipantRequest::RetireConditional(sender)),
            receiver,
            reply_cancelled: false,
            result: None,
        }
    }

    pub(super) fn begin(
        request: Arc<super::enrollment_transcript_wire::AdmittedNegativeTerminalRequest>,
    ) -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            pending: Some(ParticipantRequest::BeginNegativeTerminal(request, sender)),
            receiver,
            reply_cancelled: false,
            result: None,
        }
    }
}
impl RetainedParticipantCall<FrostPublicCommitment> {
    pub(super) fn commitment() -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            pending: Some(ParticipantRequest::Commitment(sender)),
            receiver,
            reply_cancelled: false,
            result: None,
        }
    }
}
impl RetainedParticipantCall<Vec<u8>> {
    pub(super) fn share(package: FrostSigningPackage) -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            pending: Some(ParticipantRequest::Share(package, sender)),
            receiver,
            reply_cancelled: false,
            result: None,
        }
    }
}

#[derive(Default)]
pub(super) struct ConditionalOutcomeCustody {
    operation: Option<Result<(), AuraError>>,
    validation: Option<Result<aura_core::time::PhysicalTime, aura_core::TimeoutBudgetError>>,
    first_operation_failure: Option<AuraError>,
    first_validation_failure: Option<aura_core::TimeoutBudgetError>,
}
impl ConditionalOutcomeCustody {
    pub(super) fn retain<P>(
        &mut self,
        outcome: aura_core::time::timeout::NegativeCleanupExecution<P, (), AuraError>,
    ) -> Result<(), AuraError> {
        if let Some(Err(source)) = &outcome.operation {
            self.first_operation_failure
                .get_or_insert_with(|| source.clone());
        }
        if let Err(source) = &outcome.validation {
            self.first_validation_failure
                .get_or_insert_with(|| source.clone());
        }
        self.operation = outcome.operation;
        self.validation = Some(outcome.validation);
        if let Some(source) = &self.first_operation_failure {
            return Err(source.clone());
        }
        if let Some(source) = &self.first_validation_failure {
            return Err(AuraError::from(source.clone()));
        }
        self.operation
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original conditional transport interrupted"))?
            .clone()?;
        self.validation
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original conditional transport validation missing"))?
            .clone()
            .map_err(AuraError::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod retained_call_drop_tests {
    use super::*;
    use aura_core::effects::PhysicalTimeEffects;
    use aura_core::time::timeout::{execute_with_timeout_budget, TimeoutBudget, TimeoutRunError};
    use aura_testkit::time::ManualPhysicalClock;
    use std::time::Duration;

    #[tokio::test]
    async fn retirement_admission_drop_resumes_original_reply_without_reenqueue() {
        let clock = ManualPhysicalClock::new(5_000);
        let started = clock
            .physical_time()
            .await
            .expect("selected original clock");
        let budget = TimeoutBudget::from_start_and_timeout(&started, Duration::from_secs(5))
            .expect("one original budget");
        let (ingress, mut requests, _terminal) =
            remote_public_ingress(DeviceId::new_from_entropy([212; 32]), 1);
        let mut call = RetainedParticipantCall::retire();
        execute_with_timeout_budget(&clock, &budget, || call.admit(&ingress))
            .await
            .expect("send-only original retirement admitted");
        let actual = requests.try_recv().expect("actual retirement request");
        let ParticipantRequest::RetireConditional(reply) = actual else {
            panic!("original retirement request");
        };
        assert!(call.pending.is_none());
        let mut waiting = Box::pin(execute_with_timeout_budget(&clock, &budget, || {
            call.run(&ingress)
        }));
        assert!(futures::poll!(waiting.as_mut()).is_pending());
        drop(waiting);
        assert!(
            !call.reply_cancelled,
            "original receiver remains live after observer Drop"
        );
        reply
            .send(Ok(()))
            .expect("same actual actor reply receiver retained");
        execute_with_timeout_budget(&clock, &budget, || call.run(&ingress))
            .await
            .expect("same original retirement reply resumes");
        call.retained_result()
            .expect("actual reply deposited before Ready");
        execute_with_timeout_budget(&clock, &budget, || call.run(&ingress))
            .await
            .expect("completed original receipt is replayed without receiver repoll");
        assert!(matches!(
            requests.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(clock.physical_time().await.unwrap(), started);
    }

    #[tokio::test]
    async fn cancelled_reply_drop_resumes_original_terminal_without_repoll_or_reenqueue() {
        let clock = ManualPhysicalClock::new(5_000);
        let started = clock
            .physical_time()
            .await
            .expect("selected original clock");
        let budget = TimeoutBudget::from_start_and_timeout(&started, Duration::from_secs(5))
            .expect("one original budget");
        let (ingress, mut requests, terminal) =
            remote_public_ingress(DeviceId::new_from_entropy([211; 32]), 1);
        let mut call = RetainedParticipantCall::commitment();
        let mut original = Box::pin(execute_with_timeout_budget(&clock, &budget, || {
            call.run(&ingress)
        }));
        assert!(futures::poll!(original.as_mut()).is_pending());
        let actual = requests
            .try_recv()
            .expect("actual bounded request admitted");
        let ParticipantRequest::Commitment(reply) = actual else {
            panic!("original commitment request");
        };
        drop(reply);
        assert!(
            futures::poll!(original.as_mut()).is_pending(),
            "cancelled reply waits for original concrete actor terminal"
        );
        drop(original);
        assert!(
            call.reply_cancelled,
            "completed receiver disposition survives Drop"
        );
        assert!(
            call.pending.is_none(),
            "actual request was already sent once"
        );
        let native = Arc::new(std::io::Error::other("original-retained-reply-terminal"));
        terminal.send_replace(Some(AuraError::crypto_with_source(
            "original actor terminal",
            native.clone(),
        )));
        let resumed = execute_with_timeout_budget(&clock, &budget, || call.run(&ingress)).await;
        let Err(TimeoutRunError::Operation(error)) = resumed else {
            panic!("same original call must return the actor terminal source");
        };
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        loop {
            if let Some(actual) = cause.downcast_ref::<std::io::Error>() {
                assert!(std::ptr::eq(actual, native.as_ref()));
                break;
            }
            cause = cause.source().expect("original native source retained");
        }
        assert!(matches!(
            requests.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(clock.physical_time().await.unwrap(), started);
    }
}

#[derive(Default)]
struct OriginalPositiveRound {
    failure: Option<AuraError>,
    request: Option<Arc<super::positive_terminal::AdmittedPositiveTerminalRequest>>,
    interruption: Option<AuraError>,
    nonce_consumed: bool,
    birth_started: bool,
    readback: Option<Vec<u8>>,
    nonces: Option<aura_core::crypto::tree_signing::FrostNonces>,
    commitment: Option<FrostPublicCommitment>,
    share: Option<Vec<u8>>,
    outcome: ConditionalOutcomeCustody,
}
impl RetainedParticipantCall<()> {
    pub(super) fn begin_positive(
        request: Arc<super::positive_terminal::AdmittedPositiveTerminalRequest>,
    ) -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            pending: Some(ParticipantRequest::BeginPositiveTerminal(request, sender)),
            receiver,
            reply_cancelled: false,
            result: None,
        }
    }
}
impl OriginalEnrollmentTranscriptActor {
    async fn run_positive_inner(
        &mut self,
        material: &super::OriginalPositiveTerminalSigningMaterial,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        window: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        if !self.initial_completed
            || self.device != material.device
            || self.index != material.index
            || self.conditional.request.is_some()
        {
            return Err(rejected(EnrollmentTranscriptSigningError::PackageBinding));
        }
        let observed = window
            .execute_original_positive_terminal(material, approval, || async {
                let result = async {
                    if self.positive.share.is_some() {
                        return Ok(());
                    }
                    if self.positive.request.is_none() {
                        match self.receiver.recv().await.ok_or_else(|| {
                            rejected(EnrollmentTranscriptSigningError::ActorStopped)
                        })? {
                            ParticipantRequest::BeginPositiveTerminal(request, response) => {
                                request.require_material(material, window)?;
                                self.positive.request = Some(request);
                                response.send(Ok(())).map_err(|_| {
                                    rejected(EnrollmentTranscriptSigningError::ResponseCancelled)
                                })?;
                            }
                            _ => {
                                return Err(rejected(
                                    EnrollmentTranscriptSigningError::PackageBinding,
                                ))
                            }
                        }
                    }
                    let request = Arc::clone(self.positive.request.as_ref().ok_or_else(|| {
                        rejected(EnrollmentTranscriptSigningError::PackageBinding)
                    })?);
                    request.require_material(material, window)?;
                    if self.positive.birth_started {
                        return Err(self.positive.interruption.clone().unwrap_or_else(|| {
                            rejected(EnrollmentTranscriptSigningError::ApprovalConsumed)
                        }));
                    }
                    self.positive.birth_started = true;
                    self.positive.interruption = Some(rejected(
                        EnrollmentTranscriptSigningError::PositiveInterrupted {
                            stage: "original immutable nonce intent",
                        },
                    ));
                    let (digest, location) = request.retirement_binding()?;
                    let created = material
                        .effects
                        .secure_store_immutable(
                            &location,
                            &digest,
                            &[
                                SecureStorageCapability::Read,
                                SecureStorageCapability::Write,
                            ],
                        )
                        .await?;
                    if created != ImmutableSecureStoreOutcome::Created {
                        return Err(rejected(EnrollmentTranscriptSigningError::ApprovalConsumed));
                    }
                    #[cfg(test)]
                    if let Some(test) = &self.positive_test {
                        if test.mode == OriginalPositiveActorTestMode::NativeFailure {
                            assert!(
                                material
                                    .effects
                                    .fault_remove_secure_record_for_test(&location)
                                    .await?
                            );
                        }
                    }
                    let readback = material
                        .effects
                        .secure_retrieve(&location, &[SecureStorageCapability::Read])
                        .await?;
                    if readback != digest {
                        return Err(rejected(EnrollmentTranscriptSigningError::PackageBinding));
                    }
                    self.positive.readback = Some(readback);
                    self.positive.interruption = Some(rejected(
                        EnrollmentTranscriptSigningError::PositiveInterrupted {
                            stage: "original nonce birth",
                        },
                    ));
                    self.positive.nonces = Some(
                        material
                            .effects
                            .frost_generate_nonces(&material.local_share)
                            .await?,
                    );
                    let retirement = material.effects.enrollment_nonce_retirement();
                    loop {
                        request.require_material(material, window)?;
                        match self.receiver.recv().await.ok_or_else(|| {
                            rejected(EnrollmentTranscriptSigningError::ActorStopped)
                        })? {
                            ParticipantRequest::Commitment(response) => {
                                if self.positive.commitment.is_some() {
                                    return Err(rejected(
                                        EnrollmentTranscriptSigningError::ApprovalConsumed,
                                    ));
                                }
                                let commitment = self
                                    .positive
                                    .nonces
                                    .as_ref()
                                    .ok_or_else(|| {
                                        rejected(EnrollmentTranscriptSigningError::ApprovalConsumed)
                                    })?
                                    .public_commitment();
                                if commitment.participant_index != material.index {
                                    return Err(rejected(
                                        EnrollmentTranscriptSigningError::PackageBinding,
                                    ));
                                }
                                self.positive.commitment = Some(commitment.clone());
                                response.send(Ok(commitment)).map_err(|_| {
                                    rejected(EnrollmentTranscriptSigningError::ResponseCancelled)
                                })?;
                            }
                            ParticipantRequest::Share(package, response) => {
                                let message = request.message()?;
                                if package.message != message
                                    || package.public_key_package != material.public_package
                                    || !package.participants.contains(&material.index)
                                    || !native_commitment_matches(
                                        &package,
                                        material.index,
                                        self.positive
                                            .commitment
                                            .as_ref()
                                            .map(|v| &v.commitment_bytes),
                                    )?
                                {
                                    return Err(rejected(
                                        EnrollmentTranscriptSigningError::PackageBinding,
                                    ));
                                }
                                let nonce = self.positive.nonces.take().ok_or_else(|| {
                                    rejected(EnrollmentTranscriptSigningError::ApprovalConsumed)
                                })?;
                                self.positive.nonce_consumed = true;
                                self.positive.interruption = Some(rejected(
                                    EnrollmentTranscriptSigningError::PositiveInterrupted {
                                        stage: "original nonce retirement and signing",
                                    },
                                ));
                                #[cfg(test)]
                                if let Some(test) = &self.positive_test {
                                    if test.mode == OriginalPositiveActorTestMode::ConsumedNonce {
                                        test.entered.notify_one();
                                        test.release.notified().await;
                                    }
                                }
                                let retired = nonce.retire(retirement.as_ref()).await?;
                                let share = material
                                    .effects
                                    .frost_sign_share_for_message(
                                        &package,
                                        &material.local_share,
                                        retired,
                                        &message,
                                        &material.public_package,
                                        material.threshold,
                                    )
                                    .await?;
                                self.positive.share = Some(share.clone());
                                self.positive.interruption = None;
                                response.send(Ok(share)).map_err(|_| {
                                    rejected(EnrollmentTranscriptSigningError::ResponseCancelled)
                                })?;
                                return Ok(());
                            }
                            _ => {
                                return Err(rejected(
                                    EnrollmentTranscriptSigningError::ApprovalConsumed,
                                ))
                            }
                        }
                    }
                }
                .await;
                if let Err(source) = &result {
                    self.positive.failure.get_or_insert_with(|| source.clone());
                    #[cfg(test)]
                    if let Some(test) = &self.positive_test {
                        if test.mode == OriginalPositiveActorTestMode::NativeFailure {
                            test.armed.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                }
                result
            })
            .await?;
        self.positive.outcome.retain(observed)
    }
}
impl OriginalEnrollmentTranscriptActor {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalPositiveTerminalSigningMaterial", receiver_type = OriginalEnrollmentTranscriptActor, family = "runtime_helper")]
    pub(in crate::runtime::services) async fn run_positive(
        &mut self,
        material: &super::OriginalPositiveTerminalSigningMaterial,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        window: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        if let Some(source) = &self.positive.failure {
            return Err(source.clone());
        }
        #[cfg(test)]
        if let Some(test) = self.positive_test.as_ref().map(Arc::clone) {
            if !test.driven.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let mut actual = Box::pin(self.run_positive_inner(material, approval, window));
                tokio::select! {
                    _ = test.entered.notified() => {},
                    result = &mut actual => panic!("actual positive actor ended before owned Drop gate: {result:?}"),
                }
                drop(actual);
                assert!(self.positive.request.is_some());
                assert!(self.positive.birth_started);
                assert!(self.positive.share.is_none());
                let native = match test.mode {
                    OriginalPositiveActorTestMode::ConsumedNonce => {
                        assert!(self.positive.nonce_consumed);
                        assert!(self.positive.nonces.is_none());
                        assert!(self.positive.commitment.is_some());
                        assert!(self.positive.readback.is_some());
                        assert!(self.positive.failure.is_none());
                        None
                    }
                    OriginalPositiveActorTestMode::NativeFailure => {
                        assert!(!self.positive.nonce_consumed);
                        assert!(self.positive.nonces.is_none());
                        let source = self
                            .positive
                            .failure
                            .as_ref()
                            .expect("actual native source deposited before postread")
                            .clone();
                        let mut current: &(dyn std::error::Error + 'static) = &source;
                        let mut missing = false;
                        loop {
                            missing |= current.downcast_ref::<aura_core::effects::secure::SecureStorageRecordMissing>().is_some();
                            match current.source() {
                                Some(next) => current = next,
                                None => break,
                            }
                        }
                        assert!(
                            missing,
                            "concrete native missing-record cause retained: {source:?}"
                        );
                        Some(source)
                    }
                };
                test.release.notify_one();
                let resumed = Box::pin(self.run_positive(material, approval, window))
                    .await
                    .expect_err("same original actor reentry must refuse");
                if let Some(native) = native {
                    assert_eq!(native.to_string(), resumed.to_string());
                    let left = std::error::Error::source(&native).expect("concrete native source");
                    let right =
                        std::error::Error::source(&resumed).expect("same concrete native source");
                    assert!(std::ptr::eq(left, right), "same original native source Arc");
                } else {
                    let mut current: &(dyn std::error::Error + 'static) = &resumed;
                    let mut interrupted = false;
                    loop {
                        interrupted |= matches!(
                            current.downcast_ref::<EnrollmentTranscriptSigningError>(),
                            Some(EnrollmentTranscriptSigningError::PositiveInterrupted {
                                stage: "original nonce retirement and signing"
                            })
                        );
                        match current.source() {
                            Some(next) => current = next,
                            None => break,
                        }
                    }
                    assert!(
                        interrupted,
                        "actual consumed-nonce interruption retained: {resumed:?}"
                    );
                }
                assert!(self.positive.nonces.is_none());
                assert!(self.positive.share.is_none());
                test.completed
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                test.finished.notify_one();
                return Err(resumed);
            }
        }
        let result = self.run_positive_inner(material, approval, window).await;
        if let Err(source) = &result {
            self.positive.failure.get_or_insert_with(|| source.clone());
            self.terminal_sender.send_replace(Some(source.clone()));
        }
        result
    }
}
impl OriginalEnrollmentTranscriptActor {
    /// End the retained original nonce custody; this supplies no outcome ACK.
    pub(in crate::runtime::services) fn dispose_original_positive_nonces(&mut self) {
        self.positive.nonces = None;
    }
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OriginalPositiveActorTestMode {
    ConsumedNonce,
    NativeFailure,
}
#[cfg(test)]
pub(crate) struct OriginalPositiveActorTest {
    clock: aura_testkit::time::ManualPhysicalClock,
    mode: OriginalPositiveActorTestMode,
    armed: std::sync::atomic::AtomicBool,
    driven: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    completed: std::sync::atomic::AtomicBool,
    finished: tokio::sync::Notify,
}
#[cfg(test)]
impl OriginalPositiveActorTest {
    pub(crate) fn new(
        clock: aura_testkit::time::ManualPhysicalClock,
        mode: OriginalPositiveActorTestMode,
    ) -> Self {
        Self {
            clock,
            mode,
            armed: false.into(),
            driven: false.into(),
            entered: tokio::sync::Notify::default(),
            release: tokio::sync::Notify::default(),
            completed: false.into(),
            finished: tokio::sync::Notify::default(),
        }
    }
    pub(crate) async fn await_completed(&self) {
        self.finished.notified().await;
    }
    pub(crate) fn assert_completed(&self) {
        assert!(self.completed.load(std::sync::atomic::Ordering::SeqCst));
    }
}
#[cfg(test)]
impl OriginalEnrollmentTranscriptActor {
    pub(super) fn install_positive_test(&mut self, test: Arc<OriginalPositiveActorTest>) {
        assert!(self.positive_test.is_none());
        self.positive_test = Some(test);
    }
}
#[cfg(test)]
#[async_trait::async_trait]
impl aura_core::effects::PhysicalTimeEffects for OriginalPositiveActorTest {
    async fn physical_time(
        &self,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.clock.physical_time().await
    }
    async fn sleep_ms(&self, duration: u64) -> Result<(), aura_core::effects::TimeError> {
        self.clock.sleep_ms(duration).await
    }
    async fn wait_until_physical_deadline(
        &self,
        endpoint: aura_core::types::window::WindowPosition<
            aura_core::types::window::PhysicalMillis,
        >,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        self.clock.wait_until_physical_deadline(endpoint).await
    }
}
